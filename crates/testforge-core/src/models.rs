//! Domain types shared across all TestForge components.
//!
//! These structures form the common vocabulary of the system:
//! the indexer produces [`Symbol`]s, the search engine returns [`SearchResult`]s,
//! and the generator consumes [`CodeContext`] to produce tests.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Language ─────────────────────────────────────────────────────────

/// Programming languages supported by the indexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    JavaScript,
    TypeScript,
    Rust,
    Java,
    Go,
    CSharp,
}

impl Language {
    /// Detect language from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "py" => Some(Self::Python),
            "js" | "jsx" => Some(Self::JavaScript),
            "ts" | "tsx" => Some(Self::TypeScript),
            "rs" => Some(Self::Rust),
            "java" => Some(Self::Java),
            "go" => Some(Self::Go),
            "cs" => Some(Self::CSharp),
            _ => None,
        }
    }

    /// Return canonical file extensions for this language.
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Python => &["py"],
            Self::JavaScript => &["js", "jsx"],
            Self::TypeScript => &["ts", "tsx"],
            Self::Rust => &["rs"],
            Self::Java => &["java"],
            Self::Go => &["go"],
            Self::CSharp => &["cs"],
        }
    }

    /// Returns the default test framework for this language.
    pub fn default_test_framework(&self) -> &'static str {
        match self {
            Self::Python => "pytest",
            Self::JavaScript | Self::TypeScript => "jest",
            Self::Rust => "cargo-test",
            Self::Java => "junit",
            Self::Go => "go-test",
            Self::CSharp => "xunit",
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Python => write!(f, "python"),
            Self::JavaScript => write!(f, "javascript"),
            Self::TypeScript => write!(f, "typescript"),
            Self::Rust => write!(f, "rust"),
            Self::Java => write!(f, "java"),
            Self::Go => write!(f, "go"),
            Self::CSharp => write!(f, "csharp"),
        }
    }
}

// ── Symbol ───────────────────────────────────────────────────────────

/// The kind of code symbol extracted from the AST.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Module,
    Constant,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function => write!(f, "function"),
            Self::Method => write!(f, "method"),
            Self::Class => write!(f, "class"),
            Self::Struct => write!(f, "struct"),
            Self::Enum => write!(f, "enum"),
            Self::Interface => write!(f, "interface"),
            Self::Trait => write!(f, "trait"),
            Self::Module => write!(f, "module"),
            Self::Constant => write!(f, "constant"),
        }
    }
}

/// A code symbol extracted from parsing a source file.
///
/// Symbols are the fundamental indexing unit. Each represents a discrete
/// named entity in the codebase (function, class, method, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Unique identifier for this symbol.
    pub id: Uuid,

    /// Human-readable name (e.g., `"authenticate_user"`).
    pub name: String,

    /// Fully qualified name including parent scope (e.g., `"AuthService.authenticate_user"`).
    pub qualified_name: String,

    /// What kind of symbol this is.
    pub kind: SymbolKind,

    /// Language the symbol is written in.
    pub language: Language,

    /// File path relative to the project root.
    pub file_path: PathBuf,

    /// 1-based starting line number.
    pub start_line: usize,

    /// 1-based ending line number (inclusive).
    pub end_line: usize,

    /// The raw source code of this symbol.
    pub source: String,

    /// Extracted signature (e.g., `"def authenticate_user(self, username: str, password: str) -> bool"`).
    pub signature: Option<String>,

    /// Docstring or documentation comment, if present.
    pub docstring: Option<String>,

    /// Symbols that this symbol depends on (names of called functions, used types).
    pub dependencies: Vec<String>,

    /// Parent symbol name (e.g., class name for methods).
    pub parent: Option<String>,

    /// Visibility / access modifier.
    pub visibility: Visibility,

    /// SHA-256 hash of the source for change detection.
    pub content_hash: String,
}

/// Visibility level of a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Public,
    Private,
    Protected,
    Internal,
}

impl Symbol {
    /// Number of lines in this symbol's source.
    pub fn line_count(&self) -> usize {
        self.end_line.saturating_sub(self.start_line) + 1
    }

