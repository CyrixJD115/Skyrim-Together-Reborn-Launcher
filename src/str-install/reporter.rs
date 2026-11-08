//! Progress reporting plumbing shared by every install / undo stage.
//!
//! [`Reporter`] is the step/log facade handed to each pipeline stage;
//! [`ThrottledProgress`] caps byte-progress events at ~100 per operation so
//! the UI thread is not flooded during big downloads and extractions.

use tokio::sync::mpsc;

use super::InstallSummary;

/// Events streamed from a pipeline run to the UI.
#[derive(Debug, Clone)]
pub enum Event {
    /// A numbered step started.
    Step { label: String, index: usize, total: usize },
    /// Sub-operation progress.
    Progress { label: String, done: u64, total: u64 },
    Log(String),
    /// Steam is running, so the compatibility-tool change was deferred — the
    /// UI must confirm with a "Restart Steam" button (Linux).
    #[cfg(all(unix, not(target_os = "macos")))]
    NeedsSteamRestart { steam_root: std::path::PathBuf, tool: String },
    Done(Result<InstallSummary, String>),
}

/// Step/log facade handed to every pipeline stage.
pub struct Reporter {
    tx: mpsc::Sender<Event>,
}

impl Reporter {
    pub fn new(tx: mpsc::Sender<Event>) -> Self {
        Reporter { tx }
    }

    pub async fn step(&self, label: &str, index: usize) {
        let _ = self
            .tx
            .send(Event::Step { label: label.into(), index, total: super::TOTAL_STEPS })
            .await;
    }

    pub async fn log(&self, msg: impl Into<String>) {
        let _ = self.tx.send(Event::Log(msg.into())).await;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    pub fn sender(&self) -> mpsc::Sender<Event> {
        self.tx.clone()
    }

    pub fn progress_fn(&self, label: String) -> ThrottledProgress {
        ThrottledProgress::new(self.tx.clone(), label)
    }

    /// Send the terminal `Done` event for a pipeline run.
    pub async fn done(&self, result: Result<InstallSummary, String>) {
        let _ = self.tx.send(Event::Done(result)).await;
    }
}

/// Sends at most ~100 progress updates per operation.
pub struct ThrottledProgress {
    tx: mpsc::Sender<Event>,
    label: String,
    last: u64,
}

impl ThrottledProgress {
    pub fn new(tx: mpsc::Sender<Event>, label: String) -> Self {
        ThrottledProgress { tx, label, last: 0 }
    }

    pub fn send(&mut self, done: u64, total: u64) {
        if total == 0 {
            return;
        }
        if done == total || done * 100 >= (self.last + 1) * total / 100 {
            self.last = done;
            let _ = self.tx.try_send(Event::Progress {
                label: self.label.clone(),
                done,
                total,
            });
        }
    }
}
