//! # testforge-indexer
//!
//! Walks a project directory, parses every supported source file with
//! tree-sitter, extracts symbols, builds a dependency graph, and
//! produces a complete [`ProjectIndex`] ready for search.

pub mod graph;
pub mod languages;
pub mod parser;
pub mod symbols;
pub mod watcher;

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Utc;
use ignore::WalkBuilder;
use rayon::prelude::*;
use tracing::{debug, info, instrument, warn};

use testforge_core::config::TestForgeConfig;
use testforge_core::models::{IndexStats, IndexedFile, LanguageStat};
use testforge_core::{CodeSymbol, Language, Result, TestForgeError};

use crate::graph::DependencyGraph;
use crate::watcher::{ChangeDetector, compute_hash};

// ─── Project Index ──────────────────────────────────────────────────

/// The complete, in-memory representation of an indexed project.
///
/// This is the central data structure that downstream consumers
/// (search engine, test generator) operate on.
#[derive(Debug)]
pub struct ProjectIndex {
    /// Root directory of the project.
    pub root: PathBuf,

    /// All extracted symbols, keyed by their ID.
    pub symbols: Vec<CodeSymbol>,

    /// Metadata for each indexed file.
    pub files: Vec<IndexedFile>,

    /// Call / dependency graph.
    pub graph: DependencyGraph,

    /// Change detector for incremental updates.
    pub change_detector: ChangeDetector,

    /// Summary statistics.
    pub stats: IndexStats,
}

impl ProjectIndex {
    /// Look up a symbol by its unique ID.
    pub fn get_symbol(&self, id: &str) -> Option<&CodeSymbol> {
        self.symbols.iter().find(|s| s.id == id)
    }

    /// Find symbols by name (may return multiple due to overloads).
    pub fn find_by_name(&self, name: &str) -> Vec<&CodeSymbol> {
        self.symbols.iter().filter(|s| s.name == name).collect()
    }

    /// Get all symbols in a specific file.
    pub fn symbols_in_file(&self, path: &Path) -> Vec<&CodeSymbol> {
        self.symbols
            .iter()
            .filter(|s| s.file_path == path)
            .collect()
    }

    /// Get all symbols of a specific kind.
    pub fn symbols_of_kind(&self, kind: testforge_core::SymbolKind) -> Vec<&CodeSymbol> {
        self.symbols.iter().filter(|s| s.kind == kind).collect()
    }
}

// ─── Indexer ────────────────────────────────────────────────────────

/// The indexing engine.  Call [`Indexer::index`] to build a
/// [`ProjectIndex`] from a directory tree.
pub struct Indexer {
    config: TestForgeConfig,
}

impl Indexer {
    /// Create an indexer with the given configuration.
    pub fn new(config: TestForgeConfig) -> Self {
        Self { config }
    }

    /// Create an indexer with default configuration.
    pub fn with_defaults() -> Self {
        Self {
            config: TestForgeConfig::default(),
        }
    }

    /// Index a project rooted at `root` and return the full index.
    #[instrument(skip(self), fields(root = %root.display()))]
    pub fn index(&self, root: &Path) -> Result<ProjectIndex> {
        let start = Instant::now();

        if !root.exists() {
            return Err(TestForgeError::IndexingError {
                path: root.to_path_buf(),
                reason: "directory does not exist".into(),
            });
        }

        info!(root = %root.display(), "starting indexation");

        // 1) Discover files.
        let files = self.discover_files(root)?;
        info!(count = files.len(), "source files discovered");

        // 2) Parse files in parallel.
        let parse_results: Vec<_> = files
            .par_iter()
            .filter_map(|(path, language)| {
                match self.parse_single_file(root, path, *language) {
                    Ok(result) => Some(result),
                    Err(e) => {
                        warn!(file = %path.display(), error = %e, "skipping file");
                        None
                    }
                }
            })
            .collect();

        // 3) Flatten symbols and build metadata.
        let mut all_symbols = Vec::new();
        let mut indexed_files = Vec::new();
        let mut change_detector = ChangeDetector::new();

        for (file_meta, file_symbols) in &parse_results {
            indexed_files.push(file_meta.clone());
            all_symbols.extend(file_symbols.iter().cloned());
        }

        // Record hashes for change detection.
        for f in &indexed_files {
            let full_path = root.join(&f.path);
            if let Ok(content) = std::fs::read(&full_path) {
                change_detector.record(&f.path, &content);
            }
        }

        // 4) Build dependency graph.
        let graph = DependencyGraph::build(&all_symbols);

        // 5) Compute statistics.
        let duration = start.elapsed();
        let stats = self.compute_stats(&indexed_files, &all_symbols, duration);

        info!(
            files = stats.total_files,
            symbols = stats.total_symbols,
            edges = graph.edge_count(),
            duration_ms = stats.index_duration_ms,
            "indexation complete"
        );

        Ok(ProjectIndex {
            root: root.to_path_buf(),
            symbols: all_symbols,
            files: indexed_files,
            graph,
            change_detector,
            stats,
        })
    }

