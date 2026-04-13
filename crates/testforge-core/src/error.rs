//! Unified error types for the TestForge ecosystem.
//!
//! All crates in the workspace surface errors through [`TestForgeError`],
//! keeping the public API consistent and making it easy for callers to
//! pattern-match on failure modes.

use std::path::PathBuf;
use thiserror::Error;

/// Top-level error type returned by every public function in TestForge.
#[derive(Debug, Error)]
pub enum TestForgeError {
    // ── Configuration ───────────────────────────────────────────────
    #[error("configuration file not found: {path}")]
    ConfigNotFound { path: PathBuf },

    #[error("invalid configuration: {message}")]
    ConfigInvalid { message: String },

    #[error("failed to parse configuration: {source}")]
    ConfigParse {
        #[from]
        source: toml::de::Error,
    },

    // ── Indexing ────────────────────────────────────────────────────
    #[error("unsupported language: {language}")]
    UnsupportedLanguage { language: String },

    #[error("failed to parse file {path}: {reason}")]
    ParseError { path: PathBuf, reason: String },

    #[error("indexing failed for {path}: {reason}")]
    IndexingError { path: PathBuf, reason: String },

    // ── Search ──────────────────────────────────────────────────────
    #[error("index not initialised – run `testforge index` first")]
    IndexNotReady,

    #[error("embedding generation failed: {reason}")]
    EmbeddingError { reason: String },

    #[error("search failed: {reason}")]
    SearchError { reason: String },

    // ── Generation ──────────────────────────────────────────────────
    #[error("test generation failed for {target}: {reason}")]
    GenerationError { target: String, reason: String },

    #[error("LLM provider error: {reason}")]
    LlmError { reason: String },

    // ── I/O & Infra ─────────────────────────────────────────────────
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Internal(String),
}

/// Convenience alias used throughout the codebase.
pub type Result<T> = std::result::Result<T, TestForgeError>;