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
//!    - Linux: GE-Proton install + selection in Steam, launcher replacement,
//!      protonfixes, Proton-prefix registry keys and a standalone launch
//!      script + desktop entry (see [`linux_setup`]).
//!
//! Supporting submodules: [`reporter`] (progress events), [`files`]
//! (archive-root discovery, merging, manifest removal) and [`undo`].

mod files;
#[cfg(all(unix, not(target_os = "macos")))]
mod linux_setup;
mod reporter;
mod undo;

pub use reporter::{Event, Reporter, ThrottledProgress};

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use crate::archive;
use crate::cc_clean;
use crate::config::{Config, InstallManifest, LaunchMode, NEXUS_ADDRESS_LIBRARY, NEXUS_STR};
use crate::nexus::{ModFile, NexusClient};
use crate::pe_version;
#[cfg(target_os = "windows")]
use crate::platform;
use crate::paths::atomic_write;
use crate::plugins;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct InstallSummary {
    pub str_version: Option<String>,
    pub address_library_version: Option<String>,
    pub game_version: Option<String>,
    pub cc_removed: Vec<String>,
    pub warnings: Vec<String>,
    pub shortcuts: Vec<String>,
}

#[derive(Clone)]
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

pub async fn run(params: Params, tx: mpsc::Sender<Event>) {
    let rep = Reporter::new(tx);
    rep.done(run_inner(&params, &rep).await).await;
}

async fn run_inner(params: &Params, rep: &Reporter) -> Result<InstallSummary, String> {
    let (game_dir, data_dir, game_version, old_manifest, mut warnings) =
        validate_game(params, rep).await?;
    let ((str_archive, str_meta), (al_archive, al_meta)) = fetch_archives(params, rep).await?;
    let (str_root, al_root, extracted_version) =
        extract_archives(params, rep, str_archive.as_ref(), al_archive.as_ref()).await?;

    replace_previous_install(rep, &game_dir, &old_manifest, &mut warnings).await;
    let installed_files = install_files(rep, &game_dir, &data_dir, &str_root, &al_root).await?;

    let al_version = al_meta.as_ref().map(mod_version_label);
    let str_version_label = str_meta.as_ref().map(mod_version_label).or(extracted_version);

    let cc_removed = clean_creation_club(params, rep, &game_dir, &mut warnings).await;
    enable_plugins(params, rep, &data_dir, &mut warnings).await;
    let shortcuts = configure_platform(params, rep, &mut warnings).await?;

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

    Ok(InstallSummary {
        str_version: str_version_label,
        address_library_version: al_version,
        game_version,
        cc_removed,
        warnings,
        shortcuts,
    })
}

/// Update the stored profile after a successful install.
pub fn apply_summary_to_profile(cfg: &mut Config, summary: &InstallSummary, manifest: &InstallManifest) {
    let p = cfg.active_mut();
    p.installed_str_version = summary.str_version.clone();
    p.installed_str_file_id = manifest.str_file_id;
    p.installed_al_version = summary.address_library_version.clone();
    p.installed_al_file_id = manifest.address_library_file_id;
}

// ---------------------------------------------------------------------------
// Step 0: validate
// ---------------------------------------------------------------------------

/// Validate the game folder and collect early warnings. Returns
/// `(game_dir, data_dir, game_version, existing manifest, warnings)`.
async fn validate_game(
    params: &Params,
    rep: &Reporter,
) -> Result<(PathBuf, PathBuf, Option<String>, InstallManifest, Vec<String>), String> {
    rep.step("Validating Skyrim Special Edition", 0).await;
    let game_dir = params.game_dir.clone();
    let data_dir = game_dir.join("Data");
    let mut warnings = Vec::new();

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
    let game_version =
        pe_version::file_version(&game_dir.join("SkyrimSE.exe")).map(|v| v.to_string());
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
    if !check_launched_once(params) {
        warnings.push(
            "The game has never been launched through Steam. Start it once and reach the main \
             menu before playing multiplayer."
                .into(),
        );
    }

    Ok((game_dir, data_dir, game_version, manifest, warnings))
}

// ---------------------------------------------------------------------------
// Steps 1-2: downloads
// ---------------------------------------------------------------------------

/// `(archive path, Nexus metadata when downloaded online)`.
type Fetch = (Option<PathBuf>, Option<ModFile>);

/// How and what to fetch for one mod during steps 1-2.
struct FetchSpec<'a> {
    mod_id: u64,
    offline: Option<&'a PathBuf>,
    downloads_dir: &'a Path,
    step_index: usize,
    step_label: &'static str,
    progress_label: &'static str,
    /// Name used in success/error logs ("Address Library", "Skyrim Together Reborn").
    display_name: &'static str,
    /// Short name used in offline-mode logs ("Address Library", "STR").
    offline_label: &'static str,
}

