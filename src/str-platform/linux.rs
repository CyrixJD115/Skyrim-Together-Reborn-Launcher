//! Linux specifics: Steam roots (native + Flatpak), desktop entries, Steam
//! process management and detached launching.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform::PlatformResult;

/// Candidate Steam client roots (native installs + Flatpak).
pub fn steam_roots() -> Vec<PathBuf> {
    let home = match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return Vec::new(),
    };
    let mut candidates = vec![
        home.join(".local/share/Steam"),
        home.join(".steam/root"),
        home.join(".steam/debian-installation"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
    ];
    if let Ok(root) = std::env::var("STEAM_ROOT") {
        candidates.insert(0, PathBuf::from(root));
    }
    let mut roots = Vec::new();
    for c in candidates {
        if let Ok(real) = std::fs::canonicalize(&c) {
            if real.join("steamapps").is_dir() && !roots.contains(&real) {
                roots.push(real);
            }
        }
    }
    roots
}

pub fn is_steam_running() -> bool {
    // Read /proc directly: no pgrep dependency needed.
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
                if comm.trim() == "steam" {
                    return true;
                }
            }
        }
    }
    false
}

pub fn desktop_entry_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("applications");
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".local/share/applications")
}

pub fn desktop_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        out.push(PathBuf::from(&home).join("Desktop"));
    }
    out.into_iter().filter(|p| p.is_dir()).collect()
}

/// Write an XDG desktop entry.
pub fn write_desktop_entry(
    path: &Path,
    name: &str,
    comment: &str,
    exec: &str,
    icon: &str,
    terminal: bool,
) -> PlatformResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment={comment}\nExec={exec}\n\
         Icon={icon}\nTerminal={}\nCategories=Game;\nStartupNotify=false\n",
        if terminal { "true" } else { "false" }
    );
    std::fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
    let _ = Command::new("update-desktop-database")
        .arg(desktop_entry_dir())
        .output();
    Ok(())
}

/// Spawn a script/program detached from our process so it survives us.
pub fn spawn_detached(program: &Path, args: &[&str], cwd: &Path) -> PlatformResult<()> {
    use std::os::unix::process::CommandExt;
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .process_group(0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch {}: {e}", program.display()))
}

/// Append a section to a Wine registry file (`user.reg`), skipping when the
/// exact values are already present.
pub fn append_user_reg_if_missing(user_reg: &Path, section: &str, values: &[String]) -> PlatformResult<bool> {
    let existing = std::fs::read_to_string(user_reg)
        .map_err(|e| format!("read {}: {e}", user_reg.display()))?;
    if values.iter().all(|v| existing.contains(v)) {
        return Ok(false);
    }
    let mut out = existing;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(section);
    out.push('\n');
    for v in values {
        out.push_str(v);
        out.push('\n');
    }
    std::fs::write(user_reg, out).map_err(|e| format!("write {}: {e}", user_reg.display()))?;
    Ok(true)
}

/// Convert a Linux path to its Wine `Z:` drive form, with VDF-style escaping
/// used inside registry string values (`\\` per separator).
pub fn wine_z_path(path: &Path) -> String {
    let p = path.to_string_lossy();
    format!("Z:{}", p.replace('/', "\\\\"))
}
