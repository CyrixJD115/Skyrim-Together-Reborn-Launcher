//! Diagnostics logging: a size-rotating file log in the app-dirs config
//! folder plus a panic hook that records every panic into the same file.
//!
//! The in-app activity pane stays the user-facing channel; this file log is
//! for bug reports and post-mortems (especially on Windows, where a
//! GUI-subsystem binary has no console and panics are otherwise invisible).
//!
//! Logging must never fail the app: every filesystem error is swallowed, and
//! writing happens through short-lived append handles so rotation works even
//! when another thread holds the file open.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use tracing_subscriber::fmt::MakeWriter;

/// `launcher.log` inside the config directory; rotated copy gets `.1`.
const LOG_FILE: &str = "launcher.log";
/// Rotate (keep one old copy) once the log reaches this size.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Resolve the config directory without depending on the app's own
/// [`AppDirs::init`] result (logging must come up even when that fails).
fn config_dir() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("io", "strlauncher", crate::paths::APP_NAME)?;
    Some(dirs.config_dir().to_path_buf())
}

/// The log file path for a config directory (also used by tests).
fn log_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LOG_FILE)
}

/// Initialize the global `tracing` subscriber (writing to
/// `<config>/launcher.log`) and the panic hook. Returns the log path when
/// logging is active, so callers can show it in the UI.
pub fn init() -> Option<PathBuf> {
    let dir = config_dir()?;
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = log_path(&dir);
    let rotating = RotatingLog::new(path.clone());

    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(rotating)
        .finish();
    if tracing::subscriber::set_global_default(subscriber).is_err() {
        return None;
    }

    install_panic_hook(path.clone());
    tracing::info!(
        "STR Launcher v{} starting (log: {})",
        crate::paths::APP_VERSION,
        path.display()
    );
    Some(path)
}

/// A `MakeWriter` that appends to one file, rotating it to `<name>.1` when it
/// grows past [`MAX_BYTES`]. Rotation failure never blocks the write.
struct RotatingLog {
    path: PathBuf,
}

impl RotatingLog {
    fn new(path: PathBuf) -> RotatingLog {
        RotatingLog { path }
    }

    fn rotated(&self) -> PathBuf {
        self.path.with_extension("log.1")
    }

    fn append(&self, bytes: &[u8]) {
        let size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if size >= MAX_BYTES {
            // One backup only: replace any previous .1, then move the current
            // log aside. Failure is fine — we just keep appending.
            let _ = std::fs::remove_file(self.rotated());
            let _ = std::fs::rename(&self.path, self.rotated());
        }
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = file.write_all(bytes);
        }
    }
}

struct FileWriter<'a> {
    log: &'a RotatingLog,
}

impl std::io::Write for FileWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.log.append(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for RotatingLog {
    type Writer = FileWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        FileWriter { log: self }
    }
}

/// Write one timestamped, panic-shaped line straight to the file — used by the
/// panic hook, which must not go back through the subscriber.
fn append_raw(path: &Path, line: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

/// Install a panic hook that records the panic (message + location) into the
/// log file before handing over to the default hook.
fn install_panic_hook(path: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".into());
        append_raw(
            &path,
            &format!(
                "PANIC at {location}: {payload}\n  launcher v{} ({} {})",
                crate::paths::APP_VERSION,
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        );
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("str-log-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn appends_and_rotates_at_the_size_limit() {
        let dir = tmp_dir("rotate");
        let path = log_path(&dir);
        let log = RotatingLog::new(path.clone());

        // Small writes accumulate in one file.
        log.append(b"line one\n");
        log.append(b"line two\n");
        assert!(path.is_file());
        assert_eq!(std::fs::read(&path).unwrap(), b"line one\nline two\n");

        // Pretend the file reached the limit: the next append rotates it.
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize]).unwrap();
        log.append(b"after rotation\n");
        assert_eq!(
            std::fs::read(path.with_extension("log.1")).unwrap(),
            vec![b'x'; MAX_BYTES as usize]
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"after rotation\n");

        // Appending again must not rotate again (current log is small).
        log.append(b"more\n");
        assert_eq!(std::fs::read(&path).unwrap(), b"after rotation\nmore\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_raw_creates_the_file_and_keeps_newlines() {
        let dir = tmp_dir("raw");
        let path = log_path(&dir);
        append_raw(&path, "PANIC at main.rs:1:1: boom");
        append_raw(&path, "PANIC at main.rs:2:1: again");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "PANIC at main.rs:1:1: boom\nPANIC at main.rs:2:1: again\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn make_writer_routes_through_append() {
        let dir = tmp_dir("makewriter");
        let path = log_path(&dir);
        let log = RotatingLog::new(path.clone());
        use std::io::Write;
        let mut writer = MakeWriter::make_writer(&log);
        writer.write_all(b"hello\n").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
