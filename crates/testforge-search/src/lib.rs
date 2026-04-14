//! # TestForge Search
//!
//! A hybrid search engine that combines **semantic vector search** with
//! **full-text keyword search** to find code symbols by intent.
//!
//! ## Architecture
//!
//! ```text
//! Query ──┬──► VectorStore (cosine similarity) ──► Top-K semantic
//!         │
//!         └──► TextIndex  (tantivy BM25)        ──► Top-K keyword
//!                                                        │
//!                                          Reciprocal Rank Fusion
//!                                                        │
//!                                                  Ranked results
//! ```
//!
//! ## Usage
//!
//! ```no_run
//! use testforge_search::SearchEngine;
//! use testforge_core::Config;
//! use std::path::Path;
//!
//! let engine = SearchEngine::open(Path::new(".testforge/search"), &Config::default())?;
//! let results = engine.search("payment validation", 10)?;
//! ```

pub mod hybrid;
pub mod ranking;
pub mod text_search;
pub mod vector_store;

use std::path::Path;

use testforge_core::models::{Language, SearchResult, Symbol};
use testforge_core::{Config, Result, TestForgeError};
use tracing::{debug, info};

use hybrid::HybridSearcher;
use text_search::TextIndex;
use vector_store::VectorStore;

/// Unified search engine combining vector and full-text search.
///
/// This is the main public API. It manages both indexes and provides
/// a single `search()` method that performs hybrid retrieval.
pub struct SearchEngine {
    vector_store: VectorStore,
    text_index: TextIndex,
    hybrid: HybridSearcher,
}

/// Options for a search query.
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// The search text (natural language or keywords).
    pub query: String,
    /// Maximum number of results to return.
    pub limit: usize,
    /// Optional language filter.
    pub language: Option<Language>,
    /// Optional file path prefix filter.
    pub path_prefix: Option<String>,
    /// Optional symbol kind filter (e.g., "function", "class").
    pub kind_filter: Option<String>,
    /// Weight for semantic results in [0.0, 1.0]. Default 0.6.
    pub semantic_weight: f32,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            limit: 10,
            language: None,
            path_prefix: None,
            kind_filter: None,
            semantic_weight: 0.6,
        }
    }
}

impl SearchQuery {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            ..Default::default()
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_language(mut self, lang: Language) -> Self {
        self.language = Some(lang);
        self
    }

    pub fn with_path_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.path_prefix = Some(prefix.into());
        self
    }

    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind_filter = Some(kind.into());
        self
    }

    pub fn with_semantic_weight(mut self, weight: f32) -> Self {
        self.semantic_weight = weight.clamp(0.0, 1.0);
        self
    }
}

impl SearchEngine {
    /// Open or create a search engine at the given directory.
    ///
    /// The directory will contain:
    /// - `vectors/` — serialized vector store
    /// - `tantivy/` — full-text search index
    pub fn open(search_dir: &Path, _config: &Config) -> Result<Self> {
        std::fs::create_dir_all(search_dir)?;

        let vector_dir = search_dir.join("vectors");
        let text_dir = search_dir.join("tantivy");

        let vector_store = VectorStore::open(&vector_dir)?;
        let text_index = TextIndex::open(&text_dir)?;
        let hybrid = HybridSearcher::new();

        info!(
            vectors = vector_store.len(),
            "Search engine opened at {}",
            search_dir.display()
        );

        Ok(Self {
            vector_store,
            text_index,
            hybrid,
        })
    }

    /// Index a symbol for both vector and full-text search.
    ///
    /// Call this after extracting symbols from source code and computing
    /// their embeddings.
    pub fn index_symbol(&mut self, symbol: &Symbol, embedding: Option<&[f32]>) -> Result<()> {
        // Add to full-text index
        self.text_index.add_symbol(symbol)?;

        // Add to vector store (if embedding is available)
        if let Some(vec) = embedding {
            self.vector_store.add(symbol.id, vec)?;
        }

        Ok(())
    }

    /// Batch-index multiple symbols with their embeddings.
    pub fn index_symbols(
        &mut self,
        symbols: &[Symbol],
        embeddings: &[Option<Vec<f32>>],
    ) -> Result<()> {
        if symbols.len() != embeddings.len() {
            return Err(TestForgeError::internal(
                "symbols and embeddings must have the same length",
            ));
        }

        for (sym, emb) in symbols.iter().zip(embeddings.iter()) {
            self.index_symbol(sym, emb.as_deref())?;
        }

        // Commit the text index after batch
        self.text_index.commit()?;

        info!(
            count = symbols.len(),
            vectors = self.vector_store.len(),
            "Batch indexed symbols"
        );

        Ok(())
    }

    /// Perform a hybrid search combining semantic and full-text results.
    ///
    /// If a query embedding is provided, both vector and text search run.
    /// Otherwise, only full-text search is used (graceful degradation).
    pub fn search(
        &self,
        query: &SearchQuery,
        query_embedding: Option<&[f32]>,
    ) -> Result<Vec<SearchResult>> {
        if query.query.trim().is_empty() {
            return Err(TestForgeError::EmptyQuery);
        }

        let k = query.limit * 3; // Over-fetch for re-ranking

        // Full-text search (always available)
        let text_results = self.text_index.search(&query.query, k)?;
        debug!(count = text_results.len(), "Full-text results");

        // Vector search (only if embedding is provided)
        let vector_results = if let Some(embedding) = query_embedding {
            let results = self.vector_store.search(embedding, k)?;
            debug!(count = results.len(), "Vector results");
            results
        } else {
            debug!("No query embedding — skipping vector search");
            Vec::new()
        };

        // Hybrid fusion
        let fused = self.hybrid.fuse(
            &text_results,
            &vector_results,
            query.semantic_weight,
            query.limit,
        );

        debug!(count = fused.len(), "Hybrid results after fusion");

        // Apply filters
        let filtered = self.apply_filters(fused, query);

        Ok(filtered)
    }

    /// Full-text only search (no embeddings required).
    pub fn search_text(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        if query.trim().is_empty() {
            return Err(TestForgeError::EmptyQuery);
        }
        self.text_index.search(query, limit)
    }

    /// Vector-only search.
    pub fn search_vectors(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<vector_store::VectorMatch>> {
        self.vector_store.search(embedding, limit)
    }

    /// Commit any pending writes to the text index.
    pub fn commit(&mut self) -> Result<()> {
        self.text_index.commit()
    }

    /// Clear all search indexes.
    pub fn clear(&mut self) -> Result<()> {
        self.vector_store.clear()?;
        self.text_index.clear()?;
        Ok(())
    }

    /// Number of vectors in the store.
    pub fn vector_count(&self) -> usize {
        self.vector_store.len()
    }

    /// Number of documents in the text index.
    pub fn text_doc_count(&self) -> Result<usize> {
        self.text_index.doc_count()
    }

    /// Apply post-search filters (language, path, kind).
    fn apply_filters(&self, results: Vec<SearchResult>, query: &SearchQuery) -> Vec<SearchResult> {
        results
            .into_iter()
            .filter(|r| {
                if let Some(ref lang) = query.language {
                    if r.symbol.language != *lang {
                        return false;
                    }
                }
                if let Some(ref prefix) = query.path_prefix {
                    if !r.symbol.file_path.to_string_lossy().starts_with(prefix.as_str()) {
                        return false;
                    }
                }
                if let Some(ref kind) = query.kind_filter {
                    if r.symbol.kind.to_string() != *kind {
                        return false;
                    }
                }
                true
            })
            .take(query.limit)
            .collect()
    }
}