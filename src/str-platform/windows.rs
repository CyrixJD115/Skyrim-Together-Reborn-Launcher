//! Windows specifics: Steam discovery through the registry, STR's registry
//! keys, Start Menu / desktop shortcuts and detached process launching.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform::PlatformResult;

/// `CREATE_NO_WINDOW`: keep console children (tasklist, cmd, …) from flashing
/// a terminal window now that the app itself is a GUI-subsystem binary.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Steam install roots discovered through the Windows registry.
pub fn steam_roots() -> Vec<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let mut roots = Vec::new();
    for (hive, sub) in [
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam"),
        (HKEY_CURRENT_USER, r"Software\Valve\Steam"),
    ] {
        if let Ok(key) = winreg::RegKey::predef(hive).open_subkey(sub) {
            if let Ok(path) = key.get_value::<String, _>("InstallPath") {
                let p = PathBuf::from(path);
                if !roots.contains(&p) && p.join("steam.exe").is_file() {
                    roots.push(p);
                }
            }
        }
    }
    roots
}

/// Non-Steam Skyrim installs: GOG Galaxy (registry), Epic manifests, Heroic
/// books and the usual manual-install folders.
pub fn extra_game_dirs(steam_roots: &[PathBuf]) -> Vec<PathBuf> {
    let _ = steam_roots;
    let mut out = Vec::new();

    // GOG Galaxy: every game registers under GOG.com\Games with a "path".
    for (hive, sub) in [
        (winreg::enums::HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\GOG.com\Games"),
        (winreg::enums::HKEY_LOCAL_MACHINE, r"SOFTWARE\GOG.com\Games"),
    ] {
        if let Ok(games) = winreg::RegKey::predef(hive).open_subkey(sub) {
            for key in games.enum_keys().flatten() {
                if let Ok(game) = games.open_subkey(&key) {
                    if let Ok(path) = game.get_value::<String, _>("path") {
                        let dir = PathBuf::from(path);
                        if crate::other_stores::is_skyrim_dir(&dir) && !out.contains(&dir) {
                            out.push(dir);
                        }
                    }
                }
            }
        }
    }

    // Epic Games Launcher manifests.
    if let Ok(programdata) = std::env::var("PROGRAMDATA") {
        for dir in crate::other_stores::epic_manifests(&PathBuf::from(&programdata)
            .join("Epic/EpicGamesLauncher/Data/Manifests")) {
            if !out.contains(&dir) {
                out.push(dir);
            }
        }
    }

    // Heroic (legendary / gogdl / nile books) under %APPDATA%.
    if let Ok(appdata) = std::env::var("APPDATA") {
        for dir in crate::other_stores::heroic_installed(&PathBuf::from(&appdata)) {
            if !out.contains(&dir) {
                out.push(dir);
            }
        }
    }

    // Manual installs in the usual spots.
    let home = std::env::var("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default();
    for dir in crate::other_stores::common_dirs(&home) {
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// The HKCU key STR reads TitleExe/TitlePath from.
pub const STR_REGISTRY_KEY: &str = r"Software\TiltedPhoques\TiltedEvolution\Skyrim Special Edition";

/// Write the registry values Skyrim Together uses to locate SkyrimSE.exe, so
/// the "select your SkyrimSE.exe" dialog never appears.
pub fn write_str_registry(game_dir: &Path) -> PlatformResult<()> {
    let exe = game_dir.join("SkyrimSE.exe");
    if !exe.is_file() {
        return Err("SkyrimSE.exe not found in the game folder".into());
    }
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key_path = STR_REGISTRY_KEY;
    let (key, _) = hkcu
        .create_subkey(key_path)
        .map_err(|e| format!("failed to create registry key {key_path}: {e}"))?;
    let exe_str = exe.to_string_lossy().to_string();
    let dir_str = game_dir.to_string_lossy().to_string();
    key.set_value("TitleExe", &exe_str)
        .map_err(|e| format!("failed to write TitleExe: {e}"))?;
    key.set_value("TitlePath", &dir_str)
        .map_err(|e| format!("failed to write TitlePath: {e}"))?;
    Ok(())
}

/// The .lnk locations older builds used for their "Skyrim Together Reborn"
/// shortcuts. Install no longer creates shortcuts (the STR Launcher tool is
/// the way in), but Undo still cleans these up for previous installs.
fn shortcut_paths(game_dir: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut out = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        out.push((
            PathBuf::from(appdata)
                .join("Microsoft/Windows/Start Menu/Programs")
                .join("Skyrim Together Reborn.lnk"),
            "Start Menu",
        ));
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        out.push((
            PathBuf::from(userprofile)
                .join("Desktop")
                .join("Skyrim Together Reborn.lnk"),
            "Desktop",
        ));
    }
    let _ = game_dir;
    out
}

pub fn remove_shortcuts(game_dir: &Path) {
    for (lnk, _) in shortcut_paths(game_dir) {
        let _ = std::fs::remove_file(lnk);
    }
}

/// Launch a program detached from our process.
pub fn spawn_detached(program: &Path, args: &[&str], cwd: &Path) -> PlatformResult<()> {
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch {}: {e}", program.display()))
}

/// Is the Steam client running?
pub fn is_steam_running() -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq steam.exe", "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("steam.exe"),
        Err(_) => false,
    }
}

/// Ask Steam to shut down cleanly (it flushes shortcuts.vdf and config.vdf
/// on exit, so edits require a stopped client).
pub fn shutdown_steam() {
    if let Some(root) = steam_roots().first() {
        let _ = Command::new(root.join("steam.exe"))
            .arg("-shutdown")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

/// Start Steam again.
pub fn launch_steam() {
    if let Some(root) = steam_roots().first() {
        let _ = Command::new(root.join("steam.exe"))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

/// Delete the registry key STR wrote (`TitleExe`/`TitlePath`). Missing key is
/// success (nothing to undo).
pub fn remove_str_registry() -> PlatformResult<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    let key_path = STR_REGISTRY_KEY;
    let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
    let exists = hkcu.open_subkey(key_path).is_ok();
    if !exists {
        return Ok(());
    }
    hkcu.delete_subkey(key_path)
        .map_err(|e| format!("failed to delete registry key {key_path}: {e}"))
}

/// Best-effort start of the Steam client (Skyrim needs it for steam_api).
pub fn ensure_steam_running() {
    if !is_steam_running() {
        if let Some(root) = steam_roots().first() {
            let _ = Command::new(root.join("steam.exe"))
                .arg("-silent")
                .spawn();
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    }
}
