//! Nexus Mods API v1 client.
//!
//! Skyrim Together Reborn (69993) and Address Library (32444) are only
//! distributed through Nexus Mods, so the launcher talks to the official API
//! using the user's personal API key (created at
//! <https://www.nexusmods.com/settings/api-keys>). The key never leaves
//! the machine except to api.nexusmods.com. An offline mode is available for
//! users who prefer to drop the archives in manually.

use serde::Deserialize;

use crate::download;
use crate::paths::APP_NAME;

pub const API_BASE: &str = "https://api.nexusmods.com/v1";
/// Nexus file categories.
const CATEGORY_MAIN: i64 = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct ModFile {
    #[serde(rename = "file_id")]
    pub file_id: u64,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(rename = "category_id")]
    pub category_id: i64,
    #[serde(default)]
    pub size: u64,
    /// Unix seconds. Nexus has returned both integers and ISO-8601 text for
    /// this field in the wild, so parse either form.
    #[serde(
        rename = "uploaded_time",
        default,
        deserialize_with = "flexible_timestamp"
    )]
    pub uploaded_time: i64,
    #[serde(rename = "mod_version", default)]
    pub mod_version: String,
}

#[derive(Debug, Clone, Deserialize)]
struct FilesResponse {
    files: Vec<ModFile>,
}

pub struct NexusClient {
    key: String,
}

impl NexusClient {
    pub fn new(key: &str) -> Self {
        NexusClient { key: key.trim().to_string() }
    }

    fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("apikey", self.key.clone()),
            ("accept", "application/json".into()),
            ("Application-Name", APP_NAME.into()),
            ("Application-Version", crate::paths::APP_VERSION.into()),
        ]
    }

    pub async fn validate_key(&self) -> Result<(), String> {
        if self.key.is_empty() {
            return Err("no Nexus API key configured".into());
        }
        let headers = self.headers();
        let header_refs: Vec<(&str, &str)> =
            headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let url = format!("{API_BASE}/users/validate.json");
        let value = download::get_json(&url, &header_refs).await?;
        match value.get("user_id").and_then(|v| v.as_u64()) {
            Some(_) => Ok(()),
            None => Err("Nexus API key was not accepted (validate.json has no user_id)".into()),
        }
    }

    /// List files of a mod (`domain` is e.g. `skyrimspecialedition`).
    pub async fn mod_files(&self, domain: &str, mod_id: u64) -> Result<Vec<ModFile>, String> {
        if self.key.is_empty() {
            return Err(
                "Nexus API key required for automatic downloads.\n\
                 Use GET A KEY, paste it in Advanced settings, or use Offline mode."
                    .into(),
            );
        }
        let headers = self.headers();
        let header_refs: Vec<(&str, &str)> =
            headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let url = format!("{API_BASE}/games/{domain}/mods/{mod_id}/files.json");
        let value = download::get_json(&url, &header_refs).await?;
        let parsed: FilesResponse =
            serde_json::from_value(value).map_err(|e| format!("unexpected files.json: {e}"))?;
        Ok(parsed.files)
    }

    /// Pick the newest *main* file (what users should install).
    pub fn pick_main(files: &[ModFile]) -> Option<&ModFile> {
        files
            .iter()
            .filter(|f| f.category_id == CATEGORY_MAIN)
            .max_by_key(|f| (f.uploaded_time, f.file_id))
    }

    /// Resolve the temporary CDN download URL for a file.
    pub async fn download_link(&self, domain: &str, mod_id: u64, file_id: u64) -> Result<String, String> {
        let headers = self.headers();
        let header_refs: Vec<(&str, &str)> =
            headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let url = format!(
            "{API_BASE}/games/{domain}/mods/{mod_id}/files/{file_id}/download_link.json"
        );
        let value = download::get_json(&url, &header_refs).await?;
        let urls = value
            .as_array()
            .and_then(|a| a.first())
            .and_then(|o| o.get("URI"))
            .and_then(|u| u.as_str())
            .ok_or("download_link.json did not contain a URI")?;
        Ok(urls.to_string())
    }

    /// Download the latest main file of a mod into `dest`, with progress.
    pub async fn download_main_file(
        &self,
        domain: &str,
        mod_id: u64,
        dest_dir: &std::path::Path,
        progress: &mut download::ProgressFn<'_>,
    ) -> Result<(std::path::PathBuf, ModFile), String> {
        let files = self.mod_files(domain, mod_id).await?;
        let main = Self::pick_main(&files)
            .ok_or_else(|| format!("mod {mod_id} has no main file available"))?
            .clone();
        let dest = download::cache_path(dest_dir, mod_id, main.file_id, &main.name);
        // Reuse the cache when the file is already complete.
        let expected = if main.size > 0 { main.size } else { 0 };
        if dest.is_file() && expected > 0 && crate::paths::file_size(&dest) == expected {
            progress(expected, expected);
            return Ok((dest, main));
        }
        let url = self.download_link(domain, mod_id, main.file_id).await?;
        download::download(&url, &dest, progress, 4)
            .await
            .map_err(|e| e.to_string())?;
        Ok((dest, main))
    }

    /// Check for updates: returns the latest main file if it is newer than
    /// `installed_file_id`.
    pub async fn latest_main_file(&self, domain: &str, mod_id: u64) -> Result<Option<ModFile>, String> {
        let files = self.mod_files(domain, mod_id).await?;
        Ok(Self::pick_main(&files).cloned())
    }
}

