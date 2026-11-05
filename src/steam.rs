//! Steam library scanning and Skyrim Special Edition discovery.
//!
//! Works identically on Windows and Linux: read `libraryfolders.vdf` from each
//! known Steam root, then look for `appmanifest_72850.acf`… except Skyrim
//! Special Edition's app id is **489830** (72850 is classic Skyrim).

use std::path::{Path, PathBuf};

use crate::vdf;
use crate::vdf::Vdf;

pub const SKYRIM_APPID: &str = "489830";
pub const DEFAULT_PROTON_TOOL: &str = "GE-Proton10-34";

#[derive(Debug, Clone, serde::Serialize)]
pub struct GameInstall {
    pub game_dir: PathBuf,
    pub library: PathBuf,
    pub steam_root: PathBuf,
    /// Linux: `<library>/steamapps/compatdata/489830`, if present.
    pub compat_dir: Option<PathBuf>,
    pub build_id: Option<String>,
}


/// All plausible Steam roots for this platform.
pub fn steam_roots() -> Vec<PathBuf> {
    crate::platform::native::steam_roots()
}

/// Parse a `libraryfolders.vdf` and return every library path it declares.
/// Handles the modern shape (`"libraryfolders" { "0" { "path" ... } }`) and
/// the legacy shapes: numbered entries at the top level and/or the path given
/// as a plain value instead of a `"path"` key.
fn parse_library_folders(text: &str) -> Vec<PathBuf> {
    let Ok(tree) = vdf::parse(text) else {
        return Vec::new();
    };
    let folders = match tree.map() {
        Some(map) => map
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("libraryfolders"))
            .map(|(_, v)| v)
            .unwrap_or(&tree),
        None => &tree,
    };
    let Some(map) = folders.map() else {
        return Vec::new();
    };
    let mut libs = Vec::new();
    for (_, entry) in map {
        let path = match entry {
            // Legacy: "1"  "/mnt/games/SteamLibrary"
            Vdf::Str(path) => Some(path.as_str()),
            // Modern: "1" { "path" "/mnt/games/SteamLibrary" ... }
            Vdf::Map(_) => entry.get("path").and_then(|p| p.as_str()),
        };
        if let Some(path) = path {
            let p = PathBuf::from(path);
            if !p.as_os_str().is_empty() && !libs.contains(&p) {
                libs.push(p);
            }
        }
    }
    libs
}

fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.to_path_buf()];
    for vdf_path in [
        root.join("steamapps/libraryfolders.vdf"),
        root.join("config/libraryfolders.vdf"),
    ] {
        if let Ok(text) = std::fs::read_to_string(&vdf_path) {
            for lib in parse_library_folders(&text) {
                if !libs.contains(&lib) {
                    libs.push(lib);
                }
            }
        }
    }
    libs
}

/// Locate every Skyrim Special Edition install across all Steam roots.
pub fn find_installs(roots: &[PathBuf]) -> Vec<GameInstall> {
    let mut out: Vec<GameInstall> = Vec::new();
    for root in roots {
        for lib in library_folders(root) {
            if let Some(install) = inspect_library(&lib, root) {
                if !out.iter().any(|i| i.game_dir == install.game_dir) {
                    out.push(install);
                }
            }
        }
    }
    // Supplement: libraries on other drives may not be in any VDF (drive
    // mounted after Steam started, non-default setups). Scan the usual mount
    // points so installs under /mnt, /media or /run/media are still found.
    for lib in scan_mount_bases(&mount_scan_bases()) {
        if let Some(install) = inspect_library(&lib, &root_fallback(&lib, roots)) {
            if !out.iter().any(|i| i.game_dir == install.game_dir) {
                out.push(install);
            }
        }
    }
    out
}

