//! Language-aware symbol extraction from tree-sitter ASTs.
//!
//! Each supported language has a dedicated extractor that walks the AST
//! and produces [`Symbol`] instances. The extractors understand
//! language-specific idioms: Python decorators, Rust `impl` blocks,
//! JavaScript arrow functions, etc.

use std::path::Path;

use testforge_core::models::{Language, Symbol, SymbolKind, Visibility};
use testforge_core::Result;
use tree_sitter::Node;
use uuid::Uuid;

use crate::compute_hash;

/// Entry point: dispatch to the appropriate language extractor.
pub fn extract_symbols(
    source: &str,
    root: &Node,
    language: Language,
    file_path: &Path,
) -> Result<Vec<Symbol>> {
    match language {
        Language::Python => extract_python_symbols(source, root, file_path),
        Language::JavaScript | Language::TypeScript => {
            extract_javascript_symbols(source, root, file_path, language)
        }
        Language::Rust => extract_rust_symbols(source, root, file_path),
        other => Ok(Vec::new()),
    }
}

// ── Python Extractor ─────────────────────────────────────────────────

fn extract_python_symbols(
    source: &str,
    root: &Node,
    file_path: &Path,
) -> Result<Vec<Symbol>> {
    let mut symbols = Vec::new();
    let source_bytes = source.as_bytes();

    walk_python_node(root, source_bytes, file_path, None, &mut symbols);

    Ok(symbols)
}

fn walk_python_node(
    node: &Node,
    source: &[u8],
    file_path: &Path,
    parent_class: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                if let Some(sym) = extract_python_function(&child, source, file_path, parent_class)
                {
                    symbols.push(sym);
                }
            }
            "class_definition" => {
                if let Some(class_sym) = extract_python_class(&child, source, file_path) {
                    let class_name = class_sym.name.clone();
                    symbols.push(class_sym);

                    // Recurse into class body to extract methods
                    if let Some(body) = child.child_by_field_name("body") {
                        walk_python_node(&body, source, file_path, Some(&class_name), symbols);
                    }
                }
            }
            "decorated_definition" => {
                // Unwrap decorated definitions to get the inner function/class
                walk_python_node(&child, source, file_path, parent_class, symbols);
            }
            _ => {
                // Recurse into other compound nodes (if/else, try/except, etc.)
                // to find nested definitions — Python allows functions anywhere.
                if child.child_count() > 0 && parent_class.is_none() {
                    // Only recurse at module level; inside classes we handle it above.
                    walk_python_node(&child, source, file_path, parent_class, symbols);
                }
            }
        }
    }
}

fn extract_python_function(
    node: &Node,
    source: &[u8],
    file_path: &Path,
    parent_class: Option<&str>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, source)?;

    let kind = if parent_class.is_some() {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };

    let qualified_name = match parent_class {
        Some(cls) => format!("{cls}.{name}"),
        None => name.clone(),
    };

    let full_source = node_text(node, source)?;
    let signature = extract_python_signature(node, source);
    let docstring = extract_python_docstring(node, source);
    let dependencies = extract_python_calls(node, source);

    let visibility = if name.starts_with("__") && name.ends_with("__") {
        Visibility::Public // dunder methods are public
    } else if name.starts_with('_') {
        Visibility::Private
    } else {
        Visibility::Public
    };

    Some(Symbol {
        id: Uuid::new_v4(),
        name,
        qualified_name,
        kind,
        language: Language::Python,
        file_path: file_path.to_path_buf(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        source: full_source,
        signature,
        docstring,
        dependencies,
        parent: parent_class.map(String::from),
        visibility,
        content_hash: compute_hash(&node_text(node, source).unwrap_or_default()),
    })
}

fn extract_python_class(
    node: &Node,
    source: &[u8],
    file_path: &Path,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, source)?;
    let full_source = node_text(node, source)?;
    let docstring = extract_python_docstring(node, source);

    Some(Symbol {
        id: Uuid::new_v4(),
        name: name.clone(),
        qualified_name: name,
        kind: SymbolKind::Class,
        language: Language::Python,
        file_path: file_path.to_path_buf(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        source: full_source,
        signature: None,
        docstring,
        dependencies: Vec::new(),
        parent: None,
        visibility: Visibility::Public,
        content_hash: compute_hash(&node_text(node, source).unwrap_or_default()),
    })
}

/// Extract the function signature line (e.g., `def foo(a: int, b: str) -> bool`).
fn extract_python_signature(node: &Node, source: &[u8]) -> Option<String> {
    let name = node_text(&node.child_by_field_name("name")?, source)?;
    let params = node_text(&node.child_by_field_name("parameters")?, source)?;
    let return_type = node
        .child_by_field_name("return_type")
        .and_then(|n| node_text(&n, source));

    let sig = match return_type {
        Some(rt) => format!("def {name}{params} -> {rt}"),
        None => format!("def {name}{params}"),
    };
    Some(sig)
}

