//! `Plugins.txt` handling.
//!
//! Skyrim Together Reborn ships `SkyrimTogether.esp` which must be active.
//! We only touch files that already exist (a missing `Plugins.txt` means the
//! game has not been run yet or will auto-load everything), and we mirror the
//! reference behaviour: any entry matching a `SkyrimTogether*` plugin is forced
//! to be active (`*` prefix), appending it if missing.
//!
//! Recent game versions read the list from `%LOCALAPPDATA%\Skyrim Special
//! Edition`, older ones from `Documents\My Games\Skyrim Special Edition`, so
//! every existing candidate location is updated.

/// Update the text of a Plugins.txt so all `str_plugins` are active.
pub fn update_plugins_text(text: &mut String, str_plugins: &[String]) {
    let mut lines: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).collect();
    for plugin in str_plugins {
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
            lines.push(format!("*{plugin}"));
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
}
