//! Version manifest: known-good fingerprints for the files we install.
//!
//! `version-manifest.json` (repo root, embedded via `include_bytes!` as the
//! offline fallback) maps each published version of Skyrim Together Reborn /
//! Address Library to the expected SHA-256 + size of its key files, plus a
//! game-runtime compatibility matrix. A fresher copy is fetched from
//! raw.githubusercontent.com when the network is available anyway (install
//! runs); the checklist always verifies offline against the embedded copy.
//!
//! Verification is deliberately conservative: a size mismatch is conclusive
//! without hashing, small files are always hashed, and huge archives (the
//! ~900 MB `_ResourcePack.bsa`) are size-checked only in the quick/UI pass so
//! checklist refreshes stay instant. An unknown version never fails — it is
//! simply unverifiable.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::paths::safe_rel_path;
use crate::release::LAUNCHER_REPO;

/// The embedded fallback manifest (kept in the repo).
const EMBEDDED_JSON: &[u8] = include_bytes!("../../version-manifest.json");

/// Files above this size are size-checked but not hashed in the quick pass.
const QUICK_HASH_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct FileFingerprint {
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModVersion {
    #[serde(rename = "nexus_file_id")]
    pub nexus_file_id: Option<u64>,
    /// Game-root relative path → fingerprint.
    pub files: BTreeMap<String, FileFingerprint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModSection {
    pub nexus_mod_id: u64,
    /// Version label ("1.8.4", "13") → version entry.
    pub versions: BTreeMap<String, ModVersion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GameVersionRow {
    pub runtime: String,
    pub str_versions: Vec<String>,
    pub supported: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    pub str: ModSection,
    pub address_library: ModSection,
    #[serde(default)]
    pub game_versions: Vec<GameVersionRow>,
    /// Oldest SSE runtime STR 1.8 accepts.
    #[serde(default)]
    pub min_runtime: Option<String>,
}

/// The embedded manifest, parsed once.
pub fn embedded() -> &'static VersionManifest {
    static EMBEDDED: OnceLock<VersionManifest> = OnceLock::new();
    EMBEDDED.get_or_init(|| {
        serde_json::from_slice(EMBEDDED_JSON)
            .expect("embedded version-manifest.json must parse (checked by tests)")
    })
}

/// Try to fetch a fresher manifest; the embedded copy is the fallback.
/// Returns `(manifest, source)` where source is "remote" or "embedded".
pub async fn latest() -> (VersionManifest, &'static str) {
    let url = format!(
        "https://raw.githubusercontent.com/{LAUNCHER_REPO}/main/version-manifest.json"
    );
    match crate::download::get_json(&url, &[]).await {
        Ok(value) => match serde_json::from_value::<VersionManifest>(value) {
            Ok(manifest) => return (manifest, "remote"),
            Err(e) => tracing::warn!("remote version-manifest.json invalid ({e}); using embedded"),
        },
        Err(e) => tracing::info!("version manifest fetch failed ({e}); using embedded"),
    }
    (embedded().clone(), "embedded")
}

/// Find the manifest entry for an installed version label. Tolerates the two
/// shapes a label arrives in: `1.8.4` (Nexus) and `1.8.4.0` (PE resource) —
/// a manifest key that is a numeric prefix of the label matches.
pub fn lookup_version<'a>(
    section: &'a ModSection,
    label: &str,
) -> Option<&'a ModVersion> {
    let normalize = |v: &str| v.trim().trim_start_matches('v').to_lowercase();
    let want = normalize(label);
    if let Some(found) = section.versions.get(&want) {
        return Some(found);
    }
    let want_trimmed = want.strip_suffix(".0").unwrap_or(&want);
    section
        .versions
        .iter()
        .find(|(key, _)| {
            let key = normalize(key);
            key == want_trimmed || want.starts_with(&format!("{key}."))
        })
        .map(|(_, v)| v)
}

/// One verification finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileCheck {
    Ok(String),
    /// Size differs (conclusive — no hash needed).
    SizeMismatch { path: String, expected: u64, actual: u64 },
    /// Hash differs (small file, hashed).
    HashMismatch { path: String },
    /// File we installed is gone.
    Missing(String),
}

