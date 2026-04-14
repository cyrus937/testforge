# Architecture

This document describes the internal architecture of TestForge, the data flows
between components, and the design decisions behind the Rust + Python hybrid approach.

## Design Philosophy

TestForge is built around three core principles:

1. **Context is king.** Most AI test generators process functions in isolation. TestForge
   indexes the entire codebase — dependency graphs, call chains, existing tests, and
   conventions — so the LLM has the full picture when generating tests.

2. **Speed enables workflow.** A developer won't use a tool that takes 30 seconds to
   index their repo. Rust handles all parsing, indexing, and I/O to keep everything
   under a second, even on large codebases.

3. **Offline-first.** The core engine (search, indexing) works entirely offline.
   The AI layer only reaches out to external APIs when generating tests, and
   even that can be swapped for a local model.

## System Overview

```
┌──────────────────────────────────────────────────────────────────┐
│                         INTERFACES                               │
│                                                                  │
│   CLI (Rust/Clap)    REST API (Rust/Axum)    VS Code Extension   │
│                                                                  │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│                      CORE ENGINE (Rust)                          │
│                                                                  │
│   ┌──────────┐   ┌────────────┐   ┌──────────┐   ┌──────────┐  │
│   │ Indexer   │   │  Search    │   │  File    │   │  Store   │  │
│   │          │   │  Engine    │   │  Watcher │   │ (SQLite) │  │
│   │ • walker │   │ • vector   │   │ • notify │   │          │  │
│   │ • parser │   │ • fulltext │   │ • dedup  │   │ • files  │  │
│   │ • symbols│   │ • hybrid   │   │ • batch  │   │ • symbols│  │
│   └──────────┘   └────────────┘   └──────────┘   └──────────┘  │
│                                                                  │
├──────────────────────────────────────────────────────────────────┤
│                                                                  │
│                      AI LAYER (Python)                           │
│                                                                  │
│   ┌──────────┐   ┌────────────┐   ┌──────────┐   ┌──────────┐  │
│   │Embeddings│   │  Test Gen  │   │  Context │   │ Complexity│  │
│   │          │   │  Engine    │   │  Builder │   │ Analyzer  │  │
│   │ • local  │   │ • prompts  │   │ • deps   │   │           │  │
│   │ • openai │   │ • claude   │   │ • tests  │   │ • cyclo.  │  │
│   │ • cache  │   │ • postproc │   │ • convent│   │ • risk    │  │
│   └──────────┘   └────────────┘   └──────────┘   └──────────┘  │
│                                                                  │
├──────────────────────────────────────────────────────────────────┤
│                      BRIDGE (PyO3)                               │
│   Rust ←→ Python zero-copy data exchange                         │
└──────────────────────────────────────────────────────────────────┘
```

## Crate Structure

The Rust side is organized as a Cargo workspace with three crates:

### `testforge-core`

Foundational types shared by all other crates. Contains no business logic,
only data definitions.

- `config.rs` — Loads, validates, and saves `.testforge/config.toml`. Supports
  directory discovery (walks up to find the project root).
- `models.rs` — Domain types: `Symbol`, `Language`, `SymbolKind`, `SearchResult`,
  `CodeContext`. These are the common vocabulary of the system.
- `error.rs` — Unified `TestForgeError` enum with rich context and user-facing
  suggestions. All crates funnel their errors through this type.

### `testforge-indexer`

The heart of the system: reads source files, parses them into ASTs,
extracts symbols, and persists everything.

- `walker.rs` — `.gitignore`-aware file discovery using the `ignore` crate.
  Filters by language, file size, and configured exclude patterns.
- `parser.rs` — Wraps tree-sitter in a safe API. Configures the correct grammar
  per language and delegates to the symbol extractor.
- `languages.rs` — Grammar registry. Maps `Language` enum variants to compiled
  tree-sitter grammars and S-expression queries.
- `symbols.rs` — Language-specific AST walkers. Each extractor understands its
  language's idioms (Python decorators, Rust `impl` blocks, JS arrow functions).
  Produces `Symbol` structs with name, signature, docstring, dependencies, and
  visibility.
- `store.rs` — SQLite persistence layer. Uses WAL mode for concurrent reads,
  content hashing for incremental updates, and indexes on name/kind/file for
  fast lookups.
- `watcher.rs` — Cross-platform file watcher using `notify`. Debounces rapid
  changes (e.g., from `git checkout`) and deduplicates events by path.

### `testforge-cli`

The command-line interface. Each subcommand is a separate module:

- `init` — Creates `.testforge/` with a default `config.toml`
- `index` — Runs the full indexing pipeline with progress display
- `search` — Keyword search against the SQLite index (Phase 1);
  semantic search via embeddings (Phase 2+)
- `status` — Shows file/symbol counts, languages, last index time

## Python Layer

The Python side provides AI capabilities that would be impractical to
implement in Rust (ML model loading, prompt engineering, API clients).

### Embeddings

The embedding pipeline converts code symbols into dense vectors for
semantic search:

1. **Provider interface** (`provider.py`) — Abstract base class that all
   providers implement. Defines `embed_texts()`, `embed_code()`, `embed_query()`.
