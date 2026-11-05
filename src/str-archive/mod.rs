//! Archive extraction (ZIP for mods, tar.gz for GE-Proton) with progress and
//! path-traversal protection.

use std::io::Read;
use std::path::{Path, PathBuf};

fn progress_cb(done: &mut u64, total: u64, cb: &mut dyn FnMut(u64, u64)) {
    cb(*done, total);
}

/// Extract a ZIP archive into `dest`, calling progress with (done, total)
/// uncompressed bytes. Returns the number of extracted entries.
pub fn extract_zip(
    archive_path: &Path,
    dest: &Path,
    cb: &mut dyn FnMut(u64, u64),
) -> Result<usize, String> {
    let file = std::fs::File::open(archive_path)
        .map_err(|e| format!("open {}: {e}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("read zip {}: {e}", archive_path.display()))?;

    let mut total: u64 = 0;
    for i in 0..archive.len() {
        if let Ok(f) = archive.by_index(i) {
            total += f.size();
        }
    }
    let mut done: u64 = 0;
    let mut count = 0usize;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        let Some(rel) = entry
            .enclosed_name()
            .map(|p| p.to_path_buf())
            .or_else(|| {
                // Fallback for entries without a clean relative name.
                let name = entry.name();
                crate::paths::safe_rel_path(name)
            })
        else {
            continue; // unsafe path — skip, never escape the destination
        };
        if entry.is_dir() {
            std::fs::create_dir_all(dest.join(&rel)).map_err(|e| e.to_string())?;
            continue;
        }
        let out_path = dest.join(&rel);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("create {}: {e}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("extract {}: {e}", rel.display()))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
        }
        done += entry.size();
        count += 1;
        progress_cb(&mut done, total, cb);
    }
    Ok(count)
}

/// Extract a `.tar.gz` (GE-Proton ships this way) into `dest`.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn extract_tar_gz(
    archive_path: &Path,
    dest: &Path,
    cb: &mut dyn FnMut(u64, u64),
) -> Result<usize, String> {
    let file = std::fs::File::open(archive_path)
        .map_err(|e| format!("open {}: {e}", archive_path.display()))?;
    let total = crate::paths::file_size(archive_path);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counted = CountingReader { inner: file, done: done.clone() };
    let gz = flate2::read::GzDecoder::new(counted);
    let mut archive = tar::Archive::new(gz);
    archive.set_preserve_permissions(true);
    archive.set_unpack_xattrs(false);
    let mut count = 0usize;
    let report = |done: &std::sync::Arc<std::sync::atomic::AtomicU64>,
                  cb: &mut dyn FnMut(u64, u64)| {
        cb(done.load(std::sync::atomic::Ordering::Relaxed), total);
    };
    let entries = archive
        .entries()
        .map_err(|e| format!("read tar entries: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("tar entry: {e}"))?;
        let path = entry.path().map_err(|e| e.to_string())?.to_path_buf();
        // Reject absolute paths and traversal.
        let mut comps_ok = true;
        for comp in path.components() {
            match comp {
                std::path::Component::Normal(_) => {}
                _ => {
                    comps_ok = false;
                    break;
                }
            }
        }
        if !comps_ok || path.components().count() == 0 {
            continue;
        }
        entry
            .unpack_in(dest)
            .map_err(|e| format!("unpack {}: {e}", path.display()))?;
        count += 1;
        report(&done, cb);
    }
    report(&done, cb);
    Ok(count)
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
struct CountingReader {
    inner: std::fs::File,
    done: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.done.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(n)
    }
}

/// After extracting an archive, descend through wrapper folders (mod archives
/// often wrap everything in a single folder like `SkyrimTogetherReborn/`), but
/// stop before meaningful top-level names such as `SKSE` or `Data`.
pub fn resolve_mod_root(dir: &Path) -> PathBuf {
    descend_wrappers(dir, &["skse", "data"])
}

/// Descend through single-folder wrappers (up to 4 levels). `stop_before`
/// lists lowercase folder names that count as content, not wrappers; pass an
/// empty slice to always descend.
pub fn descend_wrappers(dir: &Path, stop_before: &[&str]) -> PathBuf {
    let mut cur = dir.to_path_buf();
    for _ in 0..4 {
        let entries: Vec<_> = std::fs::read_dir(&cur)
            .map(|rd| rd.flatten().collect())
            .unwrap_or_default();
        if entries.len() == 1 && entries[0].path().is_dir() {
            let name = entries[0].file_name().to_string_lossy().to_lowercase();
            if stop_before.contains(&name.as_str()) {
                break;
            }
            cur = entries[0].path();
        } else {
            break;
        }
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("str-zip-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src/nested")).unwrap();
        std::fs::write(tmp.join("src/one.txt"), b"hello").unwrap();
        std::fs::write(tmp.join("src/nested/two.txt"), b"world").unwrap();

        let zip_path = tmp.join("test.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        zw.start_file("one.txt", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut zw, b"hello").unwrap();
        zw.start_file("nested/two.txt", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut zw, b"world").unwrap();
        zw.finish().unwrap();

        let out = tmp.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let mut cb = |_: u64, _: u64| {};
        let n = extract_zip(&zip_path, &out, &mut cb).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read(out.join("one.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(out.join("nested/two.txt")).unwrap(), b"world");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
