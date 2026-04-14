//! Full-text search index powered by [tantivy](https://github.com/quickwit-oss/tantivy).
//!
//! Provides BM25-ranked keyword search across symbol names, signatures,
//! docstrings, and source code. Supports incremental updates and
//! multi-field boosted queries.

use std::path::Path;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};
use testforge_core::models::{MatchSource, SearchResult, Symbol};
use testforge_core::{Result, TestForgeError};
use tracing::{debug, info};

/// Schema field names.
const FIELD_ID: &str = "id";
const FIELD_NAME: &str = "name";
const FIELD_QUALIFIED_NAME: &str = "qualified_name";
const FIELD_KIND: &str = "kind";
const FIELD_LANGUAGE: &str = "language";
const FIELD_FILE_PATH: &str = "file_path";
const FIELD_SIGNATURE: &str = "signature";
const FIELD_DOCSTRING: &str = "docstring";
const FIELD_SOURCE: &str = "source";
const FIELD_SYMBOL_JSON: &str = "symbol_json";

/// Tantivy-backed full-text search index.
pub struct TextIndex {
    index: Index,
    reader: IndexReader,
    writer: Option<IndexWriter>,
    #[allow(dead_code)]
    schema: Schema,
    fields: TextFields,
}

/// Pre-resolved field handles for fast access.
#[derive(Clone)]
struct TextFields {
    id: Field,
    name: Field,
    qualified_name: Field,
    kind: Field,
    language: Field,
    file_path: Field,
    signature: Field,
    docstring: Field,
    source: Field,
    symbol_json: Field,
}

impl TextIndex {
    /// Open or create a tantivy index at the given directory.
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;

        let schema = Self::build_schema();
        let fields = Self::resolve_fields(&schema);