2. **Local provider** (`local.py`) — Uses `sentence-transformers` with the
   `all-MiniLM-L6-v2` model. Runs entirely offline, ~80 MB disk footprint.
3. **Cache** (`cache.py`) — Disk-backed cache keyed by content hash. Avoids
   recomputing embeddings for unchanged code. Stores vectors as `.npy` files.

### Test Generator

The generation pipeline:

1. **Context assembly** (`analysis/context.py`) — Builds a rich `CodeContext`
   from the index: target symbol + dependencies + siblings + existing tests +
   imports + detected conventions. Trims to fit within the LLM's context window.
2. **Prompt construction** (`generator/prompts/`) — Assembles structured prompts
   with the context. Separate templates for unit tests and edge cases.
3. **LLM call** (`generator/providers/`) — Currently supports Claude via the
   Anthropic API. Provider interface allows easy addition of OpenAI or local models.
4. **Post-processing** (`generator/engine.py`) — Extracts code blocks from LLM
   output, validates syntax, counts tests.

### Bridge

`bridge.py` provides two execution modes:

- **Native mode** — PyO3 compiled extension. The Rust index engine runs in-process
  with zero serialization overhead. Used in production.
- **Subprocess mode** — Falls back to invoking the `testforge` CLI binary and
  parsing JSON output. Used during development or on platforms where PyO3
  compilation is difficult.

## Data Flows

### Indexing Flow

```
Source file on disk
    │
    ▼
FileWalker (ignore crate)
    │  Filters: .gitignore, exclude patterns, file size, language
    ▼
Parser (tree-sitter)
    │  Configures grammar, produces AST
    ▼
SymbolExtractor (language-specific)
    │  Walks AST, extracts functions/classes/methods
    │  Resolves: name, signature, docstring, dependencies, visibility
    ▼
Content hash (SHA-256)
    │  Compared against stored hash → skip if unchanged
    ▼
IndexStore (SQLite)
    │  Upserts file metadata and symbols
    ▼
Done — symbols queryable via search
```

### Search Flow (Phase 1: Keyword)

```
User query: "payment validation"
    │
    ▼
SQLite LIKE query on name, qualified_name, docstring, source
    │
    ▼
Relevance scoring (name match > docstring match > source match)
    │
    ▼
Ranked results returned to CLI/API
```

### Search Flow (Phase 2+: Semantic)

```
User query: "where do we check if a credit card is valid?"
    │
    ├──► Embed query (Python, sentence-transformers)
    │        ▼
    │    HNSW vector search → Top-K semantic matches
    │
    ├──► Full-text search (tantivy) → Top-K keyword matches
    │
    ▼
Reciprocal Rank Fusion (RRF) → merged, re-ranked results
```

### Test Generation Flow

```
Target symbol (e.g., UserService.create_user)
    │
    ▼
ContextBuilder
    │  Resolves dependency graph (who does this function call?)
    │  Finds sibling symbols (what else is in this file?)
    │  Locates existing tests (any tests already covering this?)
    │  Extracts imports (what modules are used?)
    │  Detects conventions (pytest? fixtures? mock library?)
    │
    ▼
Prompt Builder
    │  Assembles structured prompt with all context
    │  Adds edge case analysis (division by zero? empty input?)
    │
    ▼
LLM Provider (Claude API)
    │  Generates complete test file
    │
    ▼
Post-processor
    │  Extracts code blocks
    │  Validates syntax
    │  Formats with black/ruff
    │
    ▼
Generated test file written to output_dir
```

## Storage

All persistent data lives under `.testforge/` in the project root:

```
.testforge/
├── config.toml          # User configuration
├── index/
│   └── testforge.db     # SQLite database (files + symbols)
└── cache/
    └── embeddings/      # Cached embedding vectors (.npy files)
```

The SQLite database uses WAL journal mode for concurrent read access
(important when the file watcher and search run simultaneously).

## Concurrency Model

- **Indexing** is currently single-threaded (Phase 1). Phase 2 will add
  parallel parsing using `rayon`, with one tree-sitter `Parser` instance
  per thread (tree-sitter parsers are not `Send`).
- **File watching** runs in a dedicated thread, sending events through
  an `mpsc` channel with 200ms debouncing.
- **The API server** (Phase 4) will use `tokio` with `axum` for async
  request handling.
- **Embedding computation** can be parallelized on the Python side via
  `sentence-transformers`' built-in batching.

## Why Rust + Python?

We evaluated three architectures:

| Approach | Pros | Cons |
|----------|------|------|
| Pure Python | Fast iteration, rich ML ecosystem | Slow indexing, high memory use |
| Pure Rust | Maximum performance | No ML ecosystem, complex LLM integration |
| **Rust + Python** | Best of both worlds | Build complexity (PyO3/maturin) |

The hybrid approach lets us use Rust where performance matters (parsing,
indexing, search, CLI) and Python where ecosystem matters (embeddings,
LLM APIs, prompt engineering). PyO3 + maturin make the bridge nearly
transparent — the Python side imports Rust functions as if they were native.
