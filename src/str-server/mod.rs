//! Dedicated server helper: download the server archive from Nexus, keep the
//! `config/STServer.ini` editable and launch it (natively on Windows, under
//! the game's Proton on Linux).

use std::path::{Path, PathBuf};

use crate::config::NEXUS_STR;
use crate::nexus::NexusClient;

pub const INI_HINT: &str =
    "Start the server once — it generates config/STServer.ini next to the server exe — then press Reload.";

/// Download the latest server archive for mod 69993 and extract it.
pub async fn ensure_server(
    nexus_key: &str,
    offline_archive: Option<PathBuf>,
    dirs: &crate::paths::AppDirs,
    progress: &mut crate::download::ProgressFn<'_>,
) -> Result<PathBuf, String> {
    let server_dir = crate::launch::server_dir(dirs);
    let archive_path: PathBuf = match offline_archive {
        Some(p) => p,
        None => {
            let client = NexusClient::new(nexus_key);
            let files = client.mod_files("skyrimspecialedition", NEXUS_STR).await?;
            let server_file = files
                .iter()
                .filter(|f| f.name.to_lowercase().contains("server"))
                .max_by_key(|f| (f.uploaded_time, f.file_id))
                .ok_or(
                    "No server file was found on the Skyrim Together Reborn Nexus page.\n\
                     Download it manually and use the offline archive picker.",
                )?
                .clone();
            let dest = crate::download::cache_path(
                &dirs.downloads,
                NEXUS_STR,
                server_file.file_id,
                &server_file.name,
            );
            if !crate::download::cache_is_complete(&dest, server_file.size) {
                let url = client
                    .download_link("skyrimspecialedition", NEXUS_STR, server_file.file_id)
                    .await?;
                crate::download::download(&url, &dest, progress, 4)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            dest
        }
    };

    let staging = dirs.staging.join("server");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    crate::archive::extract_zip(&archive_path, &staging, progress)?;
    let root = crate::archive::descend_wrappers(&staging, &[]);
    if crate::launch::find_server_exe(&root).is_none() {
        return Err(format!(
            "{} does not contain SkyrimTogetherServer.exe — this does not look like the server archive.",
            archive_path.display()
        ));
    }
    // Replace the previous server install.
    let _ = std::fs::remove_dir_all(&server_dir);
    std::fs::create_dir_all(
        server_dir
            .parent()
            .ok_or("invalid server directory")?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&root, &server_dir).map_err(|e| {
        format!(
            "could not move the server into place ({e}); is {} on another drive?",
            root.display()
        )
    })?;
    Ok(server_dir)
}

#[derive(Debug, Clone, Default)]
pub struct ServerSettings {
    pub port: Option<String>,
    pub name: Option<String>,
    pub password: Option<String>,
    pub mod_check: Option<String>,
    pub max_players: Option<String>,
    pub announce: Option<String>,
}

pub fn find_stserver_ini(server_dir: &Path) -> Option<PathBuf> {
    // Documented location: config/STServer.ini next to the server exe; fall
    // back to a recursive search for robustness.
    let exe = crate::launch::find_server_exe(server_dir)?;
    let base = exe.parent()?;
    let direct = base.join("config/STServer.ini");
    if direct.is_file() {
        return Some(direct);
    }
    let sibling = base.join("STServer.ini");
    if sibling.is_file() {
        return Some(sibling);
    }
    fn walk(d: &Path, depth: usize) -> Option<PathBuf> {
        if depth > 3 {
            return None;
        }
        if let Ok(entries) = std::fs::read_dir(d) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_file() && p.file_name().unwrap_or_default() == "STServer.ini" {
                    return Some(p);
                }
                if p.is_dir() {
                    if let Some(found) = walk(&p, depth + 1) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
    walk(base, 0)
}

pub fn read_settings(server_dir: &Path) -> Option<(PathBuf, ServerSettings)> {
    let path = find_stserver_ini(server_dir)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let mut s = ServerSettings::default();
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        match key.as_str() {
            "uport" | "port" => s.port = Some(value),
            "sservername" | "name" | "servername" => s.name = Some(value),
            "spassword" | "password" => s.password = Some(value),
            "benablemodcheck" => s.mod_check = Some(value),
            "umaxplayercount" | "maxplayers" => s.max_players = Some(value),
            "bannounceserver" => s.announce = Some(value),
            _ => {}
        }
    }
    Some((path, s))
}

/// Write one key back into the INI, replacing the existing line or appending.
pub fn write_setting(path: &Path, key: &str, value: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut replaced = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| {
            let trimmed = l.trim();
            if let Some((k, _)) = trimmed.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    replaced = true;
                    return format!("{key}={value}");
                }
            }
            l.to_string()
        })
        .collect();
    if !replaced {
        lines.push(format!("{key}={value}"));
    }
    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    crate::paths::atomic_write(path, out.as_bytes())
}
