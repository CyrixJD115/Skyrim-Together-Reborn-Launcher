//! The Undo pipeline: reverse everything the installer did — shortcuts,
//! manifest files, launch integration, Creation Club backups and plugin list
//! entries.

use super::{files, linux_setup, Params, Reporter};
use crate::cc_clean;
use crate::config::InstallManifest;

pub(super) async fn undo_inner(params: &Params, rep: &Reporter) -> Result<(), String> {
    let game_dir = params.game_dir.clone();
    rep.step("Removing Skyrim Together Reborn", 0).await;

    // Remove shortcuts first so nothing dangles.
    #[cfg(target_os = "windows")]
    crate::platform::native::remove_shortcuts(&game_dir);
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use crate::platform::linux as plat;
        let _ = std::fs::remove_file(plat::desktop_entry_dir().join("str-launcher-skyrim-together.desktop"));
        for desk in plat::desktop_dirs() {
            let _ = std::fs::remove_file(desk.join("str-launcher-skyrim-together.desktop"));
        }
    }

    let manifest = InstallManifest::load(&game_dir);
    files::remove_manifest_files(&game_dir, &manifest, rep).await;

    // Restore the original launcher and drop the Steam game entry.
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux_setup::restore_launcher(&game_dir, rep).await;
        let plan = crate::steam_entry::EntryPlan {
            script: params.dirs.scripts.join("launch-skyrim-together.sh"),
            start_dir: game_dir.clone(),
            icon: std::path::PathBuf::new(),
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
    for profile_dir in crate::plugins::candidate_game_profile_dirs(&params.game_dir) {
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
