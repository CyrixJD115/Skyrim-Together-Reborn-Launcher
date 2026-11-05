//! File transfer primitives for the install pipeline: locating archive roots,
//! merging trees into the game folder, and undoing previous installs
//! (manifest-driven removal plus legacy leftovers).

use std::path::{Path, PathBuf};

use super::Reporter;
use crate::archive;
use crate::config::InstallManifest;
use crate::paths::safe_rel_path;

/// Locate the extracted STR archive root that contains `SkyrimTogether.exe`.
pub(super) fn find_str_root(stage: &Path) -> Option<PathBuf> {
    let root = archive::resolve_mod_root(stage);
    [
        root.clone(),
        root.join("SkyrimTogetherReborn"),
        root.join("Data/SkyrimTogetherReborn"),
    ]
    .into_iter()
    .find(|candidate| candidate.join("SkyrimTogether.exe").is_file())
}

/// Locate the extracted Address Library root that contains the `SKSE` folder.
pub(super) fn find_al_root(stage: &Path) -> Option<PathBuf> {
    let root = archive::resolve_mod_root(stage);
    if root.join("SKSE").is_dir() {
        return Some(root);
    }
    if root.join("Data/SKSE").is_dir() {
        return Some(root.join("Data"));
    }
    None
}

/// Recursively merge `src_root` into `merge_dest`, recording every installed
/// file as a path relative to `game_root` (the manifest's frame of reference).
pub(super) fn merge_tree(
    src_root: &Path,
    merge_dest: &Path,
    game_root: &Path,
    collected: &mut Vec<String>,
) -> Result<(), String> {
    fn walk(
        src: &Path,
        dest: &Path,
        rel: &Path,
        game_root: &Path,
        collected: &mut Vec<String>,
    ) -> Result<(), String> {
        for entry in std::fs::read_dir(src).map_err(|e| format!("read {}: {e}", src.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let file_type = entry.file_type().map_err(|e| e.to_string())?;
            let child_rel = rel.join(entry.file_name());
            let dest_path = dest.join(&child_rel);
            if file_type.is_dir() {
                std::fs::create_dir_all(&dest_path)
                    .map_err(|e| format!("create {}: {e}", dest_path.display()))?;
                walk(&entry.path(), dest, &child_rel, game_root, collected)?;
            } else if file_type.is_file() {
                // Overwrite anything stale (including old symlinks).
                let _ = std::fs::remove_file(&dest_path);
                std::fs::copy(entry.path(), &dest_path)
                    .map_err(|e| format!("copy {}: {e}", entry.path().display()))?;
                if let Ok(rel_from_root) = dest_path.strip_prefix(game_root) {
                    if let Some(rel_str) = rel_from_root.to_str() {
                        collected.push(rel_str.replace('\\', "/"));
                    }
                }
            }
        }
        Ok(())
    }
    walk(src_root, merge_dest, Path::new(""), game_root, collected)
}

/// Delete every file listed in `manifest` (plus directories we emptied on the
/// way up to the game root).
pub(super) async fn remove_manifest_files(game_dir: &Path, manifest: &InstallManifest, rep: &Reporter) {
    if manifest.files.is_empty() {
        return;
    }
    let mut removed = 0usize;
    for rel in &manifest.files {
        let Some(safe) = safe_rel_path(rel) else { continue };
        let path = game_dir.join(&safe);
        if path.is_symlink() || path.is_file() {
            let _ = std::fs::remove_file(&path);
            removed += 1;
            // Clean up now-empty directories we created (up to the game root).
            let mut dir = path.parent().map(|p| p.to_path_buf());
            while let Some(d) = dir {
                if d == game_dir || !d.starts_with(game_dir) {
                    break;
                }
                if std::fs::read_dir(&d).map(|mut i| i.next().is_none()).unwrap_or(false) {
                    let _ = std::fs::remove_dir(&d);
                    dir = d.parent().map(|p| p.to_path_buf());
                } else {
                    break;
                }
            }
        }
    }
    rep.log(format!("Removed {removed} files from the previous install.")).await;
}

/// Remove files left by older STR versions / manual installs that we can
/// safely identify.
pub(super) async fn remove_legacy_str_files(game_dir: &Path, rep: &Reporter) {
    let mut removed = 0usize;
    let mut consider = |path: PathBuf| {
        if path.is_symlink() {
            if let Ok(target) = std::fs::read_link(&path) {
                let t = target.to_string_lossy().to_lowercase();
                if t.contains("skyrimtogether") {
                    let _ = std::fs::remove_file(&path);
                    removed += 1;
                }
            }
        }
    };
    if let Ok(entries) = std::fs::read_dir(game_dir) {
        for e in entries.flatten() {
            consider(e.path());
        }
    }
    let data = game_dir.join("Data");
    if let Ok(entries) = std::fs::read_dir(&data) {
        for e in entries.flatten() {
            consider(e.path());
        }
    }
    // Stale SKSE plugin copies from previous STR versions.
    let skse = data.join("SKSE/Plugins");
    if let Ok(entries) = std::fs::read_dir(&skse) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.contains("skyrimtogether") && e.path().is_file() {
                let _ = std::fs::remove_file(e.path());
                removed += 1;
            }
        }
    }
    if removed > 0 {
        rep.log(format!("Cleaned {removed} leftover files from older STR versions.")).await;
    }
}
