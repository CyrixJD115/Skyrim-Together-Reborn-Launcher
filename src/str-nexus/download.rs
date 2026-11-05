//! Async file downloader with progress reporting, retries and resume.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;

pub type ProgressFn<'a> = dyn FnMut(u64, u64) + Send + 'a;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Client(String),
    #[error("download failed after {attempts} attempts: {last}")]
    RetriesExhausted { attempts: u32, last: String },
}

impl From<String> for DownloadError {
    fn from(e: String) -> Self {
        DownloadError::Client(e)
    }
}

pub const USER_AGENT: &str =
    concat!("STR-Launcher/", env!("CARGO_PKG_VERSION"), " (universal Skyrim Together Reborn launcher)");

/// One process-wide HTTP client, so every request reuses its connection pool
/// (TLS handshakes and keep-alive connections are expensive to rebuild).
fn http_client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: std::sync::OnceLock<Result<reqwest::Client, String>> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(USER_AGENT)
                // No total timeout: large archives (hundreds of MB) may
                // legitimately take a long time. Idle reads are caught by the
                // stream error path.
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(60))
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Exponential backoff between download retries (2s, 4s, 8s, capped at 16s).
async fn backoff(attempt: u32) {
    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt.min(4)))).await;
}

/// Download `url` to `dest`, calling `progress(bytes_done, bytes_total)`.
/// Retries with HTTP range resume; if the server does not support ranges the
/// download restarts from scratch.
pub async fn download(
    url: &str,
    dest: &Path,
    progress: &mut ProgressFn<'_>,
    max_attempts: u32,
) -> Result<(), DownloadError> {
    let client = http_client()?;
    let mut attempt: u32 = 0;
    let mut last_err = String::from("unknown");
    while attempt < max_attempts {
        attempt += 1;
        let partial = dest.with_extension("part");
        let start = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
        let mut request = client.get(url);
        if start > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={start}-"));
        }
        let response = match request.send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("request failed: {e}");
                backoff(attempt).await;
                continue;
            }
        };
        let resuming_206 = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if !(response.status().is_success() || resuming_206) {
            last_err = format!("HTTP {}", response.status());
            // 4xx will not get better by retrying (except 408/429).
            if response.status().as_u16() == 408 || response.status().as_u16() == 429 {
                backoff(attempt).await;
                continue;
            }
            return Err(DownloadError::RetriesExhausted { attempts: attempt, last: last_err });
        }
        let total = response
            .content_length()
            .map(|l| if resuming_206 { l + start } else { l })
            .unwrap_or(0);
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(resuming_206)
            .write(true)
            .open(&partial)
            .await
        {
            Ok(f) => f,
            Err(e) => return Err(DownloadError::Io(e)),
        };
        if !resuming_206 {
            file.set_len(0).await.map_err(DownloadError::Io)?;
        }
        let mut done = if resuming_206 { start } else { 0 };
        progress(done, total);
        let mut stream = response.bytes_stream();
        let mut failed: Option<String> = None;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = file.write_all(&bytes).await {
                        failed = Some(format!("write failed: {e}"));
                        break;
                    }
                    done += bytes.len() as u64;
                    progress(done, total);
                }
                Err(e) => {
                    failed = Some(format!("stream error: {e}"));
                    break;
                }
            }
        }
        let _ = file.sync_all().await;
        match failed {
            None => {
                std::fs::rename(&partial, dest).map_err(DownloadError::Io)?;
                progress(done, done.max(total));
                return Ok(());
            }
            Some(e) => {
                last_err = e;
                backoff(attempt).await;
            }
        }
    }
    Err(DownloadError::RetriesExhausted { attempts: max_attempts, last: last_err })
}

/// Fetch a URL as text (small payloads: APIs).
pub async fn get_text(url: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    let client = http_client()?;
    let mut req = client.get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.send().await.map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| format!("read body: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", body.chars().take(300).collect::<String>()));
    }
    Ok(body)
}

pub async fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<serde_json::Value, String> {
    let text = get_text(url, headers).await?;
    serde_json::from_str(&text).map_err(|e| format!("invalid JSON from {url}: {e}"))
}

/// Where a mod's downloads are cached.
pub fn cache_path(dir: &Path, mod_id: u64, file_id: u64, name: &str) -> PathBuf {
    let safe_name: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    dir.join(format!("{mod_id}_{file_id}_{safe_name}"))
}

/// Is a cached download complete? `expected` is the size reported by the
/// API (0 means unknown, in which case the cache is never trusted).
pub fn cache_is_complete(path: &Path, expected: u64) -> bool {
    expected > 0 && path.is_file() && crate::paths::file_size(path) == expected
}
