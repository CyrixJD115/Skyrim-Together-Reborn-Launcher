//! Store-front install discovery beyond Steam: Epic Games Launcher manifests
//! and Heroic's legendary/gogdl/nile `installed.json` books. GOG Galaxy is
//! handled through its registry keys on Windows (see `platform::windows`).
//!
//! The parsing helpers here are pure (dir-in, dirs-out) so they are tested on
//! every platform; only the Windows side wires them to the real filesystem
//! locations, hence the dead-code allowance elsewhere.

#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::path::{Path, PathBuf};

/// Does `dir` contain the Windows build of Skyrim Special Edition?
pub fn is_skyrim_dir(dir: &Path) -> bool {
    dir.join("SkyrimSE.exe").is_file()
}

/// Walk a JSON value and collect every string under one of `keys`
/// (case-insensitive), at any depth — Epic and Heroic stores keep reshuffling
/// their schemas, and this accepts every shape we have seen.
fn collect_string_fields(value: &serde_json::Value, keys: &[&str], out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if keys.iter().any(|key| k.eq_ignore_ascii_case(key)) {
                    if let Some(s) = v.as_str() {
                        if !s.is_empty() {
                            out.push(s.to_string());
                        }
                    }
                }
                collect_string_fields(v, keys, out);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_string_fields(item, keys, out);
            }
        }
        _ => {}
    }
}

/// Dedup + validate candidate install dirs: keep only real directories that
/// contain SkyrimSE.exe.
fn valid_skyrim_dirs(candidates: Vec<String>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        let dir = PathBuf::from(candidate);
        if is_skyrim_dir(&dir) && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// Epic Games Launcher: every `.item` manifest in
/// `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests` carries an
/// `InstallLocation`.
pub fn epic_manifests(manifests_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(manifests_dir) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("item") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                collect_string_fields(&value, &["InstallLocation"], &mut candidates);
            }
        }
    }
    valid_skyrim_dirs(candidates)
}

/// Heroic Games Launcher: `installed.json` books written by legendary, gogdl
/// and nile under Heroic's config dirs. Every entry points at an install
/// path, whatever the surrounding schema looks like.
pub fn heroic_installed(config_dir: &Path) -> Vec<PathBuf> {
    const BOOKS: [&str; 3] = [
        "legendary/installed.json",
        "gogdl/installed.json",
        "nile/installed.json",
    ];
    let mut candidates = Vec::new();
    for book in BOOKS {
        let path = config_dir.join(book);
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                collect_string_fields(&value, &["install_path", "InstallLocation"], &mut candidates);
            }
        }
    }
    valid_skyrim_dirs(candidates)
}

/// Common fallback locations people install Skyrim into outside any manager
/// (GOG's classic offline installers, manual copies).
pub fn common_dirs(home: &Path) -> Vec<PathBuf> {
    let candidates = [
        home.join("GOG Games/Skyrim Special Edition"),
        home.join("Games/Skyrim Special Edition"),
        PathBuf::from(r"C:\Games\Skyrim Special Edition"),
        PathBuf::from(r"C:\GOG Games\Skyrim Special Edition"),
    ];
    let strings: Vec<String> = candidates
        .into_iter()
        .map(|d| d.to_string_lossy().to_string())
        .collect();
    valid_skyrim_dirs(strings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("str-stores-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn skyrim_install_under(base: &Path, name: &str) -> PathBuf {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SkyrimSE.exe"), b"mz").unwrap();
        dir
    }

    #[test]
    fn epic_manifests_are_parsed_and_validated() {
        let base = tmp("epic");
        let manifests = base.join("Manifests");
        std::fs::create_dir_all(&manifests).unwrap();
        let game = skyrim_install_under(&base, "SSE");

        // A Skyrim manifest plus unrelated ones (non-Skyrim + malformed).
        std::fs::write(
            manifests.join("skyrim.item"),
            format!(
                r#"{{"InstallLocation": "{}", "DisplayName": "Skyrim Special Edition", "LaunchExecutable": "SkyrimSE.exe"}}"#,
                game.display()
            ),
        )
        .unwrap();
        std::fs::write(
            manifests.join("other.item"),
            r#"{"InstallLocation": "C:/SomethingElse", "DisplayName": "Other"}"#,
        )
        .unwrap();
        std::fs::write(manifests.join("broken.item"), "not json at all").unwrap();
        // A non-.item file must be ignored even if it parses.
        std::fs::write(manifests.join("notitem.json"), r#"{"InstallLocation": "C:/x"}"#).unwrap();

        let found = epic_manifests(&manifests);
        assert_eq!(found, vec![game]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn heroic_books_parse_in_every_schema_shape() {
        let base = tmp("heroic");
        let cfg = base.join("heroic");
        std::fs::create_dir_all(cfg.join("legendary")).unwrap();
        std::fs::create_dir_all(cfg.join("gogdl")).unwrap();
        let game1 = skyrim_install_under(&base, "legendary-game");
        let game2 = skyrim_install_under(&base, "gogdl-game");

        // legendary: map keyed by app name.
        std::fs::write(
            cfg.join("legendary/installed.json"),
            format!(r#"{{"appname": {{"install_path": "{}", "version": "1"}}}}"#, game1.display()),
        )
        .unwrap();
        // gogdl: {"installed": [...]} list shape.
        std::fs::write(
            cfg.join("gogdl/installed.json"),
            format!(r#"{{"installed": [{{"install_path": "{}", "appName": "1715"}}], "dlc": []}}"#, game2.display()),
        )
        .unwrap();

        let mut found = heroic_installed(&cfg);
        found.sort();
        let mut expected = vec![game1, game2];
        expected.sort();
        assert_eq!(found, expected);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn non_skyrim_locations_are_filtered_out() {
        let base = tmp("filter");
        let manifests = base.join("Manifests");
        std::fs::create_dir_all(&manifests).unwrap();
        // InstallLocation that exists but is not a Skyrim dir.
        std::fs::create_dir_all(base.join("not-skyrim")).unwrap();
        std::fs::write(
            manifests.join("x.item"),
            format!(r#"{{"InstallLocation": "{}"}}"#, base.join("not-skyrim").display()),
        )
        .unwrap();
        assert!(epic_manifests(&manifests).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn common_dirs_only_return_existing_skyrim_folders() {
        let base = tmp("common");
        let game = skyrim_install_under(&base, "GOG Games/Skyrim Special Edition");
        let found = common_dirs(&base);
        assert_eq!(found, vec![game]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
