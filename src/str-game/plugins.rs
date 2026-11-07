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

/// Update the text of a Plugins.txt so all `ensure_active` plugins are
/// active: an existing line for one is starred if it isn't yet, and missing
/// ones are added — base plugins go in before the first `SkyrimTogether*`
/// line (STR's client requires `_ResourcePack.esl` to load before the esps),
/// Together plugins at the end.
pub fn update_plugins_text(text: &mut String, ensure_active: &[String]) {
    let mut lines: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).collect();
    for plugin in ensure_active {
        let matched = lines.iter_mut().any(|line| {
            if line.lstrip_star().eq_ignore_ascii_case(plugin) {
                if !line.starts_with('*') {
                    *line = format!("*{line}");
                }
                true
            } else {
                false
            }
        });
        if !matched {
            let new_line = format!("*{plugin}");
            if plugin.to_lowercase().starts_with("skyrimtogether") {
                lines.push(new_line);
            } else {
                let at = lines
                    .iter()
                    .position(|l| l.lstrip_star().to_lowercase().starts_with("skyrimtogether"))
                    .unwrap_or(lines.len());
                lines.insert(at, new_line);
            }
        }
    }
    text.clear();
    text.push_str(&lines.join("\n"));
    text.push('\n');
}

trait StripStar {
    fn lstrip_star(&self) -> &str;
}

impl StripStar for String {
    fn lstrip_star(&self) -> &str {
        self.strip_prefix('*').unwrap_or(self).trim()
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
            compat.join("pfx/drive_c/users/steamuser/AppData/Local/Skyrim Special Edition/Plugins.txt")
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
    fn enables_str_plugin() {
        let mut text = "*Skyrim.esm\nUpdate.esm\nDawnguard.esm\n".to_string();
        update_plugins_text(
            &mut text,
            &["SkyrimTogether.esp".to_string(), "Dawnguard.esm".to_string()],
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "*Skyrim.esm");
        assert_eq!(lines[1], "Update.esm");
        assert_eq!(lines[2], "*Dawnguard.esm");
        assert_eq!(lines[3], "*SkyrimTogether.esp");
    }

    #[test]
    fn base_plugins_load_before_together_esps() {
        let mut text =
            "*Skyrim.esm\n*Update.esm\n*SkyrimTogether.esp\n*SkyrimTogetherQuestPatches.esp\n"
                .to_string();
        update_plugins_text(&mut text, &["_ResourcePack.esl".to_string()]);
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