    // ── File Discovery ──────────────────────────────────────────

    /// Walk the directory tree and collect supported source files.
    fn discover_files(&self, root: &Path) -> Result<Vec<(PathBuf, Language)>> {
        let max_size = self.config.indexer.max_file_size_kb * 1024;
        let exclude = &self.config.project.exclude;

        let mut files = Vec::new();

        let walker = WalkBuilder::new(root)
            .hidden(true) // respect hidden files
            .git_ignore(true) // respect .gitignore
            .build();

        for entry in walker {
            let entry = entry.map_err(|e| TestForgeError::IndexingError {
                path: root.to_path_buf(),
                reason: format!("walk error: {e}"),
            })?;

            let path = entry.path();

            // Skip directories.
            if !path.is_file() {
                continue;
            }

            // Skip excluded paths.
            if self.is_excluded(path, root, exclude) {
                continue;
            }

            // Skip oversized files.
            if let Ok(meta) = path.metadata() {
                if meta.len() > max_size {
                    debug!(file = %path.display(), "skipping oversized file");
                    continue;
                }
            }

            // Detect language.
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            if let Some(lang) = Language::from_extension(ext) {
                // Filter by configured languages if explicitly set.
                if !self.config.project.languages.is_empty()
                    && !self.config.project.languages.contains(&lang)
                {
                    continue;
                }
                files.push((path.to_path_buf(), lang));
            }
        }

        Ok(files)
    }

    /// Check if a path matches any exclusion pattern.
    fn is_excluded(&self, path: &Path, root: &Path, excludes: &[String]) -> bool {
        let relative = path.strip_prefix(root).unwrap_or(path);

        for component in relative.components() {
            let name = component.as_os_str().to_str().unwrap_or("");
            if excludes.iter().any(|ex| name == ex.as_str()) {
                return true;
            }
        }

        false
    }

    // ── Single File Parsing ─────────────────────────────────────

    /// Read and parse a single source file.
    fn parse_single_file(
        &self,
        root: &Path,
        path: &Path,
        language: Language,
    ) -> Result<(IndexedFile, Vec<CodeSymbol>)> {
        let content = std::fs::read_to_string(path)?;
        let relative = path.strip_prefix(root).unwrap_or(path);
        let content_hash = compute_hash(content.as_bytes());

        let symbols = parser::parse_file(&content, relative, language)?;

        let meta = IndexedFile {
            path: relative.to_path_buf(),
            language,
            content_hash,
            symbol_count: symbols.len(),
            indexed_at: Utc::now(),
        };

        Ok((meta, symbols))
    }

    // ── Statistics ───────────────────────────────────────────────

    fn compute_stats(
        &self,
        files: &[IndexedFile],
        symbols: &[CodeSymbol],
        duration: std::time::Duration,
    ) -> IndexStats {
        let mut lang_map: std::collections::HashMap<Language, (usize, usize)> =
            std::collections::HashMap::new();

        for f in files {
            let entry = lang_map.entry(f.language).or_default();
            entry.0 += 1;
            entry.1 += f.symbol_count;
        }

        let languages: Vec<LanguageStat> = lang_map
            .into_iter()
            .map(|(lang, (file_count, sym_count))| LanguageStat {
                language: lang,
                files: file_count,
                symbols: sym_count,
            })
            .collect();

        IndexStats {
            total_files: files.len(),
            total_symbols: symbols.len(),
            languages,
            last_indexed: Some(Utc::now()),
            index_duration_ms: duration.as_millis() as u64,
        }
    }
}