// ---------------------------------------------------------------------------
// Timestamp parsing
// ---------------------------------------------------------------------------

/// Accept every shape Nexus has been seen to use for `uploaded_time` and
/// normalise it to Unix seconds: `1669037815`, `"1669037815"`, or
/// ISO-8601 text like `"2022-06-21T10:56:55.000+00:00"`.
fn flexible_timestamp<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Unix(i64),
        Float(f64),
        Iso(String),
    }

    match Raw::deserialize(deserializer)? {
        Raw::Unix(secs) => Ok(secs),
        // Fractional seconds from some API versions — truncate.
        Raw::Float(secs) => Ok(secs as i64),
        Raw::Iso(text) => parse_iso8601(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("unrecognised uploaded_time: {text}"))),
    }
}

/// Parse ISO-8601 date-times into Unix seconds. Handles `Z`, `+HH:MM` and
/// `-HHMM` offsets, optional fractional seconds and a missing seconds part.
/// Also accepts a plain digit string as Unix seconds (or milliseconds when
/// more than 10 digits long).
fn parse_iso8601(text: &str) -> Option<i64> {
    let text = text.trim();

    // Bare digit string: seconds, or milliseconds when too long.
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        let parsed = text.parse::<i64>().ok()?;
        return Some(if text.len() > 10 { parsed / 1000 } else { parsed });
    }

    // "YYYY-MM-DD" — the 'T' separator is optional; missing time means 00:00.
    let (date, rest) = match text.split_once(['T', 't', ' ']) {
        Some((date, rest)) => (date, rest),
        None => (text, "00:00:00"),
    };

    // Peel off the timezone suffix, converting it to seconds to subtract.
    // The offset sign only occurs after the time, never in the date.
    let (time, offset) = if let Some(stripped) = rest.strip_suffix(['Z', 'z']) {
        (stripped, 0)
    } else {
        match rest.find(['+', '-']) {
            Some(index) => {
                let (time, offset_text) = rest.split_at(index);
                (time, offset_seconds(&offset_text[1..], offset_text.starts_with('-'))?)
            }
            None => (rest, 0),
        }
    };

    // Date: YYYY-MM-DD.
    let (year, month, day) = {
        let mut parts = date.split('-');
        (parts.next()?, parts.next()?, parts.next()?)
    };
    let year: i64 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;

    // Time: HH:MM[:SS], fractional seconds dropped.
    let time = time.split('.').next().unwrap_or(time);
    let mut clock = time.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next().unwrap_or("0").parse().ok()?;
    let second: i64 = clock.next().unwrap_or("0").parse().ok()?;

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second - offset)
}

/// `+2`, `+02:30` or `-0145` styles, signed by `negative`.
fn offset_seconds(text: &str, negative: bool) -> Option<i64> {
    let mut parts = text.split(':');
    let hours: i64 = parts.next()?.parse().ok()?;
    let minutes: i64 = parts.next().unwrap_or("0").parse().ok()?;
    let seconds = hours * 3_600 + minutes * 60;
    Some(if negative { -seconds } else { seconds })
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let month = month as i64;
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::parse_iso8601;
    use super::ModFile;

    #[test]
    fn parses_unix_seconds() {
        assert_eq!(parse_iso8601("1669037815"), Some(1_669_037_815));
    }

    #[test]
    fn parses_unix_milliseconds() {
        assert_eq!(parse_iso8601("1669037815000"), Some(1_669_037_815));
    }

    #[test]
    fn parses_iso_utc() {
        // 2022-06-21T10:56:55.000+00:00
        assert_eq!(parse_iso8601("2022-06-21T10:56:55.000+00:00"), Some(1_655_809_015));
    }

    #[test]
    fn parses_iso_z() {
        assert_eq!(parse_iso8601("2022-06-21T10:56:55Z"), Some(1_655_809_015));
    }

    #[test]
    fn parses_dates_only() {
        assert_eq!(parse_iso8601("2022-06-21"), Some(1_655_769_600));
    }

    #[test]
    fn applies_positive_offset() {
        assert_eq!(
            parse_iso8601("2022-06-21T12:56:55+02:00"),
            Some(1_655_809_015)
        );
    }

    #[test]
    fn applies_negative_offset() {
        assert_eq!(
            parse_iso8601("2022-06-21T09:26:55-01:30"),
            Some(1_655_809_015)
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_iso8601("not-a-date"), None);
    }

    #[test]
    fn deserialises_integer_timestamp() {
        let file: ModFile = serde_json::from_str(
            r#"{"file_id":1,"name":"a","category_id":1,"size":0,"uploaded_time":1669037815,"mod_version":"1"}"#,
        )
        .unwrap();
        assert_eq!(file.uploaded_time, 1_669_037_815);
    }

    #[test]
    fn deserialises_iso_timestamp() {
        let file: ModFile = serde_json::from_str(
            r#"{"file_id":2,"name":"b","category_id":1,"size":0,"uploaded_time":"2022-06-21T10:56:55.000+00:00","mod_version":"1"}"#,
        )
        .unwrap();
        assert_eq!(file.uploaded_time, 1_655_809_015);
    }
}

