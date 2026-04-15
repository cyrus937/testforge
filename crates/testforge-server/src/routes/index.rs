//! Indexing endpoint — triggers full or incremental re-indexing.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::health::ErrorResponse;
use crate::state::{AppState, JobProgress};

#[derive(Deserialize)]
pub struct IndexRequest {
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default)]
    pub clean: bool,
}

fn default_path() -> String {
    ".".into()
}

#[derive(Serialize)]
pub struct IndexJobResponse {
    pub job_id: String,
    pub status: String,
    pub progress_ws: String,
}

/// `POST /api/index` — trigger indexing (async).
pub async fn trigger_index(
    State(state): State<Arc<AppState>>,
    Json(req): Json<IndexRequest>,
) -> Result<(StatusCode, Json<IndexJobResponse>), (StatusCode, Json<ErrorResponse>)> {
    let (job_id, tx) = state.create_job("idx").await;
    let ws_url = format!(
        "ws://{}:{}/ws/progress/{}",
        state.config.server.host, state.config.server.port, job_id
    );

    // Spawn async indexing task
    let state_clone = state.clone();
    let job_id_clone = job_id.clone();
    let clean = req.clean;

    tokio::spawn(async move {
        // Notify start
        let _ = tx.send(JobProgress::Progress {
            message: "Starting indexing...".into(),
            percent: Some(0.0),
            current_item: None,
        });

        // Run indexing
        let result = {
            let mut indexer = state_clone.indexer.lock().await;

            if clean {
                let _ = indexer.clear();
                let _ = tx.send(JobProgress::Progress {
                    message: "Cleared existing index".into(),
                    percent: Some(5.0),
                    current_item: None,
                });
            }

            indexer.index_full()
        };

        match result {
            Ok(report) => {
                // Build search index from symbols
                let _ = tx.send(JobProgress::Progress {
                    message: "Building search index...".into(),
                    percent: Some(80.0),
                    current_item: None,
                });

                let symbols = {
                    let indexer = state_clone.indexer.lock().await;
                    indexer.all_symbols().ok()
                };
                if let Some(symbols) = symbols {
                    let mut search = state_clone.search_engine.write().await;
                    let no_embeddings: Vec<Option<Vec<f32>>> = vec![None; symbols.len()];
                    let _ = search.index_symbols(&symbols, &no_embeddings);
                    let _ = search.commit();
                }

                let _ = tx.send(JobProgress::Complete {
                    message: "Indexing complete".into(),
                    result: serde_json::json!({
                        "files_indexed": report.files_indexed,
                        "symbols_extracted": report.symbols_extracted,
                        "files_skipped": report.files_skipped,
                        "files_failed": report.files_failed,
                    }),
                });
            }
            Err(e) => {
                let _ = tx.send(JobProgress::Error {
                    message: e.to_string(),
                });
            }
        }

        // Cleanup
        state_clone.remove_job(&job_id_clone).await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(IndexJobResponse {
            job_id,
            status: "running".into(),
            progress_ws: ws_url,
        }),
    ))
}
