//! The "add a Steam game" launch mode (Linux): registers Skyrim Together
//! Reborn as its own entry in Steam's non-Steam shortcut list, complete with
//! SteamGridDB artwork.
//!
//! The shortcut's `exe` is the generated launch script (never the game exe),
//! so Steam can start it natively while the script itself drives Proton with
//! the real Skyrim prefix (`compatdata/489830`). This avoids Steam creating a
//! second Proton prefix for the shortcut — STR's registry keys and the game's
//! prefix stay in one place.
//!
//! `shortcuts.vdf` must be edited while Steam is *not* running (Steam rewrites
//! the file from memory on exit), so Steam is paused around the edit and
//! restarted afterwards.

use std::path::{Path, PathBuf};

use crate::shortcuts_vdf::{self, ShortcutSpec};
use crate::str_install::Reporter;

pub const APP_NAME: &str = "Skyrim Together Reborn";
/// Identifies script-backed entries when matching/removing.
pub const EXE_TAG: &str = "launch-skyrim-together.sh";

/// Everything needed to (re)create the entry.
#[derive(Debug, Clone)]
pub struct EntryPlan {
    /// The generated launch script — becomes the shortcut's `exe`.
    pub script: PathBuf,
    /// The Skyrim Special Edition folder — becomes `startdir`.
    pub start_dir: PathBuf,
    /// A PNG used for the shortcut's `icon` field (already written to disk).
    pub icon: PathBuf,
}

impl EntryPlan {
    /// The strings as they are stored in shortcuts.vdf (Steam quotes paths).
    pub fn spec(&self) -> ShortcutSpec {
        let quoted = |p: &PathBuf| format!("\"{}\"", p.display());
        ShortcutSpec {
            app_name: APP_NAME.to_string(),
            exe: quoted(&self.script),
            start_dir: quoted(&self.start_dir),
            icon: if self.icon.as_os_str().is_empty() {
                String::new()
            } else {
                quoted(&self.icon)
            },
        }
    }
}

/// Every logged-in Steam account's `config` dir, across all Steam roots
/// (native + Flatpak). Multi-account machines get the entry for each account,
/// matching how Steam ROM Manager handles it.
pub fn user_config_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in crate::steam::steam_roots() {
        let Ok(entries) = std::fs::read_dir(root.join("userdata")) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let numeric = !name.is_empty()
                && name != "0"
                && name.bytes().all(|b| b.is_ascii_digit());
            if numeric && e.path().is_dir() {
                out.push(e.path().join("config"));
            }
        }
    }
    out
}

fn shortcuts_path(config_dir: &Path) -> PathBuf {
    config_dir.join("shortcuts.vdf")
}

/// Is the entry currently registered? Returns the stored app name / exe /
/// app id for the preflight checklist.
pub fn find_entry() -> Option<(String, String, u32)> {
    for dir in user_config_dirs() {
        let path = shortcuts_path(&dir);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if let Some(found) = shortcuts_vdf::find_matching(&bytes, APP_NAME, EXE_TAG) {
            return Some(found);
        }
    }
    None
}

