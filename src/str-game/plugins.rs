//! `Plugins.txt` handling.
//!
//! Skyrim Together Reborn 1.8+ ships `SkyrimTogether.esp` and
//! `SkyrimTogetherQuestPatches.esp` which must be active, and its client
//! rejects any install whose loaded plugin list is not exactly the vanilla
//! set plus those two (the "not purely vanilla" dialog). We mirror that set:
//! any line matching a `SkyrimTogether*` plugin is forced active (`*` prefix)
//! and appended if missing, `_ResourcePack.esl` (a base-game file since
//! runtime 1.6.x — no STR esp masters it, so the game would never load it on
//! its own) is ensured as well, and when no Plugins.txt exists yet — a fresh
//! Proton prefix only gets one after the game's first run — Install writes a
//! canonical one so the very first launch already loads the right set.
//!
//! Recent game versions read the list from `%LOCALAPPDATA%\Skyrim Special
//! Edition`, older ones from `Documents\My Games\Skyrim Special Edition`, so
//! every existing candidate location is updated.

/// The plugin set the game loads besides the `SkyrimTogether*` esps, in load
/// order. STR's client (`DiscoveryService::IsDefaultModlist`) accepts only
/// this list plus the shipped esps, in exact order, nothing else.
pub const BASE_PLUGINS: [&str; 6] = ["Skyrim.esm", "Update.esm", "Dawnguard.esm", "HearthFires.esm", "Dragonborn.esm", "_ResourcePack.esl"];

/// Full Plugins.txt for a fresh install: Skyrim's own header comments, the
/// base set and the `SkyrimTogether*` esps, all active, in load order.
pub fn canonical_plugins_text(str_plugins: &[String]) -> String {
    let mut lines: Vec<String> = vec![
        "# This file is used by Skyrim to keep track of your downloaded content.".to_string(),
        "# Please do not modify this file.".to_string(),
    ];
    lines.extend(BASE_PLUGINS.iter().map(|p| format!("*{p}")));
    lines.extend(str_plugins.iter().map(|p| format!("*{p}")));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Normalize required entries in the supplied order, enabling each exactly
/// once. Preserve unrelated entries so additional mods can be reported.
pub fn update_plugins_text(text: &mut String, ensure_active: &[String]) {
    // Rebuild required entries in canonical order, removing duplicates and
    // fixing existing entries that were in the wrong position. Preserve other
    // mods: the readiness check reports them instead of silently disabling them.
    let mut lines: Vec<String> = text.lines()
        .filter(|line| !ensure_active.iter().any(|p| plugin_name(line).eq_ignore_ascii_case(p)))
        .map(str::to_string)
        .collect();
    lines.extend(ensure_active.iter().map(|p| format!("*{p}")));
    *text = format!("{}\n", lines.join("\n"));
}

fn plugin_name(line: &str) -> &str {
    line.trim().trim_start_matches('\u{feff}').trim_start_matches('*').trim()
}

/// Wine uses case-insensitive filenames, but the host filesystem may not.
/// Reuse the existing spelling rather than creating a second Plugins.txt.
pub fn plugins_path(dir: &std::path::Path) -> std::path::PathBuf {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().eq_ignore_ascii_case("Plugins.txt") {
                return entry.path();
            }
        }
    }
    dir.join("Plugins.txt")
}

/// Validate active optional plugins. The five official masters load implicitly.
pub fn validate_plugins_text(text: &str, str_plugins: &[String]) -> Result<(), String> {
    let required: Vec<&str> = std::iter::once("_ResourcePack.esl")
        .chain(str_plugins.iter().map(String::as_str)).collect();
    let active: Vec<&str> = text.lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .filter_map(|l| l.strip_prefix('*').map(str::trim))
        .filter(|p| !BASE_PLUGINS[..5].iter().any(|b| p.eq_ignore_ascii_case(b)))
        .collect();
    if active.len() == required.len() && active.iter().zip(&required).all(|(a,b)| a.eq_ignore_ascii_case(b)) {
        Ok(())
    } else {
        Err(format!("Active plugins differ from the recommended set/order. Expected: {}. Found: {}. Press Install to repair required entries; disable additional mods separately.", required.join(", "), active.join(", ")))
    }
}

