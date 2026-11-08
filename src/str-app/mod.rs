//! The Iced application: shared state, messages and helpers.
//!
//! Split into [`update`] (message handling) and the view side: [`view`] (the
//! shell: header, banner, progress) plus [`tab_client`], [`tab_server`],
//! [`tab_info`] and [`tab_settings`], built on [`widgets`] primitives.

mod tab_client;
mod tab_info;
mod tab_server;
mod tab_settings;
mod update;
mod view;
mod widgets;

pub use view::view;

/// Free entry points for the iced program builder.
pub fn boot() -> (App, Task<Message>) {
    App::new()
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    App::update(app, message)
}

use std::collections::VecDeque;
use std::path::PathBuf;

use iced::Task;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::config::Config;
use crate::launch::LaunchContext;
use crate::paths::AppDirs;
use crate::server::ServerSettings;
use crate::steam::GameInstall;
use crate::str_install::{self, Event as PipeEvent, Params};

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub enum Message {
    Loaded(Box<LoadResult>),
    Detect,
    Detected(Vec<GameInstall>),
    Install,
    Undo,
    Pipeline(PipeEvent),
    LaunchTogether,
    LaunchResult(Result<(), String>),
    LaunchVanilla,
    VanillaResult(Result<(), String>),
    CheckUpdates,
    UpdatesChecked(Result<Option<String>, String>),
    SelfUpdateChecked(Result<Option<String>, String>),
    DismissUpdateBanner,
    RestartSteamForCompatTool,
    SteamCompatApplied(Result<bool, String>),
    DismissSteamRestart,
    ApiKeyChanged(String),
    GamePathChanged(String),
    GamePathSubmitted,
    BrowseGameDir,
    GameDirPicked(Option<PathBuf>),
    ProtonPicked(String),
    LaunchModePicked(crate::config::LaunchMode),
    TabSelected(Tab),
    ToggleCleanCc(bool),
    BrowseOffline(u8),
    OfflinePicked(u8, Option<PathBuf>),
    ClearOffline(u8),
    OpenFolder(OpenTarget),
    OpenLink(&'static str),
    DismissBanner,
    ToggleChecks,
    ProfilePicked(usize),
    ProfileNew,
    ProfileDelete,
    ServerGet,
    ServerLaunch,
    ServerLaunchResult(Result<(), String>),
    ServerReloadConfig,
    ServerConfigLoaded(Option<(PathBuf, ServerSettings)>),
    ServerField(ServerField, String),
    ServerSaveConfig,
    ServerSaved(Result<(), String>),
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenTarget {
    Game,
    Downloads,
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ServerField {
    Port,
    Name,
    Password,
    MaxPlayers,
    ModCheck,
}

/// The main sections of the launcher, shown as tabs in the top bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Client,
    // Hidden from TABS (view.rs) until the dedicated-server flow is tested.
    #[allow(dead_code)]
    Server,
    Info,
    Settings,
}

#[derive(Clone, Debug)]
pub struct LoadResult {
    pub dirs: Result<AppDirs, String>,
    pub cfg: Config,
    pub installs: Vec<GameInstall>,
    pub proton_names: Vec<String>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Phase {
    Idle,
    Detecting,
    Installing,
    Undoing,
    GettingServer,
}

pub(crate) enum Banner {
    Error(String),
    Info(String),
}

pub struct App {
    pub(crate) dirs: Option<AppDirs>,
    pub(crate) cfg: Config,
    pub(crate) phase: Phase,
    pub(crate) installs: Vec<GameInstall>,
    pub(crate) game_dir: Option<PathBuf>,
    pub(crate) steam_root: Option<PathBuf>,
    pub(crate) log: VecDeque<String>,
    pub(crate) step_label: String,
    pub(crate) step_index: usize,
    pub(crate) step_total: usize,
    pub(crate) progress: Option<(String, f32)>,
    pub(crate) banner: Option<Banner>,
    pub(crate) game_version: Option<String>,
    pub(crate) str_version: Option<String>,
    pub(crate) al_version: Option<String>,
    pub(crate) cc_present: bool,
    pub(crate) api_key: String,
    pub(crate) game_path_input: String,
    pub(crate) proton_names: Vec<String>,
    pub(crate) proton_selected: Option<String>,
    pub(crate) tab: Tab,
    pub(crate) offline: [Option<PathBuf>; 3], // STR, Address Library, server
    pub(crate) server_installed: bool,
    pub(crate) server_ini: Option<(PathBuf, ServerSettings)>,
    pub(crate) server_inputs: [String; 5], // port, name, password, max players, modcheck
    pub(crate) checking: bool,
    /// Version of a newer published launcher release (None = up to date or
    /// the check failed/has not run). Rendered as its own non-blocking banner.
    pub(crate) launcher_update: Option<String>,
    /// Deferred compatibility-tool change (Linux): Steam was running during
    /// install, so the change waits for an explicit "Restart Steam" click.
    #[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
    pub(crate) pending_steam: Option<(PathBuf, String)>,
    pub(crate) checks: Vec<crate::checks::Check>,
    /// Launch-readiness checklist expanded? Collapsed by default — the
    /// header row shows the summary either way.
    pub(crate) show_checks: bool,
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let app = App {
            dirs: None,
            cfg: Config::default(),
            phase: Phase::Idle,
            installs: Vec::new(),
            game_dir: None,
            steam_root: None,
            log: VecDeque::new(),
            step_label: String::new(),
            step_index: 0,
            step_total: str_install::TOTAL_STEPS,
            progress: None,
            banner: None,
            game_version: None,
            str_version: None,
            al_version: None,
            cc_present: false,
            api_key: String::new(),
            game_path_input: String::new(),
            proton_names: Vec::new(),
            proton_selected: None,
            tab: Tab::default(),
            offline: [None, None, None],
            server_installed: false,
            server_ini: None,
            server_inputs: Default::default(),
            checking: false,
            launcher_update: None,
            pending_steam: None,
            checks: Vec::new(),
            show_checks: false,
        };
        (
            app,
            Task::batch([
                Task::perform(load(), |r| Message::Loaded(Box::new(r))),
                // Non-blocking self-update check: query the published
                // releases once at startup, compare semantically, and let the
                // view show a dismissible banner. Never auto-installs.
                Task::perform(
                    async {
                        match crate::release::latest(crate::release::LAUNCHER_REPO).await {
                            Ok(release) => {
                                let newer = crate::release::is_newer(
                                    crate::paths::APP_VERSION,
                                    &release.tag_name,
                                );
                                Ok(newer.then_some(release.tag_name))
                            }
                            Err(e) => Err(e),
                        }
                    },
                    Message::SelfUpdateChecked,
                ),
            ]),
        )
    }

    pub(crate) fn push_log(&mut self, line: impl Into<String>) {
        let line = line.into();
        // The activity pane doubles as the file log: everything the user can
        // see during a run is also in launcher.log for post-mortems.
        tracing::info!(target: "activity", "{line}");
        self.log.push_back(line);
        while self.log.len() > 400 {
            self.log.pop_front();
        }
    }

    pub(crate) fn game_dir_or_err(&self) -> Result<PathBuf, String> {
        self.game_dir.clone().ok_or_else(|| {
            "No Skyrim Special Edition install found.\nInstall it through Steam, or set the game \
             folder under Advanced settings."
                .to_string()
        })
    }

    pub(crate) fn build_params(&self) -> Result<Params, String> {
        let dirs = self.dirs.clone().ok_or("application data is not ready")?;
        let game_dir = self.game_dir_or_err()?;
        let cc_backup_dir = game_dir
            .parent()
            .and_then(|common| common.parent())
            .map(|steamapps| steamapps.join("str-cc-backup"))
            .unwrap_or_else(|| dirs.cc_backup.clone());
        Ok(Params {
            game_dir,
            steam_root: self.steam_root.clone(),
            nexus_key: self.api_key.trim().to_string(),
            offline_str: self.offline[0].clone(),
            offline_al: self.offline[1].clone(),
            proton_tool: self
                .proton_selected
                .clone()
                .unwrap_or_else(crate::proton::default_tool_name),
            clean_cc: self.cfg.clean_cc,
            launch_mode: self.cfg.launch_mode,
            dirs,
            cc_backup_dir,
        })
    }

    pub(crate) fn refresh_probe(&mut self) {
        if let Some(dir) = &self.game_dir {
            let probe = str_install::probe(dir);
            self.str_version = probe.str_version;
            self.cc_present = probe.cc_present;
            // Read the actual build from SkyrimSE.exe's version resource
            // (e.g. 1.7.104.0) so it shows immediately after detection.
            self.game_version = crate::pe_version::file_version(&dir.join("SkyrimSE.exe"))
                .map(|v| v.to_string());
        } else {
            self.game_version = None;
        }
        self.refresh_checks();
        if let Some(dirs) = &self.dirs {
            self.server_installed =
                crate::launch::find_server_exe(&crate::launch::server_dir(dirs)).is_some();
            if let Some((path, settings)) =
                crate::server::read_settings(&crate::launch::server_dir(dirs))
            {
                self.apply_server_settings(path, settings);
            }
        }
    }

    /// Store a freshly read STServer.ini and mirror its values into the
    /// editor inputs (shared by detection and the Server tab's Reload).
    pub(crate) fn apply_server_settings(&mut self, path: PathBuf, settings: ServerSettings) {
        self.server_ini = Some((path, settings.clone()));
        self.server_inputs = [
            settings.port.unwrap_or_default(),
            settings.name.unwrap_or_default(),
            settings.password.unwrap_or_default(),
            settings.max_players.unwrap_or_default(),
            settings.mod_check.unwrap_or_default(),
        ];
    }

    /// Recompute the launch-readiness checklist (fast, read-only).
    pub(crate) fn refresh_checks(&mut self) {
        let proton_tool = self
            .proton_selected
            .clone()
            .unwrap_or_else(crate::proton::default_tool_name);
        self.checks = crate::checks::run(crate::checks::Input {
            game_dir: self.game_dir.as_deref(),
            game_version: self.game_version.as_deref(),
            str_version: self.str_version.as_deref(),
            steam_root: self.steam_root.as_deref(),
            proton_tool: &proton_tool,
            launch_mode: self.cfg.launch_mode,
        });
    }

    pub(crate) fn launch_ctx(&self) -> Result<LaunchContext, String> {
        let dirs = self.dirs.clone().ok_or("application data is not ready")?;
        Ok(LaunchContext {
            game_dir: self.game_dir_or_err()?,
            steam_root: self.steam_root.clone(),
            proton_tool: self
                .proton_selected
                .clone()
                .unwrap_or_else(crate::proton::default_tool_name),
            dirs,
        })
    }

    pub(crate) fn save_cfg(&mut self) {
        if let Some(dirs) = self.dirs.clone() {
            if let Err(e) = self.cfg.save(&dirs) {
                tracing::error!("saving config.json failed: {e}");
            }
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    pub(crate) fn installed(&self) -> bool {
        self.str_version.is_some()
    }

    /// Human-readable state shown at the top of the Client tab.
    pub(crate) fn status_line(&self) -> String {
        match self.phase {
            Phase::Idle => {
                if let Some(v) = &self.str_version {
                    format!("Ready to play — Skyrim Together Reborn v{v} is installed.")
                } else if self.game_dir.is_some() {
                    "Skyrim found. Press Install to set up Skyrim Together Reborn."
                        .to_string()
                } else {
                    "Looking for Skyrim Special Edition…".to_string()
                }
            }
            Phase::Detecting => "Detecting Steam libraries…".to_string(),
            Phase::Installing => self.step_label.clone(),
            Phase::Undoing => "Removing Skyrim Together Reborn…".to_string(),
            Phase::GettingServer => "Downloading the dedicated server…".to_string(),
        }
    }
}

async fn load() -> LoadResult {
    let dirs = AppDirs::init();
    let cfg = match &dirs {
        Ok(d) => Config::load(d),
        Err(_) => Config::default(),
    };
    let installs = tokio::task::spawn_blocking(crate::steam::detect)
        .await
        .unwrap_or_default();
    let roots = crate::steam::steam_roots();
    let proton_names = crate::proton::list_tools(&roots)
        .into_iter()
        .map(|t| t.name)
        .collect();
    LoadResult { dirs, cfg, installs, proton_names }
}

/// Spawn a job that reports through the pipeline channel and surface its
/// events to the app as a stream of messages. The setup closure runs on the
/// app's async runtime, so it may itself `tokio::spawn` background work.
pub(crate) fn pipe_task<F, T>(setup: F) -> Task<Message>
where
    F: FnOnce(tokio::sync::mpsc::Sender<PipeEvent>) -> T + Send + 'static,
    T: std::future::Future<Output = ()> + Send + 'static,
{
    Task::stream(futures::stream::unfold(
        (Some(setup), None::<ReceiverStream<PipeEvent>>),
        |(setup, rx)| async move {
            let mut rx = match setup {
                // First poll: launch the job and take over its receiver.
                Some(f) => {
                    let (tx, rx) = tokio::sync::mpsc::channel::<PipeEvent>(256);
                    tokio::spawn(async move { f(tx).await });
                    ReceiverStream::new(rx)
                }
                None => rx?,
            };
            rx.next().await.map(|ev| (Message::Pipeline(ev), (None, Some(rx))))
        },
    ))
}
