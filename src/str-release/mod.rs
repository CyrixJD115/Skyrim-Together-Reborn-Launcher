//! GitHub Releases clients.
//!
//! Two uses:
//! - the launcher's own self-update check (compare the running version with
//!   the latest published release, never auto-install);
//! - the dedicated-server fallback download from the official TiltedPhoques
//!   repository when Nexus is unavailable (see [`crate::server`]).
//!
//! Only the JSON fields we consume are deserialized; GitHub may add more at
//! any time. Unauthenticated requests are rate-limited (60/h) — fine for a
//! startup check, so failures must degrade to "no update shown".

use serde::Deserialize;

/// The repository that publishes launcher releases.
pub const LAUNCHER_REPO: &str = "CyrixJD115/Skyrim-Together-Reborn-Launcher";

#[derive(Debug, Clone, Deserialize)]
pub struct LatestRelease {
    #[serde(rename = "tag_name")]
    pub tag_name: String,
}

/// GET `repos/<repo>/releases/latest` (plain download path, no API key).
pub async fn latest(repo: &str) -> Result<LatestRelease, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let value = crate::download::get_json(&url, &[]).await?;
    serde_json::from_value(value).map_err(|e| format!("unexpected releases/latest JSON: {e}"))
}

/// Strip a leading `v`/`V` from a release tag (`v0.1.2` → `0.1.2`).
fn strip_tag_prefix(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

/// Semantic comparison of two `X.Y.Z…` version strings: numeric parts compare
/// numerically and length-independently (`0.2` > `0.1.9`), a prerelease
/// suffix (`-rc1`) sorts *before* the same version without one, and garbage
/// segments compare as 0 so a malformed tag never crashes the check.
///
/// Returns `Ordering::Less` when `a` is older than `b`.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let parse = |v: &str| -> (Vec<u64>, bool) {
        let v = strip_tag_prefix(v);
        // "1.2.3-rc1" or "1.2.3+build" — only the '-' part is a prerelease.
        let (numbers, prerelease) = match v.split_once('-') {
            Some((n, _)) => (n, true),
            None => (v.split_once('+').map_or(v, |(n, _)| n), false),
        };
        let parts: Vec<u64> = numbers
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect();
        (parts, prerelease)
    };

    let (a_parts, a_pre) = parse(a);
    let (b_parts, b_pre) = parse(b);
    let len = a_parts.len().max(b_parts.len());
    for i in 0..len {
        let av = a_parts.get(i).copied().unwrap_or(0);
        let bv = b_parts.get(i).copied().unwrap_or(0);
        if av != bv {
            return av.cmp(&bv);
        }
    }
    // Numerically equal: the release beats the prerelease.
    match (a_pre, b_pre) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

/// Is `latest` (a release tag or bare version) newer than `running`?
pub fn is_newer(running: &str, latest: &str) -> bool {
    compare_versions(running, latest) == std::cmp::Ordering::Less
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_numerically_not_lexically() {
        assert!(is_newer("0.1.2", "0.1.3"));
        assert!(!is_newer("0.1.2", "0.1.2"));
        assert!(!is_newer("0.1.3", "0.1.2"));
        // Lexical comparison would say "0.1.9" > "0.1.10".
        assert!(is_newer("0.1.9", "0.1.10"));
        assert!(is_newer("0.2.0", "1.0.0"));
    }

    #[test]
    fn handles_v_prefixes_and_mismatched_lengths() {
        assert!(is_newer("0.1.2", "v0.1.3"));
        assert!(!is_newer("0.1.3", "v0.1.3"));
        assert!(is_newer("0.2", "0.2.1")); // 0.2 < 0.2.1
        assert!(!is_newer("0.2.1", "0.2")); // 0.2.1 > 0.2
        assert!(!is_newer("1.0", "v1.0.0"));
    }

    #[test]
    fn prerelease_sorts_before_the_release() {
        assert!(is_newer("1.0.0-rc1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0-rc1"));
        assert!(is_newer("0.1.2-rc1", "0.1.3"));
    }

    #[test]
    fn garbage_segments_never_crash() {
        assert!(is_newer("garbage", "1.0.0"));
        assert!(!is_newer("1.0.0", "garbage"));
        assert!(!is_newer("", ""));
    }

    #[test]
    fn latest_release_deserialises_minimal_json() {
        let release: LatestRelease =
            serde_json::from_str(r#"{"tag_name": "v0.1.1", "html_url": "https://example.com"}"#)
                .unwrap();
        assert_eq!(release.tag_name, "v0.1.1");
    }
}
