//! Symbol listing and detail endpoints.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::health::ErrorResponse;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct ListQuery {
    pub file: Option<String>,
    pub kind: Option<String>,
    pub language: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    100
}

#[derive(Serialize)]
pub struct ListResponse {
    pub symbols: Vec<serde_json::Value>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// `GET /api/symbols` — list indexed symbols with filtering.
pub async fn list_symbols(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<ListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let indexer = state.indexer.lock().await;
    let all = indexer.all_symbols().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::internal(&e.to_string())),
        )
    })?;

    let filtered: Vec<_> = all
        .into_iter()
        .filter(|s| {
            if let Some(ref file) = query.file {
                if !s.file_path.to_string_lossy().contains(file.as_str()) {
                    return false;
                }
            }
            if let Some(ref kind) = query.kind {
                if s.kind.to_string() != kind.to_lowercase() {
                    return false;
                }
            }
            if let Some(ref lang) = query.language {
                if s.language.to_string() != lang.to_lowercase() {
                    return false;
                }
            }
            true
        })
        .collect();

    let total = filtered.len();
    let page: Vec<_> = filtered
        .into_iter()
        .skip(query.offset)
        .take(query.limit)
        .filter_map(|s| serde_json::to_value(&s).ok())
        .collect();

    Ok(Json(ListResponse {
        symbols: page,
        total,
        limit: query.limit,
        offset: query.offset,
    }))
}

#[derive(Serialize)]
pub struct SymbolDetailResponse {
    pub symbol: serde_json::Value,
    pub context: SymbolContext,
}

#[derive(Serialize)]
pub struct SymbolContext {
    pub dependencies: Vec<String>,
    pub callers: Vec<String>,
    pub siblings: Vec<String>,
}

/// `GET /api/symbols/:name` — get details for a specific symbol.
pub async fn get_symbol(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<SymbolDetailResponse>, (StatusCode, Json<ErrorResponse>)> {
    let indexer = state.indexer.lock().await;
    let all = indexer.all_symbols().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::internal(&e.to_string())),
        )
    })?;

    let symbol = all
        .iter()
        .find(|s| s.qualified_name == name || s.name == name)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::not_found(&format!(
                    "Symbol '{}' not found",
                    name
                ))),
            )
        })?;

    // Find callers (symbols whose dependencies include this one)
    let callers: Vec<String> = all
        .iter()
        .filter(|s| s.dependencies.contains(&symbol.name))
        .map(|s| s.qualified_name.clone())
        .collect();

    // Find siblings (same file)
    let siblings: Vec<String> = all
        .iter()
        .filter(|s| s.file_path == symbol.file_path && s.qualified_name != symbol.qualified_name)
        .map(|s| s.qualified_name.clone())
        .collect();

    Ok(Json(SymbolDetailResponse {
        symbol: serde_json::to_value(symbol).unwrap_or_default(),
        context: SymbolContext {
            dependencies: symbol.dependencies.clone(),
            callers,
            siblings,
        },
    }))
}
