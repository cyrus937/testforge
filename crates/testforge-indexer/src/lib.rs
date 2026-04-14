//! # TestForge Indexer
//!
//! Parses source code using [tree-sitter](https://tree-sitter.github.io/) grammars,
//! extracts symbols (functions, classes, methods), computes content hashes, and
//! persists everything to a local SQLite database for fast retrieval.
//!
//! ## Architecture
//!
//! ```text
//! Source files ──► FileWalker ──► Parser ──► SymbolExtractor ──► Store
//!                   (ignore)     (tree-    (language-specific   (SQLite)
//!                                sitter)   AST queries)
//! ```

pub mod languages;
pub mod parser;
pub mod store;
pub mod symbols;
pub mod walker;
pub mod watcher;

use std::path::Path;

use testforge_core::models::{IndexStatus, IndexedFile, Language, Symbol};
use testforge_core::{Config, Result, TestForgeError};
use tracing::{info, warn};

pub use parser::Parser;
pub use store::IndexStore;
pub use walker::FileWalker;

/// High-level indexing orchestrator.
///
/// Ties together file walking, parsing, symbol extraction, and persistence.
pub struct Indexer {
    config: Config,
    project_root: std::path::PathBuf,
    store: IndexStore,
    parser: Parser,
}

impl Indexer {
    /// Create a new indexer for the given project root.
    pub fn new(config: Config, project_root: &Path) -> Result<Self> {
        let db_path = project_root
            .join(testforge_core::config::CONFIG_DIR)
            .join("index")
            .join("testforge.db");

        let store = IndexStore::open(&db_path)?;
        let parser = Parser::new()?;

        Ok(Self {
            config,
            project_root: project_root.to_path_buf(),
            store,
            parser,
        })
    }

    /// Perform a full index of the project.
    ///
    /// Walks all source files, parses them, extracts symbols, and stores
    /// them in the database. Uses content hashing to skip unchanged files.
    pub fn index_full(&mut self) -> Result<IndexReport> {
        let walker = FileWalker::new(&self.config, &self.project_root);
        let files = walker.collect_files()?;

        let mut report = IndexReport::default();

        info!(
            file_count = files.len(),
            "Starting full index of {}",
            self.project_root.display()
        );

        for file_path in &files {
            match self.index_file(file_path) {
                Ok(file_report) => {
                    report.files_indexed += 1;
                    report.symbols_extracted += file_report.symbol_count;
                    if file_report.was_skipped {
                        report.files_skipped += 1;
                    }
                }
                Err(e) => {
                    warn!(
                        path = %file_path.display(),
                        error = %e,
                        "Failed to index file"
                    );
                    report.files_failed += 1;
                    report.errors.push((file_path.clone(), e.to_string()));
                }
            }
        }

        info!(
            files = report.files_indexed,
            symbols = report.symbols_extracted,
            skipped = report.files_skipped,
            failed = report.files_failed,
            "Indexing complete"
        );

        Ok(report)
    }

    /// Index a single file. Returns information about what was extracted.
    ///
    /// If the file hasn't changed since the last index (same content hash),
    /// it is skipped and `FileReport::was_skipped` is set to `true`.
    pub fn index_file(&mut self, path: &Path) -> Result<FileReport> {
        let abs_path = if path.is_relative() {
            self.project_root.join(path)
        } else {
            path.to_path_buf()
        };

        let rel_path = abs_path
            .strip_prefix(&self.project_root)
            .unwrap_or(&abs_path)
            .to_path_buf();

        // Detect language
        let extension = abs_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let language = Language::from_extension(extension).ok_or_else(|| {
            TestForgeError::UnsupportedLanguage {
                language: extension.to_string(),
            }
        })?;

        // Check file size
        let metadata = std::fs::metadata(&abs_path)?;
        let size_kb = metadata.len() / 1024;
        if size_kb > self.config.indexer.max_file_size_kb {
            return Err(TestForgeError::FileTooLarge {
                path: rel_path,
                size_kb,
                max_kb: self.config.indexer.max_file_size_kb,
            });
        }

        // Read source and compute hash
        let source = std::fs::read_to_string(&abs_path)?;
        let content_hash = compute_hash(&source);

        // Skip if unchanged
        if let Some(existing) = self.store.get_file_hash(&rel_path)? {
            if existing == content_hash {
                return Ok(FileReport {
                    symbol_count: 0,
                    was_skipped: true,
                });
            }
        }

        // Parse and extract symbols
        let symbols = self
            .parser
            .parse_and_extract(&source, language, &rel_path)?;
        let symbol_count = symbols.len();

        // Store results
        let indexed_file = IndexedFile {
            path: rel_path,
            language,
            content_hash,
            symbol_count,
            line_count: source.lines().count(),
            indexed_at: chrono::Utc::now(),
        };

        self.store.upsert_file(&indexed_file)?;
        self.store.upsert_symbols(&symbols)?;

        Ok(FileReport {
            symbol_count,
            was_skipped: false,
        })
    }

    /// Get all symbols from the index.
    pub fn all_symbols(&self) -> Result<Vec<Symbol>> {
        self.store.all_symbols()
    }

    /// Get the current index status.
    pub fn status(&self) -> Result<IndexStatus> {
        self.store.status()
    }

    /// Clear the entire index.
    pub fn clear(&self) -> Result<()> {
        self.store.clear()
    }
}

/// Result of indexing a single file.
#[derive(Debug, Default)]
pub struct FileReport {
    pub symbol_count: usize,
    pub was_skipped: bool,
}

/// Result of a full indexing run.
#[derive(Debug, Default)]
pub struct IndexReport {
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub files_failed: usize,
    pub symbols_extracted: usize,
    pub errors: Vec<(std::path::PathBuf, String)>,
}

impl IndexReport {
    /// Formatted summary string for CLI output.
    pub fn summary(&self) -> String {
        format!(
            "Indexed {} files ({} symbols), skipped {}, failed {}",
            self.files_indexed, self.symbols_extracted, self.files_skipped, self.files_failed
        )
    }
}

/// Compute a SHA-256 hash of the given content, returned as a hex string.
pub fn compute_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}
