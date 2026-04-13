//! Filesystem watcher for incremental re-indexing.
//!
//! Phase 1 provides a simple hash-based change detection approach.
//! Full `notify`-based live watching is planned for Phase 4.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Tracks file content hashes to detect changes.
#[derive(Debug, Default)]
pub struct ChangeDetector {
    /// Maps file path → SHA-256 hex digest of last-indexed content.
    hashes: HashMap<PathBuf, String>,
}

impl ChangeDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a file's content hash.
    pub fn record(&mut self, path: &Path, content: &[u8]) {
        let hash = compute_hash(content);
        self.hashes.insert(path.to_path_buf(), hash);
    }

    /// Check whether a file has changed since last recording.
    ///
    /// Returns `true` if:
    /// - The file has never been recorded, or
    /// - Its content hash differs from the recorded value.
    pub fn has_changed(&self, path: &Path, content: &[u8]) -> bool {
        match self.hashes.get(path) {
            Some(prev) => *prev != compute_hash(content),
            None => true,
        }
    }

    /// Remove a file from tracking (e.g. when deleted).
    pub fn remove(&mut self, path: &Path) {
        self.hashes.remove(path);
    }

    /// Number of tracked files.
    pub fn tracked_count(&self) -> usize {
        self.hashes.len()
    }

    /// Export hashes for persistence.
    pub fn export(&self) -> &HashMap<PathBuf, String> {
        &self.hashes
    }

    /// Import previously-persisted hashes.
    pub fn import(&mut self, data: HashMap<PathBuf, String>) {
        self.hashes = data;
    }
}

/// Compute the SHA-256 hex digest of `data`.
pub fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}