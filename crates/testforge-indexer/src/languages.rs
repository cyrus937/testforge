//! Language grammar registry for tree-sitter.
//!
//! Each supported language is backed by a compiled tree-sitter grammar.
//! This module provides a central registry to look up the right
//! [`tree_sitter::Language`] for a given [`testforge_core::Language`].

use std::collections::HashMap;
use std::sync::OnceLock;

use testforge_core::{Language, TestForgeError, Result};

/// Wrapper around tree-sitter language + query strings specific
/// to each supported language.
#[derive(Clone)]
pub struct LanguageSupport {
    /// The compiled tree-sitter grammar.
    pub ts_language: tree_sitter::Language,

    /// S-expression query that extracts function definitions.
    pub functions_query: &'static str,

    /// S-expression query that extracts class / struct definitions.
    pub classes_query: &'static str,

    /// S-expression query that extracts import statements.
    pub imports_query: &'static str,

    /// S-expression query that extracts call expressions.
    pub calls_query: &'static str,
}

/// Global, lazily-initialized language registry.
static REGISTRY: OnceLock<HashMap<Language, LanguageSupport>> = OnceLock::new();

/// Return the [`LanguageSupport`] for `lang`, or an error if unsupported.
pub fn get_language_support(lang: Language) -> Result<&'static LanguageSupport> {
    let registry = REGISTRY.get_or_init(build_registry);
    registry.get(&lang).ok_or(TestForgeError::UnsupportedLanguage {
        language: lang.to_string(),
    })
}

/// Return every language that has a grammar loaded.
pub fn supported_languages() -> Vec<Language> {
    let registry = REGISTRY.get_or_init(build_registry);
    registry.keys().copied().collect()
}

// ─── Registry Builder ───────────────────────────────────────────────

fn build_registry() -> HashMap<Language, LanguageSupport> {
    let mut m = HashMap::new();

    // ── Python ──────────────────────────────────────────────────
    m.insert(
        Language::Python,
        LanguageSupport {
            ts_language: tree_sitter_python::LANGUAGE.into(),
            functions_query: r#"
                (function_definition
                    name: (identifier) @func.name
                    parameters: (parameters) @func.params
                    body: (block) @func.body
                ) @func.def
            "#,
            classes_query: r#"
                (class_definition
                    name: (identifier) @class.name
                    body: (block) @class.body
                ) @class.def
            "#,
            imports_query: r#"
                [
                    (import_statement
                        name: (dotted_name) @import.name
                    ) @import.def
                    (import_from_statement
                        module_name: (dotted_name) @import.module
                    ) @import.def
                ]
            "#,
            calls_query: r#"
                (call
                    function: [
                        (identifier) @call.name
                        (attribute
                            attribute: (identifier) @call.name
                        )
                    ]
                ) @call.expr
            "#,
        },
    );

    // ── Rust ────────────────────────────────────────────────────
    m.insert(
        Language::Rust,
        LanguageSupport {
            ts_language: tree_sitter_rust::LANGUAGE.into(),
            functions_query: r#"
                (function_item
                    name: (identifier) @func.name
                    parameters: (parameters) @func.params
                    body: (block) @func.body
                ) @func.def
            "#,
            classes_query: r#"
                [
                    (struct_item
                        name: (type_identifier) @class.name
                        body: (field_declaration_list)? @class.body
                    ) @class.def
                    (impl_item
                        type: (type_identifier) @class.name
                        body: (declaration_list) @class.body
                    ) @class.def
                ]
            "#,
            imports_query: r#"
                (use_declaration
                    argument: (_) @import.name
                ) @import.def
            "#,
            calls_query: r#"
                (call_expression
                    function: [
                        (identifier) @call.name
                        (field_expression
                            field: (field_identifier) @call.name
                        )
                        (scoped_identifier
                            name: (identifier) @call.name
                        )
                    ]
                ) @call.expr
            "#,
        },
    );

    // ── JavaScript ──────────────────────────────────────────────
    m.insert(
        Language::JavaScript,
        LanguageSupport {
            ts_language: tree_sitter_javascript::LANGUAGE.into(),
            functions_query: r#"
                [
                    (function_declaration
                        name: (identifier) @func.name
                        body: (statement_block) @func.body
                    ) @func.def
                    (arrow_function
                        body: (_) @func.body
                    ) @func.def
                ]
            "#,
            classes_query: r#"
                (class_declaration
                    name: (identifier) @class.name
                    body: (class_body) @class.body
                ) @class.def
            "#,
            imports_query: r#"
                (import_statement
                    source: (string) @import.name
                ) @import.def
            "#,
            calls_query: r#"
                (call_expression
                    function: [
                        (identifier) @call.name
                        (member_expression
                            property: (property_identifier) @call.name
                        )
                    ]
                ) @call.expr
            "#,
        },
    );

    // ── TypeScript (re-uses JS grammar augmented with types) ────
    m.insert(
        Language::TypeScript,
        LanguageSupport {
            ts_language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            // TypeScript shares the same node names as JavaScript
            // for function/class/import queries.
            functions_query: r#"
                [
                    (function_declaration
                        name: (identifier) @func.name
                        body: (statement_block) @func.body
                    ) @func.def
                    (arrow_function
                        body: (_) @func.body
                    ) @func.def
                ]
            "#,
            classes_query: r#"
                (class_declaration
                    name: (type_identifier) @class.name
                    body: (class_body) @class.body
                ) @class.def
            "#,
            imports_query: r#"
                (import_statement
                    source: (string) @import.name
                ) @import.def
            "#,
            calls_query: r#"
                (call_expression
                    function: [
                        (identifier) @call.name
                        (member_expression
                            property: (property_identifier) @call.name
                        )
                    ]
                ) @call.expr
            "#,
        },
    );

    // ── Java ────────────────────────────────────────────────────
    m.insert(
        Language::Java,
        LanguageSupport {
            ts_language: tree_sitter_java::LANGUAGE.into(),
            functions_query: r#"
                (method_declaration
                    name: (identifier) @func.name
                    body: (block) @func.body
                ) @func.def
            "#,
            classes_query: r#"
                (class_declaration
                    name: (identifier) @class.name
                    body: (class_body) @class.body
                ) @class.def
            "#,
            imports_query: r#"
                (import_declaration
                    (scoped_identifier) @import.name
                ) @import.def
            "#,
            calls_query: r#"
                (method_invocation
                    name: (identifier) @call.name
                ) @call.expr
            "#,
        },
    );

    m
}