//! Unified error types for TestForge.
//!
//! All crates in the workspace funnel their errors through [`TestForgeError`],
//! which provides rich context for CLI display and API error responses.

use std::path::PathBuf;

/// Convenience type alias used throughout the project.
pub type Result<T> = std::result::Result<T, TestForgeError>;

/// Top-level error type for all TestForge operations.
///
/// Each variant carries enough context to produce a helpful error message
/// without requiring the caller to attach additional information.
#[derive(Debug, thiserror::Error)]
pub enum TestForgeError {
    // ── Configuration ────────────────────────────────────────────────
    #[error("Configuration file not found at {path}")]
    ConfigNotFound { path: PathBuf },

    #[error("Invalid configuration: {message}")]
    ConfigInvalid { message: String },

    #[error("Failed to parse configuration: {source}")]
    ConfigParse {
        #[from]
        source: toml::de::Error,
    },

    // ── Indexing ─────────────────────────────────────────────────────
    #[error("Unsupported language: {language}")]
    UnsupportedLanguage { language: String },

    #[error("Failed to parse file {path}: {reason}")]
    ParseError { path: PathBuf, reason: String },

    #[error("File not found: {path}")]
    FileNotFound { path: PathBuf },

    #[error("File too large ({size_kb} KB > {max_kb} KB limit): {path}")]
    FileTooLarge {
        path: PathBuf,
        size_kb: u64,
        max_kb: u64,
    },

    // ── Search ───────────────────────────────────────────────────────
    #[error("Index not initialized. Run `testforge index` first.")]
    IndexNotReady,

    #[error("Search query cannot be empty")]
    EmptyQuery,

    // ── AI / Embeddings ──────────────────────────────────────────────
    #[error("Embedding provider error: {message}")]
    EmbeddingError { message: String },

    #[error("LLM provider error: {message}")]
    LlmError { message: String },

    // ── Database ─────────────────────────────────────────────────────
    #[error("Database error: {source}")]
    Database {
        #[from]
        source: rusqlite::Error,
    },

    // ── I/O ──────────────────────────────────────────────────────────
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    #[error("JSON error: {source}")]
    Json {
        #[from]
        source: serde_json::Error,
    },

    // ── Generic ──────────────────────────────────────────────────────
    #[error("{0}")]
    Internal(String),
}

impl TestForgeError {
    /// Create an internal error from any displayable message.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Convenience constructor for parse errors.
    pub fn parse_error(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self::ParseError {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Returns a user-friendly suggestion for how to resolve this error.
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::ConfigNotFound { .. } => {
                Some("Run `testforge init` to create a configuration file.")
            }
            Self::IndexNotReady => Some("Run `testforge index .` to build the search index."),
            Self::UnsupportedLanguage { .. } => {
                Some("Supported languages: python, javascript, typescript, rust, java, go.")
            }
            Self::FileTooLarge { .. } => {
                Some("Increase `indexer.max_file_size_kb` in .testforge/config.toml.")
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_includes_path() {
        let err = TestForgeError::FileNotFound {
            path: PathBuf::from("src/main.rs"),
        };
        assert!(err.to_string().contains("src/main.rs"));
    }

    #[test]
    fn suggestion_returns_some_for_known_errors() {
        let err = TestForgeError::IndexNotReady;
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn suggestion_returns_none_for_generic_errors() {
        let err = TestForgeError::internal("something broke");
        assert!(err.suggestion().is_none());
    }
}