    /// Produce a concise one-line summary for CLI display.
    pub fn display_summary(&self) -> String {
        format!(
            "{} {} ({}:{}-{})",
            self.kind,
            self.qualified_name,
            self.file_path.display(),
            self.start_line,
            self.end_line,
        )
    }
}

// ── Indexed File ─────────────────────────────────────────────────────

/// Metadata about an indexed source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    /// Path relative to project root.
    pub path: PathBuf,

    /// Detected language.
    pub language: Language,

    /// SHA-256 of file contents (for incremental re-indexing).
    pub content_hash: String,

    /// Number of symbols extracted from this file.
    pub symbol_count: usize,

    /// Total lines of code.
    pub line_count: usize,

    /// Timestamp of last indexing.
    pub indexed_at: DateTime<Utc>,
}

// ── Search ───────────────────────────────────────────────────────────

/// A single result from a semantic or hybrid search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The matched symbol.
    pub symbol: Symbol,

    /// Relevance score in `[0.0, 1.0]`.
    pub score: f64,

    /// Which search method produced this result.
    pub match_source: MatchSource,
}

/// How a search result was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchSource {
    /// Matched via vector similarity (semantic).
    Semantic,
    /// Matched via full-text keyword search.
    FullText,
    /// Fused result from both methods.
    Hybrid,
}

// ── Code Context (for test generation) ───────────────────────────────

/// Rich context bundle sent to the LLM for test generation.
///
/// This is the key differentiator: instead of sending just the function
/// source, we send the full dependency graph, related tests, and project
/// conventions so the LLM can generate contextually appropriate tests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeContext {
    /// The primary symbol to generate tests for.
    pub target: Symbol,

    /// Direct dependencies (functions called, types used).
    pub dependencies: Vec<Symbol>,

    /// Existing tests in the project that test similar functionality.
    pub related_tests: Vec<Symbol>,

    /// Other symbols in the same file/module for context.
    pub siblings: Vec<Symbol>,

    /// Import statements from the target file.
    pub imports: Vec<String>,

    /// Project-level conventions detected (naming patterns, test structure).
    pub conventions: ProjectConventions,
}

/// Detected conventions in the project's existing test suite.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectConventions {
    /// Common test file naming pattern (e.g., `"test_{name}.py"`).
    pub test_file_pattern: Option<String>,

    /// Common assertion style (e.g., `"assert"`, `"expect"`).
    pub assertion_style: Option<String>,

    /// Whether fixtures/factories are used.
    pub uses_fixtures: bool,

    /// Common mock library (e.g., `"unittest.mock"`, `"jest.fn"`).
    pub mock_library: Option<String>,
}

// ── Index Status ─────────────────────────────────────────────────────

/// Summary of the current index state (for `testforge status`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatus {
    /// Total number of indexed files.
    pub file_count: usize,

    /// Total number of extracted symbols.
    pub symbol_count: usize,

    /// Total number of embeddings computed.
    pub embedding_count: usize,

    /// Languages present in the index.
    pub languages: Vec<Language>,

    /// Timestamp of last full index build.
    pub last_indexed: Option<DateTime<Utc>>,

    /// Whether the file watcher is running.
    pub watcher_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_from_extension() {
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("unknown"), None);
    }

    #[test]
    fn language_display() {
        assert_eq!(Language::Python.to_string(), "python");
        assert_eq!(Language::Rust.to_string(), "rust");
    }

    #[test]
    fn symbol_line_count() {
        let symbol = Symbol {
            id: Uuid::new_v4(),
            name: "test".into(),
            qualified_name: "test".into(),
            kind: SymbolKind::Function,
            language: Language::Python,
            file_path: "test.py".into(),
            start_line: 10,
            end_line: 25,
            source: String::new(),
            signature: None,
            docstring: None,
            dependencies: vec![],
            parent: None,
            visibility: Visibility::Public,
            content_hash: String::new(),
        };
        assert_eq!(symbol.line_count(), 16);
    }
}