/// Add or refresh the Steam entry and its artwork.
pub async fn ensure(plan: &EntryPlan, rep: &Reporter, warnings: &mut Vec<String>) {
    // Tests never touch a real Steam userdata folder.
    if std::env::var("STR_LAUNCHER_TEST_MODE").is_ok() {
        return;
    }
    let dirs = user_config_dirs();
    if dirs.is_empty() {
        warnings.push(
            "No Steam userdata folder was found — the Steam game entry could not be created. \
             Steam may not have been logged in yet."
                .into(),
        );
        return;
    }

    let spec = plan.spec();
    let app_id = spec.app_id();

    // Write the artwork next to each account's shortcuts.vdf (the icon copy
    // doubles as the shortcut's `icon` field target).
    let mut icon_path = PathBuf::new();
    for dir in &dirs {
        let grid = dir.join("grid");
        match crate::sgdb::write_artwork(&grid, app_id) {
            Ok(written) => {
                if icon_path.as_os_str().is_empty() {
                    if let Some(icon) = written.iter().find(|p| {
                        p.file_name()
                            .is_some_and(|n| n.to_string_lossy().ends_with("_icon.png"))
                    }) {
                        icon_path = icon.clone();
                    }
                }
            }
            Err(e) => warnings.push(format!("Steam artwork failed: {e}")),
        }
    }

    let spec = if icon_path.as_os_str().is_empty() {
        spec
    } else {
        let mut with_icon = plan.clone();
        with_icon.icon = icon_path;
        with_icon.spec()
    };

    enum Outcome {
        Created(PathBuf),
        Updated(PathBuf),
        Unchanged(PathBuf),
        Failed(PathBuf, String),
    }

    let results = pause_steam(rep, || {
        let mut results = Vec::new();
        for dir in &dirs {
            let path = shortcuts_path(dir);
            let outcome = match std::fs::read(&path) {
                Ok(bytes) => match shortcuts_vdf::upsert(&bytes, &spec) {
                    Ok(new_bytes) => {
                        if new_bytes != bytes {
                            match crate::paths::atomic_write(&path, &new_bytes) {
                                Ok(()) => Outcome::Updated(path),
                                Err(e) => Outcome::Failed(path, e),
                            }
                        } else {
                            Outcome::Unchanged(path)
                        }
                    }
                    Err(e) => Outcome::Failed(path, e),
                },
                Err(_) => {
                    if let Err(e) = std::fs::create_dir_all(dir) {
                        Outcome::Failed(path, format!("create {}: {e}", dir.display()))
                    } else {
                        match crate::paths::atomic_write(
                            &path,
                            &shortcuts_vdf::serialize_file(std::slice::from_ref(&spec)),
                        ) {
                            Ok(()) => Outcome::Created(path),
                            Err(e) => Outcome::Failed(path, e),
                        }
                    }
                },
            };
            results.push(outcome);
        }
        results
    })
    .await;

    let mut created = 0usize;
    let mut updated = 0usize;
    let mut ok = true;
    for outcome in results.unwrap_or_default() {
        match outcome {
            Outcome::Created(path) => {
                created += 1;
                rep.log(format!("Steam entry created in {}", path.display())).await;
            }
            Outcome::Updated(path) => {
                updated += 1;
                rep.log(format!("Steam entry refreshed in {}", path.display())).await;
            }
            Outcome::Unchanged(path) => {
                rep.log(format!("Steam entry already up to date in {}", path.display())).await;
            }
            Outcome::Failed(path, e) => {
                ok = false;
                warnings.push(format!("{}: {e}", path.display()));
            }
        }
    }

    if ok {
        rep.log(format!(
            "\"{APP_NAME}\" is now a Steam game (app id {app_id}, {created} created, \
             {updated} refreshed) with hero, grids, logo and icon artwork."
        ))
        .await;
    }
}

/// Remove the Steam entry and its artwork (used by Undo and by mode switches).
pub async fn remove(plan: Option<&EntryPlan>, rep: &Reporter) {
    // Tests never touch a real Steam userdata folder.
    if std::env::var("STR_LAUNCHER_TEST_MODE").is_ok() {
        return;
    }
    // Common case (launcher-replacement mode): nothing to do — skip the
    // Steam restart dance entirely.
    if find_entry().is_none() && plan.is_none() {
        return;
    }
    let expected_id = plan.map(|p| p.spec().app_id());

    let removed = pause_steam(rep, || {
        let mut removed = 0usize;
        let mut cleaned_grid_ids: Vec<(PathBuf, u32)> = Vec::new();
        for dir in user_config_dirs() {
            let path = shortcuts_path(&dir);
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if let Ok((new_bytes, ids)) = shortcuts_vdf::remove_matching(&bytes, APP_NAME, EXE_TAG) {
                if !ids.is_empty() && crate::paths::atomic_write(&path, &new_bytes).is_ok() {
                    removed += ids.len();
                    for id in &ids {
                        cleaned_grid_ids.push((dir.clone(), *id));
                    }
                }
            }
        }
        // Artwork for our standard app id, even if no entry matched (e.g. a
        // previous run was interrupted before shortcuts.vdf was written).
        if let Some(id) = expected_id {
            for dir in user_config_dirs() {
                cleaned_grid_ids.push((dir, id));
            }
        }
        for (dir, id) in &cleaned_grid_ids {
            crate::sgdb::remove_artwork(&dir.join("grid"), *id);
        }
        removed
    })
    .await
    .unwrap_or(0);

    if removed > 0 {
        rep.log(format!(
            "Removed {removed} Steam game entr{} and their artwork.",
            if removed == 1 { "y" } else { "ies" }
        ))
        .await;
    }
}

/// Pause Steam (if running) around a filesystem edit, then restart it.
async fn pause_steam<T>(rep: &Reporter, action: impl FnOnce() -> T) -> Result<T, String> {
    // Tests must never touch a real, running Steam session.
    if std::env::var("STR_LAUNCHER_TEST_MODE").is_ok() {
        return Ok(action());
    }
    let was_running = crate::platform::linux::is_steam_running();
    if was_running {
        rep.log("Restarting Steam so the shortcut file can be edited…".to_string()).await;
        let _ = std::process::Command::new("steam").arg("-shutdown").spawn();
        for _ in 0..30 {
            if !crate::platform::linux::is_steam_running() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    let result = action();

    if was_running {
        let _ = std::process::Command::new("steam").spawn();
        rep.log("Steam restarted.".to_string()).await;
    }
    Ok(result)
}