/// Extract the docstring from the first expression statement in the body.
fn extract_python_docstring(node: &Node, source: &[u8]) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    let first_stmt = body.child(0)?;

    if first_stmt.kind() == "expression_statement" {
        let expr = first_stmt.child(0)?;
        if expr.kind() == "string" {
            let raw = node_text(&expr, source)?;
            // Strip triple quotes
            let trimmed = raw
                .trim_start_matches("\"\"\"")
                .trim_start_matches("'''")
                .trim_end_matches("\"\"\"")
                .trim_end_matches("'''")
                .trim();
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Collect names of functions called within this node.
fn extract_python_calls(node: &Node, source: &[u8]) -> Vec<String> {
    let mut calls = Vec::new();
    collect_calls_recursive(node, source, &mut calls);
    calls.sort();
    calls.dedup();
    calls
}

fn collect_calls_recursive(node: &Node, source: &[u8], calls: &mut Vec<String>) {
    if node.kind() == "call" {
        if let Some(func) = node.child_by_field_name("function") {
            if let Some(name) = node_text(&func, source) {
                calls.push(name);
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_calls_recursive(&child, source, calls);
    }
}

// ── JavaScript / TypeScript Extractor ────────────────────────────────

fn extract_javascript_symbols(
    source: &str,
    root: &Node,
    file_path: &Path,
    language: Language,
) -> Result<Vec<Symbol>> {
    let mut symbols = Vec::new();
    let source_bytes = source.as_bytes();

    walk_js_node(root, source_bytes, file_path, language, None, &mut symbols);

    Ok(symbols)
}

fn walk_js_node(
    node: &Node,
    source: &[u8],
    file_path: &Path,
    language: Language,
    parent_class: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" => {
                if let Some(sym) = extract_js_function(&child, source, file_path, language) {
                    symbols.push(sym);
                }
            }
            "class_declaration" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Some(name) = node_text(&name_node, source) {
                        let full_source = node_text(&child, source).unwrap_or_default();

                        symbols.push(Symbol {
                            id: Uuid::new_v4(),
                            name: name.clone(),
                            qualified_name: name.clone(),
                            kind: SymbolKind::Class,
                            language,
                            file_path: file_path.to_path_buf(),
                            start_line: child.start_position().row + 1,
                            end_line: child.end_position().row + 1,
                            source: full_source.clone(),
                            signature: None,
                            docstring: None,
                            dependencies: Vec::new(),
                            parent: None,
                            visibility: Visibility::Public,
                            content_hash: compute_hash(&full_source),
                        });

                        // Recurse into class body for methods
                        if let Some(body) = child.child_by_field_name("body") {
                            walk_js_node(&body, source, file_path, language, Some(&name), symbols);
                        }
                    }
                }
            }
            "method_definition" => {
                if let Some(sym) =
                    extract_js_method(&child, source, file_path, language, parent_class)
                {
                    symbols.push(sym);
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                // Look for arrow function assignments
                if let Some(sym) = extract_js_arrow_function(&child, source, file_path, language) {
                    symbols.push(sym);
                }
            }
            "export_statement" => {
                walk_js_node(&child, source, file_path, language, parent_class, symbols);
            }
            _ => {
                if child.child_count() > 0 {
                    walk_js_node(&child, source, file_path, language, parent_class, symbols);
                }
            }
        }
    }
}

fn extract_js_function(
    node: &Node,
    source: &[u8],
    file_path: &Path,
    language: Language,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, source)?;
    let full_source = node_text(node, source)?;
    let params = node
        .child_by_field_name("parameters")
        .and_then(|n| node_text(&n, source));

    let signature = params.map(|p| format!("function {name}{p}"));

    Some(Symbol {
        id: Uuid::new_v4(),
        name: name.clone(),
        qualified_name: name,
        kind: SymbolKind::Function,
        language,
        file_path: file_path.to_path_buf(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        source: full_source.clone(),
        signature,
        docstring: None,
        dependencies: Vec::new(),
        parent: None,
        visibility: Visibility::Public,
        content_hash: compute_hash(&full_source),
    })
}

fn extract_js_method(
    node: &Node,
    source: &[u8],
    file_path: &Path,
    language: Language,
    parent_class: Option<&str>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, source)?;
    let full_source = node_text(node, source)?;

    let qualified_name = match parent_class {
        Some(cls) => format!("{cls}.{name}"),
        None => name.clone(),
    };

    Some(Symbol {
        id: Uuid::new_v4(),
        name,
        qualified_name,
        kind: SymbolKind::Method,
        language,
        file_path: file_path.to_path_buf(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        source: full_source.clone(),
        signature: None,
        docstring: None,
        dependencies: Vec::new(),
        parent: parent_class.map(String::from),
        visibility: Visibility::Public,
        content_hash: compute_hash(&full_source),
    })
}

