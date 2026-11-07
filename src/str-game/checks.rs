//! The Client tab's launch-readiness checklist.
//!
//! Every entry is a fast read-only filesystem probe — no side effects — so it
//! can be recomputed on every detect / install / settings change and shows
//! exactly why a launch would fail instead of failing silently.

use std::path::Path;

use crate::config::LaunchMode;

#[derive(Debug, Clone)]
pub struct Check {
    pub ok: bool,
    pub label: &'static str,
    pub detail: String,
}

impl Check {
    fn new(ok: bool, label: &'static str, detail: impl Into<String>) -> Check {
        Check { ok, label, detail: detail.into() }
    }
}

/// `steam_root`, `proton_tool` and `launch_mode` are only read by the Linux
/// (Proton) checks; they stay on the struct so callers build one `Input`
/// everywhere.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
pub struct Input<'a> {
    pub game_dir: Option<&'a Path>,
    pub game_version: Option<&'a str>,
    pub str_version: Option<&'a str>,
    pub steam_root: Option<&'a Path>,
    pub proton_tool: &'a str,
    pub launch_mode: LaunchMode,
}

pub fn run(input: Input<'_>) -> Vec<Check> {
    let mut checks = Vec::new();
    let dir = input.game_dir;

    // 1. Game folder --------------------------------------------------------
    checks.push(match dir {
        Some(d) => Check::new(true, "Skyrim Special Edition", d.display().to_string()),
        None => Check::new(
            false,
            "Skyrim Special Edition",
            "not detected — install it through Steam or set the folder in Settings",
        ),
    });
    let Some(dir) = dir else {
        return checks;
    };

    // 2. Game build ---------------------------------------------------------
    checks.push(match input.game_version {
        Some(v) => {
            let too_old = v
                .split('.')
                .nth(1)
                .and_then(|m| m.parse::<u64>().ok())
                .map(|minor| minor < 6)
                .unwrap_or(false);
            Check::new(
                !too_old,
                "Game build",
                if too_old {
                    format!("{v} is too old — STR needs 1.6+; let Steam update the game")
                } else {
                    format!("SkyrimSE.exe {v}")
                },
            )
        }
        None => Check::new(false, "Game build", "SkyrimSE.exe version unreadable"),
    });

    // 3. Skyrim Together Reborn --------------------------------------------
    let together_exe = dir.join("SkyrimTogether.exe");
    checks.push(Check::new(
        together_exe.is_file(),
        "Skyrim Together Reborn",
        match input.str_version {
            Some(v) if together_exe.is_file() => format!("v{v} installed"),
            _ => "not installed — press Install".to_string(),
        },
    ));

    // 4. Address Library ----------------------------------------------------
    let al_dll = find_address_library(dir);
    checks.push(Check::new(
        al_dll.is_some(),
        "Address Library",
        al_dll
            .clone()
            .unwrap_or_else(|| {
                "Data/SKSE/Plugins/version*(lib)*-*.dll/.bin missing — press Install".into()
            }),
    ));

    // 5. Plugins.txt --------------------------------------------------------
    let (plugins_ok, plugins_detail) = check_plugins(dir);
    checks.push(Check::new(plugins_ok, "Plugins.txt", plugins_detail));

    // Platform-specific checks ----------------------------------------------
    #[cfg(all(unix, not(target_os = "macos")))]
    linux_checks(dir, &mut checks, &input);

    #[cfg(target_os = "windows")]
    windows_checks(&mut checks);

    checks
}

/// The newest Address Library file (All in One) in `Data/SKSE/Plugins`.
///
/// Two naming schemes exist in the wild, and both are legitimate:
/// - classic DLL drops: `version-1-6-1170-0.dll`
/// - the current All-in-one archives: `version-1-5-97-0.bin` and
///   `versionlib-1-6-1170-0.bin` (`.bin` since SKSE 2.2.x / runtime 1.6.629+,
///   covering the 1.7.x runtimes too).
fn find_address_library(game_dir: &Path) -> Option<String> {
    let mut best: Option<(String, [u64; 4])> = None;
    let plugins = game_dir.join("Data/SKSE/Plugins");
    for entry in std::fs::read_dir(&plugins).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some((version, key)) = address_library_version(&name) else {
            continue;
        };
        if best.as_ref().map(|b| key > b.1).unwrap_or(true) {
            best = Some((format!("Data/SKSE/Plugins/{name} ({version})"), key));
        }
    }
    best.map(|(name, _)| name)
}

