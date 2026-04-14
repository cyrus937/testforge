//! Hybrid search via Reciprocal Rank Fusion (RRF).
//!
//! Combines results from vector (semantic) and full-text (keyword) search
//! into a single ranked list. RRF is a robust fusion method that works
//! without score calibration between the two retrieval systems.
//!
//! ## Algorithm
//!
//! For each document `d` appearing in any result list:
//!
//! ```text
//! RRF(d) = Σ (weight_i / (k + rank_i(d)))
//! ```
//!
//! where `k` is a smoothing constant (default 60) and `rank_i(d)` is the
//! 1-based rank of `d` in list `i`. Documents not in a list get rank ∞
//! (contribute 0).
//!
//! ## References
//!
//! Cormack, Clarke, Buettcher. "Reciprocal Rank Fusion outperforms
//! Condorcet and individual Rank Learning Methods." SIGIR 2009.

use std::collections::HashMap;

use testforge_core::models::{MatchSource, SearchResult, Symbol};
use tracing::debug;
use uuid::Uuid;

use crate::vector_store::VectorMatch;

/// Smoothing constant for RRF. Higher values give more weight to lower-ranked
/// results. 60 is the standard value from the original paper.
const RRF_K: f64 = 60.0;

/// Combines vector and text search results using Reciprocal Rank Fusion.
pub struct HybridSearcher {
    /// RRF smoothing constant.
    k: f64,
}

impl HybridSearcher {
    pub fn new() -> Self {
        Self { k: RRF_K }
    }

    /// Create with a custom smoothing constant.
    pub fn with_k(k: f64) -> Self {
        Self { k }
    }

