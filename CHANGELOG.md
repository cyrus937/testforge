# Changelog

All notable changes to TestForge will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2025-01-15

### Added

#### Phase 1 — Foundations
- **testforge-core**: Configuration management with TOML, directory discovery,
  unified error types with user-facing suggestions, domain models (Symbol,
  Language, SymbolKind, SearchResult, CodeContext)
- **testforge-indexer**: Tree-sitter AST parsing for Python, JavaScript, Rust;
  symbol extraction (functions, classes, methods, structs, enums, traits);
  `.gitignore`-aware file walking; SHA-256 content hashing for incremental
  re-indexing; SQLite persistence with WAL mode; cross-platform file watcher
  with debouncing
- **testforge-cli**: `init`, `index`, `search`, `status` commands with colored
  output, progress bars, and JSON output mode
- **Python AI layer**: Embedding provider interface, local sentence-transformers
  provider, disk-backed embedding cache, PyO3 bridge with subprocess fallback
- Documentation: architecture, getting-started, configuration, API reference,
  contributing guides

#### Phase 2 — Search
- **testforge-search**: In-memory vector store with cosine similarity,
  tantivy full-text index with multi-field BM25, Reciprocal Rank Fusion (RRF)
  for hybrid search, post-retrieval re-ranking, deduplication, diversity filter
- **Embedding pipeline**: Full symbol→chunk→embedding→vector file pipeline,
  Python CLI (`testforge-ai embed/search/stats`)
- `testforge index` now builds the tantivy search index automatically

#### Phase 3 — Test Generation
- **`testforge gen-tests`**: Generate tests by file, symbol name, or file::symbol
  notation; supports `--recursive`, `--dry-run`, `--framework`, `--provider`
- **Post-processor**: Code extraction from markdown fences, missing import
  injection, syntax validation (AST parse for Python), tab→space fixing,
  placeholder detection, file header generation
- **Prompt templates**: Unit tests, edge cases (heuristic analysis of
  signature + body), mock builder (strategy per dependency type: DB, HTTP,
  file I/O, cache), integration scenarios (happy path, error propagation,
  state consistency)
- **Complexity analyzer**: Cyclomatic complexity estimation, risk scoring,
  symbol prioritization for testing

#### Phase 4 — API & Extension
- **testforge-server**: Axum REST API with 7 endpoints (`/health`, `/status`,
  `/search`, `/index`, `/generate-tests`, `/symbols`, `/symbols/:name`);
  WebSocket progress streaming; CORS support; async job management
- **`testforge serve`**: CLI command to start the API server
- **VS Code Extension**: Command Palette search with QuickPick navigation,
  right-click "Generate Tests" with live progress and save dialog, sidebar
  tree view for search results, status bar with connectivity indicator,
  auto-index on save option

#### Phase 5 — Polish
- **Extended language support**: TypeScript, Java, Go extractors with
  full symbol extraction (classes, methods, interfaces, structs, enums,
  Go receiver methods, Java visibility modifiers, Go exported/unexported)
- **`testforge ci`**: CI/CD analysis with coverage gap detection,
  per-file coverage breakdown, JSON report generation, `--strict` mode
  with configurable threshold, visual progress bars
- **Release automation**: Multi-platform GitHub Actions workflow
  (Linux x86/ARM/musl, macOS x86/ARM, Windows), Python wheel build,
  Docker multi-arch image, automated GitHub Release creation

[0.1.0]: https://github.com/testforge/testforge/releases/tag/v0.1.0
