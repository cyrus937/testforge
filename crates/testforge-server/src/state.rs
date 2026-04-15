//! Shared application state for the API server.
//!
//! Wraps the indexer, search engine, and configuration in thread-safe
//! containers accessible from all route handlers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use testforge_core::{Config, Result};
use testforge_indexer::Indexer;
use testforge_search::SearchEngine;
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::info;

/// Shared state accessible by all route handlers.
pub struct AppState {
    pub config: Config,
    pub project_root: PathBuf,
    pub indexer: Mutex<Indexer>,
    pub search_engine: RwLock<SearchEngine>,
    pub start_time: Instant,
    /// Active job progress channels, keyed by job ID.
    pub jobs: RwLock<HashMap<String, broadcast::Sender<JobProgress>>>,
}

/// A progress update for a long-running job.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum JobProgress {
    #[serde(rename = "progress")]
    Progress {
        message: String,
        percent: Option<f32>,
        current_item: Option<String>,
    },
    #[serde(rename = "complete")]
    Complete {
        message: String,
        result: serde_json::Value,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

impl AppState {
    /// Initialize the application state.
    pub fn new(config: Config, project_root: &Path) -> Result<Self> {
        let index_dir = project_root
            .join(testforge_core::config::CONFIG_DIR)
            .join("index");
        let search_dir = project_root
            .join(testforge_core::config::CONFIG_DIR)
            .join("search");

        // Ensure directories exist
        std::fs::create_dir_all(&index_dir)?;
        std::fs::create_dir_all(&search_dir)?;

        let indexer = Indexer::new(config.clone(), project_root)?;
        let search_engine = SearchEngine::open(&search_dir, &config)?;

        info!("Server state initialized for {}", project_root.display());

        Ok(Self {
            config,
            project_root: project_root.to_path_buf(),
            indexer: Mutex::new(indexer),
            search_engine: RwLock::new(search_engine),
            start_time: Instant::now(),
            jobs: RwLock::new(HashMap::new()),
        })
    }

    /// Create a new job progress channel and return the job ID + sender.
    pub async fn create_job(&self, prefix: &str) -> (String, broadcast::Sender<JobProgress>) {
        let job_id = format!(
            "{}_{}",
            prefix,
            uuid::Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("x")
        );
        let (tx, _) = broadcast::channel(64);
        self.jobs.write().await.insert(job_id.clone(), tx.clone());
        (job_id, tx)
    }

    /// Remove a completed job.
    pub async fn remove_job(&self, job_id: &str) {
        self.jobs.write().await.remove(job_id);
    }

    /// Server uptime in seconds.
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }
}
