//! SQLite-backed persistence for indexed symbols and file metadata.
//!
//! The store handles deduplication, incremental updates, and provides
//! query methods for the search engine and CLI.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use testforge_core::models::*;
use testforge_core::{Result, TestForgeError};
use tracing::debug;

/// Persistent storage for the code index.
pub struct IndexStore {
    conn: Connection,
}

impl IndexStore {
    /// Open (or create) the SQLite database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Performance tuning for a local-only database
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA cache_size = -64000;",  // 64 MB cache
        )?;

        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Open an in-memory database (for testing).
    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    /// Run schema migrations.
    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS files (
                path        TEXT PRIMARY KEY,
                language    TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                symbol_count INTEGER NOT NULL DEFAULT 0,
                line_count  INTEGER NOT NULL DEFAULT 0,
                indexed_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS symbols (
                id              TEXT PRIMARY KEY,
                name            TEXT NOT NULL,
                qualified_name  TEXT NOT NULL,
                kind            TEXT NOT NULL,
                language        TEXT NOT NULL,
                file_path       TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
                start_line      INTEGER NOT NULL,
                end_line        INTEGER NOT NULL,
                source          TEXT NOT NULL,
                signature       TEXT,
                docstring       TEXT,
                dependencies    TEXT NOT NULL DEFAULT '[]',
                parent          TEXT,
                visibility      TEXT NOT NULL DEFAULT 'public',
                content_hash    TEXT NOT NULL,
                embedding       BLOB
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);
            CREATE INDEX IF NOT EXISTS idx_symbols_qualified ON symbols(qualified_name);
            ",
        )?;

        debug!("Database schema up to date");
        Ok(())
    }

    // ── File operations ──────────────────────────────────────────────

    /// Insert or update a file record.
    pub fn upsert_file(&self, file: &IndexedFile) -> Result<()> {
        self.conn.execute(
            "INSERT INTO files (path, language, content_hash, symbol_count, line_count, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
                language = excluded.language,
                content_hash = excluded.content_hash,
                symbol_count = excluded.symbol_count,
                line_count = excluded.line_count,
                indexed_at = excluded.indexed_at",
            params![
                file.path.to_string_lossy().to_string(),
                serde_json::to_string(&file.language)
                    .map_err(|e| TestForgeError::internal(e.to_string()))?,
                file.content_hash,
                file.symbol_count,
                file.line_count,
                file.indexed_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Get the stored content hash for a file (for change detection).
    pub fn get_file_hash(&self, path: &Path) -> Result<Option<String>> {
        let hash = self
            .conn
            .query_row(
                "SELECT content_hash FROM files WHERE path = ?1",
                params![path.to_string_lossy().to_string()],
                |row| row.get(0),
            )
            .optional()?;
        Ok(hash)
    }

    /// Remove a file and all its symbols from the index.
    pub fn remove_file(&self, path: &Path) -> Result<()> {
        // Symbols are cascade-deleted via FK
        self.conn.execute(
            "DELETE FROM files WHERE path = ?1",
            params![path.to_string_lossy().to_string()],
        )?;
        Ok(())
    }

    // ── Symbol operations ────────────────────────────────────────────

    /// Insert or update symbols, replacing any existing symbols for the same file.
    pub fn upsert_symbols(&self, symbols: &[Symbol]) -> Result<()> {
        if symbols.is_empty() {
            return Ok(());
        }

        // Delete existing symbols for this file first
        let file_path = &symbols[0].file_path;
        self.conn.execute(
            "DELETE FROM symbols WHERE file_path = ?1",
            params![file_path.to_string_lossy().to_string()],
        )?;

        let mut stmt = self.conn.prepare(
            "INSERT INTO symbols (
                id, name, qualified_name, kind, language, file_path,
                start_line, end_line, source, signature, docstring,
                dependencies, parent, visibility, content_hash
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )?;

        for sym in symbols {
            let deps_json = serde_json::to_string(&sym.dependencies)
                .map_err(|e| TestForgeError::internal(e.to_string()))?;
            let kind_str = serde_json::to_string(&sym.kind)
                .map_err(|e| TestForgeError::internal(e.to_string()))?;
            let lang_str = serde_json::to_string(&sym.language)
                .map_err(|e| TestForgeError::internal(e.to_string()))?;
            let vis_str = serde_json::to_string(&sym.visibility)
                .map_err(|e| TestForgeError::internal(e.to_string()))?;

            stmt.execute(params![
                sym.id.to_string(),
                sym.name,
                sym.qualified_name,
                kind_str,
                lang_str,
                sym.file_path.to_string_lossy().to_string(),
                sym.start_line,
                sym.end_line,
                sym.source,
                sym.signature,
                sym.docstring,
                deps_json,
                sym.parent,
                vis_str,
                sym.content_hash,
            ])?;
        }

        Ok(())
    }

    /// Retrieve all symbols from the index.
    pub fn all_symbols(&self) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, qualified_name, kind, language, file_path,
                    start_line, end_line, source, signature, docstring,
                    dependencies, parent, visibility, content_hash
             FROM symbols
             ORDER BY file_path, start_line",
        )?;

        let symbols = stmt
            .query_map([], |row| {
                Ok(SymbolRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    qualified_name: row.get(2)?,
                    kind: row.get(3)?,
                    language: row.get(4)?,
                    file_path: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    source: row.get(8)?,
                    signature: row.get(9)?,
                    docstring: row.get(10)?,
                    dependencies: row.get(11)?,
                    parent: row.get(12)?,
                    visibility: row.get(13)?,
                    content_hash: row.get(14)?,
                })
            })?
            .filter_map(|row| row.ok())
            .filter_map(|row| row.into_symbol().ok())
            .collect();

        Ok(symbols)
    }

    /// Search symbols by name (case-insensitive prefix match).
    pub fn search_by_name(&self, query: &str) -> Result<Vec<Symbol>> {
        let pattern = format!("%{query}%");
        let mut stmt = self.conn.prepare(
            "SELECT id, name, qualified_name, kind, language, file_path,
                    start_line, end_line, source, signature, docstring,
                    dependencies, parent, visibility, content_hash
             FROM symbols
             WHERE name LIKE ?1 OR qualified_name LIKE ?1
             ORDER BY
                CASE WHEN name = ?2 THEN 0
                     WHEN name LIKE ?2 || '%' THEN 1
                     ELSE 2
                END,
                name
             LIMIT 50",
        )?;

        let symbols = stmt
            .query_map(params![pattern, query], |row| {
                Ok(SymbolRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    qualified_name: row.get(2)?,
                    kind: row.get(3)?,
                    language: row.get(4)?,
                    file_path: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    source: row.get(8)?,
                    signature: row.get(9)?,
                    docstring: row.get(10)?,
                    dependencies: row.get(11)?,
                    parent: row.get(12)?,
                    visibility: row.get(13)?,
                    content_hash: row.get(14)?,
                })
            })?
            .filter_map(|row| row.ok())
            .filter_map(|row| row.into_symbol().ok())
            .collect();

        Ok(symbols)
    }

    // ── Status & maintenance ─────────────────────────────────────────

    /// Get overall index statistics.
    pub fn status(&self) -> Result<IndexStatus> {
        let file_count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;

        let symbol_count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;

        let embedding_count: usize = self.conn.query_row(
            "SELECT COUNT(*) FROM symbols WHERE embedding IS NOT NULL",
            [],
            |r| r.get(0),
        )?;

        let last_indexed: Option<String> = self
            .conn
            .query_row(
                "SELECT MAX(indexed_at) FROM files",
                [],
                |r| r.get(0),
            )
            .optional()?
            .flatten();

        let last_indexed = last_indexed
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));

        // Collect distinct languages
        let mut lang_stmt = self.conn.prepare("SELECT DISTINCT language FROM files")?;
        let languages: Vec<Language> = lang_stmt
            .query_map([], |row| {
                let s: String = row.get(0)?;
                Ok(s)
            })?
            .filter_map(|r| r.ok())
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect();

        Ok(IndexStatus {
            file_count,
            symbol_count,
            embedding_count,
            languages,
            last_indexed,
            watcher_active: false,
        })
    }

    /// Clear the entire index.
    pub fn clear(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM symbols;
             DELETE FROM files;",
        )?;
        Ok(())
    }
}

