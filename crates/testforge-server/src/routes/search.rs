//! Search endpoint — hybrid semantic + full-text search.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use testforge_core::models::Language;
use testforge_search::{ranking, SearchQuery};

use super::health::ErrorResponse;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub filters: SearchFilters,
    #[serde(default = "default_semantic_weight")]
    pub semantic_weight: f32,
}

fn default_limit() -> usize {
    10
}
fn default_semantic_weight() -> f32 {
    0.6
}

#[derive(Default, Deserialize)]
pub struct SearchFilters {
    pub languages: Option<Vec<String>>,
    pub kinds: Option<Vec<String>>,
    pub paths: Option<Vec<String>>,
    pub visibility: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub total_results: usize,
    pub search_time_ms: u64,
}

#[derive(Serialize)]
pub struct SearchResultItem {
    pub symbol: serde_json::Value,
    pub score: f64,
    pub match_source: String,
}

/// `POST /api/search` — hybrid search.
pub async fn search(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<ErrorResponse>)> {
    if req.query.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request("Search query cannot be empty")),
        ));
    }

    let start = std::time::Instant::now();
    let engine = state.search_engine.read().await;

    // Build query
    let mut query = SearchQuery::new(&req.query)
        .with_limit(req.limit.min(100))
        .with_semantic_weight(req.semantic_weight);

    // Apply language filter (first one only for now)
    if let Some(ref langs) = req.filters.languages {
        if let Some(first) = langs.first() {
            if let Some(lang) = parse_language(first) {
                query = query.with_language(lang);
            }
        }
    }

    // Apply kind filter
    if let Some(ref kinds) = req.filters.kinds {
        if let Some(first) = kinds.first() {
            query = query.with_kind(first.to_lowercase());
        }
    }

    // Apply path filter
    if let Some(ref paths) = req.filters.paths {
        if let Some(first) = paths.first() {
            query = query.with_path_prefix(first.clone());
        }
    }

    // Execute search
    let mut results = engine.search(&query, None).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::internal(&e.to_string())),
        )
    })?;

    // Fallback to SQLite if search engine empty
    if results.is_empty() {
        drop(engine);
        let indexer = state.indexer.lock().await;
        let all = indexer.all_symbols().unwrap_or_default();
        let q_lower = req.query.to_lowercase();

        results = all
            .into_iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&q_lower)
                    || s.qualified_name.to_lowercase().contains(&q_lower)
                    || s.docstring
                        .as_ref()
                        .map(|d| d.to_lowercase().contains(&q_lower))
                        .unwrap_or(false)
                    || s.source.to_lowercase().contains(&q_lower)
            })
            .map(|s| testforge_core::models::SearchResult {
                symbol: s,
                score: 0.5,
                match_source: testforge_core::models::MatchSource::FullText,
            })
            .collect();
    }

    // Post-process
    ranking::rerank(&mut results);
    ranking::deduplicate(&mut results);
    ranking::diversify(&mut results, 5);
    results.truncate(req.limit);

    let elapsed = start.elapsed().as_millis() as u64;
    let total = results.len();

    let items: Vec<SearchResultItem> = results
        .into_iter()
        .map(|r| SearchResultItem {
            symbol: serde_json::to_value(&r.symbol).unwrap_or_default(),
            score: r.score,
            match_source: format!("{:?}", r.match_source).to_lowercase(),
        })
        .collect();

    Ok(Json(SearchResponse {
        results: items,
        total_results: total,
        search_time_ms: elapsed,
    }))
}

fn parse_language(s: &str) -> Option<Language> {
    match s.to_lowercase().as_str() {
        "python" | "py" => Some(Language::Python),
        "javascript" | "js" => Some(Language::JavaScript),
        "typescript" | "ts" => Some(Language::TypeScript),
        "rust" | "rs" => Some(Language::Rust),
        "java" => Some(Language::Java),
        "go" => Some(Language::Go),
        _ => None,
    }
}