fn inspect_library(lib: &Path, steam_root: &Path) -> Option<GameInstall> {
    let steamapps = lib.join("steamapps");
    let manifest = steamapps.join(format!("appmanifest_{SKYRIM_APPID}.acf"));
    let text = std::fs::read_to_string(&manifest).ok()?;
    let parsed = vdf::parse(&text).ok();
    let installdir = parsed
        .as_ref()
        .and_then(|t| {
            t.get("AppState")
                .and_then(|s| s.get("installdir"))
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| "Skyrim Special Edition".to_string());
    let game_dir = steamapps.join("common").join(&installdir);
    if !game_dir.join("SkyrimSE.exe").is_file() {
        return None;
    }
    let build_id = parsed.as_ref().and_then(|t| {
        t.get("AppState")
            .and_then(|s| s.get("buildid"))
            .and_then(|v| v.as_str())
            .map(String::from)
    });
    let compat_dir = {
        let c = steamapps.join("compatdata").join(SKYRIM_APPID);
        c.is_dir().then_some(c)
    };
    Some(GameInstall {
        game_dir,
        library: lib.to_path_buf(),
        steam_root: steam_root.to_path_buf(),
        compat_dir,
        build_id,
    })
}

/// Mount-point bases scanned for Steam libraries that no VDF mentions.
pub fn mount_scan_bases() -> Vec<PathBuf> {
    let mut bases = vec![PathBuf::from("/mnt"), PathBuf::from("/media")];
    if let Ok(user) = std::env::var("USER") {
        if !user.is_empty() {
            bases.push(PathBuf::from("/run/media").join(&user));
            bases.push(PathBuf::from("/media").join(&user));
        }
    }
    bases.push(PathBuf::from("/run/media"));
    bases
}

/// Scan common removable/secondary-drive locations for Steam libraries:
/// `<base>/<drive>/steamapps`, `<base>/<drive>/Steam/steamapps` and
/// `<base>/<drive>/SteamLibrary/steamapps` (two levels deep).
pub fn scan_mount_bases(bases: &[PathBuf]) -> Vec<PathBuf> {
    let mut libs = Vec::new();
    for base in bases {
        let Ok(drives) = std::fs::read_dir(base) else {
            continue;
        };
        for drive in drives.flatten() {
            let root = drive.path();
            if !root.is_dir() {
                continue;
            }
            for candidate in [
                root.clone(),
                root.join("Steam"),
                root.join("SteamLibrary"),
                root.join("steamapps").join("..").join("SteamLibrary"),
            ] {
                if candidate.join("steamapps/appmanifest_489830.acf").is_file()
                    && !libs.contains(&candidate)
                {
                    libs.push(candidate);
                }
            }
        }
    }
    libs
}

/// Pick the most plausible Steam root to associate with a library found by
/// scanning: a root whose config.vdf exists, else the first known root.
fn root_fallback(lib: &Path, roots: &[PathBuf]) -> PathBuf {
    roots
        .iter()
        .find(|r| r.join("config/config.vdf").is_file())
        .or_else(|| roots.first())
        .cloned()
        .unwrap_or_else(|| lib.to_path_buf())
}

/// Convenience: detect all installs using platform roots.
pub fn detect() -> Vec<GameInstall> {
    let roots = steam_roots();
    find_installs(&roots)
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
/// The Proton compatdata dir for a game folder, following the standard
/// `<library>/steamapps/compatdata/<appid>` layout (a sibling of `common`).
pub fn compat_dir_for(game_dir: &Path) -> Option<PathBuf> {
    let steamapps = game_dir.parent()?.parent()?; // common -> steamapps
    if steamapps.file_name()? != "steamapps" {
        return None;
    }
    Some(steamapps.join("compatdata").join(SKYRIM_APPID))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_dir_layout() {
        let game = Path::new("/home/x/.local/share/Steam/steamapps/common/Skyrim Special Edition");
        let compat = compat_dir_for(game).unwrap();
        assert_eq!(
            compat,
            Path::new("/home/x/.local/share/Steam/steamapps/compatdata/489830")
        );
        assert!(compat_dir_for(Path::new("/games/Skyrim")).is_none());
    }

    #[test]
    fn parses_realistic_library_folders() {
        // Mirrors a real libraryfolders.vdf (label/contentid/... keys, multi
        // libraries incl. a secondary drive) — regression for the descend bug.
        let text = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/user/.local/share/Steam"
		"label"		""
		"contentid"		"7358716539485018772"
		"totalsize"		"0"
		"apps"
		{
			"228980"		"324088518"
		}
	}
	"1"
	{
		"path"		"/mnt/gaming/Steam"
		"label"		""
		"contentid"		"71429212898253548"
		"apps"
		{
			"489830"		"16092533099"
		}
	}
	"2"
	{
		"path"		"/mnt/gamingex/Steam"
		"label"		""
		"apps"
		{
			"72850"		"999"
		}
	}
}
"#;
        let libs = parse_library_folders(text);
        assert_eq!(libs.len(), 3);
        assert!(libs.contains(&PathBuf::from("/mnt/gaming/Steam")));
        assert!(libs.contains(&PathBuf::from("/mnt/gamingex/Steam")));
    }

    #[test]
    fn parses_legacy_flat_library_folders() {
        let text = r#"
"LibraryFolders"
{
	"1"		"/mnt/games/SteamLibrary"
	"2"		"/mnt/more/Library"
}
"#;
        let libs = parse_library_folders(text);
        assert!(libs.contains(&PathBuf::from("/mnt/games/SteamLibrary")));
        assert!(libs.contains(&PathBuf::from("/mnt/more/Library")));
    }

    #[test]
    fn scans_mount_bases_for_libraries() {
        let tmp = std::env::temp_dir().join(format!("str-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // <base>/<drive>/Steam/steamapps/common/Skyrim Special Edition
        let game = tmp.join("gaming/Steam/steamapps/common/Skyrim Special Edition");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("SkyrimSE.exe"), b"x").unwrap();
        std::fs::write(
            tmp.join("gaming/Steam/steamapps/appmanifest_489830.acf"),
            "\"AppState\"\n{\n\t\"installdir\"\t\t\"Skyrim Special Edition\"\n\t\"buildid\"\t\t\"24914197\"\n}\n",
        )
        .unwrap();

        let libs = scan_mount_bases(&[tmp.clone()]);
        assert_eq!(libs, vec![tmp.join("gaming/Steam")]);

        let install = inspect_library(&libs[0], &tmp).unwrap();
        assert_eq!(install.game_dir, game);
        assert_eq!(install.build_id.as_deref(), Some("24914197"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod env_tests {
    use super::*;

    /// Environment-dependent check: run with
    /// `cargo test detect_finds_local_skyrim -- --ignored --nocapture`
    /// on a machine that has Skyrim installed through Steam.
    #[test]
    #[ignore = "run manually on a machine with Steam + Skyrim installed"]
    fn detect_finds_local_skyrim() {
        let installs = detect();
        eprintln!("{installs:#?}");
        assert!(!installs.is_empty(), "no Skyrim install detected on this machine");
        for i in &installs {
            assert!(i.game_dir.join("SkyrimSE.exe").is_file());
        }
        // The version resource of the real SkyrimSE.exe must parse (this
        // machine's build is 1.7.104.0).
        let version = crate::pe_version::file_version(
            &installs[0].game_dir.join("SkyrimSE.exe"),
        );
        eprintln!("real SkyrimSE.exe version: {version:?}");
        assert!(version.is_some(), "PE version parse failed on the real exe");
    }
}