/// Verify the files listed in `version` against `root` (a game dir or a
/// staging root). `quick` skips hashing above [`QUICK_HASH_LIMIT`] (size
/// still checked) — used by the checklist; installs hash everything.
pub fn verify_files(
    version: &ModVersion,
    root: &Path,
    quick: bool,
) -> Vec<FileCheck> {
    let mut out = Vec::new();
    for (rel, fingerprint) in &version.files {
        let Some(rel_path) = safe_rel_path(rel) else {
            continue;
        };
        let path = root.join(rel_path);
        let actual_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX);
        if actual_size == u64::MAX {
            out.push(FileCheck::Missing(rel.clone()));
            continue;
        }
        if actual_size != fingerprint.size {
            out.push(FileCheck::SizeMismatch {
                path: rel.clone(),
                expected: fingerprint.size,
                actual: actual_size,
            });
            continue;
        }
        if quick && fingerprint.size > QUICK_HASH_LIMIT {
            // Size already verified; the hash is skipped to keep the pass
            // instant, so the file counts as ok.
            out.push(FileCheck::Ok(rel.clone()));
            continue;
        }
        match sha256_file(&path) {
            Ok(hash) if hash == fingerprint.sha256.to_lowercase() => {
                out.push(FileCheck::Ok(rel.clone()));
            }
            Ok(_) => out.push(FileCheck::HashMismatch { path: rel.clone() }),
            Err(e) => tracing::warn!("could not hash {}: {e}", path.display()),
        }
    }
    out
}

impl std::fmt::Display for FileCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileCheck::Ok(path) => write!(f, "{path} ok"),
            FileCheck::SizeMismatch { path, expected, actual } => write!(
                f,
                "{path} has the wrong size ({actual} bytes, expected {expected})"
            ),
            FileCheck::HashMismatch { path } => write!(f, "{path} does not match the manifest hash"),
            FileCheck::Missing(path) => write!(f, "{path} is missing"),
        }
    }
}

