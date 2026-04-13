//! Shared domain models used across all TestForge crates.
//!
//! These structs form the canonical representation of indexed code —
//! every crate agrees on what a "symbol" or a "search result" looks like.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─── Language Support ───────────────────────────────────────────────

/// Programming languages TestForge can parse and understand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    Rust,
    JavaScript,
    TypeScript,
    Java,
    Go,
    CSharp,
}

impl Language {
    /// File extensions associated with this language.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Python => &["py", "pyi"],
            Self::Rust => &["rs"],
            Self::JavaScript => &["js", "jsx", "mjs", "cjs"],
            Self::TypeScript => &["ts", "tsx"],
            Self::Java => &["java"],
            Self::Go => &["go"],
            Self::CSharp => &["cs"],
        }
    }

    /// Attempt to detect the language from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "py" | "pyi" => Some(Self::Python),
            "rs" => Some(Self::Rust),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" | "tsx" => Some(Self::TypeScript),
            "java" => Some(Self::Java),
            "go" => Some(Self::Go),
            "cs" => Some(Self::CSharp),
            _ => None,
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Python => write!(f, "python"),
            Self::Rust => write!(f, "rust"),
            Self::JavaScript => write!(f, "javascript"),
            Self::TypeScript => write!(f, "typescript"),
            Self::Java => write!(f, "java"),
            Self::Go => write!(f, "go"),
            Self::CSharp => write!(f, "csharp"),
        }
    }
}

// ─── Symbol Types ───────────────────────────────────────────────────

/// The kind of code symbol extracted during indexing.
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
    Variable,
    Constant,
    Import,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Function => "fn",
            Self::Method => "method",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Module => "module",
            Self::Variable => "var",
            Self::Constant => "const",
            Self::Import => "import",
        };
        write!(f, "{label}")
    }
}

// ─── Code Symbol ────────────────────────────────────────────────────

/// A single symbol extracted from a source file.
///
/// This is the fundamental unit TestForge indexes.  Every function,
/// class, method, etc. becomes one `CodeSymbol`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSymbol {
    /// Fully-qualified name (e.g. `auth.permissions.check_access`).
    pub name: String,

    /// The kind of symbol.
    pub kind: SymbolKind,

    /// Language the symbol is written in.
    pub language: Language,

    /// Path to the source file (relative to project root).
    pub file_path: PathBuf,

    /// Inclusive line range `[start, end]` (1-based).
    pub line_start: usize,
    pub line_end: usize,

    /// The raw source code of the symbol body.
    pub body: String,

    /// Signature / declaration line (without body).
    pub signature: String,

    /// Docstring or leading comment, if present.
    pub doc: Option<String>,

    /// Fully-qualified names this symbol depends on (calls / imports).
    pub dependencies: Vec<String>,

    /// Unique identifier (derived from path + name + line).
    pub id: String,
}

impl CodeSymbol {
    /// Build a deterministic ID for this symbol.
    pub fn compute_id(file_path: &PathBuf, name: &str, line_start: usize) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        file_path.hash(&mut hasher);
        name.hash(&mut hasher);
        line_start.hash(&mut hasher);
        format!("sym_{:016x}", hasher.finish())
    }
}

// ─── Indexed File Metadata ──────────────────────────────────────────

/// Metadata about a file that has been indexed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    /// Path relative to project root.
    pub path: PathBuf,

    /// Detected language.
    pub language: Language,

    /// SHA-256 hash of the file contents at index time.
    pub content_hash: String,

    /// Number of symbols extracted.
    pub symbol_count: usize,

    /// When this file was last indexed.
    pub indexed_at: DateTime<Utc>,
}

// ─── Search ─────────────────────────────────────────────────────────

/// A single search result returned to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The matched symbol.
    pub symbol: CodeSymbol,

    /// Relevance score in `[0.0, 1.0]`.
    pub score: f64,

    /// Short explanation of *why* this result matched.
    pub match_reason: Option<String>,
}

// ─── Index Statistics ───────────────────────────────────────────────

/// Summary statistics for the current index state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub total_files: usize,
    pub total_symbols: usize,
    pub languages: Vec<LanguageStat>,
    pub last_indexed: Option<DateTime<Utc>>,
    pub index_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageStat {
    pub language: Language,
    pub files: usize,
    pub symbols: usize,
}