/// Find `SkyrimTogether*.esp` (or .esm/.esl) plugins in the game's Data dir.
pub fn find_str_plugins(data_dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(data_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("skyrimtogether")
                && (name.ends_with(".esp") || name.ends_with(".esm") || name.ends_with(".esl"))
            {
                out.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    out.sort();
    out
}

/// Candidate profile folders that may contain Plugins.txt, inside a Proton
/// prefix user dir or a Windows user profile dir.
pub fn candidate_profile_dirs(user_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    vec![
        user_dir.join("Documents").join("My Games").join("Skyrim Special Edition"),
        user_dir
            .join("AppData")
            .join("Local")
            .join("Skyrim Special Edition"),
        user_dir
            .join("My Documents")
            .join("My Games")
            .join("Skyrim Special Edition"),
    ]
    .into_iter()
    .filter(|p| p.is_dir())
    .collect()
}

/// Every place a Plugins.txt could live for this game install: the Proton
/// prefix user dir on Linux, the Windows user profile on Windows, nothing on
/// macOS. Shared by the install pipeline and the readiness checklist.
#[allow(unused_variables)]
pub fn candidate_game_profile_dirs(game_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        crate::steam::compat_dir_for(game_dir)
            .map(|compat| candidate_profile_dirs(&compat.join("pfx/drive_c/users/steamuser")))
            .unwrap_or_default()
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .ok()
            .map(|user| candidate_profile_dirs(&std::path::PathBuf::from(user)))
            .unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        Vec::new()
    }
}

/// The Plugins.txt path current Skyrim (1.6.x) reads, even when it does not
/// exist yet: the Proton prefix user dir on Linux, `%LOCALAPPDATA%` on
/// Windows. The legacy `Documents\My Games` locations are only updated,
/// never created.
#[allow(unused_variables)]
pub fn primary_plugins_txt_path(game_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        crate::steam::compat_dir_for(game_dir).map(|compat| {
            plugins_path(&compat.join("pfx/drive_c/users/steamuser/AppData/Local/Skyrim Special Edition"))
        })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .ok()
            .map(|user| std::path::PathBuf::from(user).join("AppData/Local/Skyrim Special Edition/Plugins.txt"))
    }
    #[cfg(target_os = "macos")]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_order_duplicates_and_inactive_entries_idempotently() {
        let required = vec!["_ResourcePack.esl".into(), "SkyrimTogether.esp".into(), "SkyrimTogetherQuestPatches.esp".into()];
        let mut text = "*SkyrimTogetherQuestPatches.esp\nSkyrimTogether.esp\n*_ResourcePack.esl\n*SkyrimTogether.esp\n".to_string();
        assert!(validate_plugins_text(&text, &required[1..]).is_err());
        update_plugins_text(&mut text, &required);
        assert!(validate_plugins_text(&text, &required[1..]).is_ok());
        let once = text.clone();
        update_plugins_text(&mut text, &required);
        assert_eq!(text, once);
        text.push_str("*OtherMod.esp\n");
        assert!(validate_plugins_text(&text, &required[1..]).is_err());
    }

    #[test]
    fn inactive_or_missing_required_plugin_is_not_ready() {
        let required = vec!["SkyrimTogether.esp".into(), "SkyrimTogetherQuestPatches.esp".into()];
        assert!(validate_plugins_text("*_ResourcePack.esl\nSkyrimTogether.esp\n*SkyrimTogetherQuestPatches.esp\n", &required).is_err());
        assert!(validate_plugins_text("*SkyrimTogether.esp\n*SkyrimTogetherQuestPatches.esp\n", &required).is_err());
    }

    #[test]
    fn enables_str_plugin() {
        let mut text = "*Skyrim.esm\nUpdate.esm\nDawnguard.esm\n".to_string();
        update_plugins_text(
            &mut text,
            &["SkyrimTogether.esp".to_string(), "Dawnguard.esm".to_string()],
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "*Skyrim.esm");
        assert_eq!(lines[1], "Update.esm");
        assert_eq!(lines[2], "*SkyrimTogether.esp");
        assert_eq!(lines[3], "*Dawnguard.esm");
    }

    #[test]
    fn base_plugins_load_before_together_esps() {
        let mut text =
            "*Skyrim.esm\n*Update.esm\n*SkyrimTogether.esp\n*SkyrimTogetherQuestPatches.esp\n"
                .to_string();
        update_plugins_text(&mut text, &["_ResourcePack.esl".to_string(), "SkyrimTogether.esp".to_string(), "SkyrimTogetherQuestPatches.esp".to_string()]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[2], "*_ResourcePack.esl");
        assert_eq!(lines[3], "*SkyrimTogether.esp");
        assert_eq!(lines[4], "*SkyrimTogetherQuestPatches.esp");
    }

    #[test]
    fn canonical_text_matches_str_expected_list() {
        let text = canonical_plugins_text(&[
            "SkyrimTogether.esp".to_string(),
            "SkyrimTogetherQuestPatches.esp".to_string(),
        ]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[2], "*Skyrim.esm");
        assert_eq!(lines[3], "*Update.esm");
        assert_eq!(lines[4], "*Dawnguard.esm");
        assert_eq!(lines[5], "*HearthFires.esm");
        assert_eq!(lines[6], "*Dragonborn.esm");
        assert_eq!(lines[7], "*_ResourcePack.esl");
        assert_eq!(lines[8], "*SkyrimTogether.esp");
        assert_eq!(lines[9], "*SkyrimTogetherQuestPatches.esp");
    }
}
