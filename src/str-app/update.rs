//! Message handling for the app.

use std::path::PathBuf;

use iced::Task;

use super::{pipe_task, App, Banner, Message, OpenTarget, Phase, ServerField};
use crate::config::Profile;
use crate::nexus::NexusClient;
use crate::server::INI_HINT;
use crate::str_install::Event as PipeEvent;
use crate::{config, str_install};

impl App {
    /// Point the app at a validated game folder. Shows an error banner when
    /// the folder does not contain SkyrimSE.exe (shared by the path input and
    /// the folder picker).
    fn set_game_dir(&mut self, p: PathBuf) {
        if !p.join("SkyrimSE.exe").is_file() {
            self.banner = Some(Banner::Error(format!(
                "{} does not contain SkyrimSE.exe.",
                p.display()
            )));
            return;
        }
        self.game_dir = Some(p.clone());
        self.game_path_input = p.display().to_string();
        self.steam_root = None;
        self.cfg.active_mut().game_dir = Some(p.clone());
        self.save_cfg();
        self.banner = Some(Banner::Info(format!("Game folder set to {}", p.display())));
        self.refresh_probe();
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Noop => Task::none(),

            Message::Loaded(result) => {
                let super::LoadResult { dirs, cfg, installs, proton_names } = *result;
                self.proton_names = proton_names;
                self.proton_selected = self
                    .proton_names
                    .iter()
                    .find(|n| n.starts_with("GE-Proton10"))
                    .cloned()
                    .or_else(|| self.proton_names.first().cloned())
                    .or_else(|| Some(crate::proton::default_tool_name()));
                match dirs {
                    Ok(d) => {
                        self.cfg = cfg;
                        self.api_key = self.cfg.nexus_api_key.clone();
                        // Honor the saved pick only while the tool still exists;
                        // a stale entry would otherwise override GE-Proton10
                        // forever (the readiness check flags bad picks anyway).
                        let saved = self.cfg.active().proton_tool.clone();
                        if let Some(active) = saved {
                            if self.proton_names.contains(&active) {
                                self.proton_selected = Some(active);
                            }
                        }
                        self.dirs = Some(d);
                    }
                    Err(e) => {
                        tracing::error!("app data directories unavailable: {e}");
                        self.banner = Some(Banner::Error(e));
                    }
                }
                if self.game_dir.is_none() {
                    if let Some(active) = self.cfg.active().game_dir.clone() {
                        self.game_dir = Some(active);
                    } else if let Some(found) = installs.first().cloned() {
                        self.game_dir = Some(found.game_dir.clone());
                        self.steam_root = Some(found.steam_root.clone());
                    }
                }
                tracing::info!("startup: {} Skyrim install(s) detected", installs.len());
                self.installs = installs;
                if let Some(gd) = &self.game_dir {
                    self.game_path_input = gd.display().to_string();
                }
                self.refresh_probe();
                Task::none()
            }

            Message::Detect => {
                self.phase = Phase::Detecting;
                Task::perform(
                    async {
                        tokio::task::spawn_blocking(crate::steam::detect)
                            .await
                            .unwrap_or_default()
                    },
                    Message::Detected,
                )
            }
            Message::Detected(installs) => {
                self.phase = Phase::Idle;
                self.installs = installs;
                match self.installs.first() {
                    Some(found) => {
                        self.game_dir = Some(found.game_dir.clone());
                        self.steam_root = Some(found.steam_root.clone());
                        self.game_path_input = found.game_dir.display().to_string();
                        self.banner = Some(Banner::Info(format!(
                            "Found Skyrim Special Edition: {}",
                            found.game_dir.display()
                        )));
                    }
                    None => {
                        self.banner = Some(Banner::Error(
                            "Skyrim Special Edition was not found in any Steam library (all\n\
                             libraryfolders.vdf lists + common mount points were scanned).\n\
                             \u{2022} Steam users: install the game through Steam, then Re-detect.\n\
                             \u{2022} GOG / Epic / Heroic / manual installs: auto-detection is Steam-only \u{2014}\n\
                             set the game folder under Advanced \u{2192} Game folder override."
                                .into(),
                        ));
                    }
                }
                self.refresh_probe();
                Task::none()
            }

            Message::Install => match self.build_params() {
                Ok(params) => {
                    self.cfg.nexus_api_key = self.api_key.trim().to_string();
                    self.save_cfg();
                    self.phase = Phase::Installing;
                    self.progress = None;
                    self.step_label = "Preparing…".into();
                    self.push_log("── Starting install ──");
                    pipe_task(move |tx| str_install::run(params, tx))
                }
                Err(e) => {
                    self.banner = Some(Banner::Error(e));
                    Task::none()
                }
            },

            Message::Undo => match self.build_params() {
                Ok(params) => {
                    self.phase = Phase::Undoing;
                    self.push_log("── Undo: removing Skyrim Together Reborn ──");
                    pipe_task(move |tx| str_install::undo(params, tx))
                }
                Err(e) => {
                    self.banner = Some(Banner::Error(e));
                    Task::none()
                }
            },

            Message::Pipeline(event) => {
                match event {
                    PipeEvent::Step { label, index, total } => {
                        self.step_index = index;
                        self.step_total = total;
                        self.push_log(format!("▸ {label}"));
                        self.step_label = label;
                    }
                    PipeEvent::Progress { label, done, total } => {
                        self.progress = Some((
                            label,
                            if total > 0 { done as f32 / total as f32 } else { 0.0 },
                        ));
                    }
                    PipeEvent::Log(line) => self.push_log(line),
                    PipeEvent::Done(result) => {
                        let was_server_get = self.phase == Phase::GettingServer;
                        match &result {
                            Ok(_) if was_server_get => tracing::info!("server pipeline finished"),
                            Ok(_) => tracing::info!("install pipeline finished"),
                            Err(e) => tracing::error!("pipeline failed: {e}"),
                        }
                        self.phase = Phase::Idle;
                        self.progress = None;
                        match result {
                            Ok(_summary) if was_server_get => {
                                self.server_installed = true;
                                self.banner = Some(Banner::Info(
                                    "Dedicated server installed. Run it once to generate \
                                     config/STServer.ini, then press Reload in the Server section."
                                        .into(),
                                ));
                                self.refresh_probe();
                            }
                            Ok(summary) => {
                                self.game_version = summary.game_version.clone();
                                self.str_version = summary.str_version.clone();
                                self.al_version = summary.address_library_version.clone();
                                for w in &summary.warnings {
                                    self.push_log(format!("⚠ {w}"));
                                }
                                let installed_ver =
                                    summary.str_version.clone().unwrap_or_default();
                                self.banner = Some(Banner::Info(if installed_ver.is_empty() {
                                    "Finished.".into()
                                } else {
                                    format!(
                                        "Done! Skyrim Together Reborn {installed_ver} is installed. \
                                         Launch the game and press F2 (or Right Ctrl) to open the \
                                         multiplayer menu."
                                    )
                                }));
                                if let Some(dir) = self.game_dir.clone() {
                                    let manifest = config::InstallManifest::load(&dir);
                                    str_install::apply_summary_to_profile(
                                        &mut self.cfg,
                                        &summary,
                                        &manifest,
                                    );
                                    self.save_cfg();
                                }
                                self.refresh_probe();
                            }
                            Err(e) => self.banner = Some(Banner::Error(e)),
                        }
                    }
                }
                Task::none()
            }

            Message::LaunchTogether => match self.launch_ctx() {
                Ok(ctx) => Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || crate::launch::launch_together(&ctx))
                            .await
                            .map_err(|e| e.to_string())?
                    },
                    Message::LaunchResult,
                ),
                Err(e) => {
                    self.banner = Some(Banner::Error(e));
                    Task::none()
                }
            },
            Message::LaunchResult(result) => {
                match result {
                    Ok(()) => self.push_log("Skyrim Together launched."),
                    Err(e) => {
                        tracing::error!("launching Skyrim Together failed: {e}");
                        self.banner = Some(Banner::Error(e));
                    }
                }
                Task::none()
            }
            Message::LaunchVanilla => match self.launch_ctx() {
                Ok(ctx) => Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || crate::launch::launch_vanilla(&ctx))
                            .await
                            .map_err(|e| e.to_string())?
                    },
                    Message::VanillaResult,
                ),
                Err(e) => {
                    self.banner = Some(Banner::Error(e));
                    Task::none()
                }
            },
            Message::VanillaResult(result) => {
                match result {
                    Ok(()) => self.push_log("Launched through Steam."),
                    Err(e) => self.banner = Some(Banner::Error(e)),
                }
                Task::none()
            }

            Message::CheckUpdates => {
                if self.api_key.trim().is_empty() {
                    self.banner = Some(Banner::Error(
                        "Checking for updates needs a Nexus API key.\n\
                         Use GET A KEY, then paste it in Advanced settings."
                            .into(),
                    ));
                    return Task::none();
                }
                self.checking = true;
                let key = self.api_key.trim().to_string();
                Task::perform(
                    async move {
                        let client = NexusClient::new(&key);
                        client.validate_key().await?;
                        let latest = client
                            .latest_main_file("skyrimspecialedition", config::NEXUS_STR)
                            .await?;
                        Ok(latest.map(|f| {
                            if f.mod_version.is_empty() { f.version } else { f.mod_version }
                        }))
                    },
                    Message::UpdatesChecked,
                )
            }
            Message::SelfUpdateChecked(result) => {
                match result {
                    Ok(Some(latest)) => {
                        tracing::info!(
                            "launcher update available: {latest} (running {})",
                            crate::paths::APP_VERSION
                        );
                        self.launcher_update = Some(latest);
                    }
                    Ok(None) => {
                        tracing::info!("launcher is up to date (v{})", crate::paths::APP_VERSION)
                    }
                    Err(e) => {
                        // A failed update check must never nag the user at
                        // startup — it just stays in the file log.
                        tracing::warn!("launcher self-update check failed: {e}");
                    }
                }
                Task::none()
            }
            Message::DismissUpdateBanner => {
                self.launcher_update = None;
                Task::none()
            }

            Message::UpdatesChecked(result) => {
                self.checking = false;
                match result {
                    Ok(Some(latest)) => {
                        let installed = self
                            .cfg
                            .active()
                            .installed_str_version
                            .clone()
                            .unwrap_or_default();
                        if installed.is_empty() || installed != latest {
                            self.banner = Some(Banner::Info(format!(
                                "Update available: Skyrim Together Reborn {latest} \
                                 (installed: {}). Press Install to update.",
                                if installed.is_empty() { "none" } else { installed.as_str() }
                            )));
                        } else {
                            self.banner = Some(Banner::Info(format!(
                                "You are on the latest Skyrim Together Reborn ({latest})."
                            )));
                        }
                    }
                    Ok(None) => {
                        self.banner = Some(Banner::Info("No main file found on Nexus.".into()));
                    }
                    Err(e) => {
                        tracing::warn!("Nexus update check failed: {e}");
                        self.banner = Some(Banner::Error(format!("Update check failed:\n{e}")))
                    }
                }
                Task::none()
            }

            Message::ApiKeyChanged(v) => {
                self.api_key = v;
                self.cfg.nexus_api_key = self.api_key.trim().to_string();
                self.save_cfg();
                Task::none()
            }
            Message::GamePathChanged(v) => {
                self.game_path_input = v;
                Task::none()
            }
            Message::GamePathSubmitted => {
                self.set_game_dir(PathBuf::from(self.game_path_input.trim()));
                Task::none()
            }
            Message::BrowseGameDir => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Select the Skyrim Special Edition folder (contains SkyrimSE.exe)")
                        .pick_folder()
                        .await
                        .map(|f| f.path().to_path_buf())
                },
                Message::GameDirPicked,
            ),
            Message::GameDirPicked(path) => {
                if let Some(p) = path {
                    self.set_game_dir(p);
                }
                Task::none()
            }
            Message::ProtonPicked(name) => {
                self.proton_selected = Some(name.clone());
                self.cfg.active_mut().proton_tool = Some(name);
                self.save_cfg();
                Task::none()
            }
            Message::TabSelected(tab) => {
                self.tab = tab;
                Task::none()
            }
            Message::ToggleCleanCc(v) => {
                self.cfg.clean_cc = v;
                self.save_cfg();
                Task::none()
            }
            Message::LaunchModePicked(mode) => {
                self.cfg.launch_mode = mode;
                self.save_cfg();
                self.refresh_checks();
                self.push_log(format!(
                    "Launch mode set to: {}",
                    match mode {
                        config::LaunchMode::LauncherReplacement => "replace the Skyrim launcher",
                        config::LaunchMode::SteamGame => "add a separate Steam game",
                    }
                ));
                Task::none()
            }
            Message::BrowseOffline(which) => {
                let spec: (&'static str, &str, Vec<&str>) = match which {
                    0 => ("Select the Skyrim Together Reborn .zip", "Archives", vec!["zip"]),
                    1 => ("Select the Address Library .zip", "Archives", vec!["zip"]),
                    _ => ("Select the Skyrim Together server .zip", "Archives", vec!["zip"]),
                };
                Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title(spec.0)
                            .add_filter(spec.1, &spec.2)
                            .pick_file()
                            .await
                            .map(|f| f.path().to_path_buf())
                    },
                    move |p| Message::OfflinePicked(which, p),
                )
            }
            Message::OfflinePicked(which, path) => {
                if (which as usize) < self.offline.len() {
                    self.offline[which as usize] = path;
                }
                Task::none()
            }
            Message::ClearOffline(which) => {
                if (which as usize) < self.offline.len() {
                    self.offline[which as usize] = None;
                }
                Task::none()
            }
            Message::OpenFolder(target) => {
                let dirs = self.dirs.clone();
                let path = match (target, dirs) {
                    (OpenTarget::Game, _) => self.game_dir.clone(),
                    (OpenTarget::Downloads, Some(d)) => Some(d.downloads.clone()),
                    (OpenTarget::Server, Some(d)) => Some(crate::launch::server_dir(&d)),
                    _ => None,
                };
                if let Some(p) = path {
                    Task::perform(
                        async move {
                            let _ = tokio::task::spawn_blocking(move || crate::paths::open_folder(&p)).await;
                        },
                        |_| Message::Noop,
                    )
                } else {
                    Task::none()
                }
            }
            Message::OpenLink(url) => {
                let url = url.to_string();
                Task::perform(
                    async move {
                        let _ = tokio::task::spawn_blocking(move || crate::paths::open_url(&url)).await;
                    },
                    |_| Message::Noop,
                )
            }
            Message::DismissBanner => {
                self.banner = None;
                Task::none()
            }
            Message::ToggleChecks => {
                self.show_checks = !self.show_checks;
                Task::none()
            }
            Message::ProfilePicked(idx) => {
                self.cfg.active_profile = idx.min(self.cfg.profiles.len().saturating_sub(1));
                let active = self.cfg.active().clone();
                self.game_dir = active.game_dir.clone();
                self.game_path_input = active
                    .game_dir
                    .clone()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default();
                self.refresh_probe();
                self.save_cfg();
                Task::none()
            }
            Message::ProfileNew => {
                let n = self.cfg.profiles.len() + 1;
                self.cfg.profiles.push(Profile {
                    name: format!("Profile {n}"),
                    ..Default::default()
                });
                self.cfg.active_profile = self.cfg.profiles.len() - 1;
                self.save_cfg();
                Task::none()
            }
            Message::ProfileDelete => {
                if self.cfg.profiles.len() > 1 {
                    let idx = self.cfg.active_profile.min(self.cfg.profiles.len() - 1);
                    self.cfg.profiles.remove(idx);
                    self.cfg.active_profile = idx.min(self.cfg.profiles.len() - 1);
                    let active = self.cfg.active().clone();
                    self.game_dir = active.game_dir.clone();
                    self.refresh_probe();
                    self.save_cfg();
                }
                Task::none()
            }

            Message::ServerGet => match self.dirs.clone() {
                Some(dirs) => {
                    let key = self.api_key.trim().to_string();
                    let offline = self.offline[2].clone();
                    self.phase = Phase::GettingServer;
                    self.push_log("── Downloading the dedicated server ──");
                    pipe_task(move |tx| async move {
                        let rep = str_install::Reporter::new(tx.clone());
                        match crate::server::ensure_server(&key, offline, &dirs, &rep).await {
                            Ok(dir) => {
                                let _ = tx
                                    .send(PipeEvent::Log(format!(
                                        "Server installed to {}",
                                        dir.display()
                                    )))
                                    .await;
                                let _ = tx.send(PipeEvent::Done(Ok(Default::default()))).await;
                            }
                            Err(e) => {
                                let _ = tx.send(PipeEvent::Done(Err(e))).await;
                            }
                        }
                    })
                }
                None => Task::none(),
            },
            Message::ServerLaunch => match (self.launch_ctx(), self.dirs.clone()) {
                (Ok(ctx), Some(dirs)) => Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            #[cfg(target_os = "windows")]
                            let _ = &ctx;
                            #[cfg(target_os = "windows")]
                            let result = crate::launch::launch_server(&dirs);
                            #[cfg(all(unix, not(target_os = "macos")))]
                            let result = crate::launch::launch_server(&ctx, &dirs);
                            #[cfg(target_os = "macos")]
                            let result: Result<(), String> =
                                Err("Server hosting is not supported on macOS".into());
                            result
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::ServerLaunchResult,
                ),
                (Err(e), _) => {
                    self.banner = Some(Banner::Error(e));
                    Task::none()
                }
                (_, None) => Task::none(),
            },
            Message::ServerLaunchResult(result) => {
                match result {
                    Ok(()) => self.push_log("Server launched (log in the data folder)."),
                    Err(e) => {
                        tracing::error!("launching the server failed: {e}");
                        self.banner = Some(Banner::Error(e));
                    }
                }
                Task::none()
            }
            Message::ServerReloadConfig => match self.dirs.clone() {
                Some(dirs) => Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            crate::server::read_settings(&crate::launch::server_dir(&dirs))
                        })
                        .await
                        .unwrap_or(None)
                    },
                    Message::ServerConfigLoaded,
                ),
                None => Task::none(),
            },
            Message::ServerConfigLoaded(found) => match found {
                Some((path, settings)) => {
                    self.apply_server_settings(path, settings);
                    Task::none()
                }
                None => {
                    self.banner = Some(Banner::Error(format!("No STServer.ini found.\n{INI_HINT}")));
                    Task::none()
                }
            },
            Message::ServerField(field, value) => {
                let idx = match field {
                    ServerField::Port => 0,
                    ServerField::Name => 1,
                    ServerField::Password => 2,
                    ServerField::MaxPlayers => 3,
                    ServerField::ModCheck => 4,
                };
                self.server_inputs[idx] = value;
                Task::none()
            }
            Message::ServerSaveConfig => {
                let Some((path, _)) = self.server_ini.clone() else {
                    self.banner = Some(Banner::Error(format!("No STServer.ini found.\n{INI_HINT}")));
                    return Task::none();
                };
                let port = self.server_inputs[0].trim().to_string();
                let name = self.server_inputs[1].trim().to_string();
                let password = self.server_inputs[2].trim().to_string();
                let max = self.server_inputs[3].trim().to_string();
                let modcheck = self.server_inputs[4].trim().to_string();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            if !port.is_empty() {
                                crate::server::write_setting(&path, "uPort", &port)?;
                            }
                            if !name.is_empty() {
                                crate::server::write_setting(&path, "sServerName", &name)?;
                            }
                            crate::server::write_setting(&path, "sPassword", &password)?;
                            if !max.is_empty() {
                                crate::server::write_setting(&path, "uMaxPlayerCount", &max)?;
                            }
                            if !modcheck.is_empty() {
                                crate::server::write_setting(&path, "bEnableModCheck", &modcheck)?;
                            }
                            Ok(())
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::ServerSaved,
                )
            }
            Message::ServerSaved(result) => {
                match result {
                    Ok(()) => self.banner = Some(Banner::Info("Server settings saved.".into())),
                    Err(e) => {
                        tracing::error!("saving STServer.ini failed: {e}");
                        self.banner = Some(Banner::Error(e));
                    }
                }
                Task::none()
            }
        }
    }
}