/// Parse an Address Library file name into its `1.6.1170.0`-style version
/// label and a numeric sort key. Returns `None` for files we don't manage.
fn address_library_version(name: &str) -> Option<(String, [u64; 4])> {
    let lower = name.to_lowercase();
    let stem = lower
        .strip_prefix("version-")
        .or_else(|| lower.strip_prefix("versionlib-"))?;
    let stem = stem.strip_suffix(".dll").or_else(|| stem.strip_suffix(".bin"))?;
    let parts: Vec<u64> = stem.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.is_empty() {
        return None;
    }
    let key: [u64; 4] = [
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
        parts.get(3).copied().unwrap_or(0),
    ];
    Some((parts.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("."), key))
}

/// Any known Plugins.txt location lists SkyrimTogether.esp?
///
/// Skyrim Together 1.8+ ships `SkyrimTogether.esp` and
/// `SkyrimTogetherQuestPatches.esp`; older archives ship none and inject at
/// runtime. So a missing entry only matters when a `SkyrimTogether*` plugin
/// actually exists in the game's Data folder.
fn check_plugins(game_dir: &Path) -> (bool, String) {
    let str_plugins = crate::plugins::find_str_plugins(&game_dir.join("Data"));
    let mut locations = Vec::new();
    let mut enabled = 0usize;
    let mut found_any = false;

    for profile in crate::plugins::candidate_game_profile_dirs(game_dir) {
        let plugins_txt = profile.join("Plugins.txt");
        if !plugins_txt.is_file() {
            continue;
        }
        found_any = true;
        locations.push(plugins_txt.display().to_string());
        if let Ok(text) = std::fs::read_to_string(&plugins_txt) {
            if text
                .lines()
                .any(|l| l.trim_start_matches('*').trim().to_lowercase().starts_with("skyrimtogether"))
            {
                enabled += 1;
            }
        }
    }

    if str_plugins.is_empty() {
        (
            true,
            "this Skyrim Together version ships no .esp plugin — no Plugins.txt entry needed"
                .to_string(),
        )
    } else if !found_any {
        (
            false,
            "no Plugins.txt yet — press Install to create one".to_string(),
        )
    } else if enabled > 0 {
        (true, format!("SkyrimTogether.esp enabled in {enabled} location(s)"))
    } else {
        (false, format!("SkyrimTogether.esp missing in {} — press Install", locations.join(", ")))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_checks(dir: &Path, checks: &mut Vec<Check>, input: &Input<'_>) {
    // Proton tool — scope to the detected Steam root when we have one.
    let mut roots = crate::steam::steam_roots();
    if let Some(root) = input.steam_root {
        roots.insert(0, root.to_path_buf());
    }
    let tool = crate::proton::find_tool(&roots, input.proton_tool);
    // STR 1.8 closes instantly on Wine 11 (Proton 11 / Experimental); GE-Proton
    // 10 is the tested family, so anything else fails this check with the fix.
    let is_ge10 = input.proton_tool.starts_with("GE-Proton10");
    checks.push(Check::new(
        tool.is_some() && is_ge10,
        "Proton tool",
        match &tool {
            Some(t) if is_ge10 => format!("{} ({})", t.name, t.path.display()),
            Some(t) => format!(
                "{} is installed and selected, but STR 1.8 closes instantly on Wine 11 builds \
                 (Proton 11 / Experimental). Pick GE-Proton10 in Settings — or press Install, \
                 which selects it automatically.",
                t.name
            ),
            None => format!(
                "{name} not found — press Install to download it into Steam",
                name = input.proton_tool
            ),
        },
    ));

    // Prefix + STR registry keys.
    let compat = crate::steam::compat_dir_for(dir);
    let user_reg = compat.as_ref().map(|c| c.join("pfx/user.reg"));
    let reg_ok = user_reg
        .as_ref()
        .map(|p| p.is_file() && std::fs::read_to_string(p).is_ok_and(|t| t.contains("TitlePath")))
        .unwrap_or(false);
    checks.push(Check::new(
        reg_ok,
        "Proton prefix",
        if reg_ok {
            "prefix exists and STR's registry keys are set".to_string()
        } else {
            "missing — run Skyrim once through Steam, then press Install again".to_string()
        },
    ));

    // Launch integration, per mode.
    use crate::config::LaunchMode;
    match input.launch_mode {
        LaunchMode::LauncherReplacement => {
            let launcher = dir.join("SkyrimSELauncher.exe");
            let is_link = std::fs::symlink_metadata(&launcher)
                .map(|m| m.is_symlink())
                .unwrap_or(false);
            checks.push(Check::new(
                is_link,
                "Launch mode — replace exe",
                if is_link {
                    "SkyrimSELauncher.exe → SkyrimTogether.exe (launching Skyrim in Steam starts multiplayer)"
                        .to_string()
                } else {
                    "launcher not replaced — press Install to activate"
                        .to_string()
                },
            ));
        }
        LaunchMode::SteamGame => match crate::steam_entry::find_entry() {
            Some((_, exe, app_id)) => checks.push(Check::new(
                true,
                "Launch mode — Steam game",
                format!("Steam entry app id {} ({})", app_id & 0x7fff_ffff, exe),
            )),
            None => checks.push(Check::new(
                false,
                "Launch mode — Steam game",
                "no \"Skyrim Together Reborn\" entry in Steam yet — press Install to create it",
            )),
        },
    }
}

#[cfg(target_os = "windows")]
fn windows_checks(checks: &mut Vec<Check>) {
    checks.push(Check::new(
        true,
        "Launch mode",
        "SkyrimTogether.exe is started directly; Steam link not needed on Windows".to_string(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_address_library_naming_schemes() {
        // Classic DLL drops.
        let (v, k) = address_library_version("version-1-6-1170-0.dll").unwrap();
        assert_eq!(v, "1.6.1170.0");
        assert_eq!(k, [1, 6, 1170, 0]);
        // Current All-in-one .bin layout (SE 1.5.x legacy + versionlib).
        let (_, k) = address_library_version("version-1-5-97-0.bin").unwrap();
        assert_eq!(k, [1, 5, 97, 0]);
        let (v, k) = address_library_version("versionlib-1-7-104-0.bin").unwrap();
        assert_eq!(v, "1.7.104.0");
        assert_eq!(k, [1, 7, 104, 0]);
        // Case-insensitive extension.
        assert!(address_library_version("versionlib-1-6-640-0.BIN").is_some());
        // Not our files.
        assert!(address_library_version("SkyrimTogether.dll").is_none());
        assert!(address_library_version("versionlib-1-6-640-0.bak").is_none());
        assert!(address_library_version("readme.txt").is_none());
    }

    #[test]
    fn newest_version_wins_regardless_of_extension() {
        let tmp = std::env::temp_dir().join(format!("str-al-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let plugins = tmp.join("Data/SKSE/Plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        // Regression: a .bin-only install (current All-in-one archives, e.g.
        // for runtime 1.7.x) must be detected — the old code only saw .dll.
        std::fs::write(plugins.join("versionlib-1-7-104-0.bin"), b"x").unwrap();
        let found = find_address_library(&tmp).unwrap();
        assert!(found.contains("1.7.104"), "got: {found}");
        // A newer classic DLL beats it.
        std::fs::write(plugins.join("version-1-6-1170-0.dll"), b"x").unwrap();
        std::fs::write(plugins.join("versionlib-1-6-1180-0.bin"), b"x").unwrap();
        let found = find_address_library(&tmp).unwrap();
        assert!(found.contains("1.7.104"), "got: {found}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