/// Intermediate row type for SQLite → Symbol conversion.
struct SymbolRow {
    id: String,
    name: String,
    qualified_name: String,
    kind: String,
    language: String,
    file_path: String,
    start_line: usize,
    end_line: usize,
    source: String,
    signature: Option<String>,
    docstring: Option<String>,
    dependencies: String,
    parent: Option<String>,
    visibility: String,
    content_hash: String,
}

impl SymbolRow {
    fn into_symbol(self) -> std::result::Result<Symbol, String> {
        let id = uuid::Uuid::parse_str(&self.id).map_err(|e| e.to_string())?;
        let kind: SymbolKind =
            serde_json::from_str(&self.kind).map_err(|e| e.to_string())?;
        let language: Language =
            serde_json::from_str(&self.language).map_err(|e| e.to_string())?;
        let dependencies: Vec<String> =
            serde_json::from_str(&self.dependencies).unwrap_or_default();
        let visibility: Visibility =
            serde_json::from_str(&self.visibility).unwrap_or(Visibility::Public);

        Ok(Symbol {
            id,
            name: self.name,
            qualified_name: self.qualified_name,
            kind,
            language,
            file_path: PathBuf::from(self.file_path),
            start_line: self.start_line,
            end_line: self.end_line,
            source: self.source,
            signature: self.signature,
            docstring: self.docstring,
            dependencies,
            parent: self.parent,
            visibility,
            content_hash: self.content_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_symbol(name: &str, file: &str) -> Symbol {
        Symbol {
            id: Uuid::new_v4(),
            name: name.to_string(),
            qualified_name: name.to_string(),
            kind: SymbolKind::Function,
            language: Language::Python,
            file_path: PathBuf::from(file),
            start_line: 1,
            end_line: 5,
            source: format!("def {name}(): pass"),
            signature: Some(format!("def {name}()")),
            docstring: None,
            dependencies: vec![],
            parent: None,
            visibility: Visibility::Public,
            content_hash: "abc123".to_string(),
        }
    }

    #[test]
    fn store_and_retrieve_symbols() {
        let store = IndexStore::in_memory().unwrap();

        let file = IndexedFile {
            path: PathBuf::from("test.py"),
            language: Language::Python,
            content_hash: "hash1".into(),
            symbol_count: 2,
            line_count: 10,
            indexed_at: Utc::now(),
        };
        store.upsert_file(&file).unwrap();

        let symbols = vec![
            make_symbol("foo", "test.py"),
            make_symbol("bar", "test.py"),
        ];
        store.upsert_symbols(&symbols).unwrap();

        let retrieved = store.all_symbols().unwrap();
        assert_eq!(retrieved.len(), 2);
    }

    #[test]
    fn search_by_name_finds_partial_match() {
        let store = IndexStore::in_memory().unwrap();

        let file = IndexedFile {
            path: PathBuf::from("auth.py"),
            language: Language::Python,
            content_hash: "h".into(),
            symbol_count: 1,
            line_count: 5,
            indexed_at: Utc::now(),
        };
        store.upsert_file(&file).unwrap();

        let symbols = vec![make_symbol("authenticate_user", "auth.py")];
        store.upsert_symbols(&symbols).unwrap();

        let results = store.search_by_name("auth").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "authenticate_user");
    }

    #[test]
    fn upsert_replaces_existing_symbols() {
        let store = IndexStore::in_memory().unwrap();

        let file = IndexedFile {
            path: PathBuf::from("test.py"),
            language: Language::Python,
            content_hash: "v1".into(),
            symbol_count: 1,
            line_count: 5,
            indexed_at: Utc::now(),
        };
        store.upsert_file(&file).unwrap();

        store.upsert_symbols(&[make_symbol("old_func", "test.py")]).unwrap();
        store.upsert_symbols(&[make_symbol("new_func", "test.py")]).unwrap();

        let all = store.all_symbols().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "new_func");
    }

    #[test]
    fn status_reports_correct_counts() {
        let store = IndexStore::in_memory().unwrap();

        let file = IndexedFile {
            path: PathBuf::from("a.py"),
            language: Language::Python,
            content_hash: "h".into(),
            symbol_count: 2,
            line_count: 20,
            indexed_at: Utc::now(),
        };
        store.upsert_file(&file).unwrap();
        store
            .upsert_symbols(&[
                make_symbol("f1", "a.py"),
                make_symbol("f2", "a.py"),
            ])
            .unwrap();

        let status = store.status().unwrap();
        assert_eq!(status.file_count, 1);
        assert_eq!(status.symbol_count, 2);
    }
}