/// Download both mods (or pick up the offline archives), returning
/// `(str, address library)` results.
async fn fetch_archives(params: &Params, rep: &Reporter) -> Result<(Fetch, Fetch), String> {
    let client = NexusClient::new(&params.nexus_key);
    let al = fetch_archive(
        &client,
        rep,
        FetchSpec {
            mod_id: NEXUS_ADDRESS_LIBRARY,
            offline: params.offline_al.as_ref(),
            downloads_dir: &params.dirs.downloads,
            step_index: 1,
            step_label: "Downloading Address Library (All in One)",
            progress_label: "Downloading Address Library",
            display_name: "Address Library",
            offline_label: "Address Library",
        },
    )
    .await?;
    let str = fetch_archive(
        &client,
        rep,
        FetchSpec {
            mod_id: NEXUS_STR,
            offline: params.offline_str.as_ref(),
            downloads_dir: &params.dirs.downloads,
            step_index: 2,
            step_label: "Downloading Skyrim Together Reborn",
            progress_label: "Downloading Skyrim Together Reborn",
            display_name: "Skyrim Together Reborn",
            offline_label: "STR",
        },
    )
    .await?;
    Ok((str, al))
}

/// Download one mod's latest main file — or take the user's offline archive.
async fn fetch_archive(client: &NexusClient, rep: &Reporter, spec: FetchSpec<'_>) -> Result<Fetch, String> {
    if let Some(path) = spec.offline {
        rep.log(format!(
            "Offline mode: using {} archive {}",
            spec.offline_label,
            path.display()
        ))
        .await;
        return Ok((Some(path.clone()), None));
    }
    rep.step(spec.step_label, spec.step_index).await;
    let mut prog = rep.progress_fn(spec.progress_label.to_string());
    let (path, meta) = client
        .download_main_file("skyrimspecialedition", spec.mod_id, spec.downloads_dir, &mut |d, t| {
            prog.send(d, t)
        })
        .await
        .map_err(|e| format!("{} download failed:\n{e}", spec.display_name))?;
    rep.log(format!(
        "{}: {} (v{}, {:.1} MiB)",
        spec.display_name,
        meta.name,
        meta.mod_version,
        meta.size as f64 / 1048576.0
    ))
    .await;
    Ok((Some(path), Some(meta)))
}

/// `mod_version` when present, else the generic `version` field.
fn mod_version_label(m: &ModFile) -> String {
    if m.mod_version.is_empty() { m.version.clone() } else { m.mod_version.clone() }
}

// ---------------------------------------------------------------------------
// Step 3: extract
// ---------------------------------------------------------------------------

/// Extract both archives into staging. Returns the STR archive root, the
/// Address Library root and the SkyrimTogether.exe version read from the PE.
async fn extract_archives(
    params: &Params,
    rep: &Reporter,
    str_archive: Option<&PathBuf>,
    al_archive: Option<&PathBuf>,
) -> Result<(PathBuf, PathBuf, Option<String>), String> {
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
    archive::extract_zip(str_path, &str_stage, &mut |d, t| prog.send(d, t))?;
    let str_root = files::find_str_root(&str_stage).ok_or_else(|| {
        format!(
            "{} does not look like a Skyrim Together Reborn archive (no SkyrimTogether.exe).\n\
             Download it from nexusmods.com/skyrimspecialedition/mods/69993 (Files tab).",
            str_path.display()
        )
    })?;
    let str_version =
        pe_version::file_version(&str_root.join("SkyrimTogether.exe")).map(|v| v.to_string());

    let al_path = al_archive.ok_or("no Address Library archive available")?;
    let mut prog = rep.progress_fn("Extracting Address Library".to_string());
    archive::extract_zip(al_path, &al_stage, &mut |d, t| prog.send(d, t))?;
    let al_root = files::find_al_root(&al_stage).ok_or_else(|| {
        format!(
            "{} does not look like an Address Library archive (no SKSE folder).\n\
             Download the latest \"All in one\" file from nexusmods.com/skyrimspecialedition/mods/32444.",
            al_path.display()
        )
    })?;

    Ok((str_root, al_root, str_version))
}

// ---------------------------------------------------------------------------
// Step 4: remove previous install
// ---------------------------------------------------------------------------

