//! Call / dependency graph construction.
//!
//! Builds an in-memory directed graph where nodes are symbols and
//! edges represent "A calls B" relationships.  This graph is used
//! later to give the LLM richer context when generating tests.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use testforge_core::CodeSymbol;

/// A lightweight directed graph of symbol dependencies.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DependencyGraph {
    /// Maps symbol ID → list of symbol IDs it calls.
    edges: HashMap<String, Vec<String>>,

    /// Reverse index: symbol ID → list of symbol IDs that call it.
    reverse_edges: HashMap<String, Vec<String>>,

    /// Quick lookup: symbol name → symbol IDs (handles overloads).
    name_index: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    /// Build the graph from a flat list of symbols.
    pub fn build(symbols: &[CodeSymbol]) -> Self {
        let mut graph = Self::default();

        // Index symbols by name for dependency resolution.
        for sym in symbols {
            graph
                .name_index
                .entry(sym.name.clone())
                .or_default()
                .push(sym.id.clone());
        }

        // Wire up edges.
        for sym in symbols {
            let mut targets = Vec::new();
            for dep_name in &sym.dependencies {
                if let Some(ids) = graph.name_index.get(dep_name) {
                    targets.extend(ids.iter().cloned());
                }
            }
            targets.sort();
            targets.dedup();

            for target in &targets {
                graph
                    .reverse_edges
                    .entry(target.clone())
                    .or_default()
                    .push(sym.id.clone());
            }

            graph.edges.insert(sym.id.clone(), targets);
        }

        graph
    }

    /// What does this symbol call?
    pub fn callees(&self, symbol_id: &str) -> &[String] {
        self.edges.get(symbol_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// What calls this symbol?
    pub fn callers(&self, symbol_id: &str) -> &[String] {
        self.reverse_edges
            .get(symbol_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Resolve a plain name to symbol IDs.
    pub fn resolve_name(&self, name: &str) -> &[String] {
        self.name_index.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Total number of unique edges.
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|v| v.len()).sum()
    }

    /// Total number of nodes (symbols with at least one edge).
    pub fn node_count(&self) -> usize {
        let mut nodes: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (k, vs) in &self.edges {
            nodes.insert(k);
            for v in vs {
                nodes.insert(v);
            }
        }
        nodes.len()
    }
}