/// Is a game runtime version (e.g. `1.6.1170.0`) at least `min` (e.g.
/// `1.6.640`, from the manifest)? Numeric, length-independent; unparsable
/// input returns false (fail closed) unless `min` itself is unparsable.
pub fn runtime_meets_min(version: &str, min: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let v = parts(version);
    let m = parts(min);
    if m.is_empty() {
        return false;
    }
    for i in 0..m.len() {
        let a = v.get(i).copied().unwrap_or(0);
        let b = m.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    true
}

/// SHA-256 of a file, hex encoded.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::Digest;
    let mut file = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("str-manifest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The embedded manifest must always parse — it is the offline fallback.
    #[test]
    fn embedded_manifest_parses_and_has_the_184_entry() {
        let manifest = embedded();
        let v = lookup_version(&manifest.str, "1.8.4").unwrap();
        assert!(v.files.contains_key("SkyrimTogether.exe"));
        assert!(v.files.contains_key("Data/_ResourcePack.esl"));
        assert!(lookup_version(&manifest.address_library, "13").is_some());
        assert!(manifest.game_versions.iter().any(|g| g.supported));
        assert_eq!(manifest.min_runtime.as_deref(), Some("1.6.640"));
    }

    #[test]
    fn version_lookup_tolerates_label_shapes() {
        let manifest = embedded();
        assert!(lookup_version(&manifest.str, "1.8.4").is_some());
        assert!(lookup_version(&manifest.str, "v1.8.4").is_some());
        assert!(lookup_version(&manifest.str, "1.8.4.0").is_some()); // PE shape
        assert!(lookup_version(&manifest.str, "9.9.9").is_none());
        assert!(lookup_version(&manifest.str, "").is_none());
    }

    #[test]
    fn verifies_good_bad_and_missing_files() {
        use sha2::Digest;
        let root = tmp_root("verify");
        let good = root.join("SkyrimTogether.exe");
        std::fs::write(&good, b"identical contents").unwrap();

        let mut version = ModVersion {
            nexus_file_id: None,
            files: BTreeMap::new(),
        };
        // Good file: correct size + hash of what we wrote.
        let mut hasher = sha2::Sha256::new();
        hasher.update(b"identical contents");
        version.files.insert(
            "SkyrimTogether.exe".into(),
            FileFingerprint { sha256: format!("{:x}", hasher.finalize()), size: 18 },
        );
        // Wrong size.
        version.files.insert(
            "Data/_ResourcePack.esl".into(),
            FileFingerprint { sha256: "00".repeat(32), size: 99 },
        );
        // Right size, wrong hash.
        version.files.insert(
            "Data/tiny.bin".into(),
            FileFingerprint { sha256: "11".repeat(32), size: 5 },
        );
        std::fs::create_dir_all(root.join("Data")).unwrap();
        std::fs::write(root.join("Data/tiny.bin"), b"12345").unwrap();
        // Exists but with the wrong size.
        std::fs::write(root.join("Data/_ResourcePack.esl"), b"12345").unwrap();
        // Missing file entirely.
        version.files.insert(
            "Data/gone.bsa".into(),
            FileFingerprint { sha256: "22".repeat(32), size: 1 },
        );

        let results = verify_files(&version, &root, true);
        assert!(results.contains(&FileCheck::Ok("SkyrimTogether.exe".into())));
        assert_eq!(
            results.iter().find(|c| matches!(c, FileCheck::SizeMismatch { path, .. } if path == "Data/_ResourcePack.esl")).unwrap(),
            &FileCheck::SizeMismatch { path: "Data/_ResourcePack.esl".into(), expected: 99, actual: 5 }
        );
        assert!(results.iter().any(|c| matches!(c, FileCheck::HashMismatch { path } if path == "Data/tiny.bin")));
        assert_eq!(results.iter().find(|c| matches!(c, FileCheck::Missing(p) if p == "Data/gone.bsa")).unwrap(), &FileCheck::Missing("Data/gone.bsa".into()));

        // Traversal-safe: a hostile manifest path is skipped, not joined.
        let mut hostile = ModVersion { nexus_file_id: None, files: BTreeMap::new() };
        hostile.files.insert(
            "../escape.txt".into(),
            FileFingerprint { sha256: "33".repeat(32), size: 1 },
        );
        assert!(verify_files(&hostile, &root, true).is_empty());
        assert!(!root.parent().unwrap().join("escape.txt").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Quick mode skips hashing huge files (size check still runs).
    #[test]
    fn quick_pass_skips_hashes_above_the_limit() {
        let root = tmp_root("quick");
        let big = root.join("Data/_ResourcePack.bsa");
        std::fs::create_dir_all(big.parent().unwrap()).unwrap();
        // Larger than QUICK_HASH_LIMIT (16 MiB) so quick mode skips hashing.
        let mut f = std::fs::File::create(&big).unwrap();
        f.write_all(&[0u8; 17 * 1024 * 1024]).unwrap();
        drop(f);

        let mut version = ModVersion { nexus_file_id: None, files: BTreeMap::new() };
        version.files.insert(
            "Data/_ResourcePack.bsa".into(),
            FileFingerprint { sha256: "44".repeat(32), size: 17 * 1024 * 1024 },
        );

        // Quick: size matches → Ok without hashing the (wrong-hash) contents.
        let quick = verify_files(&version, &root, true);
        assert_eq!(quick, vec![FileCheck::Ok("Data/_ResourcePack.bsa".into())]);
        // Full pass: hash catches the wrong content.
        let full = verify_files(&version, &root, false);
        assert!(matches!(full[0], FileCheck::HashMismatch { .. }));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn runtime_minimum_matches_the_compatibility_matrix() {
        let min = embedded().min_runtime.clone().unwrap();
        assert!(runtime_meets_min("1.6.1170.0", &min));
        assert!(runtime_meets_min("1.6.640", &min));
        assert!(runtime_meets_min("1.7.0.0", &min)); // newer major/minor
        assert!(!runtime_meets_min("1.6.353", &min));
        assert!(!runtime_meets_min("1.5.97.0", &min));
        // Length independence: "1.6" parses as (1,6) which is below (1,6,640).
        assert!(!runtime_meets_min("1.6", &min));
        assert!(!runtime_meets_min("garbage", &min));
    }

    #[test]
    fn sha256_matches_a_known_vector() {
        let dir = tmp_dir_for_sha();
        let path = dir.join("abc");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tmp_dir_for_sha() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("str-manifest-sha-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
