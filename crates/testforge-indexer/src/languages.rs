//! Tree-sitter grammar registry.
//!
//! Maps [`Language`] variants to their tree-sitter grammar and the
//! S-expression queries used to locate symbols in the AST.

use testforge_core::models::Language;
use tree_sitter::Language as TsLanguage;

/// Returns the compiled tree-sitter grammar for a given language.
pub fn grammar_for(language: Language) -> Option<TsLanguage> {
    match language {
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        // Languages with grammars not yet wired up return None.
        _ => None,
    }
}

/// Returns the tree-sitter S-expression query that captures symbols
/// (functions, classes, methods) for a given language.
///
/// Each query uses named captures:
/// - `@name`      — the symbol's identifier
/// - `@definition` — the full definition node (used for source range)
/// - `@docstring`  — documentation comment, if any
/// - `@body`       — the function/method body
pub fn symbol_query_for(language: Language) -> Option<&'static str> {
    match language {
        Language::Python => Some(PYTHON_SYMBOLS_QUERY),
        Language::JavaScript => Some(JAVASCRIPT_SYMBOLS_QUERY),
        Language::Rust => Some(RUST_SYMBOLS_QUERY),
        _ => None,
    }
}

// ── Python ───────────────────────────────────────────────────────────

const PYTHON_SYMBOLS_QUERY: &str = r#"
; Top-level and nested function definitions
(function_definition
  name: (identifier) @name
  parameters: (parameters) @params
  body: (block) @body
) @definition

; Class definitions
(class_definition
  name: (identifier) @class_name
  body: (block) @class_body
) @class_definition

; Method definitions inside classes
(class_definition
  body: (block
    (function_definition
      name: (identifier) @method_name
      parameters: (parameters) @method_params
      body: (block) @method_body
    ) @method_definition
  )
)

; Decorated definitions
(decorated_definition
  (decorator) @decorator
  definition: [
    (function_definition
      name: (identifier) @decorated_name
    )
    (class_definition
      name: (identifier) @decorated_class_name
    )
  ]
) @decorated_definition

; Module-level assignments (constants)
(module
  (expression_statement
    (assignment
      left: (identifier) @const_name
      right: (_) @const_value
    )
  ) @constant_definition
)
"#;

// ── JavaScript / TypeScript ──────────────────────────────────────────

const JAVASCRIPT_SYMBOLS_QUERY: &str = r#"
; Function declarations
(function_declaration
  name: (identifier) @name
  parameters: (formal_parameters) @params
  body: (statement_block) @body
) @definition

; Arrow functions assigned to variables
(lexical_declaration
  (variable_declarator
    name: (identifier) @name
    value: (arrow_function
      parameters: (formal_parameters) @params
      body: (_) @body
    )
  )
) @definition

; Class declarations
(class_declaration
  name: (identifier) @class_name
  body: (class_body) @class_body
) @class_definition

; Method definitions inside classes
(class_declaration
  body: (class_body
    (method_definition
      name: (property_identifier) @method_name
      parameters: (formal_parameters) @method_params
      body: (statement_block) @method_body
    ) @method_definition
  )
)

; Exported function declarations
(export_statement
  declaration: (function_declaration
    name: (identifier) @exported_name
  )
) @export_definition
"#;

// ── Rust ─────────────────────────────────────────────────────────────

const RUST_SYMBOLS_QUERY: &str = r#"
; Function definitions (including pub)
(function_item
  name: (identifier) @name
  parameters: (parameters) @params
  body: (block) @body
) @definition

; Struct definitions
(struct_item
  name: (type_identifier) @struct_name
) @struct_definition

; Enum definitions
(enum_item
  name: (type_identifier) @enum_name
) @enum_definition

; Trait definitions
(trait_item
  name: (type_identifier) @trait_name
) @trait_definition

; Impl blocks with methods
(impl_item
  type: (type_identifier) @impl_type
  body: (declaration_list
    (function_item
      name: (identifier) @method_name
      parameters: (parameters) @method_params
      body: (block) @method_body
    ) @method_definition
  )
)

; Module declarations
(mod_item
  name: (identifier) @mod_name
) @mod_definition
"#;

/// Check if a language has a tree-sitter grammar available.
pub fn is_supported(language: Language) -> bool {
    grammar_for(language).is_some()
}

/// Return all currently supported languages.
pub fn supported_languages() -> Vec<Language> {
    vec![Language::Python, Language::JavaScript, Language::Rust]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_grammar_loads() {
        assert!(grammar_for(Language::Python).is_some());
    }

    #[test]
    fn javascript_grammar_loads() {
        assert!(grammar_for(Language::JavaScript).is_some());
    }

    #[test]
    fn rust_grammar_loads() {
        assert!(grammar_for(Language::Rust).is_some());
    }

    #[test]
    fn unsupported_language_returns_none() {
        assert!(grammar_for(Language::Go).is_none());
    }

    #[test]
    fn supported_languages_matches_grammars() {
        for lang in supported_languages() {
            assert!(
                is_supported(lang),
                "{lang} listed as supported but has no grammar"
            );
        }
    }
}
