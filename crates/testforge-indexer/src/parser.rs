//! Tree-sitter parsing engine.
//!
//! Wraps tree-sitter's C parser in a safe Rust API and delegates
//! symbol extraction to language-specific handlers in [`symbols`].

use std::path::Path;

use testforge_core::models::{Language, Symbol};
use testforge_core::{Result, TestForgeError};
use tracing::debug;

use crate::languages;
use crate::symbols;

/// Thread-local tree-sitter parser wrapper.
///
/// Tree-sitter parsers are not `Send`, so each thread needs its own.
/// For single-threaded use (Phase 1), a single `Parser` instance suffices.
pub struct Parser {
    inner: tree_sitter::Parser,
}

impl Parser {
    /// Create a new parser instance.
    pub fn new() -> Result<Self> {
        let inner = tree_sitter::Parser::new();
        Ok(Self { inner })
    }

    /// Parse source code and extract all symbols from it.
    ///
    /// This is the main entry point: it configures tree-sitter for the
    /// given language, parses the source into an AST, then walks the tree
    /// to extract symbols.
    pub fn parse_and_extract(
        &mut self,
        source: &str,
        language: Language,
        file_path: &Path,
    ) -> Result<Vec<Symbol>> {
        let grammar = languages::grammar_for(language).ok_or_else(|| {
            TestForgeError::UnsupportedLanguage {
                language: language.to_string(),
            }
        })?;

        self.inner.set_language(&grammar).map_err(|e| {
            TestForgeError::internal(format!("Failed to set language {language}: {e}"))
        })?;

        let tree = self.inner.parse(source, None).ok_or_else(|| {
            TestForgeError::parse_error(file_path, "tree-sitter parse returned None")
        })?;

        let root = tree.root_node();

        // Check for parse errors
        if root.has_error() {
            debug!(
                path = %file_path.display(),
                "Parse tree contains errors — extracting symbols from valid subtrees"
            );
        }

        let symbols = symbols::extract_symbols(source, &root, language, file_path)?;

        debug!(
            path = %file_path.display(),
            language = %language,
            symbol_count = symbols.len(),
            "Extracted symbols"
        );

        Ok(symbols)
    }

    /// Parse source code and return the raw AST (for debugging / inspection).
    pub fn parse_to_tree(&mut self, source: &str, language: Language) -> Result<tree_sitter::Tree> {
        let grammar = languages::grammar_for(language).ok_or_else(|| {
            TestForgeError::UnsupportedLanguage {
                language: language.to_string(),
            }
        })?;

        self.inner.set_language(&grammar).map_err(|e| {
            TestForgeError::internal(format!("Failed to set language {language}: {e}"))
        })?;

        self.inner
            .parse(source, None)
            .ok_or_else(|| TestForgeError::internal("tree-sitter parse returned None"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_python_function() {
        let mut parser = Parser::new().unwrap();
        let source = r#"
def greet(name: str) -> str:
    """Say hello."""
    return f"Hello, {name}!"
"#;

        let symbols = parser
            .parse_and_extract(source, Language::Python, Path::new("test.py"))
            .unwrap();

        assert!(!symbols.is_empty(), "Should extract at least one symbol");
        let func = symbols.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(func.kind, testforge_core::models::SymbolKind::Function);
    }

    #[test]
    fn parse_python_class_with_methods() {
        let mut parser = Parser::new().unwrap();
        let source = r#"
class Calculator:
    """A simple calculator."""

    def add(self, a: int, b: int) -> int:
        return a + b

    def subtract(self, a: int, b: int) -> int:
        return a - b
"#;

        let symbols = parser
            .parse_and_extract(source, Language::Python, Path::new("calc.py"))
            .unwrap();

        let class = symbols.iter().find(|s| s.name == "Calculator");
        assert!(class.is_some(), "Should find the Calculator class");

        let methods: Vec<_> = symbols.iter().filter(|s| s.parent.is_some()).collect();
        assert!(methods.len() >= 2, "Should find at least 2 methods");
    }

    #[test]
    fn unsupported_language_returns_error() {
        let mut parser = Parser::new().unwrap();
        let result = parser.parse_and_extract("namespace Main {}", Language::CSharp, Path::new("main.cs"));
        assert!(result.is_err());
    }
}
