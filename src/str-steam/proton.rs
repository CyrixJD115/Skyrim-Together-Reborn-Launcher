//! Proton / GE-Proton management for Linux.
#![cfg_attr(target_os = "windows", allow(dead_code))]
//!
//! Skyrim Together Reborn needs a modern Proton; GE-Proton 10 is the tested
//! baseline (GE-Proton 11 / Wine 11 closes STR 1.8 right after launch, as
//! documented by the SteamDeckSTR reference). Missing GE-Proton builds are
//! downloaded from the official GloriousEggroll GitHub releases into
//! `compatibilitytools.d`, where Steam discovers them.

use std::path::{Path, PathBuf};

use crate::vdf;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ProtonTool {
    pub name: String,
    pub path: PathBuf,
    pub is_ge: bool,
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
pub const GE_RELEASE_BASE: &str = "https://github.com/GloriousEggroll/proton-ge-custom/releases/download";

/// All compatibility tools visible to Steam (custom tools + official Protons).
pub fn list_tools(steam_roots: &[PathBuf]) -> Vec<ProtonTool> {
    let mut out: Vec<ProtonTool> = Vec::new();
    let mut push = |dir: PathBuf| {
        if dir.join("proton").is_file() {
            let name = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
            let is_ge = name.starts_with("GE-Proton");
            if !out.iter().any(|t: &ProtonTool| t.name == name) {
                out.push(ProtonTool { name, path: dir, is_ge });
            }
        }
    };
    for root in steam_roots {
        let custom = root.join("compatibilitytools.d");
        if let Ok(entries) = std::fs::read_dir(&custom) {
            for e in entries.flatten() {
                push(e.path());
            }
        }
        // Official Proton builds live in the library "common" folder.
        if let Ok(entries) = std::fs::read_dir(root.join("steamapps/common")) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("Proton ") || name.starts_with("Proton - ") {
                    push(e.path());
                }
            }
        }
    }
    out.sort_by(|a, b| {
        b.is_ge
            .cmp(&a.is_ge)
            .then_with(|| numeric_version(&b.name).cmp(&numeric_version(&a.name)))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Extract a sortable version tuple from a Proton name ("GE-Proton10-34" ->
/// (10, 34)).
fn numeric_version(name: &str) -> (u64, u64) {
    let digits: Vec<u64> = name
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    (digits.first().copied().unwrap_or(0), digits.get(1).copied().unwrap_or(0))
}

pub fn find_tool(steam_roots: &[PathBuf], name: &str) -> Option<ProtonTool> {
    list_tools(steam_roots).into_iter().find(|t| t.name == name)
}

/// The tool we install/select by default for Skyrim.
pub fn default_tool_name() -> String {
    crate::steam::DEFAULT_PROTON_TOOL.to_string()
}

/// Make sure the named tool exists; install GE-Proton builds when missing.
/// Returns the tool's `proton` script path.
pub async fn ensure_tool(
    steam_roots: &[PathBuf],
    name: &str,
    staging_dir: &Path,
    progress: &mut crate::download::ProgressFn<'_>,
) -> Result<PathBuf, String> {
    if let Some(tool) = find_tool(steam_roots, name) {
        return Ok(tool.path.join("proton"));
    }
    if !name.starts_with("GE-Proton") {
        return Err(format!(
            "compatibility tool '{name}' is not installed and can only be auto-installed \
             for GE-Proton builds. Pick an installed tool or install '{name}' into \
             compatibilitytools.d manually."
        ));
    }
    let root = steam_roots
        .first()
        .ok_or("no Steam installation found")?;
    let url = format!("{GE_RELEASE_BASE}/{name}/{name}.tar.gz");
    let archive = staging_dir.join(format!("{name}.tar.gz"));
    if archive.exists() {
        let _ = std::fs::remove_file(&archive);
    }
    crate::download::download(&url, &archive, progress, 3)
        .await
        .map_err(|e| format!("downloading {name}: {e}"))?;
    let compat = root.join("compatibilitytools.d");
    std::fs::create_dir_all(&compat).map_err(|e| e.to_string())?;
    crate::archive::extract_tar_gz(&archive, &compat, progress)?;
    let _ = std::fs::remove_file(&archive);
    find_tool(steam_roots, name)
        .map(|t| t.path.join("proton"))
        .ok_or_else(|| format!("{name} was extracted but could not be found again"))
}

/// Set Steam's per-game compatibility tool (the "Force the use of a specific
/// Steam Play compatibility tool" checkbox) by patching `config.vdf`.
/// Steam must not be running while the file is edited. Returns whether a
/// change was made.
pub fn select_compat_tool(steam_root: &Path, appid: &str, tool: &str) -> Result<bool, String> {
    let config_path = steam_root.join("config/config.vdf");
    let text = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("read {}: {e}", config_path.display()))?;
    if vdf::get_compat_tool(&text, appid).as_deref() == Some(tool) {
        return Ok(false);
    }
    let new_text = vdf::set_compat_tool(&text, appid, tool)?;
    let backup = config_path.with_extension("vdf.str-launcher-backup");
    std::fs::copy(&config_path, &backup)
        .map_err(|e| format!("backup config.vdf: {e}"))?;
    std::fs::write(&config_path, new_text)
        .map_err(|e| format!("write {}: {e}", config_path.display()))?;
    Ok(true)
}

/// Shut Steam down cleanly, select the tool, and restart Steam if it was
/// running. Backs the "Restart Steam" confirmation flow: the install pipeline
/// defers the change while Steam runs, and the UI applies it here on demand.
pub fn apply_with_steam_restart(steam_root: &Path, appid: &str, tool: &str) -> Result<bool, String> {
    use crate::platform::native as plat;
    let was_running = plat::is_steam_running();
    if was_running {
        plat::shutdown_steam();
        for _ in 0..30 {
            if !plat::is_steam_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    let changed = select_compat_tool(steam_root, appid, tool)?;
    if was_running {
        plat::launch_steam();
    }
    Ok(changed)
}

/// Is the named compatibility tool from a Wine-11 family that crashes STR 1.8
/// (GE-Proton 11+, official Proton 11 / Experimental)?
pub fn is_wine11_family(name: &str) -> bool {
    let lower = name.to_lowercase();
    if lower.contains("proton 11") || lower.contains("proton - experimental") {
        return true;
    }
    lower.starts_with("ge-proton") && numeric_version(name).0 >= 11
}

pub fn current_compat_tool(steam_root: &Path, appid: &str) -> Option<String> {
    let text = std::fs::read_to_string(steam_root.join("config/config.vdf")).ok()?;
    vdf::get_compat_tool(&text, appid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_sort() {
        assert!(numeric_version("GE-Proton10-34") > numeric_version("GE-Proton9-20"));
        assert_eq!(numeric_version("GE-Proton10-34"), (10, 34));
    }

    #[test]
    fn wine11_families_are_flagged() {
        assert!(is_wine11_family("GE-Proton11-1"));
        assert!(is_wine11_family("GE-Proton12-4"));
        assert!(is_wine11_family("Proton - Experimental"));
        assert!(is_wine11_family("Proton 11.0 (Beta)"));
        // The tested families stay clean.
        assert!(!is_wine11_family("GE-Proton10-34"));
        assert!(!is_wine11_family("GE-Proton9-20"));
        assert!(!is_wine11_family("Proton 10.0"));
        assert!(!is_wine11_family("Proton - Hotfix"));
    }
}