    /// Fuse text and vector search results into a single ranked list.
    ///
    /// # Parameters
    ///
    /// - `text_results` — Results from tantivy BM25 search.
    /// - `vector_results` — Results from vector cosine similarity search.
    /// - `semantic_weight` — Weight for semantic results in [0.0, 1.0].
    ///   `0.0` = text only, `1.0` = semantic only, `0.6` = default balance.
    /// - `limit` — Maximum number of results to return.
    pub fn fuse(
        &self,
        text_results: &[SearchResult],
        vector_results: &[VectorMatch],
        semantic_weight: f32,
        limit: usize,
    ) -> Vec<SearchResult> {
        let text_weight = 1.0 - semantic_weight as f64;
        let semantic_weight = semantic_weight as f64;

        // Build a map of UUID → (RRF score, best symbol, match sources)
        let mut scores: HashMap<Uuid, FusionEntry> = HashMap::new();

        // Score text results
        for (rank, result) in text_results.iter().enumerate() {
            let rrf_score = text_weight / (self.k + (rank + 1) as f64);

            let entry = scores.entry(result.symbol.id).or_insert_with(|| FusionEntry {
                rrf_score: 0.0,
                symbol: result.symbol.clone(),
                in_text: false,
                in_vector: false,
                text_rank: None,
                vector_rank: None,
                text_score: 0.0,
                vector_score: 0.0,
            });

            entry.rrf_score += rrf_score;
            entry.in_text = true;
            entry.text_rank = Some(rank);
            entry.text_score = result.score;
        }

        // Score vector results
        // Note: vector results only have UUIDs, not full symbols.
        // We need to match them against text results or skip if not found.
        for (rank, vmatch) in vector_results.iter().enumerate() {
            let rrf_score = semantic_weight / (self.k + (rank + 1) as f64);

            if let Some(entry) = scores.get_mut(&vmatch.id) {
                entry.rrf_score += rrf_score;
                entry.in_vector = true;
                entry.vector_rank = Some(rank);
                entry.vector_score = vmatch.score as f64;
            } else {
                // Vector-only result — we don't have the full symbol.
                // Store a placeholder; the caller must resolve it.
                scores.insert(
                    vmatch.id,
                    FusionEntry {
                        rrf_score,
                        symbol: placeholder_symbol(vmatch.id),
                        in_text: false,
                        in_vector: true,
                        text_rank: None,
                        vector_rank: Some(rank),
                        text_score: 0.0,
                        vector_score: vmatch.score as f64,
                    },
                );
            }
        }

        // Sort by RRF score descending
        let mut fused: Vec<_> = scores.into_values().collect();
        fused.sort_by(|a, b| {
            b.rrf_score
                .partial_cmp(&a.rrf_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        fused.truncate(limit);

        debug!(
            text = text_results.len(),
            vector = vector_results.len(),
            fused = fused.len(),
            "RRF fusion complete"
        );

        // Convert to SearchResult
        fused
            .into_iter()
            .map(|entry| {
                let match_source = match (entry.in_text, entry.in_vector) {
                    (true, true) => MatchSource::Hybrid,
                    (true, false) => MatchSource::FullText,
                    (false, true) => MatchSource::Semantic,
                    (false, false) => MatchSource::FullText, // shouldn't happen
                };

                SearchResult {
                    symbol: entry.symbol,
                    score: entry.rrf_score,
                    match_source,
                }
            })
            .collect()
    }

    /// Fuse results from multiple ranked lists (generic version).
    ///
    /// Each list is a `Vec<(Uuid, f64)>` of (id, original_score) pairs.
    /// Returns `(Uuid, f64)` pairs sorted by RRF score.
    pub fn fuse_generic(
        &self,
        lists: &[Vec<(Uuid, f64)>],
        weights: &[f64],
        limit: usize,
    ) -> Vec<(Uuid, f64)> {
        let mut scores: HashMap<Uuid, f64> = HashMap::new();

        for (list, &weight) in lists.iter().zip(weights.iter()) {
            for (rank, (id, _original_score)) in list.iter().enumerate() {
                let rrf = weight / (self.k + (rank + 1) as f64);
                *scores.entry(*id).or_default() += rrf;
            }
        }

        let mut sorted: Vec<_> = scores.into_iter().collect();
        sorted.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted.truncate(limit);
        sorted
    }
}

impl Default for HybridSearcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Internal tracking structure for fusion.
struct FusionEntry {
    rrf_score: f64,
    symbol: Symbol,
    in_text: bool,
    in_vector: bool,
    text_rank: Option<usize>,
    vector_rank: Option<usize>,
    text_score: f64,
    vector_score: f64,
}

/// Create a placeholder symbol for vector-only results.
///
/// These need to be resolved against the full symbol database before
/// returning to the user.
fn placeholder_symbol(id: Uuid) -> Symbol {
    Symbol {
        id,
        name: String::new(),
        qualified_name: String::new(),
        kind: testforge_core::models::SymbolKind::Function,
        language: testforge_core::models::Language::Python,
        file_path: std::path::PathBuf::new(),
        start_line: 0,
        end_line: 0,
        source: String::new(),
        signature: None,
        docstring: None,
        dependencies: Vec::new(),
        parent: None,
        visibility: testforge_core::models::Visibility::Public,
        content_hash: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testforge_core::models::*;

    fn make_result(name: &str, score: f64) -> SearchResult {
        SearchResult {
            symbol: Symbol {
                id: Uuid::new_v4(),
                name: name.to_string(),
                qualified_name: name.to_string(),
                kind: SymbolKind::Function,
                language: Language::Python,
                file_path: "test.py".into(),
                start_line: 1,
                end_line: 5,
                source: format!("def {name}(): pass"),
                signature: None,
                docstring: None,
                dependencies: vec![],
                parent: None,
                visibility: Visibility::Public,
                content_hash: String::new(),
            },
            score,
            match_source: MatchSource::FullText,
        }
    }

    fn make_vector_match(id: Uuid, score: f32, rank: usize) -> VectorMatch {
        VectorMatch { id, score, rank }
    }

    #[test]
    fn text_only_returns_text_results() {
        let hybrid = HybridSearcher::new();
        let text = vec![make_result("func_a", 0.9), make_result("func_b", 0.7)];

        let results = hybrid.fuse(&text, &[], 0.5, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].match_source, MatchSource::FullText);
    }

    #[test]
    fn hybrid_results_marked_correctly() {
        let hybrid = HybridSearcher::new();

        let text = vec![make_result("shared_func", 0.9)];
        let shared_id = text[0].symbol.id;

        let vector = vec![make_vector_match(shared_id, 0.95, 0)];

        let results = hybrid.fuse(&text, &vector, 0.5, 10);

        // The shared result should be marked as Hybrid
        let shared = results.iter().find(|r| r.symbol.id == shared_id).unwrap();
        assert_eq!(shared.match_source, MatchSource::Hybrid);
    }

    #[test]
    fn hybrid_result_scores_higher_than_single_source() {
        let hybrid = HybridSearcher::new();

        let text = vec![
            make_result("both_sources", 0.9),
            make_result("text_only", 0.8),
        ];
        let both_id = text[0].symbol.id;

        let vector = vec![make_vector_match(both_id, 0.95, 0)];

        let results = hybrid.fuse(&text, &vector, 0.5, 10);

        let both = results.iter().find(|r| r.symbol.id == both_id).unwrap();
        let text_only = results.iter().find(|r| r.symbol.name == "text_only").unwrap();

        assert!(
            both.score > text_only.score,
            "Result in both lists should score higher: {} > {}",
            both.score,
            text_only.score
        );
    }

    #[test]
    fn semantic_weight_zero_ignores_vectors() {
        let hybrid = HybridSearcher::new();
        let text = vec![make_result("text_func", 0.9)];
        let vector = vec![make_vector_match(Uuid::new_v4(), 0.99, 0)];

        let results = hybrid.fuse(&text, &vector, 0.0, 10);
        // Text result should dominate
        assert_eq!(results[0].symbol.name, "text_func");
    }

    #[test]
    fn limit_respected() {
        let hybrid = HybridSearcher::new();
        let text: Vec<_> = (0..20)
            .map(|i| make_result(&format!("func_{i}"), 1.0 / (i + 1) as f64))
            .collect();

        let results = hybrid.fuse(&text, &[], 0.5, 5);
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn generic_fusion() {
        let hybrid = HybridSearcher::new();

        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let id_c = Uuid::new_v4();

        let list1 = vec![(id_a, 0.9), (id_b, 0.7)];
        let list2 = vec![(id_b, 0.95), (id_c, 0.8)];

        let results = hybrid.fuse_generic(
            &[list1, list2],
            &[0.5, 0.5],
            10,
        );

        // id_b appears in both lists → should rank highest
        assert_eq!(results[0].0, id_b);
    }
}