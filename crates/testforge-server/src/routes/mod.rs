//! API route definitions and handlers.

pub mod generate;
pub mod health;
pub mod index;
pub mod search;
pub mod symbols;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

/// Build the `/api` router with all endpoints.
pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .route("/status", get(health::index_status))
        .route("/search", post(search::search))
        .route("/index", post(index::trigger_index))
        .route("/generate-tests", post(generate::generate_tests))
        .route("/generate-tests/{job_id}", get(generate::get_job_status))
        .route("/symbols", get(symbols::list_symbols))
        .route("/symbols/{name}", get(symbols::get_symbol))
        .with_state(state)
}
