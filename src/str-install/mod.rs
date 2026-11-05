//! The automated install pipeline.
//!
//! Implements the documented setup flow without any external mod manager:
//!
//! 1. Validate the Skyrim Special Edition install.
//! 2. Download Address Library (All in One) and Skyrim Together Reborn from
//!    Nexus Mods (or use offline archives), with caching and resume.
//! 3. Extract and merge the files into the game folder, tracking everything in
//!    a manifest for clean updates and undo.
//! 4. Back up and remove the bundled Creation Club content (known to break
//!    STR multiplayer).
//! 5. Enable `SkyrimTogether.esp` in every existing `Plugins.txt` location.
//! 6. Platform configuration:
//!    - Windows: STR's registry keys (skips the "select SkyrimSE.exe" dialog)
//!      plus Start Menu / desktop shortcuts.
//!    - Linux: GE-Proten install + selection in Steam, launcher replacement
//!      (so launching through Steam starts multiplayer), protonfixes,
//!      Proton-prefix registry keys and a standalone launch script + desktop
//!      entry.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::archive;
use crate::cc_clean;
use crate::config::{Config, InstallManifest, LaunchMode, NEXUS_ADDRESS_LIBRARY, NEXUS_STR};
use crate::nexus::NexusClient;
use crate::paths::safe_rel_path;
#[cfg(target_os = "windows")]
use crate::platform;
use crate::pe_version;
use crate::LOGO_ICON_PNG;
use crate::plugins;
#[cfg(all(unix, not(target_os = "macos")))]
use crate::steam::SKYRIM_APPID;