async fn replace_previous_install(
    rep: &Reporter,
    game_dir: &Path,
    manifest: &InstallManifest,
    warnings: &mut Vec<String>,
) {
    rep.step("Removing previous Skyrim Together files", 4).await;
    files::remove_manifest_files(game_dir, manifest, rep).await;
    files::remove_legacy_str_files(game_dir, rep).await;
    if game_dir.join("Data/SkyrimTogetherReborn").is_dir() {
        warnings.push(
            "A Vortex-style deployment was found at Data/SkyrimTogetherReborn. This launcher \
             installs STR independently; remove the Vortex mod to avoid two copies fighting."
                .into(),
        );
    }
}

// ---------------------------------------------------------------------------
// Step 5: install files
// ---------------------------------------------------------------------------

/// Merge the extracted archives into the game folder and return the
/// game-root-relative paths of every installed file.
async fn install_files(
    rep: &Reporter,
    game_dir: &Path,
    data_dir: &Path,
    str_root: &Path,
    al_root: &Path,
) -> Result<Vec<String>, String> {
    rep.step("Installing Skyrim Together Reborn", 5).await;
    let mut installed_files: Vec<String> = Vec::new();
    files::merge_tree(str_root, game_dir, game_dir, &mut installed_files).map_err(|e| {
        format!(
            "failed to copy files into the game folder: {e}\n\
             Is the game folder writable? Close the game and Steam, then retry."
        )
    })?;
    // Address Library archives ship `SKSE/` at their root (Vortex deploys the
    // archive root into Data), while some repacks wrap it in a `Data/` folder.
    let al_merge_base = if al_root.join("SKSE").is_dir() { data_dir } else { game_dir };
    files::merge_tree(al_root, al_merge_base, game_dir, &mut installed_files)
        .map_err(|e| format!("failed to copy Address Library files: {e}"))?;
    installed_files.sort();
    installed_files.dedup();
    rep.log(format!("Installed {} files into the game folder.", installed_files.len())).await;
    Ok(installed_files)
}

// ---------------------------------------------------------------------------
// Step 6: Creation Club cleanup
// ---------------------------------------------------------------------------