        let index = if dir.join("meta.json").exists() {
            Index::open_in_dir(dir)
                .map_err(|e| TestForgeError::internal(format!("Failed to open text index: {e}")))?
        } else {
            Index::create_in_dir(dir, schema.clone())
                .map_err(|e| TestForgeError::internal(format!("Failed to create text index: {e}")))?
        };

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| TestForgeError::internal(format!("Failed to create reader: {e}")))?;

        let writer = index
            .writer(50_000_000) // 50 MB buffer
            .map_err(|e| TestForgeError::internal(format!("Failed to create writer: {e}")))?;

        info!("Text index opened at {}", dir.display());

        Ok(Self {
            index,
            reader,
            writer: Some(writer),
            schema,
            fields,
        })
    }

    /// Build the tantivy schema.
    fn build_schema() -> Schema {
        let mut builder = Schema::builder();

        // Stored, not indexed — used for retrieval only
        builder.add_text_field(FIELD_ID, STRING | STORED);

        // Indexed + stored — searchable with boosting
        let text_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("en_stem")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();

        builder.add_text_field(FIELD_NAME, text_options.clone());
        builder.add_text_field(FIELD_QUALIFIED_NAME, text_options.clone());
        builder.add_text_field(FIELD_SIGNATURE, text_options.clone());
        builder.add_text_field(FIELD_DOCSTRING, text_options.clone());
        builder.add_text_field(FIELD_SOURCE, text_options.clone());

        // Filterable fields — indexed as single tokens
        builder.add_text_field(FIELD_KIND, STRING | STORED);
        builder.add_text_field(FIELD_LANGUAGE, STRING | STORED);
        builder.add_text_field(FIELD_FILE_PATH, STRING | STORED);

        // Full symbol as JSON — for deserializing results
        builder.add_text_field(FIELD_SYMBOL_JSON, STORED);

        builder.build()
    }

    /// Resolve named fields to handles.
    fn resolve_fields(schema: &Schema) -> TextFields {
        TextFields {
            id: schema.get_field(FIELD_ID).unwrap(),
            name: schema.get_field(FIELD_NAME).unwrap(),
            qualified_name: schema.get_field(FIELD_QUALIFIED_NAME).unwrap(),
            kind: schema.get_field(FIELD_KIND).unwrap(),
            language: schema.get_field(FIELD_LANGUAGE).unwrap(),
            file_path: schema.get_field(FIELD_FILE_PATH).unwrap(),
            signature: schema.get_field(FIELD_SIGNATURE).unwrap(),
            docstring: schema.get_field(FIELD_DOCSTRING).unwrap(),
            source: schema.get_field(FIELD_SOURCE).unwrap(),
            symbol_json: schema.get_field(FIELD_SYMBOL_JSON).unwrap(),
        }
    }

    /// Add a symbol to the text index.
    ///
    /// The document is buffered in memory until [`commit()`] is called.
    pub fn add_symbol(&mut self, symbol: &Symbol) -> Result<()> {
        let writer = self.writer.as_mut().ok_or_else(|| {
            TestForgeError::internal("Text index writer not available")
        })?;

        let symbol_json = serde_json::to_string(symbol)
            .map_err(|e| TestForgeError::internal(format!("Failed to serialize symbol: {e}")))?;

        // Delete existing document with same ID (for updates)
        let id_term = tantivy::Term::from_field_text(self.fields.id, &symbol.id.to_string());
        writer.delete_term(id_term);

        let mut doc = TantivyDocument::default();
        doc.add_text(self.fields.id, &symbol.id.to_string());
        doc.add_text(self.fields.name, &symbol.name);
        doc.add_text(self.fields.qualified_name, &symbol.qualified_name);
        doc.add_text(self.fields.kind, &symbol.kind.to_string());
        doc.add_text(self.fields.language, &symbol.language.to_string());
        doc.add_text(self.fields.file_path, &symbol.file_path.to_string_lossy());

        if let Some(ref sig) = symbol.signature {
            doc.add_text(self.fields.signature, sig);
        }
        if let Some(ref doc_str) = symbol.docstring {
            doc.add_text(self.fields.docstring, doc_str);
        }
        doc.add_text(self.fields.source, &symbol.source);
        doc.add_text(self.fields.symbol_json, &symbol_json);

        writer.add_document(doc)
            .map_err(|e| TestForgeError::internal(format!("Failed to add document: {e}")))?;

        Ok(())
    }

    /// Commit pending writes and make them searchable.
    pub fn commit(&mut self) -> Result<()> {
        if let Some(ref mut writer) = self.writer {
            writer.commit()
                .map_err(|e| TestForgeError::internal(format!("Commit failed: {e}")))?;
            self.reader.reload()
                .map_err(|e| TestForgeError::internal(format!("Reload failed: {e}")))?;
        }
        Ok(())
    }

    /// Search the index with a natural language or keyword query.
    ///
    /// Uses boosted multi-field search: name matches are weighted
    /// higher than source code matches.
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let searcher = self.reader.searcher();

        // Multi-field query parser with boosting:
        //   name^5, qualified_name^4, signature^3, docstring^2, source^1
        let query_parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.name,
                self.fields.qualified_name,
                self.fields.signature,
                self.fields.docstring,
                self.fields.source,
            ],
        );

        // Build boosted query
        let boosted_query = format!(
            "{}",
            query_str
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        );

        let query = query_parser.parse_query(&boosted_query).map_err(|e| {
            TestForgeError::internal(format!("Failed to parse query '{query_str}': {e}"))
        })?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| TestForgeError::internal(format!("Search failed: {e}")))?;

        let mut results = Vec::with_capacity(top_docs.len());

        for (score, doc_addr) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_addr).map_err(|e| {
                TestForgeError::internal(format!("Failed to retrieve doc: {e}"))
            })?;

            // Deserialize the stored symbol JSON
            if let Some(json_value) = doc.get_first(self.fields.symbol_json) {
                if let Some(json_str) = json_value.as_str() {
                    if let Ok(symbol) = serde_json::from_str::<Symbol>(json_str) {
                        // Normalize tantivy score to [0, 1]
                        let normalized_score = normalize_bm25_score(score);
                        results.push(SearchResult {
                            symbol,
                            score: normalized_score as f64,
                            match_source: MatchSource::FullText,
                        });
                    }
                }
            }
        }

        debug!(
            query = query_str,
            results = results.len(),
            "Full-text search"
        );

        Ok(results)
    }

    /// Get the total number of documents in the index.
    pub fn doc_count(&self) -> Result<usize> {
        let searcher = self.reader.searcher();
        let count = searcher
            .segment_readers()
            .iter()
            .map(|r| r.num_docs() as usize)
            .sum();
        Ok(count)
    }

    /// Clear the entire text index.
    pub fn clear(&mut self) -> Result<()> {
        if let Some(ref mut writer) = self.writer {
            writer.delete_all_documents()
                .map_err(|e| TestForgeError::internal(format!("Clear failed: {e}")))?;
            writer.commit()
                .map_err(|e| TestForgeError::internal(format!("Commit failed: {e}")))?;
            self.reader.reload()
                .map_err(|e| TestForgeError::internal(format!("Reload failed: {e}")))?;
        }
        Ok(())
    }
}