#[derive(Debug, Clone)]
pub enum Event {
    /// A numbered step started.
    Step { label: String, index: usize, total: usize },
    /// Sub-operation progress.
    Progress { label: String, done: u64, total: u64 },
    Log(String),
    Done(Result<InstallSummary, String>),
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct InstallSummary {
    pub str_version: Option<String>,
    pub address_library_version: Option<String>,
    pub game_version: Option<String>,
    pub cc_removed: Vec<String>,
    pub warnings: Vec<String>,
    pub shortcuts: Vec<String>,
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub struct Params {
    pub game_dir: PathBuf,
    pub steam_root: Option<PathBuf>,
    pub nexus_key: String,
    /// Offline mode: path to a manually downloaded STR archive.
    pub offline_str: Option<PathBuf>,
    /// Offline mode: path to a manually downloaded Address Library archive.
    pub offline_al: Option<PathBuf>,
    pub proton_tool: String,
    pub clean_cc: bool,
    pub create_shortcuts: bool,
    pub launch_mode: LaunchMode,
    pub dirs: crate::paths::AppDirs,
    /// Where CC backups go (kept next to the game library to stay on the same
    /// filesystem, so `mv` stays instant).
    pub cc_backup_dir: PathBuf,
}

pub const TOTAL_STEPS: usize = 9;

pub struct Reporter {
    tx: mpsc::Sender<Event>,
    step: Arc<AtomicU64>,
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
impl Reporter {
    pub fn new(tx: mpsc::Sender<Event>) -> Self {
        Reporter { tx, step: Arc::new(0.into()) }
    }

    pub async fn step(&self, label: &str, index: usize) {
        self.step.store(index as u64, Ordering::Relaxed);
        let _ = self
            .tx
            .send(Event::Step { label: label.into(), index, total: TOTAL_STEPS })
            .await;
    }

    pub async fn log(&self, msg: impl Into<String>) {
        let _ = self.tx.send(Event::Log(msg.into())).await;
    }

    pub fn sender(&self) -> mpsc::Sender<Event> {
        self.tx.clone()
    }

    pub fn progress_fn(&self, label: String) -> ThrottledProgress {
        ThrottledProgress::new(self.tx.clone(), label)
    }
}

/// Sends at most ~100 progress updates per operation.
pub struct ThrottledProgress {
    tx: mpsc::Sender<Event>,
    label: String,
    last: u64,
}

impl ThrottledProgress {
    pub fn new(tx: mpsc::Sender<Event>, label: String) -> Self {
        ThrottledProgress { tx, label, last: 0 }
    }

    pub fn send(&mut self, done: u64, total: u64) {
        if total == 0 {
            return;
        }
        if done == total || done * 100 >= (self.last + 1) * total / 100 {
            self.last = done;
            let _ = self.tx.try_send(Event::Progress {
                label: self.label.clone(),
                done,
                total,
            });
        }
    }
}

pub async fn run(params: Params, tx: mpsc::Sender<Event>, cfg: Config) {
    let rep = Reporter::new(tx);
    match run_inner(&params, &rep, cfg).await {
        Ok(summary) => {
            let _ = rep.tx.send(Event::Done(Ok(summary))).await;
        }
        Err(e) => {
            let _ = rep.tx.send(Event::Done(Err(e))).await;
        }
    }
}

async fn run_inner(params: &Params, rep: &Reporter, cfg: Config) -> Result<InstallSummary, String> {
    let mut summary = InstallSummary::default();
    let mut warnings: Vec<String> = Vec::new();
    let game_dir = params.game_dir.clone();
    let data_dir = game_dir.join("Data");

    // ---------- Step 0: validate ----------
    rep.step("Validating Skyrim Special Edition", 0).await;
    if !game_dir.join("SkyrimSE.exe").is_file() {
        return Err(format!(
            "SkyrimSE.exe was not found in {}.\nThe game folder looks wrong — pick the folder \
             that contains SkyrimSE.exe (usually .../steamapps/common/Skyrim Special Edition).",
            game_dir.display()
        ));
    }
    if !data_dir.join("Skyrim.esm").is_file() {
        return Err(format!(
            "Data/Skyrim.esm was not found in {} — the install looks broken.\n\
             Use Steam → Properties → Installed Files → Verify integrity of game files.",
            game_dir.display()
        ));
    }
    let game_version = pe_version::file_version(&game_dir.join("SkyrimSE.exe"))
        .map(|v| v.to_string());
    if let Some(v) = &game_version {
        rep.log(format!("Skyrim Special Edition build: {v}")).await;
        let parts: Vec<u64> = v.split('.').filter_map(|p| p.parse().ok()).collect();
        if let Some(minor) = parts.get(1) {
            if *minor < 6 {
                warnings.push(format!(
                    "Skyrim build {v} is too old. Skyrim Together Reborn requires 1.6 or newer — \
                     let Steam update the game first."
                ));
            }
        }
    } else {
        rep.log("Could not read the game version (continuing anyway).").await;
    }

    let manifest = InstallManifest::load(&game_dir);
    if let Some(v) = &manifest.str_version {
        rep.log(format!("Existing Skyrim Together Reborn install: v{v}")).await;
    }

    // Has the game been run at least once? (generates INIs + prefix)
    let launched_once = check_launched_once(params);
    if !launched_once {
        warnings.push(
            "The game has never been launched through Steam. Start it once and reach the main \
             menu before playing multiplayer."
                .into(),
        );
    }

    // ---------- Steps 1-2: downloads ----------
    let client = NexusClient::new(&params.nexus_key);
    let str_archive: Option<PathBuf>;
    let al_archive: Option<PathBuf>;
    let mut str_meta: Option<crate::nexus::ModFile> = None;
    let mut al_meta: Option<crate::nexus::ModFile> = None;

    match &params.offline_al {
        Some(path) => {
            al_archive = Some(path.clone());
            rep.log(format!("Offline mode: using Address Library archive {}", path.display())).await;
        }
        None => {
            rep.step("Downloading Address Library (All in One)", 1).await;
            let mut prog = rep.progress_fn("Downloading Address Library".to_string());
            let (path, meta) = client
                .download_main_file("skyrimspecialedition", NEXUS_ADDRESS_LIBRARY, &params.dirs.downloads, &mut |d, t| prog.send(d, t))
                .await
                .map_err(|e| format!("Address Library download failed:\n{e}"))?;
            rep.log(format!(
                "Address Library: {} (v{}, {:.1} MiB)",
                meta.name,
                meta.mod_version,
                meta.size as f64 / 1048576.0
            ))
            .await;
            al_meta = Some(meta);
            al_archive = Some(path.clone());
        }
    }

    match &params.offline_str {
        Some(path) => {
            str_archive = Some(path.clone());
            rep.log(format!("Offline mode: using STR archive {}", path.display())).await;
        }
        None => {
            rep.step("Downloading Skyrim Together Reborn", 2).await;
            let mut prog = rep.progress_fn("Downloading Skyrim Together Reborn".to_string());
            let (path, meta) = client
                .download_main_file("skyrimspecialedition", NEXUS_STR, &params.dirs.downloads, &mut |d, t| prog.send(d, t))
                .await
                .map_err(|e| format!("Skyrim Together Reborn download failed:\n{e}"))?;
            rep.log(format!(
                "Skyrim Together Reborn: {} (v{}, {:.1} MiB)",
                meta.name,
                meta.mod_version,
                meta.size as f64 / 1048576.0
            ))
            .await;
            str_meta = Some(meta);
            str_archive = Some(path.clone());
        }
    }

    // ---------- Step 3: extract ----------
    rep.step("Extracting archives", 3).await;
    let staging = params.dirs.staging.clone();
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let str_stage = staging.join("str");
    let al_stage = staging.join("address-library");
    std::fs::create_dir_all(&str_stage).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&al_stage).map_err(|e| e.to_string())?;

    let str_path = str_archive.ok_or("no Skyrim Together Reborn archive available")?;
    let mut prog = rep.progress_fn("Extracting Skyrim Together Reborn".to_string());
    archive::extract_zip(&str_path, &str_stage, &mut |d, t| prog.send(d, t))?;
    let str_root = find_str_root(&str_stage).ok_or_else(|| {
        format!(
            "{} does not look like a Skyrim Together Reborn archive (no SkyrimTogether.exe).\n\
             Download it from nexusmods.com/skyrimspecialedition/mods/69993 (Files tab).",
            str_path.display()
        )
    })?;
    let str_version = pe_version::file_version(&str_root.join("SkyrimTogether.exe"))
        .map(|v| v.to_string());

    let al_path = al_archive.ok_or("no Address Library archive available")?;
    let mut prog = rep.progress_fn("Extracting Address Library".to_string());
    archive::extract_zip(&al_path, &al_stage, &mut |d, t| prog.send(d, t))?;
    let al_root = find_al_root(&al_stage).ok_or_else(|| {
        format!(
            "{} does not look like an Address Library archive (no SKSE folder).\n\
             Download the latest \"All in one\" file from nexusmods.com/skyrimspecialedition/mods/32444.",
            al_path.display()
        )
    })?;

    // ---------- Step 4: remove previous install ----------
    rep.step("Removing previous Skyrim Together files", 4).await;
    remove_manifest_files(&game_dir, &manifest, rep).await;
    remove_legacy_str_files(&game_dir, rep).await;
    if data_dir.join("SkyrimTogetherReborn").is_dir() {
        warnings.push(
            "A Vortex-style deployment was found at Data/SkyrimTogetherReborn. This launcher \
             installs STR independently; remove the Vortex mod to avoid two copies fighting."
                .into(),
        );
    }

    // ---------- Step 5: install files ----------
    rep.step("Installing Skyrim Together Reborn", 5).await;
    let mut installed_files: Vec<String> = Vec::new();
    merge_tree(&str_root, &game_dir, &game_dir, &mut installed_files).map_err(|e| {
        format!(
            "failed to copy files into the game folder: {e}\n\
             Is the game folder writable? Close the game and Steam, then retry."
        )
    })?;
    // Address Library archives ship `SKSE/` at their root (Vortex deploys the
    // archive root into Data), while some repacks wrap it in a `Data/` folder.
    let al_merge_base = if al_root.join("SKSE").is_dir() {
        data_dir.clone()
    } else {
        game_dir.clone()
    };
    merge_tree(&al_root, &al_merge_base, &game_dir, &mut installed_files).map_err(|e| {
        format!("failed to copy Address Library files: {e}")
    })?;
    installed_files.sort();
    installed_files.dedup();
    rep.log(format!("Installed {} files into the game folder.", installed_files.len())).await;

    let al_version = al_meta.as_ref().map(|m| {
        if m.mod_version.is_empty() { m.version.clone() } else { m.mod_version.clone() }
    });
    let str_version_label = str_meta
        .as_ref()
        .map(|m| if m.mod_version.is_empty() { m.version.clone() } else { m.mod_version.clone() })
        .or(str_version.clone());

    // ---------- Step 6: Creation Club cleanup ----------
    if params.clean_cc {
        rep.step("Cleaning bundled Creation Club content", 6).await;
        match cc_clean::backup_cc(&game_dir, &params.cc_backup_dir) {
            Ok(moved) if moved.is_empty() => {
                rep.log("No bundled Creation Club content found — nothing to remove.").await;
            }
            Ok(moved) => {
                summary.cc_removed = moved.clone();
                rep.log(format!(
                    "Backed up and removed {} CC files (Survival Mode, Fishing, ...). They can be \
                     restored with \"Undo\" or Steam's file verification.",
                    moved.len()
                ))
                .await;
            }
            Err(e) => warnings.push(format!("CC cleanup failed: {e}")),
        }
    } else {
        rep.step("Skipping Creation Club cleanup (disabled)", 6).await;
        if cc_clean::cc_present(&game_dir) {
            warnings.push(
                "Bundled Creation Club content is installed and cleanup is disabled — this is a \
                 common cause of crashes in multiplayer.".into(),
            );
        }
    }

    // ---------- Step 7: Plugins.txt ----------
    rep.step("Enabling SkyrimTogether.esp", 7).await;
    let str_plugins = plugins::find_str_plugins(&data_dir);
    if str_plugins.is_empty() {
        warnings.push("No SkyrimTogether*.esp plugin found in the game's Data folder.".into());
    }
    let mut plugin_locations_updated = 0;
    for profile_dir in profile_dirs(params) {
        let plugins_txt = profile_dir.join("Plugins.txt");
        if plugins_txt.is_file() {
            let mut text = std::fs::read_to_string(&plugins_txt).unwrap_or_default();
            plugins::update_plugins_text(&mut text, &str_plugins);
            if crate::config::atomic_write(&plugins_txt, text.as_bytes()).is_ok() {
                plugin_locations_updated += 1;
                rep.log(format!("Updated {}", plugins_txt.display())).await;
            }
        }
    }
    if plugin_locations_updated == 0 {
        rep.log(
            "No Plugins.txt found yet — the game will auto-load SkyrimTogether.esp on its next \
             launch (it is created after the first run).",
        )
        .await;
    }

    // ---------- Step 8: platform configuration ----------
    rep.step("Configuring launch integration", 8).await;
    let mut shortcuts: Vec<String> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        platform::native::write_str_registry(&game_dir)
            .map_err(|e| format!("writing STR registry keys failed: {e}"))?;
        rep.log("Registry keys written (SkyrimTogether will find SkyrimSE.exe automatically).")
            .await;
        if params.create_shortcuts {
            match platform::native::create_shortcuts(&game_dir) {
                Ok(created) => {
                    rep.log(format!("Created shortcuts: {}", created.join(", "))).await;
                    shortcuts = created;
                }
                Err(e) => warnings.push(format!("Shortcut creation failed: {e}")),
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        configure_linux(params, rep, &cfg, &game_version, &mut warnings, &mut shortcuts).await?;
    }

    #[cfg(target_os = "windows")]
    let _ = &cfg;
    // ---------- Persist manifest + config summary ----------
    let manifest = InstallManifest {
        str_version: str_version_label.clone(),
        str_file_id: str_meta.as_ref().map(|m| m.file_id),
        address_library_version: al_version.clone(),
        address_library_file_id: al_meta.as_ref().map(|m| m.file_id),
        files: installed_files,
        installed_at: crate::paths::unix_now(),
    };
    manifest.save(&game_dir).map_err(|e| format!("saving manifest: {e}"))?;

    summary.str_version = str_version_label;
    summary.address_library_version = al_version;
    summary.game_version = game_version;
    summary.warnings = warnings;
    summary.shortcuts = shortcuts;
    Ok(summary)
}

/// Update the stored profile after a successful install.
pub fn apply_summary_to_profile(cfg: &mut Config, summary: &InstallSummary, manifest: &InstallManifest) {
    let p = cfg.active_mut();
    p.installed_str_version = summary.str_version.clone();
    p.installed_str_file_id = manifest.str_file_id;
    p.installed_al_version = summary.address_library_version.clone();
    p.installed_al_file_id = manifest.address_library_file_id;
}

/// Locate the extracted STR archive root that contains `SkyrimTogether.exe`.
fn find_str_root(stage: &Path) -> Option<PathBuf> {
    let root = archive::resolve_mod_root(stage);
    [
        root.clone(),
        root.join("SkyrimTogetherReborn"),
        root.join("Data/SkyrimTogetherReborn"),
    ]
    .into_iter()
    .find(|candidate| candidate.join("SkyrimTogether.exe").is_file())
}

/// Locate the extracted Address Library root that contains the `SKSE` folder.
fn find_al_root(stage: &Path) -> Option<PathBuf> {
    let root = archive::resolve_mod_root(stage);
    if root.join("SKSE").is_dir() {
        return Some(root);
    }
    let nested = root.join("SKSE");
    let _ = nested;
    if root.join("Data/SKSE").is_dir() {
        return Some(root.join("Data"));
    }
    None
}

/// Profile folders that may contain Plugins.txt.
fn profile_dirs(params: &Params) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let _ = params;
        let user = std::env::var("USERPROFILE").map(PathBuf::from).ok();
        user.map(|u| plugins::candidate_profile_dirs(&u)).unwrap_or_default()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut dirs = Vec::new();
        if let Some(compat) = crate::steam::compat_dir_for(&params.game_dir) {
            let user = compat.join("pfx/drive_c/users/steamuser");
            dirs.extend(plugins::candidate_profile_dirs(&user));
        }
        dirs
    }
    #[cfg(target_os = "macos")]
    {
        Vec::new()
    }
}

fn check_launched_once(params: &Params) -> bool {
    #[cfg(target_os = "windows")]
    {
        let _ = params;
        if let Ok(user) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(user).join("Documents/My Games/Skyrim Special Edition");
            return p.is_dir();
        }
        false
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(compat) = crate::steam::compat_dir_for(&params.game_dir) {
            let user = compat.join("pfx/drive_c/users/steamuser");
            return user.join("Documents/My Games/Skyrim Special Edition").is_dir()
                || user.join("AppData/Local/Skyrim Special Edition").is_dir();
        }
        false
    }
    #[cfg(target_os = "macos")]
    {
        let _ = params;
        false
    }
}

