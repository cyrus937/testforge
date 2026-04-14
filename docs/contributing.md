# Contributing to TestForge

Thank you for your interest in contributing to TestForge! This guide
covers everything you need to get started: setting up the development
environment, understanding the codebase, running tests, and submitting
changes.

## Development Setup

### Prerequisites

- **Rust** ≥ 1.75 with `rustfmt` and `clippy` (`rustup component add rustfmt clippy`)
- **Python** ≥ 3.10
- **maturin** (`pip install maturin`)
- **Git**

### Clone and Build

```bash
git clone https://github.com/testforge/testforge.git
cd testforge

# Build Rust crates
cargo build

# Install Python dependencies
pip install -e ".[dev]"

# Build the PyO3 bridge (optional, needed for Python ↔ Rust integration tests)
maturin develop
```

### Verify Everything Works

```bash
# Rust tests
cargo test --workspace

# Rust lints
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# Python tests
pytest tests/ -v

# Python lints
ruff check python/
mypy python/testforge_ai/ --ignore-missing-imports
```

## Project Structure

```
testforge/
├── crates/                       # Rust workspace
│   ├── testforge-core/           # Shared types, config, errors
│   ├── testforge-indexer/        # Parsing, symbol extraction, storage
│   └── testforge-cli/            # Command-line interface
├── python/testforge_ai/          # Python AI layer
│   ├── embeddings/               # Embedding providers
│   ├── generator/                # Test generation engine
│   └── analysis/                 # Code analysis utilities
├── tests/                        # Integration tests & fixtures
│   └── fixtures/                 # Sample projects for testing
└── docs/                         # Documentation
```

See [architecture.md](architecture.md) for a detailed overview of
how the components interact.

## How to Contribute

### Reporting Issues

Before opening an issue, please search existing issues to avoid duplicates.

When reporting a bug, include:
- TestForge version (`testforge --version`)
- Operating system and version
- Steps to reproduce the issue
- Expected vs. actual behavior
- Relevant log output (run with `-vvv` for maximum verbosity)

### Suggesting Features

Open an issue with the "feature request" label. Describe:
- The problem you're trying to solve
- How you imagine the solution
- Any alternatives you've considered

### Pull Requests

1. **Fork** the repository
2. **Create a branch** from `main`: `git checkout -b feature/my-feature`
3. **Make your changes** (see coding guidelines below)
4. **Add tests** for new functionality
5. **Run the full test suite** (`cargo test && pytest`)
6. **Commit** with a clear message (see commit conventions)
7. **Push** and open a Pull Request

## Coding Guidelines

### Rust

- **Style**: Follow `rustfmt` defaults. Run `cargo fmt` before committing.
- **Lints**: Code must pass `cargo clippy -- -D warnings`.
- **Error handling**: Use `TestForgeError` from `testforge-core`. Don't use
  `.unwrap()` outside of tests. Prefer `?` propagation with context.
- **Documentation**: Public items must have doc comments (`///`). Include
  examples for non-trivial APIs.
- **Testing**: Add unit tests in the same file (`#[cfg(test)] mod tests`).
  Use `tempfile` for filesystem tests.
- **Dependencies**: Prefer workspace dependencies. Discuss new crate
  additions in the PR description.

Example of good Rust code in TestForge:

```rust
/// Extract symbols from a Python source file.
///
/// Parses the source using tree-sitter, walks the AST, and returns
/// all top-level functions and class definitions.
///
/// # Errors
///
/// Returns `TestForgeError::ParseError` if tree-sitter fails to parse
/// the source, or `TestForgeError::UnsupportedLanguage` if the language
/// grammar is not available.
pub fn extract_python_symbols(source: &str, path: &Path) -> Result<Vec<Symbol>> {
    // ...
}
```

### Python

- **Style**: Follow `ruff` defaults (line length 100).
- **Type hints**: Use them everywhere. Target Python 3.10+ syntax
  (`list[str]` not `List[str]`).
- **Docstrings**: NumPy style for public classes and functions.
- **Testing**: Use `pytest`. Fixtures go in `conftest.py`.
- **Imports**: Standard library → third-party → local, separated by blank lines.

Example of good Python code in TestForge:

```python
def embed_texts(self, texts: list[str]) -> list[EmbeddingResult]:
    """
    Generate embeddings for a batch of texts.

    Parameters
    ----------
    texts : list[str]
        Input texts to embed.

    Returns
    -------
    list[EmbeddingResult]
        One embedding per input text, in the same order.
    """
```

### Commit Messages

Follow the conventional commits format:

```
type(scope): short description

Longer description if needed. Explain *why* the change was made,
not just *what* was changed.

Closes #123
```

Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `ci`, `chore`

Scopes: `core`, `indexer`, `cli`, `search`, `embeddings`, `generator`, `api`

Examples:

```
feat(indexer): add JavaScript arrow function extraction
fix(cli): handle missing .testforge directory gracefully
docs(api): add WebSocket progress endpoint documentation
test(indexer): add fixtures for decorated Python functions
perf(search): cache compiled tree-sitter queries
```

## Adding a New Language

To add support for a new programming language:

1. **Add the tree-sitter grammar** to `Cargo.toml` workspace dependencies
2. **Register the grammar** in `crates/testforge-indexer/src/languages.rs`:
   - Add a match arm in `grammar_for()`
   - Add the S-expression query in `symbol_query_for()`
3. **Add an extractor** in `crates/testforge-indexer/src/symbols.rs`:
   - Create `extract_{language}_symbols()` function
   - Handle language-specific idioms (decorators, access modifiers, etc.)
4. **Update the `Language` enum** in `crates/testforge-core/src/models.rs`
5. **Add test fixtures** in `tests/fixtures/{language}-app/`
6. **Add prompt templates** (optional) in `python/testforge_ai/generator/prompts/`
7. **Update documentation**

### Testing Your Language Support

Create a sample project in `tests/fixtures/` that exercises all the
symbol types your extractor handles:

```
tests/fixtures/go-cli-app/
├── main.go          # Functions, types, interfaces
├── handler.go       # Methods, error handling
└── handler_test.go  # Existing tests (for convention detection)
```

Then add integration tests that verify:
- All expected symbols are extracted
- Names and qualified names are correct
- Start/end lines match
- Docstrings are captured
- Dependencies are detected

## Adding a New LLM Provider

1. Create a new provider in `python/testforge_ai/generator/providers/`
2. Implement the same interface as `ClaudeProvider`:
   - `__init__(api_key, model)`
   - `generate(prompt, max_tokens, temperature) -> str`
3. Register the provider in `TestGenerator._init_provider()`
4. Add the provider name to config validation in
   `crates/testforge-core/src/config.rs`
5. Document the provider's configuration options

## Release Process

1. Update version in `Cargo.toml` (workspace), `pyproject.toml`, and
   `python/testforge_ai/__init__.py`
2. Update `CHANGELOG.md`
3. Create a Git tag: `git tag v0.2.0`
4. Push tag: `git push origin v0.2.0`
5. CI builds and publishes:
   - Rust binaries (GitHub Releases)
   - Python package (PyPI via maturin)
   - Docker image (GitHub Container Registry)

## Getting Help

- Open a GitHub Discussion for questions
- Join the community on Discord (link in README)
- Tag maintainers on complex PRs for faster review

We appreciate all contributions — from fixing typos in docs to
implementing new language support. Thank you for helping make
TestForge better!
