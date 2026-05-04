//! Post-retrieval re-ranking and scoring utilities.
//!
//! After the initial retrieval (vector + text + RRF fusion), results
//! can be further refined using signals like symbol quality, recency,
//! and relevance to the query context.

use testforge_core::models::{SearchResult, SymbolKind, Visibility};

/// Apply post-retrieval re-ranking to search results.
///
/// Adjusts scores based on symbol quality signals:
/// - Public symbols get a boost over private ones
/// - Functions/methods get a boost over classes (more actionable)
/// - Symbols with docstrings get a boost (better documented)
/// - Very short symbols (< 3 lines) get a penalty (likely trivial)
pub fn rerank(results: &mut [SearchResult]) {
    for result in results.iter_mut() {
        let mut boost = 1.0;

        // Visibility boost
        boost *= match result.symbol.visibility {
            Visibility::Public => 1.1,
            Visibility::Internal => 1.0,
            Visibility::Protected => 0.95,
            Visibility::Private => 0.85,
        };


        // Kind boost — classes/structs are primary entities worth highlighting
        boost *= match result.symbol.kind {
            SymbolKind::Function | SymbolKind::Method => 0.95,
            SymbolKind::Class | SymbolKind::Struct => 1.1,
            SymbolKind::Trait | SymbolKind::Interface => 1.05,
            SymbolKind::Enum => 1.0,
            SymbolKind::Module => 0.9,
            SymbolKind::Constant => 0.8,
        };
        // Documentation boost
        if result.symbol.docstring.is_some() {
            boost *= 1.15;
        }
        if result.symbol.signature.is_some() {
            boost *= 1.05;
        }

        // Penalize trivially short symbols
        let lines = result.symbol.line_count();
        if lines < 3 {
            boost *= 0.7;
        } else if lines < 5 {
            boost *= 0.9;
        }

        // Penalize very long symbols (likely generated/boilerplate)
        if lines > 200 {
            boost *= 0.8;
        }

        result.score *= boost;
    }

    // Re-sort by adjusted score
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Deduplicate results that represent the same logical symbol.
///
/// This handles cases where both a method and its containing class
/// are returned, or when the same function appears from both
/// text and vector search with slightly different metadata.
pub fn deduplicate(results: &mut Vec<SearchResult>) {
    let mut seen_names = std::collections::HashSet::new();
    results.retain(|r| seen_names.insert(r.symbol.qualified_name.clone()));
}

/// Apply a diversity filter to avoid showing too many results
/// from the same file.
///
/// Ensures no single file contributes more than `max_per_file`
/// results in the final output.
pub fn diversify(results: &mut Vec<SearchResult>, max_per_file: usize) {
    let mut file_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    results.retain(|r| {
        let path = r.symbol.file_path.to_string_lossy().to_string();
        let count = file_counts.entry(path).or_insert(0);
        *count += 1;
        *count <= max_per_file
    });
}

/// Compute a relevance explanation for a search result.
///
/// Returns a human-readable string explaining why this result matched
/// and what signals contributed to its ranking.
pub fn explain_ranking(result: &SearchResult, query: &str) -> String {
    let mut factors = Vec::new();
    let query_lower = query.to_lowercase();

    // Check where the query matched
    if result.symbol.name.to_lowercase().contains(&query_lower) {
        factors.push("name match");
    }
    if result
        .symbol
        .qualified_name
        .to_lowercase()
        .contains(&query_lower)
    {
        factors.push("qualified name match");
    }
    if result
        .symbol
        .docstring
        .as_ref()
        .map(|d| d.to_lowercase().contains(&query_lower))
        .unwrap_or(false)
    {
        factors.push("docstring match");
    }
    if result.symbol.source.to_lowercase().contains(&query_lower) {
        factors.push("source code match");
    }

    // Quality signals
    if result.symbol.docstring.is_some() {
        factors.push("has documentation");
    }
    if result.symbol.visibility == Visibility::Public {
        factors.push("public symbol");
    }

    format!(
        "Score: {:.3} ({}) — {}",
        result.score,
        MatchSourceDisplay(result.match_source),
        factors.join(", ")
    )
}

pub struct MatchSourceDisplay(pub testforge_core::models::MatchSource);

impl std::fmt::Display for MatchSourceDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            testforge_core::models::MatchSource::Semantic => write!(f, "semantic"),
            testforge_core::models::MatchSource::FullText => write!(f, "full-text"),
            testforge_core::models::MatchSource::Hybrid => write!(f, "hybrid"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testforge_core::models::*;
    use uuid::Uuid;

    fn make_result(name: &str, score: f64, kind: SymbolKind, lines: usize) -> SearchResult {
        SearchResult {
            symbol: Symbol {
                id: Uuid::new_v4(),
                name: name.to_string(),
                qualified_name: name.to_string(),
                kind,
                language: Language::Python,
                file_path: "test.py".into(),
                start_line: 1,
                end_line: lines,
                source: "x".repeat(lines),
                signature: Some(format!("def {name}()")),
                docstring: Some("A documented function.".into()),
                dependencies: vec![],
                parent: None,
                visibility: Visibility::Public,
                content_hash: String::new(),
            },
            score,
            match_source: MatchSource::Hybrid,
        }
    }

    #[test]
    fn rerank_boosts_documented_functions() {
        let mut results = vec![
            {
                let mut r = make_result("undocumented", 1.0, SymbolKind::Function, 10);
                r.symbol.docstring = None;
                r.symbol.signature = None;
                r
            },
            make_result("documented", 1.0, SymbolKind::Function, 10),
        ];

        rerank(&mut results);

        // Documented should now rank higher
        assert_eq!(results[0].symbol.name, "documented");
    }

    #[test]
    fn rerank_penalizes_trivial_symbols() {
        let mut results = vec![
            make_result("trivial", 1.0, SymbolKind::Function, 2),
            make_result("substantial", 1.0, SymbolKind::Function, 20),
        ];

        rerank(&mut results);

        assert_eq!(results[0].symbol.name, "substantial");
    }

    #[test]
    fn deduplicate_removes_duplicates() {
        let mut results = vec![
            make_result("func_a", 0.9, SymbolKind::Function, 10),
            make_result("func_a", 0.8, SymbolKind::Function, 10),
            make_result("func_b", 0.7, SymbolKind::Function, 10),
        ];

        deduplicate(&mut results);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn diversify_limits_per_file() {
        let mut results = vec![
            make_result("f1", 0.9, SymbolKind::Function, 10),
            make_result("f2", 0.8, SymbolKind::Function, 10),
            make_result("f3", 0.7, SymbolKind::Function, 10),
        ];
        // All in "test.py"

        diversify(&mut results, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn explain_ranking_produces_output() {
        let result = make_result("auth_handler", 0.85, SymbolKind::Function, 15);
        let explanation = explain_ranking(&result, "auth");

        assert!(explanation.contains("0.850"));
        assert!(explanation.contains("name match"));
        assert!(explanation.contains("public symbol"));
    }
}