/// Recursively merge `src_root` into `merge_dest`, recording every installed
/// file as a path relative to `game_root` (the manifest's frame of reference).
fn merge_tree(
    src_root: &Path,
    merge_dest: &Path,
    game_root: &Path,
    collected: &mut Vec<String>,
) -> Result<(), String> {
    fn walk(
        src: &Path,
        dest: &Path,
        rel: &Path,
        game_root: &Path,
        collected: &mut Vec<String>,
    ) -> Result<(), String> {
        for entry in std::fs::read_dir(src).map_err(|e| format!("read {}: {e}", src.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let file_type = entry.file_type().map_err(|e| e.to_string())?;
            let child_rel = rel.join(entry.file_name());
            let dest_path = dest.join(&child_rel);
            if file_type.is_dir() {
                std::fs::create_dir_all(&dest_path)
                    .map_err(|e| format!("create {}: {e}", dest_path.display()))?;
                walk(&entry.path(), dest, &child_rel, game_root, collected)?;
            } else if file_type.is_file() {
                // Overwrite anything stale (including old symlinks).
                let _ = std::fs::remove_file(&dest_path);
                std::fs::copy(entry.path(), &dest_path)
                    .map_err(|e| format!("copy {}: {e}", entry.path().display()))?;
                if let Ok(rel_from_root) = dest_path.strip_prefix(game_root) {
                    if let Some(rel_str) = rel_from_root.to_str() {
                        collected.push(rel_str.replace('\\', "/"));
                    }
                }
            }
        }
        Ok(())
    }
    walk(src_root, merge_dest, Path::new(""), game_root, collected)
}

async fn remove_manifest_files(game_dir: &Path, manifest: &InstallManifest, rep: &Reporter) {
    if manifest.files.is_empty() {
        return;
    }
    let mut removed = 0usize;
    for rel in &manifest.files {
        let Some(safe) = safe_rel_path(rel) else { continue };
        let path = game_dir.join(&safe);
        if path.is_symlink() || path.is_file() {
            let _ = std::fs::remove_file(&path);
            removed += 1;
            // Clean up now-empty directories we created (up to the game root).
            let mut dir = path.parent().map(|p| p.to_path_buf());
            while let Some(d) = dir {
                if d == game_dir || !d.starts_with(game_dir) {
                    break;
                }
                if std::fs::read_dir(&d).map(|mut i| i.next().is_none()).unwrap_or(false) {
                    let _ = std::fs::remove_dir(&d);
                    dir = d.parent().map(|p| p.to_path_buf());
                } else {
                    break;
                }
            }
        }
    }
    rep.log(format!("Removed {removed} files from the previous install.")).await;
}

/// Remove files left by older STR versions / manual installs that we can
/// safely identify.
async fn remove_legacy_str_files(game_dir: &Path, rep: &Reporter) {
    let mut removed = 0usize;
    let mut consider = |path: PathBuf| {
        if path.is_symlink() {
            if let Ok(target) = std::fs::read_link(&path) {
                let t = target.to_string_lossy().to_lowercase();
                if t.contains("skyrimtogether") {
                    let _ = std::fs::remove_file(&path);
                    removed += 1;
                }
            }
        }
    };
    if let Ok(entries) = std::fs::read_dir(game_dir) {
        for e in entries.flatten() {
            consider(e.path());
        }
    }
    let data = game_dir.join("Data");
    if let Ok(entries) = std::fs::read_dir(&data) {
        for e in entries.flatten() {
            consider(e.path());
        }
    }
    // Stale SKSE plugin copies from previous STR versions.
    let skse = data.join("SKSE/Plugins");
    if let Ok(entries) = std::fs::read_dir(&skse) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.contains("skyrimtogether") && e.path().is_file() {
                let _ = std::fs::remove_file(e.path());
                removed += 1;
            }
        }
    }
    if removed > 0 {
        rep.log(format!("Cleaned {removed} leftover files from older STR versions.")).await;
    }
}

