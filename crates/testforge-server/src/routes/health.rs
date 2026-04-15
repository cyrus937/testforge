//! Health check and index status endpoints.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub index_ready: bool,
    pub uptime_seconds: u64,
}

/// `GET /api/health` — server health check.
pub async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let indexer = state.indexer.lock().await;
    let status = indexer.status();
    let index_ready = status.map(|s| s.file_count > 0).unwrap_or(false);

    Json(HealthResponse {
        status: "healthy".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        index_ready,
        uptime_seconds: state.uptime_seconds(),
    })
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub file_count: usize,
    pub symbol_count: usize,
    pub embedding_count: usize,
    pub languages: Vec<String>,
    pub last_indexed: Option<String>,
    pub watcher_active: bool,
    pub vector_count: usize,
    pub text_doc_count: usize,
}

/// `GET /api/status` — index statistics.
pub async fn index_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let indexer = state.indexer.lock().await;
    let status = indexer.status().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::internal(&e.to_string())),
        )
    })?;

    let search = state.search_engine.read().await;
    let text_count = search.text_doc_count().unwrap_or(0);
    let vec_count = search.vector_count();

    Ok(Json(StatusResponse {
        file_count: status.file_count,
        symbol_count: status.symbol_count,
        embedding_count: status.embedding_count,
        languages: status.languages.iter().map(|l| l.to_string()).collect(),
        last_indexed: status.last_indexed.map(|t| t.to_rfc3339()),
        watcher_active: status.watcher_active,
        vector_count: vec_count,
        text_doc_count: text_count,
    }))
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    pub suggestion: Option<String>,
}

impl ErrorResponse {
    pub fn new(code: &str, message: &str, suggestion: Option<&str>) -> Self {
        Self {
            error: ErrorDetail {
                code: code.into(),
                message: message.into(),
                suggestion: suggestion.map(String::from),
            },
        }
    }

    pub fn internal(message: &str) -> Self {
        Self::new("INTERNAL", message, None)
    }

    pub fn not_found(message: &str) -> Self {
        Self::new("NOT_FOUND", message, None)
    }

    pub fn bad_request(message: &str) -> Self {
        Self::new("BAD_REQUEST", message, None)
    }
}