/// Back up (or skip) the bundled Creation Club content; returns the names of
/// the files moved.
async fn clean_creation_club(
    params: &Params,
    rep: &Reporter,
    game_dir: &Path,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    if params.clean_cc {
        rep.step("Cleaning bundled Creation Club content", 6).await;
        match cc_clean::backup_cc(game_dir, &params.cc_backup_dir) {
            Ok(moved) if moved.is_empty() => {
                rep.log("No bundled Creation Club content found — nothing to remove.").await;
                Vec::new()
            }
            Ok(moved) => {
                rep.log(format!(
                    "Backed up and removed {} CC files (Survival Mode, Fishing, ...). They can be \
                     restored with \"Undo\" or Steam's file verification.",
                    moved.len()
                ))
                .await;
                moved
            }
            Err(e) => {
                warnings.push(format!("CC cleanup failed: {e}"));
                Vec::new()
            }
        }
    } else {
        rep.step("Skipping Creation Club cleanup (disabled)", 6).await;
        if cc_clean::cc_present(game_dir) {
            warnings.push(
                "Bundled Creation Club content is installed and cleanup is disabled — this is a \
                 common cause of crashes in multiplayer.".into(),
            );
        }
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Step 7: Plugins.txt
// ---------------------------------------------------------------------------

/// Enable `SkyrimTogether.esp` in every existing Plugins.txt location.
async fn enable_plugins(params: &Params, rep: &Reporter, data_dir: &Path, warnings: &mut Vec<String>) {
    rep.step("Enabling SkyrimTogether.esp", 7).await;
    let str_plugins = plugins::find_str_plugins(data_dir);
    if str_plugins.is_empty() {
        warnings.push("No SkyrimTogether*.esp plugin found in the game's Data folder.".into());
    }
    let mut plugin_locations_updated = 0;
    for profile_dir in crate::plugins::candidate_game_profile_dirs(&params.game_dir) {
        let plugins_txt = profile_dir.join("Plugins.txt");
        if plugins_txt.is_file() {
            let mut text = std::fs::read_to_string(&plugins_txt).unwrap_or_default();
            plugins::update_plugins_text(&mut text, &str_plugins);
            if atomic_write(&plugins_txt, text.as_bytes()).is_ok() {
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
}

// ---------------------------------------------------------------------------
// Step 8: platform configuration
// ---------------------------------------------------------------------------

/// Windows: STR registry keys + shortcuts. Linux: the full GE-Proton /
/// launcher-replacement / Steam-entry setup ([`linux_setup::configure_linux`]).
async fn configure_platform(
    params: &Params,
    rep: &Reporter,
    warnings: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    rep.step("Configuring launch integration", 8).await;
    let mut shortcuts: Vec<String> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        let game_dir = &params.game_dir;
        platform::native::write_str_registry(game_dir)
            .map_err(|e| format!("writing STR registry keys failed: {e}"))?;
        rep.log("Registry keys written (SkyrimTogether will find SkyrimSE.exe automatically).")
            .await;
        if params.create_shortcuts {
            match platform::native::create_shortcuts(game_dir) {
                Ok(created) => {
                    rep.log(format!("Created shortcuts: {}", created.join(", "))).await;
                    shortcuts = created;
                }
                Err(e) => warnings.push(format!("Shortcut creation failed: {e}")),
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    linux_setup::configure_linux(params, rep, warnings, &mut shortcuts).await?;
    #[cfg(target_os = "macos")]
    let _ = (params, rep, warnings);

    Ok(shortcuts)
}

// ---------------------------------------------------------------------------
// Shared platform helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Undo / uninstall
// ---------------------------------------------------------------------------

pub async fn undo(params: Params, tx: mpsc::Sender<Event>) {
    let rep = Reporter::new(tx);
    rep.done(undo::undo_inner(&params, &rep).await.map(|_| InstallSummary::default()))
        .await;
}

/// Quick status probe used by the UI.
pub struct StatusProbe {
    pub str_version: Option<String>,
    pub cc_present: bool,
}

pub fn probe(game_dir: &Path) -> StatusProbe {
    let manifest = InstallManifest::load(game_dir);
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

    fn test_params(root: &std::path::Path) -> Params {
        Params {
            game_dir: root.join("steamapps/common/Skyrim Special Edition"),
            // A fake Steam root keeps the pipeline hermetic: the launch-script
            // step resolves its Proton tool inside this root instead of the
            // host's real Steam install.
            steam_root: Some(root.join("steam")),
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

        let mut params = test_params(&root);
        std::fs::create_dir_all(&params.dirs.downloads).unwrap();
        // Fake GE-Proton tool so the pipeline never downloads or needs a real
        // compatibility-tool install.
        let fake_proton = root
            .join("steam/compatibilitytools.d")
            .join(crate::steam::DEFAULT_PROTON_TOOL);
        std::fs::create_dir_all(&fake_proton).unwrap();
        std::fs::write(fake_proton.join("proton"), b"#!/bin/sh\n").unwrap();
        params.offline_str = Some(str_zip.clone());
        params.offline_al = Some(al_zip.clone());

        let summary = run_and_wait(params.clone()).await.expect("pipeline should succeed");
        assert_eq!(summary.cc_removed, vec!["ccBGSSSE001-Fish.esm".to_string()]);
        // The fake prefix exists, so the game counts as "launched once".
        assert!(!summary.warnings.iter().any(|w| w.contains("never been launched")));

        // Files landed in the game folder.
        assert_eq!(std::fs::read(game.join("SkyrimTogether.exe")).unwrap(), b"st-exe");
        assert_eq!(std::fs::read(game.join("Data/SkyrimTogether.esp")).unwrap(), b"esp");
        assert_eq!(std::fs::read(game.join("Data/SKSE/Plugins/SkyrimTogether.dll")).unwrap(), b"dll");
        assert_eq!(
            std::fs::read(game.join("Data/SKSE/Plugins/version-1-6-1170-0.dll")).unwrap(),
            b"all-in-one"
        );
        assert_eq!(std::fs::read(game.join("resources/shaders.bin")).unwrap(), b"shaders");

        // CC file was moved to the backup folder, not deleted.
        assert!(!game.join("Data/ccBGSSSE001-Fish.esm").exists());
        assert_eq!(
            std::fs::read(root.join("steamapps/str-cc-backup/ccBGSSSE001-Fish.esm")).unwrap(),
            b"fish"
        );

        // Plugins.txt: STR plugin enabled, others untouched.
        let plugins_txt = std::fs::read_to_string(profile.join("Plugins.txt")).unwrap();
        assert!(plugins_txt.contains("*Skyrim.esm"));
        assert!(plugins_txt.contains("Dawnguard.esm"));
        assert!(plugins_txt.contains("*SkyrimTogether.esp"));

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
        let mut params2 = params;
        params2.offline_str = Some(str_zip2);
        params2.offline_al = Some(al_zip);
        run_and_wait(params2).await.expect("update install should succeed");
        assert_eq!(std::fs::read(game.join("SkyrimTogether.exe")).unwrap(), b"st-exe-v2");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Drive a pipeline run to completion, returning the final summary.
    async fn run_and_wait(params: Params) -> Result<InstallSummary, String> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1024);
        tokio::spawn(run(params, tx));
        while let Some(event) = rx.recv().await {
            if let Event::Done(result) = event {
                return result;
            }
        }
        panic!("no Done event");
    }
}