fn extract_js_arrow_function(
    node: &Node,
    source: &[u8],
    file_path: &Path,
    language: Language,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "variable_declarator" {
            let name_node = child.child_by_field_name("name")?;
            let value_node = child.child_by_field_name("value")?;
            if value_node.kind() == "arrow_function" {
                let name = node_text(&name_node, source)?;
                let full_source = node_text(node, source)?;

                return Some(Symbol {
                    id: Uuid::new_v4(),
                    name: name.clone(),
                    qualified_name: name,
                    kind: SymbolKind::Function,
                    language,
                    file_path: file_path.to_path_buf(),
                    start_line: node.start_position().row + 1,
                    end_line: node.end_position().row + 1,
                    source: full_source.clone(),
                    signature: None,
                    docstring: None,
                    dependencies: Vec::new(),
                    parent: None,
                    visibility: Visibility::Public,
                    content_hash: compute_hash(&full_source),
                });
            }
        }
    }
    None
}

// ── Rust Extractor ───────────────────────────────────────────────────

fn extract_rust_symbols(
    source: &str,
    root: &Node,
    file_path: &Path,
) -> Result<Vec<Symbol>> {
    let mut symbols = Vec::new();
    let source_bytes = source.as_bytes();

    walk_rust_node(root, source_bytes, file_path, None, &mut symbols);

    Ok(symbols)
}

fn walk_rust_node(
    node: &Node,
    source: &[u8],
    file_path: &Path,
    impl_type: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_item" => {
                if let Some(sym) = extract_rust_function(&child, source, file_path, impl_type) {
                    symbols.push(sym);
                }
            }
            "struct_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Some(name) = node_text(&name_node, source) {
                        let full_source = node_text(&child, source).unwrap_or_default();
                        let vis = detect_rust_visibility(&child, source);

                        symbols.push(Symbol {
                            id: Uuid::new_v4(),
                            name: name.clone(),
                            qualified_name: name,
                            kind: SymbolKind::Struct,
                            language: Language::Rust,
                            file_path: file_path.to_path_buf(),
                            start_line: child.start_position().row + 1,
                            end_line: child.end_position().row + 1,
                            source: full_source.clone(),
                            signature: None,
                            docstring: None,
                            dependencies: Vec::new(),
                            parent: None,
                            visibility: vis,
                            content_hash: compute_hash(&full_source),
                        });
                    }
                }
            }
            "enum_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Some(name) = node_text(&name_node, source) {
                        let full_source = node_text(&child, source).unwrap_or_default();

                        symbols.push(Symbol {
                            id: Uuid::new_v4(),
                            name: name.clone(),
                            qualified_name: name,
                            kind: SymbolKind::Enum,
                            language: Language::Rust,
                            file_path: file_path.to_path_buf(),
                            start_line: child.start_position().row + 1,
                            end_line: child.end_position().row + 1,
                            source: full_source.clone(),
                            signature: None,
                            docstring: None,
                            dependencies: Vec::new(),
                            parent: None,
                            visibility: Visibility::Public,
                            content_hash: compute_hash(&full_source),
                        });
                    }
                }
            }
            "trait_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Some(name) = node_text(&name_node, source) {
                        let full_source = node_text(&child, source).unwrap_or_default();

                        symbols.push(Symbol {
                            id: Uuid::new_v4(),
                            name: name.clone(),
                            qualified_name: name,
                            kind: SymbolKind::Trait,
                            language: Language::Rust,
                            file_path: file_path.to_path_buf(),
                            start_line: child.start_position().row + 1,
                            end_line: child.end_position().row + 1,
                            source: full_source.clone(),
                            signature: None,
                            docstring: None,
                            dependencies: Vec::new(),
                            parent: None,
                            visibility: Visibility::Public,
                            content_hash: compute_hash(&full_source),
                        });
                    }
                }
            }
            "impl_item" => {
                // Extract the type being implemented
                let type_name = child
                    .child_by_field_name("type")
                    .and_then(|n| node_text(&n, source));

                if let Some(body) = child.child_by_field_name("body") {
                    walk_rust_node(&body, source, file_path, type_name.as_deref(), symbols);
                }
            }
            "mod_item" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    if let Some(name) = node_text(&name_node, source) {
                        let full_source = node_text(&child, source).unwrap_or_default();

                        symbols.push(Symbol {
                            id: Uuid::new_v4(),
                            name: name.clone(),
                            qualified_name: name,
                            kind: SymbolKind::Module,
                            language: Language::Rust,
                            file_path: file_path.to_path_buf(),
                            start_line: child.start_position().row + 1,
                            end_line: child.end_position().row + 1,
                            source: full_source.clone(),
                            signature: None,
                            docstring: None,
                            dependencies: Vec::new(),
                            parent: None,
                            visibility: Visibility::Public,
                            content_hash: compute_hash(&full_source),
                        });
                    }
                }
            }
            _ => {
                if child.child_count() > 0 {
                    walk_rust_node(&child, source, file_path, impl_type, symbols);
                }
            }
        }
    }
}

