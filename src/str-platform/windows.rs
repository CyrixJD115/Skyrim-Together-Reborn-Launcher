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

/// Write the registry values Skyrim Together uses to locate SkyrimSE.exe, so
/// the "select your SkyrimSE.exe" dialog never appears.
pub fn write_str_registry(game_dir: &Path) -> PlatformResult<()> {
    let exe = game_dir.join("SkyrimSE.exe");
    if !exe.is_file() {
        return Err("SkyrimSE.exe not found in the game folder".into());
    }
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key_path = r"Software\TiltedPhoques\TiltedEvolution\Skyrim Special Edition";
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
pub fn steam_running() -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq steam.exe", "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("steam.exe"),
        Err(_) => false,
    }
}

/// Best-effort start of the Steam client (Skyrim needs it for steam_api).
pub fn ensure_steam_running() {
    if !steam_running() {
        if let Some(root) = steam_roots().first() {
            let _ = Command::new(root.join("steam.exe"))
                .arg("-silent")
                .spawn();
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    }
}
