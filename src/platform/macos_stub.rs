//! macOS is not a supported target (Skyrim Together Reborn requires the Steam
//! Windows build through Proton), but the code stays portable.

use std::path::PathBuf;

use crate::platform::PlatformResult;

pub fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join("Library/Application Support/Steam");
        if p.join("steamapps").is_dir() {
            roots.push(p);
        }
    }
    roots
}

pub fn write_str_registry(_game_dir: &PathBuf) -> PlatformResult<()> {
    Err("Not supported on macOS".into())
}

pub fn create_shortcuts(_game_dir: &std::path::Path) -> PlatformResult<Vec<String>> {
    Err("Not supported on macOS".into())
}

pub fn remove_shortcuts(_game_dir: &std::path::Path) {}

pub fn spawn_detached(
    program: &std::path::Path,
    args: &[&str],
    cwd: &std::path::Path,
) -> PlatformResult<()> {
    std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch {}: {e}", program.display()))
}

pub fn ensure_steam_running() {}
