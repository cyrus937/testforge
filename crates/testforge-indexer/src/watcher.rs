//! Filesystem watcher for incremental re-indexing.
//!
//! Monitors the project directory for changes and triggers re-indexing
//! of modified files. Uses [`notify`] for cross-platform support.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use testforge_core::models::Language;
use testforge_core::{Config, Result, TestForgeError};
use tracing::{debug, error, info, warn};

/// Events emitted by the file watcher.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A source file was created or modified.
    FileChanged(PathBuf),
    /// A source file was deleted.
    FileDeleted(PathBuf),
    /// A file was renamed (old path, new path).
    FileRenamed(PathBuf, PathBuf),
}

/// Watches the project directory for file changes.
pub struct FileWatcher {
    config: Config,
    root: PathBuf,
}

impl FileWatcher {
    pub fn new(config: Config, root: PathBuf) -> Self {
        Self { config, root }
    }

    /// Start watching and return a channel of watch events.
    ///
    /// This is a blocking call — run it in a dedicated thread.
    /// The watcher keeps running until the returned receiver is dropped.
    pub fn watch(&self) -> Result<(mpsc::Receiver<WatchEvent>, RecommendedWatcher)> {
        let (tx, rx) = mpsc::channel();
        let root = self.root.clone();
        let config = self.config.clone();

        let mut watcher = notify::recommended_watcher(
            move |res: std::result::Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        if let Some(watch_event) = classify_event(&event, &root, &config) {
                            if tx.send(watch_event).is_err() {
                                debug!("Watch event receiver dropped");
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "File watcher error");
                    }
                }
            },
        )
        .map_err(|e| TestForgeError::internal(format!("Failed to create watcher: {e}")))?;

        watcher
            .watch(&self.root, RecursiveMode::Recursive)
            .map_err(|e| TestForgeError::internal(format!("Failed to start watching: {e}")))?;

        info!(path = %self.root.display(), "File watcher started");

        Ok((rx, watcher))
    }

    /// Convenience: watch and process events with a callback.
    ///
    /// Debounces rapid changes (e.g., from `git checkout`) by
    /// batching events within a 200ms window.
    pub fn watch_with_handler<F>(&self, mut handler: F) -> Result<()>
    where
        F: FnMut(WatchEvent) + Send + 'static,
    {
        let (rx, _watcher) = self.watch()?;

        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(event) => {
                    // Collect any additional events that arrived
                    let mut batch = vec![event];
                    while let Ok(more) = rx.try_recv() {
                        batch.push(more);
                    }

                    // Deduplicate by path
                    batch.sort_by(|a, b| event_path(a).cmp(event_path(b)));
                    batch.dedup_by(|a, b| event_path(a) == event_path(b));

                    for evt in batch {
                        handler(evt);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    info!("File watcher stopped");
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Classify a raw notify event into a TestForge watch event.
///
/// Returns `None` for events we don't care about (non-source files,
/// excluded paths, etc.).
fn classify_event(event: &Event, root: &Path, config: &Config) -> Option<WatchEvent> {
    let paths = &event.paths;
    if paths.is_empty() {
        return None;
    }

    let path = &paths[0];

    // Only care about source code files
    let ext = path.extension()?.to_str()?;
    Language::from_extension(ext)?;

    // Skip excluded paths
    let rel_path = path.strip_prefix(root).ok()?;
    let rel_str = rel_path.to_string_lossy();
    for exclude in &config.project.exclude {
        if rel_str.contains(exclude.as_str()) {
            return None;
        }
    }

    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => {
            Some(WatchEvent::FileChanged(path.clone()))
        }
        EventKind::Remove(_) => Some(WatchEvent::FileDeleted(path.clone())),
        _ => None,
    }
}

fn event_path(event: &WatchEvent) -> &Path {
    match event {
        WatchEvent::FileChanged(p) | WatchEvent::FileDeleted(p) => p,
        WatchEvent::FileRenamed(_, new) => new,
    }
}
