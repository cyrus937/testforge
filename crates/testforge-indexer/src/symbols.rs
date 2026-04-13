//! Symbol extraction from parsed ASTs.
//!
//! Given a tree-sitter parse tree and a set of queries, this module
//! walks the AST and produces [`CodeSymbol`] values for every
//! function, class, method, and import it finds.

use std::path::Path;

use streaming_iterator::StreamingIterator;
use tracing::debug;
use tree_sitter::{Node, Query, QueryCursor};

use testforge_core::{CodeSymbol, Language, Result, SymbolKind, TestForgeError};

use crate::languages::LanguageSupport;

/// Extract all symbols from `source` using the provided language support.
pub fn extract_symbols(
    source: &str,
    file_path: &Path,
    language: Language,
    lang_support: &LanguageSupport,
    tree: &tree_sitter::Tree,
) -> Result<Vec<CodeSymbol>> {
    let root = tree.root_node();
    let src = source.as_bytes();

    let mut symbols = Vec::new();

    // 1) Extract functions
    extract_with_query(
        &mut symbols,
        src,
        source,
        root,
        lang_support,
        lang_support.functions_query,
        SymbolKind::Function,
        file_path,
        language,
    )?;

    // 2) Extract classes / structs
    extract_with_query(
        &mut symbols,
        src,
        source,
        root,
        lang_support,
        lang_support.classes_query,
        SymbolKind::Class,
        file_path,
        language,
    )?;

    // 3) Extract imports
    extract_imports(&mut symbols, src, root, lang_support, file_path, language)?;

    // 4) Detect methods inside classes (post-processing)
    promote_methods(&mut symbols);

    // 5) Extract call dependencies for each symbol
    attach_dependencies(&mut symbols, src, root, lang_support)?;

    debug!(
        file = %file_path.display(),
        count = symbols.len(),
        "symbols extracted"
    );

    Ok(symbols)
}

// ─── Internal Helpers ───────────────────────────────────────────────

/// Run a tree-sitter query and create symbols from every match.
#[allow(clippy::too_many_arguments)]
fn extract_with_query(
    out: &mut Vec<CodeSymbol>,
    src: &[u8],
    source_text: &str,
    root: Node,
    lang_support: &LanguageSupport,
    query_src: &str,
    default_kind: SymbolKind,
    file_path: &Path,
    language: Language,
) -> Result<()> {
    let query = compile_query(lang_support, query_src)?;
    let mut cursor = QueryCursor::new();

    // Identify the capture indices we care about.
    let name_idx = query.capture_index_for_name("func.name")
        .or_else(|| query.capture_index_for_name("class.name"));
    let body_idx = query.capture_index_for_name("func.body")
        .or_else(|| query.capture_index_for_name("class.body"));
    let def_idx = query.capture_index_for_name("func.def")
        .or_else(|| query.capture_index_for_name("class.def"));

    let mut matches = cursor.matches(&query, root, src);
    while let Some(m) = matches.next() {
        let name_node = name_idx.and_then(|i| find_capture(&m, i));
        let body_node = body_idx.and_then(|i| find_capture(&m, i));
        let def_node = def_idx.and_then(|i| find_capture(&m, i));

        let Some(name_n) = name_node else { continue };
        let name = node_text(name_n, src);

        let def_n = def_node.unwrap_or(name_n);
        let line_start = def_n.start_position().row + 1;
        let line_end = def_n.end_position().row + 1;

        let body = def_node
            .map(|n| node_text(n, src))
            .unwrap_or_default();

        let signature = build_signature(def_n, body_node, src);
        let doc = extract_doc_comment(def_n, source_text);

        let id = CodeSymbol::compute_id(&file_path.to_path_buf(), &name, line_start);

        out.push(CodeSymbol {
            name,
            kind: default_kind,
            language,
            file_path: file_path.to_path_buf(),
            line_start,
            line_end,
            body,
            signature,
            doc,
            dependencies: Vec::new(),
            id,
        });
    }

    Ok(())
}

/// Extract import statements.
fn extract_imports(
    out: &mut Vec<CodeSymbol>,
    src: &[u8],
    root: Node,
    lang_support: &LanguageSupport,
    file_path: &Path,
    language: Language,
) -> Result<()> {
    let query = compile_query(lang_support, lang_support.imports_query)?;
    let mut cursor = QueryCursor::new();

    let name_idx = query.capture_index_for_name("import.name")
        .or_else(|| query.capture_index_for_name("import.module"));
    let def_idx = query.capture_index_for_name("import.def");

    let mut matches = cursor.matches(&query, root, src);
    while let Some(m) = matches.next() {
        let name_node = name_idx.and_then(|i| find_capture(&m, i));
        let def_node = def_idx.and_then(|i| find_capture(&m, i));

        let Some(name_n) = name_node else { continue };
        let name = node_text(name_n, src);
        let def_n = def_node.unwrap_or(name_n);
        let line_start = def_n.start_position().row + 1;
        let line_end = def_n.end_position().row + 1;
        let body = node_text(def_n, src);

        let id = CodeSymbol::compute_id(&file_path.to_path_buf(), &name, line_start);

        out.push(CodeSymbol {
            name,
            kind: SymbolKind::Import,
            language,
            file_path: file_path.to_path_buf(),
            line_start,
            line_end,
            body: body.clone(),
            signature: body,
            doc: None,
            dependencies: Vec::new(),
            id,
        });
    }

    Ok(())
}

