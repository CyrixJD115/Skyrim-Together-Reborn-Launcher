//! Application directories and small filesystem helpers.

use directories::ProjectDirs;
use std::path::PathBuf;

pub const APP_NAME: &str = "STR Launcher";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub struct AppDirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub downloads: PathBuf,
    pub staging: PathBuf,
    pub cc_backup: PathBuf,
    pub scripts: PathBuf,
    pub logs: PathBuf,
}

impl AppDirs {
    pub fn init() -> Result<AppDirs, String> {
        let dirs = ProjectDirs::from("io", "strlauncher", "STR Launcher")
            .ok_or("could not resolve the user directories")?;
        let config = dirs.config_dir().to_path_buf();
        let data = dirs.data_dir().to_path_buf();
        let downloads = data.join("downloads");
        let staging = data.join("staging");
        let cc_backup = data.join("cc-backup");
        let scripts = data.join("scripts");
        let logs = data.join("logs");
        for d in [&config, &data, &downloads, &staging, &cc_backup, &scripts, &logs] {
            std::fs::create_dir_all(d).map_err(|e| format!("failed to create {}: {e}", d.display()))?;
        }
        Ok(AppDirs { config, data, downloads, staging, cc_backup, scripts, logs })
    }
}

/// Sanitize a relative path coming from an archive or our own manifest:
/// rejects absolute paths, `..` components and empty components.
pub fn safe_rel_path(rel: &str) -> Option<PathBuf> {
    let p = std::path::Path::new(rel);
    if p.is_absolute() {
        return None;
    }
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::Normal(c) => out.push(c),
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn file_size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Write a file via a temp sibling + rename, so readers never see a half-
/// written file (config.json, Plugins.txt, shortcuts.vdf, …).
pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    Ok(())
}

/// Best-effort "open a folder in the file manager".
pub fn open_folder(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// Best-effort "open a URL in the browser".
pub fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}