// ---------------------------------------------------------------------------
// Linux configuration
// ---------------------------------------------------------------------------

#[cfg(all(unix, not(target_os = "macos")))]
async fn configure_linux(
    params: &Params,
    rep: &Reporter,
    _cfg: &Config,
    _game_version: &Option<String>,
    warnings: &mut Vec<String>,
    shortcuts: &mut Vec<String>,
) -> Result<(), String> {
    use crate::platform::linux as plat;

    let game_dir = &params.game_dir;
    // Resolve the Steam root that owns this library.
    let roots = crate::steam::steam_roots();
    let steam_root = params.steam_root.clone().or_else(|| {
        roots
            .iter()
            .find(|r| game_dir.starts_with(r) || r.join("config/config.vdf").is_file())
            .cloned()
    });

    // 1. Make sure the Proton tool exists (downloads GE-Proton when missing).
    if let Some(root) = &steam_root {
        let mut prog = ThrottledProgress::new(
            rep.sender(),
            format!("Downloading {}", params.proton_tool),
        );
        let ensure = crate::proton::ensure_tool(
            std::slice::from_ref(root),
            &params.proton_tool,
            &params.dirs.staging,
            &mut |d, t| prog.send(d, t),
        )
        .await;
        match ensure {
            Ok(proton_path) => {
                rep.log(format!(
                    "Using compatibility tool {} ({})",
                    params.proton_tool,
                    proton_path.display()
                ))
                .await;
            }
            Err(e) => warnings.push(format!("Proton setup failed: {e}")),
        }

        // 2. Select it for Skyrim in Steam. `config.vdf` must be edited while
        //    Steam is not running, so restart Steam around the change.
        //    (Skipped in test mode: never touch a real Steam session.)
        let interactive = std::env::var("STR_LAUNCHER_TEST_MODE").is_err();
        let needs_change = crate::proton::current_compat_tool(root, SKYRIM_APPID)
            .as_deref()
            != Some(params.proton_tool.as_str());
        if needs_change && interactive {
            rep.log(format!("Selecting {} for Skyrim in Steam...", params.proton_tool)).await;
            let was_running = plat::is_steam_running();
            if was_running {
                rep.log("Restarting Steam so the compatibility tool can be applied...").await;
                let _ = std::process::Command::new("steam").arg("-shutdown").spawn();
                for _ in 0..30 {
                    if !plat::is_steam_running() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            match crate::proton::select_compat_tool(root, SKYRIM_APPID, &params.proton_tool) {
                Ok(_) => {
                    rep.log(format!("Selected {} for Skyrim.", params.proton_tool)).await;
                }
                Err(e) => warnings.push(format!(
                    "Could not select {} in Steam ({e}). Pick it manually: \
                     Steam → Skyrim → Properties → Compatibility.",
                    params.proton_tool
                )),
            }
            if was_running {
                let _ = std::process::Command::new("steam").spawn();
                rep.log("Steam restarted.").await;
            }
        } else if needs_change {
            rep.log(
                "Test mode: skipping Steam compatibility-tool selection.".to_string(),
            )
            .await;
        } else {
            rep.log(format!("Steam already uses {} for Skyrim.", params.proton_tool)).await;
        }
    } else {
        warnings.push(
            "No Steam installation root was found; the compatibility tool could not be selected \
             automatically.".into(),
        );
    }

    // 3. Proton prefix registry: let STR find SkyrimSE.exe.
    let compat = crate::steam::compat_dir_for(game_dir);
    if let Some(compat) = compat {
        let user_reg = compat.join("pfx/user.reg");
        if user_reg.is_file() {
            let z_dir = plat::wine_z_path(game_dir);
            let section = format!(
                "[Software\\\\TiltedPhoques\\\\TiltedEvolution\\\\Skyrim Special Edition] {}",
                crate::paths::unix_now()
            );
            let values = vec![
                format!("\"TitleExe\"=\"{z_dir}\\\\SkyrimSE.exe\""),
                format!("\"TitlePath\"=\"{z_dir}\""),
            ];
            match plat::append_user_reg_if_missing(&user_reg, &section, &values) {
                Ok(true) => rep.log("Proton prefix registry updated (STR will find SkyrimSE.exe).").await,
                Ok(false) => rep.log("Proton prefix registry already configured.").await,
                Err(e) => warnings.push(format!(
                    "Could not write the Proton registry keys ({e}). On first launch, \
                     SkyrimTogether may ask you to locate SkyrimSE.exe."
                )),
            }
        } else {
            warnings.push(
                "The Proton prefix does not exist yet — run the game once through Steam, then \
                 press Install again to finish configuration.".into(),
            );
        }
    }

    // 4. Launch script (shared by the Launch button, desktop entries and —
    //    in Steam-game mode — the Steam library entry itself).
    let script = write_launch_script(params, rep).await?;

    // 5. Launch integration, exactly one of the two modes.
    match params.launch_mode {
        LaunchMode::LauncherReplacement => {
            // Clean up a possible leftover Steam entry from the other mode.
            crate::steam_entry::remove(None, rep).await;
            setup_launcher_replacement(game_dir, rep, warnings).await;
            install_protonfix(game_dir, rep).await;
        }
        LaunchMode::SteamGame => {
            restore_launcher(game_dir, rep).await;
            let plan = crate::steam_entry::EntryPlan {
                script: script.clone(),
                start_dir: game_dir.clone(),
                icon: PathBuf::new(), // filled in by steam_entry from the grid artwork
            };
            crate::steam_entry::ensure(&plan, rep, warnings).await;
        }
    }

    // 6. Desktop entries (both modes — always a handy shortcut).
    write_desktop_entries(params, &script, rep, warnings, shortcuts).await;
    if params.create_shortcuts {
        let entry = plat::desktop_entry_dir().join("str-launcher-skyrim-together.desktop");
        shortcuts.push(entry.display().to_string());
    }

    Ok(())
}

/// Undo the launcher replacement: restore the original `SkyrimSELauncher.exe`.
#[cfg(all(unix, not(target_os = "macos")))]
async fn restore_launcher(game_dir: &Path, rep: &Reporter) {
    let launcher = game_dir.join("SkyrimSELauncher.exe");
    let backup = game_dir.join("_SkyrimSELauncher.exe");
    if launcher.is_symlink() {
        let _ = std::fs::remove_file(&launcher);
    }
    if backup.is_file() && !launcher.exists() {
        match std::fs::rename(&backup, &launcher) {
            Ok(()) => rep.log("Original SkyrimSELauncher.exe restored.").await,
            Err(e) => rep.log(format!("restoring SkyrimSELauncher.exe failed: {e}")).await,
        }
    }
    let _ = std::fs::remove_file(
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".config/protonfixes/localfixes/489830.py"),
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn setup_launcher_replacement(game_dir: &Path, rep: &Reporter, warnings: &mut Vec<String>) {
    let launcher = game_dir.join("SkyrimSELauncher.exe");
    let backup = game_dir.join("_SkyrimSELauncher.exe");
    let together = game_dir.join("SkyrimTogether.exe");
    if !together.is_file() {
        warnings.push("SkyrimTogether.exe missing after install — launcher not replaced.".into());
        return;
    }
    if launcher.is_symlink() {
        // Already set up.
        rep.log("Launcher replacement already in place.").await;
        return;
    }
    if launcher.is_file() {
        if backup.exists() {
            let _ = std::fs::remove_file(&launcher);
        } else if let Err(e) = std::fs::rename(&launcher, &backup) {
            warnings.push(format!("renaming SkyrimSELauncher.exe failed: {e}"));
            return;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        match symlink("SkyrimTogether.exe", &launcher) {
            Ok(()) => rep.log(
                "SkyrimSELauncher.exe now launches Skyrim Together (original kept as _SkyrimSELauncher.exe).",
            ).await,
            Err(e) => warnings.push(format!(
                "Could not link the launcher ({e}). Start multiplayer with the desktop entry or \
                 by running SkyrimTogether.exe directly."
            )),
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn install_protonfix(game_dir: &Path, rep: &Reporter) {
    // Local protonfixes fix for app 489830: keep GE-Proton from launching
    // skse64_loader.exe instead of our STR launcher when SKSE is installed.
    // (Ported from the SteamDeckSTR project, MIT licensed.)
    let home = match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return,
    };
    let mut targets = vec![home.join(".config/protonfixes/localfixes/489830.py")];
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            targets.push(PathBuf::from(xdg).join("protonfixes/localfixes/489830.py"));
        }
    }
    let content = PROTONFIX_PY;
    let mut wrote = false;
    for target in &targets {
        if let Some(parent) = target.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                continue;
            }
        }
        // Only overwrite when the launcher replacement is actually active for
        // this game, mirroring the reference behaviour.
        let is_str_launcher = std::fs::symlink_metadata(game_dir.join("SkyrimSELauncher.exe"))
            .map(|m| m.is_symlink())
            .unwrap_or(false);
        if !is_str_launcher {
            continue;
        }
        if std::fs::write(target, content).is_ok() {
            wrote = true;
        }
    }
    if wrote {
        rep.log("Installed the GE-Proton localfix for Skyrim (489830).").await;
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
const PROTONFIX_PY: &str = r#""""Skyrim Special Edition fix for GE-Proton, installed by STR Launcher.

A local fix in ~/.config/protonfixes/localfixes replaces GE-Proton's own one. It does the
same thing (start skse64_loader.exe instead of the launcher when SKSE is installed),
except while Skyrim Together Reborn is set up: then SkyrimSELauncher.exe is STR's
launcher, which loads SKSE itself, and starting SKSE's loader would skip STR.
Ported from the SteamDeckSTR project (MIT).
"""

import os

from protonfixes import util


def main() -> None:
    # Mod Organizer 2 has its own redirector
    if os.path.exists('modorganizer2'):
        return

    # STR Post-Deploy links SkyrimSELauncher.exe to SkyrimTogether.exe
    launcher = 'SkyrimSELauncher.exe'
    if os.path.islink(launcher) and os.path.realpath(launcher).lower().endswith('skyrimtogether.exe'):
        return

    if os.path.isfile('skse64_loader.exe'):
        util.replace_command(launcher, 'skse64_loader.exe')
"#;

#[cfg(all(unix, not(target_os = "macos")))]
async fn write_launch_script(params: &Params, rep: &Reporter) -> Result<PathBuf, String> {
    // Delegate to launch.rs so the app's Launch button and the Steam entry /
    // desktop shortcuts all run the exact same script.
    let ctx = crate::launch::LaunchContext {
        game_dir: params.game_dir.clone(),
        steam_root: params.steam_root.clone(),
        proton_tool: params.proton_tool.clone(),
        dirs: params.dirs.clone(),
    };
    let script_path = crate::launch::ensure_launch_script(&ctx)?;
    rep.log(format!("Launch script written to {}", script_path.display())).await;
    Ok(script_path)
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn write_desktop_entries(
    _params: &Params,
    script: &Path,
    rep: &Reporter,
    warnings: &mut Vec<String>,
    _shortcuts: &mut Vec<String>,
) {
    use crate::platform::linux as plat;
    let exec = format!("\"{}\"", script.display());
    let name = "Skyrim Together Reborn";
    let comment = "Play Skyrim Special Edition with Skyrim Together Reborn (via STR Launcher)";
    // Install the bundled dragon emblem as the entry icon.
    let icon_dir = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".local/share/icons"))
        .ok();
    let icon_name = "str-launcher.png";
    let mut icon = "applications-games".to_string();
    if let Some(icon_dir) = icon_dir {
        let icon_path = icon_dir.join(icon_name);
        if std::fs::write(&icon_path, LOGO_ICON_PNG).is_ok() {
            icon = icon_path.display().to_string();
        }
    }
    let entry_path = plat::desktop_entry_dir().join("str-launcher-skyrim-together.desktop");
    if let Err(e) = plat::write_desktop_entry(&entry_path, name, comment, &exec, &icon, false) {
        warnings.push(format!("Desktop entry failed: {e}"));
        return;
    }
    rep.log(format!("Application entry: {}", entry_path.display())).await;
    for desk in plat::desktop_dirs() {
        let target = desk.join("str-launcher-skyrim-together.desktop");
        match std::fs::copy(&entry_path, &target) {
            Ok(_) => rep.log(format!("Desktop shortcut: {}", target.display())).await,
            Err(e) => warnings.push(format!("Desktop shortcut failed ({}): {e}", target.display())),
        }
    }
}

// ---------------------------------------------------------------------------
// Undo / uninstall
// ---------------------------------------------------------------------------

pub async fn undo(params: Params, tx: mpsc::Sender<Event>) {
    let rep = Reporter::new(tx);
    let result = undo_inner(&params, &rep).await;
    let _ = rep.tx.send(Event::Done(result.map(|_| InstallSummary::default()))).await;
}

async fn undo_inner(params: &Params, rep: &Reporter) -> Result<(), String> {
    let game_dir = params.game_dir.clone();
    rep.step("Removing Skyrim Together Reborn", 0).await;

    // Remove shortcuts first so nothing dangles.
    #[cfg(target_os = "windows")]
    platform::native::remove_shortcuts(&game_dir);
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use crate::platform::linux as plat;
        let _ = std::fs::remove_file(plat::desktop_entry_dir().join("str-launcher-skyrim-together.desktop"));
        for desk in plat::desktop_dirs() {
            let _ = std::fs::remove_file(desk.join("str-launcher-skyrim-together.desktop"));
        }
    }

    let manifest = InstallManifest::load(&game_dir);
    remove_manifest_files(&game_dir, &manifest, rep).await;

    // Restore the original launcher and drop the Steam game entry.
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        restore_launcher(&game_dir, rep).await;
        let plan = crate::steam_entry::EntryPlan {
            script: params.dirs.scripts.join("launch-skyrim-together.sh"),
            start_dir: game_dir.clone(),
            icon: PathBuf::new(),
        };
        crate::steam_entry::remove(Some(&plan), rep).await;
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let backup = game_dir.join("_SkyrimSELauncher.exe");
        let launcher = game_dir.join("SkyrimSELauncher.exe");
        if backup.is_file() && !launcher.exists() {
            let _ = std::fs::rename(&backup, &launcher);
        }
    }

    // Restore CC content.
    rep.step("Restoring Creation Club content", 1).await;
    match cc_clean::restore_cc(&game_dir, &params.cc_backup_dir) {
        Ok(restored) if restored.is_empty() => rep.log("No CC backup to restore.").await,
        Ok(restored) => {
            rep.log(format!("Restored {} CC files.", restored.len())).await;
        }
        Err(e) => rep.log(format!("CC restore failed: {e}")).await,
    }

    // Remove STR plugins from Plugins.txt files we can find.
    rep.step("Cleaning plugin lists", 2).await;
    for profile_dir in profile_dirs(params) {
        let plugins_txt = profile_dir.join("Plugins.txt");
        if plugins_txt.is_file() {
            if let Ok(text) = std::fs::read_to_string(&plugins_txt) {
                let cleaned: Vec<&str> = text
                    .lines()
                    .filter(|l| !l.trim_start_matches('*').trim().to_lowercase().starts_with("skyrimtogether"))
                    .collect();
                let _ = std::fs::write(&plugins_txt, format!("{}\n", cleaned.join("\n")));
            }
        }
    }

    InstallManifest::remove(&game_dir);
    rep.log("Skyrim Together Reborn has been removed.").await;
    Ok(())
}

/// Load the install manifest for a game folder (helper for the UI).
pub fn manifest_for(game_dir: &Path) -> InstallManifest {
    InstallManifest::load(game_dir)
}

/// Quick status probe used by the UI.
pub struct StatusProbe {
    pub str_version: Option<String>,
    pub cc_present: bool,
}

pub fn probe(game_dir: &Path) -> StatusProbe {
    let manifest = InstallManifest::load(game_dir);
    let exe = game_dir.join("SkyrimTogether.exe");
    let _ = exe;
    StatusProbe {
        str_version: manifest.str_version,
        cc_present: cc_clean::cc_present(game_dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Minimal fake SkyrimSE.exe (PE parse fails gracefully) — enough bytes for
    /// the file checks.
    fn fake_exe() -> Vec<u8> {
        b"MZ\x90\x00placeholder-not-a-real-pe".to_vec()
    }

    fn make_zip(path: &std::path::Path, files: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        for (name, content) in files {
            zw.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zw.write_all(content).unwrap();
        }
        zw.finish().unwrap();
    }

    /// End-to-end: offline install into a fake game folder, with a bundled CC
    /// file present. Verifies extraction, merging, manifest, CC backup and
    /// Plugins.txt handling.
    #[tokio::test]
    async fn offline_install_pipeline() {
        // Never touch the real Steam session from tests.
        std::env::set_var("STR_LAUNCHER_TEST_MODE", "1");
        let root = std::env::temp_dir().join(format!("str-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        // Fake game install.
        let game = root.join("steamapps/common/Skyrim Special Edition");
        std::fs::create_dir_all(game.join("Data")).unwrap();
        std::fs::write(game.join("SkyrimSE.exe"), fake_exe()).unwrap();
        std::fs::write(game.join("SkyrimSELauncher.exe"), b"launcher").unwrap();
        std::fs::write(game.join("Data/Skyrim.esm"), b"skyrim").unwrap();
        std::fs::write(game.join("Data/ccBGSSSE001-Fish.esm"), b"fish").unwrap();

        // The game has been launched: a Plugins.txt exists in the Proton prefix.
        let profile = root
            .join("steamapps/compatdata/489830/pfx/drive_c/users/steamuser")
            .join("Documents/My Games/Skyrim Special Edition");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("Plugins.txt"), "*Skyrim.esm\nDawnguard.esm\n").unwrap();

        // Offline archives: STR ships a wrapper folder, Address Library does not.
        let str_zip = root.join("str.zip");
        make_zip(
            &str_zip,
            &[
                ("SkyrimTogetherReborn/SkyrimTogether.exe", b"st-exe"),
                ("SkyrimTogetherReborn/Data/SkyrimTogether.esp", b"esp"),
                ("SkyrimTogetherReborn/Data/SKSE/Plugins/SkyrimTogether.dll", b"dll"),
                ("SkyrimTogetherReborn/resources/shaders.bin", b"shaders"),
            ],
        );
        let al_zip = root.join("al.zip");
        make_zip(
            &al_zip,
            &[("SKSE/Plugins/version-1-6-1170-0.dll", b"all-in-one")],
        );

        let dirs = crate::paths::AppDirs {
            config: root.join("app/config"),
            data: root.join("app/data"),
            downloads: root.join("app/downloads"),
            staging: root.join("app/staging"),
            cc_backup: root.join("app/cc-backup"),
            scripts: root.join("app/scripts"),
            logs: root.join("app/logs"),
        };
        std::fs::create_dir_all(&dirs.downloads).unwrap();

        let params = Params {
            game_dir: game.clone(),
            steam_root: None,
            nexus_key: String::new(),
            offline_str: Some(str_zip.clone()),
            offline_al: Some(al_zip.clone()),
            proton_tool: crate::steam::DEFAULT_PROTON_TOOL.to_string(),
            clean_cc: true,
            create_shortcuts: true,
            launch_mode: LaunchMode::LauncherReplacement,
            dirs,
            cc_backup_dir: root.join("steamapps/str-cc-backup"),
        };

        let (tx, mut rx) = tokio::sync::mpsc::channel(1024);
        tokio::spawn(run(params, tx, Config::default()));

        let mut summary = None;
        while let Some(event) = rx.recv().await {
            if let Event::Done(result) = event {
                summary = Some(result.expect("pipeline should succeed"));
                break;
            }
        }
        let summary = summary.expect("no Done event");
        assert_eq!(summary.cc_removed, vec!["ccBGSSSE001-Fish.esm".to_string()]);
        // The fake prefix exists, so the game counts as "launched once".
        assert!(!summary.warnings.iter().any(|w| w.contains("never been launched")));

        // Files landed in the game folder.
        assert_eq!(std::fs::read(game.join("SkyrimTogether.exe")).unwrap(), b"st-exe");
        assert_eq!(std::fs::read(game.join("Data/SkyrimTogether.esp")).unwrap(), b"esp");
        assert_eq!(std::fs::read(game.join("Data/SKSE/Plugins/SkyrimTogether.dll")).unwrap(), b"dll");
        assert_eq!(std::fs::read(game.join("Data/SKSE/Plugins/version-1-6-1170-0.dll")).unwrap(), b"all-in-one");
        assert_eq!(std::fs::read(game.join("resources/shaders.bin")).unwrap(), b"shaders");

        // CC file was moved to the backup folder, not deleted.
        assert!(!game.join("Data/ccBGSSSE001-Fish.esm").exists());
        assert_eq!(
            std::fs::read(root.join("steamapps/str-cc-backup/ccBGSSSE001-Fish.esm")).unwrap(),
            b"fish"
        );

        // Plugins.txt: STR plugin enabled, others untouched.
        let plugins = std::fs::read_to_string(profile.join("Plugins.txt")).unwrap();
        assert!(plugins.contains("*Skyrim.esm"));
        assert!(plugins.contains("Dawnguard.esm"));
        assert!(plugins.contains("*SkyrimTogether.esp"));

        // Manifest records the install.
        let manifest = InstallManifest::load(&game);
        assert!(manifest.files.iter().any(|f| f == "SkyrimTogether.exe"));
        assert!(manifest.files.iter().any(|f| f == "Data/SKSE/Plugins/version-1-6-1170-0.dll"));

        // Launcher replacement symlink exists and points at SkyrimTogether.exe.
        #[cfg(unix)]
        {
            let link = std::fs::read_link(game.join("SkyrimSELauncher.exe")).unwrap();
            assert_eq!(link, std::path::Path::new("SkyrimTogether.exe"));
            assert!(game.join("_SkyrimSELauncher.exe").is_file());
        }

        // Second install (update) must not duplicate or fail.
        let _ = std::fs::remove_file(&str_zip);
        let str_zip2 = root.join("str2.zip");
        make_zip(
            &str_zip2,
            &[("SkyrimTogetherReborn/SkyrimTogether.exe", b"st-exe-v2")],
        );
        let mut params2 = params_clone_for_test(&root);
        params2.offline_str = Some(str_zip2);
        params2.offline_al = Some(al_zip);
        let (tx2, mut rx2) = tokio::sync::mpsc::channel(1024);
        tokio::spawn(run(params2, tx2, Config::default()));
        let mut done2 = false;
        while let Some(event) = rx2.recv().await {
            if let Event::Done(Ok(_)) = event {
                done2 = true;
                break;
            }
        }
        assert!(done2, "update install should succeed");
        assert_eq!(std::fs::read(game.join("SkyrimTogether.exe")).unwrap(), b"st-exe-v2");

        let _ = std::fs::remove_dir_all(&root);
    }

    fn params_clone_for_test(root: &std::path::Path) -> Params {
        Params {
            game_dir: root.join("steamapps/common/Skyrim Special Edition"),
            steam_root: None,
            nexus_key: String::new(),
            offline_str: None,
            offline_al: None,
            proton_tool: crate::steam::DEFAULT_PROTON_TOOL.to_string(),
            clean_cc: true,
            create_shortcuts: true,
            launch_mode: LaunchMode::LauncherReplacement,
            dirs: crate::paths::AppDirs {
                config: root.join("app/config"),
                data: root.join("app/data"),
                downloads: root.join("app/downloads"),
                staging: root.join("app/staging"),
                cc_backup: root.join("app/cc-backup"),
                scripts: root.join("app/scripts"),
                logs: root.join("app/logs"),
            },
            cc_backup_dir: root.join("steamapps/str-cc-backup"),
        }
    }
}