/// Attach call-site dependencies to each symbol by matching calls
/// within the symbol's byte range.
fn attach_dependencies(
    symbols: &mut [CodeSymbol],
    src: &[u8],
    root: Node,
    lang_support: &LanguageSupport,
) -> Result<()> {
    let query = compile_query(lang_support, lang_support.calls_query)?;
    let mut cursor = QueryCursor::new();

    let name_idx = query.capture_index_for_name("call.name");

    // Collect all call sites: (byte_offset, called_name).
    let mut calls: Vec<(usize, String)> = Vec::new();
    let mut matches = cursor.matches(&query, root, src);
    while let Some(m) = matches.next() {
        if let Some(n) = name_idx.and_then(|i| find_capture(&m, i)) {
            calls.push((n.start_byte(), node_text(n, src)));
        }
    }

    // For each symbol, collect calls that fall within its byte range.
    // We re-derive the byte range from line numbers (approximate but
    // correct for well-formed source).
    let line_offsets = build_line_offsets(src);

    for sym in symbols.iter_mut() {
        if sym.kind == SymbolKind::Import {
            continue;
        }
        let start_byte = line_offset(&line_offsets, sym.line_start);
        let end_byte = line_offset(&line_offsets, sym.line_end + 1);

        let mut deps: Vec<String> = calls
            .iter()
            .filter(|(offset, _)| *offset >= start_byte && *offset < end_byte)
            .map(|(_, name)| name.clone())
            .collect();

        deps.sort();
        deps.dedup();
        // Don't list self-recursion as a dependency.
        deps.retain(|d| d != &sym.name);

        sym.dependencies = deps;
    }

    Ok(())
}

/// If a function is defined inside a class, re-label it as a Method.
fn promote_methods(symbols: &mut [CodeSymbol]) {
    // Gather class line ranges.
    let class_ranges: Vec<(usize, usize)> = symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Class)
        .map(|s| (s.line_start, s.line_end))
        .collect();

    for sym in symbols.iter_mut() {
        if sym.kind == SymbolKind::Function {
            let inside_class = class_ranges
                .iter()
                .any(|(cs, ce)| sym.line_start >= *cs && sym.line_end <= *ce);
            if inside_class {
                sym.kind = SymbolKind::Method;
            }
        }
    }
}

// ─── Tree-Sitter Utilities ──────────────────────────────────────────

fn compile_query(
    lang_support: &LanguageSupport,
    query_src: &str,
) -> Result<Query> {
    Query::new(&lang_support.ts_language, query_src).map_err(|e| {
        TestForgeError::ParseError {
            path: "<query>".into(),
            reason: format!("invalid tree-sitter query: {e}"),
        }
    })
}

fn find_capture<'a>(
    m: &'a tree_sitter::QueryMatch<'a, 'a>,
    idx: u32,
) -> Option<Node<'a>> {
    m.captures
        .iter()
        .find(|c| c.index == idx)
        .map(|c| c.node)
}

fn node_text(node: Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

/// Build the signature line: everything from the definition node
/// up to (but not including) the body node.
fn build_signature(def_node: Node, body_node: Option<Node>, src: &[u8]) -> String {
    match body_node {
        Some(body) => {
            let start = def_node.start_byte();
            let end = body.start_byte();
            let raw = &src[start..end];
            String::from_utf8_lossy(raw).trim().to_string()
        }
        None => node_text(def_node, src),
    }
}

/// Extract a leading docstring or comment block from the node just
/// before `def_node`.
fn extract_doc_comment(def_node: Node, source: &str) -> Option<String> {
    // Check the first child for Python-style docstrings.
    // (function_definition -> body -> block -> first child = expression_statement -> string)
    if let Some(body) = def_node.child_by_field_name("body") {
        if let Some(first_stmt) = body.named_child(0) {
            if first_stmt.kind() == "expression_statement" {
                if let Some(string_node) = first_stmt.named_child(0) {
                    if string_node.kind() == "string" {
                        let text = string_node
                            .utf8_text(source.as_bytes())
                            .unwrap_or("")
                            .to_string();
                        return Some(clean_docstring(&text));
                    }
                }
            }
        }
    }

    // Check preceding sibling for comment blocks.
    let mut comments = Vec::new();
    let mut prev = def_node.prev_named_sibling();
    while let Some(node) = prev {
        if node.kind() == "comment" {
            comments.push(
                node.utf8_text(source.as_bytes())
                    .unwrap_or("")
                    .to_string(),
            );
            prev = node.prev_named_sibling();
        } else {
            break;
        }
    }

    if comments.is_empty() {
        None
    } else {
        comments.reverse();
        Some(comments.join("\n"))
    }
}

/// Strip triple-quote delimiters and normalize indentation.
fn clean_docstring(raw: &str) -> String {
    let trimmed = raw
        .trim()
        .trim_start_matches("\"\"\"")
        .trim_start_matches("'''")
        .trim_end_matches("\"\"\"")
        .trim_end_matches("'''")
        .trim();
    trimmed.to_string()
}

// ─── Line-offset helpers ────────────────────────────────────────────

fn build_line_offsets(src: &[u8]) -> Vec<usize> {
    let mut offsets = vec![0];
    for (i, &b) in src.iter().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}

fn line_offset(offsets: &[usize], line_1based: usize) -> usize {
    if line_1based == 0 || line_1based > offsets.len() {
        return offsets.last().copied().unwrap_or(0);
    }
    offsets[line_1based - 1]
}