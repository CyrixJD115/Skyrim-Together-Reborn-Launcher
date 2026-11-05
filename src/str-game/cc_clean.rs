//! Creation Club content handling.
//!
//! The Anniversary Edition update ships four Creation Club items that are known
//! to cause instability with Skyrim Together Reborn (per the official wiki and
//! the SteamDeckSTR reference implementation). The wiki strongly advises
//! removing them. We move them into a backup folder instead of deleting, so
//! they can be restored; Steam's "Verify integrity of game files" also
//! restores them.

use std::path::Path;

/// The four bundled CC items (plugin + archive for each), as removed by the
/// SteamDeckSTR reference implementation.
pub const CC_FILES: &[&str] = &[
    "ccBGSSSE001-Fish.bsa",
    "ccBGSSSE001-Fish.esm",
    "ccBGSSSE025-AdvDSGS.bsa",
    "ccBGSSSE025-AdvDSGS.esm",
    "ccBGSSSE037-Curios.bsa",
    "ccBGSSSE037-Curios.esl",
    "ccQDRSSE001-SurvivalMode.bsa",
    "ccQDRSSE001-SurvivalMode.esl",
];

pub fn cc_present(game_dir: &Path) -> bool {
    let data = game_dir.join("Data");
    CC_FILES.iter().any(|f| data.join(f).is_file())
}

/// Move bundled CC files into `backup_dir`. Returns the file names moved.
pub fn backup_cc(game_dir: &Path, backup_dir: &Path) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(backup_dir).map_err(|e| e.to_string())?;
    let data = game_dir.join("Data");
    let mut moved = Vec::new();
    for f in CC_FILES {
        let src = data.join(f);
        if src.is_file() {
            let dst = backup_dir.join(f);
            std::fs::rename(&src, &dst).map_err(|e| format!("{}: {e}", f))?;
            moved.push((*f).to_string());
        }
    }
    Ok(moved)
}

/// Restore previously backed-up CC files into the game's Data folder.
pub fn restore_cc(game_dir: &Path, backup_dir: &Path) -> Result<Vec<String>, String> {
    let data = game_dir.join("Data");
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let mut restored = Vec::new();
    if let Ok(entries) = std::fs::read_dir(backup_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.to_ascii_lowercase().starts_with("cc") {
                continue;
            }
            let dst = data.join(&name);
            if !dst.exists() {
                std::fs::rename(entry.path(), dst).map_err(|e| format!("{name}: {e}"))?;
                restored.push(name);
            }
        }
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_backs_up() {
        let tmp = std::env::temp_dir().join(format!("str-cc-test-{}", std::process::id()));
        let data = tmp.join("game/Data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("ccBGSSSE001-Fish.esm"), b"x").unwrap();
        std::fs::write(data.join("Skyrim.esm"), b"x").unwrap();
        assert!(cc_present(&tmp.join("game")));
        let moved = backup_cc(&tmp.join("game"), &tmp.join("backup")).unwrap();
        assert_eq!(moved, vec!["ccBGSSSE001-Fish.esm".to_string()]);
        assert!(!cc_present(&tmp.join("game")));
        let restored = restore_cc(&tmp.join("game"), &tmp.join("backup")).unwrap();
        assert_eq!(restored, vec!["ccBGSSSE001-Fish.esm".to_string()]);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
