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
        Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Language::Java => Some(tree_sitter_java::LANGUAGE.into()),
        Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
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
        Language::TypeScript => Some(TYPESCRIPT_SYMBOLS_QUERY),
        Language::Rust => Some(RUST_SYMBOLS_QUERY),
        Language::Java => Some(JAVA_SYMBOLS_QUERY),
        Language::Go => Some(GO_SYMBOLS_QUERY),
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

// ── Java ─────────────────────────────────────────────────────────────
const JAVA_SYMBOLS_QUERY: &str = r#"
; Class declarations
(class_declaration
  name: (identifier) @class_name
  body: (class_body) @class_body
) @class_definition

; Method declarations
(method_declaration
  name: (identifier) @method_name
  parameters: (formal_parameters) @method_params
  body: (block) @method_body
) @method_definition

; Field declarations (constants)
(field_declaration
  name: (variable_declarator
    name: (identifier) @field_name
    value: (_) @field_value
  )
) @field_definition
"#;

// ── Go ──────────────────────────────────────────────────────────────
const GO_SYMBOLS_QUERY: &str = r#"
; Function declarations
(function_declaration
  name: (identifier) @name
  parameters: (parameter_list) @params
  body: (block) @body
) @definition

; Method declarations
(method_declaration
  name: (identifier) @method_name
  parameters: (parameter_list) @method_params
  body: (block) @method_body
) @method_definition

; Type declarations (structs, interfaces)
(type_declaration
  (type_spec
    name: (type_identifier) @type_name
    type: [
      (struct_type
        field_declaration_list: (field_declaration
          name: (field_identifier) @field_name
          type: (_) @field_type
        )
      )
      (interface_type
        method_declaration_list: (method_declaration
          name: (identifier) @interface_method_name
          parameters: (parameter_list) @interface_method_params
          body: (block) @interface_method_body
        )
      )
    ]
  )
) @type_definition
"#;

// ── TypeScript ─────────────────────────────────────────────────────────────
const TYPESCRIPT_SYMBOLS_QUERY: &str = r#"
; Similar to JavaScript but also captures interfaces and type aliases 
; Interface declarations
(interface_declaration
  name: (identifier) @interface_name
  body: (object_type) @interface_body
) @interface_definition
; Type alias declarations
(type_alias_declaration
  name: (identifier) @type_alias_name
  type: (type) @type_alias_body
) @type_alias_definition
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
    vec![
        Language::Python,
        Language::JavaScript,
        Language::Rust,
        Language::TypeScript,
        Language::Java,
        Language::Go,
    ]
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
        assert!(grammar_for(Language::CSharp).is_none());
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
