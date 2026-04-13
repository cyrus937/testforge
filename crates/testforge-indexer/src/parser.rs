//! Source code parser built on tree-sitter.
//!
//! This module owns the [`tree_sitter::Parser`] instances and exposes
//! a high-level `parse_file` function that takes raw source code and
//! returns extracted [`CodeSymbol`]s.

use std::path::Path;

use tracing::{debug, instrument, warn};

use testforge_core::{CodeSymbol, Language, Result, TestForgeError};

use crate::languages::{get_language_support, LanguageSupport};
use crate::symbols::extract_symbols;

/// Parse a single source file and extract all symbols.
///
/// # Arguments
/// * `source` – the raw UTF-8 source code.
/// * `file_path` – path relative to project root (used in symbol IDs).
/// * `language` – the language to parse the file as.
///
/// # Errors
/// Returns [`TestForgeError::ParseError`] if tree-sitter cannot parse
/// the file, or [`TestForgeError::UnsupportedLanguage`] if no grammar
/// is registered for `language`.
#[instrument(skip(source), fields(file = %file_path.display()))]
pub fn parse_file(
    source: &str,
    file_path: &Path,
    language: Language,
) -> Result<Vec<CodeSymbol>> {
    let lang_support = get_language_support(language)?;
    let tree = parse_source(source, lang_support)?;

    if tree.root_node().has_error() {
        warn!(
            file = %file_path.display(),
            "tree-sitter reported parse errors – extraction may be partial"
        );
    }

    extract_symbols(source, file_path, language, lang_support, &tree)
}

/// Detect the language from a file path's extension and parse it.
///
/// Returns `Ok(None)` if the extension is unrecognised (not an error,
/// the file is simply skipped).
pub fn parse_file_auto(
    source: &str,
    file_path: &Path,
) -> Result<Option<(Language, Vec<CodeSymbol>)>> {
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match Language::from_extension(ext) {
        Some(lang) => {
            let symbols = parse_file(source, file_path, lang)?;
            Ok(Some((lang, symbols)))
        }
        None => {
            debug!(ext, "skipping file with unrecognised extension");
            Ok(None)
        }
    }
}

// ─── Internals ──────────────────────────────────────────────────────

/// Create a tree-sitter parser configured for the given language and
/// parse the source code.
fn parse_source(
    source: &str,
    lang_support: &LanguageSupport,
) -> Result<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang_support.ts_language)
        .map_err(|e| TestForgeError::ParseError {
            path: "<init>".into(),
            reason: format!("failed to set parser language: {e}"),
        })?;

    parser
        .parse(source, None)
        .ok_or_else(|| TestForgeError::ParseError {
            path: "<parse>".into(),
            reason: "tree-sitter returned no tree (timeout or cancellation)".into(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_simple_python_function() {
        let source = r#"
def greet(name: str) -> str:
    """Say hello."""
    return f"Hello, {name}!"
"#;
        let symbols = parse_file(source, &PathBuf::from("hello.py"), Language::Python)
            .expect("parse should succeed");

        assert!(!symbols.is_empty(), "should extract at least one symbol");
        let func = symbols.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(func.kind, testforge_core::SymbolKind::Function);
        assert!(func.signature.contains("greet"));
    }

    #[test]
    fn parse_python_class_with_methods() {
        let source = r#"
class UserService:
    """Manages user operations."""

    def __init__(self, db):
        self.db = db

    def get_user(self, user_id: int):
        return self.db.query(user_id)

    def delete_user(self, user_id: int):
        self.db.delete(user_id)
"#;
        let symbols = parse_file(source, &PathBuf::from("service.py"), Language::Python)
            .expect("parse should succeed");

        let class = symbols.iter().find(|s| s.name == "UserService");
        assert!(class.is_some(), "should find UserService class");

        let methods: Vec<_> = symbols
            .iter()
            .filter(|s| s.kind == testforge_core::SymbolKind::Method)
            .collect();
        assert!(
            methods.len() >= 2,
            "should find at least 2 methods, got {}",
            methods.len()
        );
    }

    #[test]
    fn parse_auto_detects_language() {
        let source = "fn main() { println!(\"hello\"); }";
        let result = parse_file_auto(source, &PathBuf::from("main.rs"))
            .expect("should not error");
        assert!(result.is_some());
        let (lang, _) = result.unwrap();
        assert_eq!(lang, Language::Rust);
    }

    #[test]
    fn unknown_extension_returns_none() {
        let result = parse_file_auto("data", &PathBuf::from("file.xyz"))
            .expect("should not error");
        assert!(result.is_none());
    }
}