//! Dedicated server helper: download the server archive (Nexus first, with a
//! fallback to the official TiltedPhoques GitHub releases), keep the
//! `config/STServer.ini` editable and launch it (natively on Windows, under
//! the game's Proton on Linux).

use std::path::{Path, PathBuf};

use crate::config::NEXUS_STR;
use crate::nexus::NexusClient;
use crate::str_install::Reporter;

pub const INI_HINT: &str =
    "Start the server once — it generates config/STServer.ini next to the server exe — then press Reload.";

/// Resolve the server archive: the user's offline pick wins; otherwise Nexus
/// is tried first and, when it fails (no API key, API error, no server file),
/// the official TiltedPhoques GitHub release is used instead. Every source is
/// labelled in the activity log.
async fn resolve_archive(
    nexus_key: &str,
    offline_archive: Option<PathBuf>,
    dirs: &crate::paths::AppDirs,
    rep: &Reporter,
    progress: &mut crate::download::ProgressFn<'_>,
) -> Result<PathBuf, String> {
    if let Some(p) = offline_archive {
        rep.log(format!("Offline mode: using server archive {}", p.display()))
            .await;
        return Ok(p);
    }
    rep.log("Server source: trying Nexus Mods…").await;
    match fetch_from_nexus(nexus_key, dirs, progress).await {
        Ok(path) => {
            rep.log("Server source: Nexus Mods (Skyrim Together Reborn).").await;
            return Ok(path);
        }
        Err(nexus_error) => {
            rep.log(format!(
                "Nexus unavailable ({nexus_error}); falling back to the official \
                 TiltedPhoques GitHub release…"
            ))
            .await;
        }
    }
    fetch_from_github(dirs, rep, progress).await
}

/// Nexus path: the newest server file on mod 69993.
async fn fetch_from_nexus(
    nexus_key: &str,
    dirs: &crate::paths::AppDirs,
    progress: &mut crate::download::ProgressFn<'_>,
) -> Result<PathBuf, String> {
    let client = NexusClient::new(nexus_key);
    let files = client.mod_files("skyrimspecialedition", NEXUS_STR).await?;
    let server_file = files
        .iter()
        .filter(|f| f.name.to_lowercase().contains("server"))
        .max_by_key(|f| (f.uploaded_time, f.file_id))
        .ok_or("no server file on the Skyrim Together Reborn Nexus page")?
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
    Ok(dest)
}

/// GitHub fallback: the latest TiltedPhoques/TiltedEvolution release's server
/// zip (the Windows client bundle ships `SkyrimTogetherServer.exe`, so the
/// normal zip extraction + launch flow works on both platforms). Cached per
/// release tag in the downloads folder.
async fn fetch_from_github(
    dirs: &crate::paths::AppDirs,
    rep: &Reporter,
    progress: &mut crate::download::ProgressFn<'_>,
) -> Result<PathBuf, String> {
    let release = crate::release::latest(crate::release::TILTED_REPO).await?;
    let asset = crate::release::server_zip_asset(&release).ok_or(
        "The TiltedEvolution GitHub release contains no usable server zip.\n\
         Download the server manually from Nexus and use the offline archive picker.",
    )?;
    let safe_name: String = asset
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dest = dirs
        .downloads
        .join(format!("github_{}_{}", release.tag_name, safe_name));
    if !crate::download::cache_is_complete(&dest, asset.size) {
        rep.log(format!("Downloading {} from GitHub…", asset.name)).await;
        crate::download::download(&asset.download_url, &dest, progress, 3)
            .await
            .map_err(|e| e.to_string())?;
    }
    rep.log(format!(
        "Server source: GitHub (TiltedPhoques/TiltedEvolution {}).",
        release.tag_name
    ))
    .await;
    Ok(dest)
}

/// Download the latest server archive and extract it into the data folder.
pub async fn ensure_server(
    nexus_key: &str,
    offline_archive: Option<PathBuf>,
    dirs: &crate::paths::AppDirs,
    rep: &Reporter,
) -> Result<PathBuf, String> {
    let mut prog = rep.progress_fn("Downloading server".into());
    let progress = &mut |d, t| prog.send(d, t);
    let server_dir = crate::launch::server_dir(dirs);
    let archive_path =
        resolve_archive(nexus_key, offline_archive, dirs, rep, progress).await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The offline path drives the same extraction + move flow as the online
    /// ones, so it doubles as a hermetic end-to-end check.
    #[tokio::test]
    async fn offline_server_archive_installs_and_is_found() {
        let root = std::env::temp_dir().join(format!("str-server-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dirs = crate::paths::AppDirs {
            config: root.join("app/config"),
            data: root.join("app/data"),
            downloads: root.join("app/downloads"),
            staging: root.join("app/staging"),
            cc_backup: root.join("app/cc-backup"),
            scripts: root.join("app/scripts"),
            logs: root.join("app/logs"),
        };
        std::fs::create_dir_all(&dirs.downloads).unwrap();

        let zip_path = root.join("server.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        for (name, content) in [
            ("SkyrimTogetherReborn/SkyrimTogetherServer.exe", b"srv" as &[u8]),
            ("SkyrimTogetherReborn/config/STServer.ini", b"[Server]\nuPort=7777\n"),
        ] {
            zw.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zw, content).unwrap();
        }
        zw.finish().unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let rep = Reporter::new(tx);
        let dir = ensure_server("", Some(zip_path), &dirs, &rep).await.unwrap();

        assert!(crate::launch::find_server_exe(&dir).is_some());
        assert!(read_settings(&dir).is_some());
        // The offline source is labelled in the activity log.
        let mut saw_offline = false;
        while let Ok(ev) = rx.try_recv() {
            if let crate::str_install::Event::Log(line) = ev {
                if line.contains("Offline mode") {
                    saw_offline = true;
                }
            }
        }
        assert!(saw_offline);
        let _ = std::fs::remove_dir_all(&root);
    }
}