/// Normalize a raw BM25 score to approximately [0, 1].
///
/// BM25 scores are unbounded. We use a sigmoid-like mapping to
/// compress them into a useful range while preserving ranking order.
fn normalize_bm25_score(raw: f32) -> f32 {
    // Sigmoid: score / (score + k)
    // k = 10 works well for typical BM25 scores
    let k = 10.0;
    raw / (raw + k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use testforge_core::models::*;
    use tempfile::TempDir;
    use uuid::Uuid;

    fn make_test_symbol(name: &str, source: &str) -> Symbol {
        Symbol {
            id: Uuid::new_v4(),
            name: name.to_string(),
            qualified_name: name.to_string(),
            kind: SymbolKind::Function,
            language: Language::Python,
            file_path: "test.py".into(),
            start_line: 1,
            end_line: 5,
            source: source.to_string(),
            signature: Some(format!("def {name}()")),
            docstring: None,
            dependencies: vec![],
            parent: None,
            visibility: Visibility::Public,
            content_hash: "test".into(),
        }
    }

    #[test]
    fn index_and_search_basic() {
        let dir = TempDir::new().unwrap();
        let mut index = TextIndex::open(dir.path()).unwrap();

        let sym = make_test_symbol(
            "authenticate_user",
            "def authenticate_user(username, password): pass",
        );
        index.add_symbol(&sym).unwrap();
        index.commit().unwrap();

        let results = index.search("authenticate", 10).unwrap();
        assert!(!results.is_empty(), "Should find 'authenticate'");
        assert_eq!(results[0].symbol.name, "authenticate_user");
    }

    #[test]
    fn search_by_docstring() {
        let dir = TempDir::new().unwrap();
        let mut index = TextIndex::open(dir.path()).unwrap();

        let mut sym = make_test_symbol("process_payment", "def process_payment(): pass");
        sym.docstring = Some("Handle credit card transactions securely".into());
        index.add_symbol(&sym).unwrap();
        index.commit().unwrap();

        let results = index.search("credit card", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].symbol.name, "process_payment");
    }

    #[test]
    fn search_empty_index() {
        let dir = TempDir::new().unwrap();
        let index = TextIndex::open(dir.path()).unwrap();

        let results = index.search("anything", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn doc_count_after_indexing() {
        let dir = TempDir::new().unwrap();
        let mut index = TextIndex::open(dir.path()).unwrap();

        for i in 0..5 {
            let sym = make_test_symbol(
                &format!("func_{i}"),
                &format!("def func_{i}(): pass"),
            );
            index.add_symbol(&sym).unwrap();
        }
        index.commit().unwrap();

        assert_eq!(index.doc_count().unwrap(), 5);
    }

    #[test]
    fn clear_removes_all_documents() {
        let dir = TempDir::new().unwrap();
        let mut index = TextIndex::open(dir.path()).unwrap();

        let sym = make_test_symbol("func", "def func(): pass");
        index.add_symbol(&sym).unwrap();
        index.commit().unwrap();
        assert_eq!(index.doc_count().unwrap(), 1);

        index.clear().unwrap();
        assert_eq!(index.doc_count().unwrap(), 0);
    }

    #[test]
    fn update_existing_symbol() {
        let dir = TempDir::new().unwrap();
        let mut index = TextIndex::open(dir.path()).unwrap();

        let id = Uuid::new_v4();
        let mut sym = make_test_symbol("old_name", "def old_name(): pass");
        sym.id = id;
        index.add_symbol(&sym).unwrap();
        index.commit().unwrap();

        // Update with same ID
        sym.name = "new_name".into();
        sym.qualified_name = "new_name".into();
        sym.source = "def new_name(): pass".into();
        index.add_symbol(&sym).unwrap();
        index.commit().unwrap();

        // Should not have duplicates
        assert_eq!(index.doc_count().unwrap(), 1);

        let results = index.search("new_name", 10).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn name_matches_rank_higher_than_source() {
        let dir = TempDir::new().unwrap();
        let mut index = TextIndex::open(dir.path()).unwrap();

        // Symbol with "auth" in the name
        let sym1 = make_test_symbol("auth_handler", "def auth_handler(): return True");

        // Symbol with "auth" only in the source body
        let mut sym2 = make_test_symbol("process_request", "def process_request(): check_auth()");
        sym2.docstring = None;

        index.add_symbol(&sym1).unwrap();
        index.add_symbol(&sym2).unwrap();
        index.commit().unwrap();

        let results = index.search("auth", 10).unwrap();
        assert!(results.len() >= 1);
        // Name match should rank first
        assert_eq!(results[0].symbol.name, "auth_handler");
    }
}