fn extract_rust_function(
    node: &Node,
    source: &[u8],
    file_path: &Path,
    impl_type: Option<&str>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(&name_node, source)?;
    let full_source = node_text(node, source)?;

    let kind = if impl_type.is_some() {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };

    let qualified_name = match impl_type {
        Some(t) => format!("{t}::{name}"),
        None => name.clone(),
    };

    let visibility = detect_rust_visibility(node, source);

    // Build signature from the function item text up to the opening brace
    let signature = full_source
        .find('{')
        .map(|idx| full_source[..idx].trim().to_string());

    Some(Symbol {
        id: Uuid::new_v4(),
        name,
        qualified_name,
        kind,
        language: Language::Rust,
        file_path: file_path.to_path_buf(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        source: full_source.clone(),
        signature,
        docstring: None,
        dependencies: Vec::new(),
        parent: impl_type.map(String::from),
        visibility,
        content_hash: compute_hash(&full_source),
    })
}

fn detect_rust_visibility(node: &Node, source: &[u8]) -> Visibility {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "visibility_modifier" {
            let text = node_text(&child, source).unwrap_or_default();
            return if text.contains("pub(crate)") {
                Visibility::Internal
            } else if text.contains("pub(super)") {
                Visibility::Protected
            } else {
                Visibility::Public
            };
        }
    }
    Visibility::Private
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Safely extract the text content of a tree-sitter node.
fn node_text(node: &Node, source: &[u8]) -> Option<String> {
    let start = node.start_byte();
    let end = node.end_byte();
    if end <= source.len() {
        std::str::from_utf8(&source[start..end])
            .ok()
            .map(String::from)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn parse_python(source: &str) -> Vec<Symbol> {
        let mut parser = Parser::new().unwrap();
        parser
            .parse_and_extract(source, Language::Python, Path::new("test.py"))
            .unwrap()
    }

    #[test]
    fn python_function_with_docstring() {
        let symbols = parse_python(
            r#"
def compute_tax(amount: float, rate: float) -> float:
    """Calculate tax for a given amount."""
    return amount * rate
"#,
        );

        let func = symbols.iter().find(|s| s.name == "compute_tax").unwrap();
        assert_eq!(func.kind, SymbolKind::Function);
        assert!(func.signature.as_ref().unwrap().contains("compute_tax"));
        assert_eq!(
            func.docstring.as_deref(),
            Some("Calculate tax for a given amount.")
        );
    }

    #[test]
    fn python_private_method_detection() {
        let symbols = parse_python(
            r#"
class Foo:
    def _private(self):
        pass

    def __dunder__(self):
        pass

    def public(self):
        pass
"#,
        );

        let private = symbols.iter().find(|s| s.name == "_private").unwrap();
        assert_eq!(private.visibility, Visibility::Private);

        let dunder = symbols.iter().find(|s| s.name == "__dunder__").unwrap();
        assert_eq!(dunder.visibility, Visibility::Public);

        let public = symbols.iter().find(|s| s.name == "public").unwrap();
        assert_eq!(public.visibility, Visibility::Public);
    }

    #[test]
    fn python_method_has_parent() {
        let symbols = parse_python(
            r#"
class Service:
    def handle(self):
        pass
"#,
        );

        let method = symbols.iter().find(|s| s.name == "handle").unwrap();
        assert_eq!(method.parent.as_deref(), Some("Service"));
        assert_eq!(method.qualified_name, "Service.handle");
        assert_eq!(method.kind, SymbolKind::Method);
    }

    #[test]
    fn python_function_dependencies() {
        let symbols = parse_python(
            r#"
def process(data):
    validated = validate(data)
    result = transform(validated)
    save(result)
    return result
"#,
        );

        let func = symbols.iter().find(|s| s.name == "process").unwrap();
        assert!(func.dependencies.contains(&"validate".to_string()));
        assert!(func.dependencies.contains(&"transform".to_string()));
        assert!(func.dependencies.contains(&"save".to_string()));
    }
}
