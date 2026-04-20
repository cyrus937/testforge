//! Python bindings for the TestForge Rust engine.
//!
//! This crate compiles into a native Python extension module called
//! `testforge_rust`.  Python code can do:
//!
//! ```python
//! import testforge_rust
//!
//! index = testforge_rust.index_project("/path/to/project")
//! results = testforge_rust.parse_source(source_code, "main.py", "python")
//! ```

use std::path::PathBuf;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use testforge_core::{Config};
use testforge_core::models::Language;
use testforge_indexer::{self, Indexer, Parser};

// ─── Helpers ────────────────────────────────────────────────────────

/// Convert a TestForgeError into a Python RuntimeError.
fn to_py_err(e: testforge_core::TestForgeError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

/// Serialize a serde-compatible value to a Python dict.
fn to_py_dict(py: Python<'_>, value: &impl serde::Serialize) -> PyResult<Py<PyDict>> {
    let json = serde_json::to_string(value).map_err(|e| {
        PyRuntimeError::new_err(format!("serialization error: {e}"))
    })?;
    let dict: Py<PyDict> = py
        .import("json")?
        .call_method1("loads", (json,))?
        .extract()?;
    Ok(dict)
}

// ─── Exported Functions ─────────────────────────────────────────────

/// Parse a single source file and return extracted symbols as a list
/// of dicts.
///
/// Args:
///     source: The raw source code as a string.
///     file_path: Relative file path (used in symbol IDs).
///     language: Language name ("python", "rust", "javascript", etc).
///
/// Returns:
///     A list of symbol dicts, each containing: name, kind, file_path,
///     line_start, line_end, body, signature, doc, dependencies, id.
#[pyfunction]
fn parse_source(
    py: Python<'_>,
    source: &str,
    file_path: &str,
    language: &str,
) -> PyResult<PyObject> {
    let lang = parse_language(language)?;
    let path = PathBuf::from(file_path);
    let mut parser = Parser::new().map_err(to_py_err)?;
    let symbols =
        parser.parse_and_extract(source, lang, &path).map_err(to_py_err)?;

    let json = serde_json::to_string(&symbols)
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let result = py.import("json")?.call_method1("loads", (json,))?;
    Ok(result.into())
}

/// Index an entire project directory and return the index as a dict.
///
/// Args:
///     root: Path to the project root.
///     config_path: Optional path to a TOML config file.
///
/// Returns:
///     A dict with keys: symbols, files, stats, graph_edges.
#[pyfunction]
#[pyo3(signature = (root, config_path=None))]
fn index_project(
    py: Python<'_>,
    root: &str,
    config_path: Option<&str>,
) -> PyResult<PyObject> {
    let root_path = PathBuf::from(root);

    let config = match config_path {
        Some(cp) => Config::load(&PathBuf::from(cp)).map_err(to_py_err)?,
        None => Config::default(),
    };

    let mut indexer = Indexer::new(config, &root_path).map_err(to_py_err)?;
    let report = indexer.index_full().map_err(to_py_err)?;

    // Build a serializable summary.
    let summary = serde_json::json!({
        "root": root,
        "files_indexed": report.files_indexed,
        "files_skipped": report.files_skipped,
        "files_failed": report.files_failed,
        "symbols_extracted": report.symbols_extracted,
        "errors": report.errors.iter().map(|(p, e)| {
            serde_json::json!({"path": p.to_string_lossy(), "error": e})
        }).collect::<Vec<_>>(),
    });

    let json = serde_json::to_string(&summary)
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let result = py.import("json")?.call_method1("loads", (json,))?;
    Ok(result.into())
}

/// Return a list of supported language names.
#[pyfunction]
fn supported_languages() -> Vec<String> {
    testforge_indexer::languages::supported_languages()
        .into_iter()
        .map(|l| l.to_string())
        .collect()
}

/// Compute the SHA-256 hash of a string.
#[pyfunction]
fn content_hash(data: &str) -> String {
    testforge_indexer::compute_hash(data)
}

// ─── Module Registration ────────────────────────────────────────────

#[pymodule]
fn testforge_rust(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse_source, m)?)?;
    m.add_function(wrap_pyfunction!(index_project, m)?)?;
    m.add_function(wrap_pyfunction!(supported_languages, m)?)?;
    m.add_function(wrap_pyfunction!(content_hash, m)?)?;

    // Expose version info.
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;

    Ok(())
}

// ─── Utilities ──────────────────────────────────────────────────────

fn parse_language(name: &str) -> PyResult<Language> {
    match name.to_lowercase().as_str() {
        "python" | "py" => Ok(Language::Python),
        "rust" | "rs" => Ok(Language::Rust),
        "javascript" | "js" => Ok(Language::JavaScript),
        "typescript" | "ts" => Ok(Language::TypeScript),
        "java" => Ok(Language::Java),
        "go" => Ok(Language::Go),
        "csharp" | "cs" | "c#" => Ok(Language::CSharp),
        _ => Err(PyRuntimeError::new_err(format!(
            "unsupported language: {name}"
        ))),
    }
}