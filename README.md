<p align="center">
  <img src="docs/assets/logo-placeholder.svg" alt="TestForge" width="120" />
</p>

<h1 align="center">TestForge</h1>

<p align="center">
  <strong>AI-powered semantic code search &amp; intelligent test generation.</strong><br/>
  Understand your codebase. Generate tests that actually make sense.
</p>

<p align="center">
  <a href="#-quickstart">Quickstart</a> ·
  <a href="#-how-it-works">How It Works</a> ·
  <a href="#-architecture">Architecture</a> ·
  <a href="#-cli-reference">CLI</a> ·
  <a href="#-api">API</a> ·
  <a href="#-contributing">Contributing</a>
</p>

---

## Why TestForge?

Most AI test generators work **function-by-function** in isolation — they see a signature, guess what the function does, and spit out generic assertions. The result? Shallow tests full of wrong mocks, missing edge cases, and zero awareness of how the code fits into the bigger picture.

**TestForge is different.** It indexes your entire codebase semantically — building a dependency graph, understanding call chains, and mapping the domain context — _before_ generating a single test. The result is tests that mock the right things, cover real edge cases, and read like a senior developer wrote them.

### Key Capabilities

- **Semantic Search** — Ask questions in plain English: _"where do we handle JWT refresh?"_ and get ranked results instantly, even if the code never mentions "JWT" in a function name.
- **Context-Aware Test Generation** — Tests that understand imports, call graphs, and domain logic. Not just `assert True`.
- **Blazing Fast Indexing** — Rust-powered AST parsing with tree-sitter. Index 100k+ LOC projects in under a second.
- **Multi-Language** — Python, Rust, JavaScript, TypeScript, and Java out of the box.
- **Three Interfaces** — CLI for your terminal, REST API for CI/CD, VS Code extension for your editor.

---

## Architecture

TestForge is a **Rust + Python** hybrid:

| Layer | Language | Responsibility |
|-------|----------|----------------|
| Core engine | Rust | AST parsing, indexing, search, CLI, API server |
| AI layer | Python | Embeddings, LLM prompts, test generation |
| Bridge | PyO3 | Zero-overhead Rust ↔ Python interop |

See [docs/architecture.md](docs/architecture.md) for the full design.

---

## Documentation

- [Getting Started](docs/getting-started.md) — Installation and first run
- [Architecture](docs/architecture.md) — System design and data flows
- [Configuration](docs/configuration.md) — All config options explained
- [API Reference](docs/api-reference.md) — REST API endpoints
- [Contributing](docs/contributing.md) — How to contribute

---

## Quickstart

### Prerequisites

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.75+ | Core engine |
| Python | 3.10+ | AI layer |
| Maturin | 1.5+ | Rust ↔ Python bridge |

### Installation

```bash
# Clone the repository
git clone https://github.com/testforge/testforge.git
cd testforge

# Build the Rust engine
cargo build --release

# Install the Python package (compiles the PyO3 bridge)
pip install maturin
maturin develop --release

# Install Python dependencies
pip install -e ".[all]"
```

### First Run

```bash
# Initialize TestForge in your project
cd /path/to/your/project
testforge init

# Index the codebase
testforge index .

# Search semantically
testforge search "authentication logic"

# Generate tests for a file
testforge gen-tests src/auth/login.py
```

---

## How It Works

TestForge operates in three stages:

### 1. Index

The Rust engine walks your project, parses every source file into an AST using [tree-sitter](https://tree-sitter.github.io/), and extracts symbols (functions, classes, methods, imports). It builds a **dependency graph** mapping which symbols call which, and computes **embedding vectors** for semantic search.

```
Source Files → tree-sitter AST → Symbol Extraction → Dependency Graph
                                                    → Embedding Vectors
```

### 2. Search

Queries are embedded using the same model and matched against the indexed vectors via HNSW (Hierarchical Navigable Small World) search. Results are re-ranked using a hybrid strategy that combines vector similarity with full-text matching (powered by [Tantivy](https://github.com/quickwit-oss/tantivy)).

```
"where do we verify permissions?"
        │
        ├─→ Vector search (semantic similarity)
        ├─→ Full-text search (keyword matching)
        └─→ Hybrid re-ranking → Top-K results with context
```

### 3. Generate

When you request tests, TestForge gathers deep context for the target symbol: its signature, body, docstring, dependencies, callers, and even similar functions found via semantic search. This enriched context is sent to an LLM (Claude, GPT, or a local model) with carefully crafted prompts. The generated tests are then validated, formatted, and optionally executed.

```
Target Function
    ├─→ Dependency graph (what it calls, what calls it)
    ├─→ Existing tests in the project (style reference)
    ├─→ Similar functions (via semantic search)
    └─→ Enriched prompt → LLM → Post-processing → Ready-to-run tests
```

---

## Architecture

TestForge is a **Rust + Python hybrid** — each language does what it does best.

```
┌─────────────────────────────────────────────────────────┐
│                     INTERFACES                          │
│   CLI (Rust/Clap)  ·  REST API (Axum)  ·  VS Code      │
├─────────────────────────────────────────────────────────┤
│                   CORE ENGINE (Rust)                    │
│                                                         │
│   Indexer          Search Engine       File Watcher     │
│   (tree-sitter)    (HNSW + Tantivy)    (notify-rs)      │
│                                                         │
│   ┌─────────────────────────────────────────────────┐   │
│   │         Vector Store · SQLite · Graph            │   │
│   └─────────────────────────────────────────────────┘   │
├──────────────────── PyO3 Bridge ────────────────────────┤
│                   AI LAYER (Python)                      │
│                                                         │
│   Embeddings            Test Generator     Prompt       │
│   (sentence-transformers  (Claude / GPT /   Engine      │
│    or OpenAI)              Ollama)                       │
└─────────────────────────────────────────────────────────┘
```

### Project Layout

```
testforge/
├── Cargo.toml                  # Rust workspace root
├── pyproject.toml              # Python package (maturin)
│
├── crates/
│   ├── testforge-core/         # Shared types, config, errors
│   ├── testforge-indexer/      # AST parsing, symbol extraction, graph
│   ├── testforge-python/       # PyO3 bridge (compiles to testforge_rust)
│   ├── testforge-cli/          # CLI interface (planned)
│   └── testforge-server/       # REST API (planned)
│
├── python/
│   └── testforge_ai/
│       ├── embeddings/         # Embedding providers (local, OpenAI)
│       ├── generator/          # LLM-based test generation
│       └── analysis/           # Context enrichment, complexity analysis
│
├── vscode-extension/           # VS Code integration (planned)
├── tests/                      # End-to-end tests
└── docs/                       # Documentation
```

### Rust Crates

| Crate | Role |
|-------|------|
| `testforge-core` | Shared types (`CodeSymbol`, `Language`, `Config`), error handling, configuration |
| `testforge-indexer` | File discovery, tree-sitter parsing, symbol extraction, dependency graph |
| `testforge-python` | PyO3 native extension — exposes `index_project()` and `parse_source()` to Python |

### Key Dependencies

**Rust side:**

| Crate | Purpose |
|-------|---------|
| `tree-sitter` + grammars | Multi-language AST parsing |
| `rayon` | Parallel file indexing |
| `ignore` | `.gitignore`-aware file walking |
| `sha2` | Content hashing for incremental indexing |
| `pyo3` | Python ↔ Rust bridge |
| `serde` / `serde_json` | Serialization |
| `tracing` | Structured logging |

**Python side:**

| Package | Purpose |
|---------|---------|
| `sentence-transformers` | Local embedding generation |
| `anthropic` | Claude API for test generation |
| `numpy` | Vector operations |
| `pydantic` | Data validation |
| `rich` | Terminal output formatting |

---

## CLI Reference

```bash
testforge <command> [options]
```

### Commands

#### `init`

Initialize TestForge in the current project. Creates a `.testforge/` directory with a default `config.toml`.

```bash
testforge init
testforge init --languages python,rust,typescript
```

#### `index`

Parse and index the codebase. Extracts symbols, builds the dependency graph, and generates embeddings.

```bash
testforge index .                    # Index current directory
testforge index /path/to/project     # Index a specific path
testforge index --watch              # Watch mode (re-index on changes)
```

#### `search`

Search the indexed codebase using natural language queries.

```bash
testforge search "error handling"
testforge search "database connection pooling" --limit 10
testforge search "JWT validation" --format json
```

#### `gen-tests`

Generate tests for a file, class, or function.

```bash
testforge gen-tests src/auth/login.py              # Entire file
testforge gen-tests src/auth/ --recursive           # Entire directory
testforge gen-tests src/api.py::UserService         # Specific class
testforge gen-tests src/utils.py --style pytest     # Specify framework
testforge gen-tests src/utils.py --edge-cases       # Include edge cases
testforge gen-tests src/main.rs --dry-run           # Preview only
```

#### `status`

Show the current state of the index.

```bash
testforge status
```

#### `config`

View or modify configuration.

```bash
testforge config set llm.provider claude
testforge config set embeddings.provider local
testforge config get llm.model
```

---

## API

### REST Endpoints (planned)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/index` | Trigger indexation |
| `GET` | `/api/status` | Index status |
| `POST` | `/api/search` | Semantic search |
| `POST` | `/api/generate-tests` | Generate tests |
| `GET` | `/api/generate-tests/:id` | Generation job status |
| `WS` | `/ws/progress` | Real-time progress |
| `GET` | `/api/health` | Health check |

### Python SDK

```python
from testforge_ai import TestForge

# Initialize and index
forge = TestForge("/path/to/project")
forge.index()

# Semantic search
results = forge.search("payment processing")
for r in results:
    print(f"  {r.symbol.name} in {r.symbol.file_path} (score: {r.score:.2f})")

# Generate tests
tests = forge.generate_tests("src/payments/stripe.py")
print(tests.code)
tests.save()  # writes to tests/generated/
```

### Rust API (direct usage)

```rust
use testforge_core::TestForgeConfig;
use testforge_indexer::Indexer;

let config = TestForgeConfig::load_or_default(&project_root)?;
let indexer = Indexer::new(config);
let index = indexer.index(&project_root)?;

// Explore the index
for sym in index.symbols_of_kind(SymbolKind::Function) {
    println!("{} ({}:{})", sym.name, sym.file_path.display(), sym.line_start);
}

// Query the dependency graph
let callees = index.graph.callees(&symbol.id);
let callers = index.graph.callers(&symbol.id);
```

---

## Configuration

TestForge reads from `.testforge/config.toml` in your project root. Every field is optional and falls back to sensible defaults.

```toml
[project]
name = "my-app"
languages = ["python", "typescript"]       # empty = all supported
exclude = ["node_modules", ".venv", "dist", "__pycache__"]

[indexer]
max_file_size_kb = 500        # Skip files larger than this
watch = false                 # Enable filesystem watching
parallelism = 8               # Indexing worker threads

[embeddings]
provider = "local"            # "local" | "openai"
model = "all-MiniLM-L6-v2"   # Model for local embeddings
cache_enabled = true          # Cache embeddings on disk

[llm]
provider = "claude"                   # "claude" | "openai" | "local"
model = "claude-sonnet-4-20250514"    # Model identifier
api_key_env = "ANTHROPIC_API_KEY"     # Env var holding the API key
max_tokens = 4096
temperature = 0.2

[generation]
test_framework = "pytest"     # "pytest" | "unittest" | "jest" | "cargo_test"
include_edge_cases = true
include_mocks = true
auto_run = false              # Run tests after generation
output_dir = "tests/generated/"

[server]
host = "127.0.0.1"
port = 7654
```

---

## Supported Languages

| Language | Extensions | Parsing | Search | Test Gen |
|----------|-----------|---------|--------|----------|
| Python | `.py`, `.pyi` | ✅ | ✅ | ✅ |
| Rust | `.rs` | ✅ | ✅ | ✅ |
| JavaScript | `.js`, `.jsx`, `.mjs` | ✅ | ✅ | ✅ |
| TypeScript | `.ts`, `.tsx` | ✅ | ✅ | ✅ |
| Java | `.java` | ✅ | ✅ | ✅ |
| Go | `.go` | 🔜 | 🔜 | 🔜 |
| C# | `.cs` | 🔜 | 🔜 | 🔜 |

---

## Roadmap

- [x] **Phase 1** — Core engine: config, models, tree-sitter parsing, symbol extraction, dependency graph, PyO3 bridge
- [ ] **Phase 2** — Search: HNSW vector store, Tantivy full-text, hybrid ranking, CLI commands
- [ ] **Phase 3** — Test generation: context builder, prompt engine, Claude/GPT integration, post-processing
- [ ] **Phase 4** — API & IDE: Axum REST server, WebSocket progress, VS Code extension, live file watcher
- [ ] **Phase 5** — Polish: Go/C# support, CI/CD mode, web dashboard, packaging (brew, cargo install, pip)

---

## Development

### Building from Source

```bash
# Rust
cargo build                     # Debug build
cargo build --release           # Optimized build
cargo test                      # Run all Rust tests

# Python bridge
maturin develop                 # Build + install in current venv
maturin develop --release       # Optimized Python extension

# Python tests
pytest tests/ -v

# Linting
cargo clippy --workspace        # Rust linting
ruff check python/              # Python linting
mypy python/                    # Type checking
```

### Running Tests

```bash
# Full test suite
cargo test --workspace && pytest tests/

# Specific crate
cargo test -p testforge-indexer

# With logging
RUST_LOG=debug cargo test -p testforge-indexer -- --nocapture
```

### Project Conventions

- **Rust:** follow standard Rust idioms, use `thiserror` for errors, `tracing` for logs
- **Python:** type hints everywhere, Pydantic for validation, Ruff for formatting
- **Commits:** conventional commits (`feat:`, `fix:`, `docs:`, `refactor:`)
- **Branches:** `main` is stable, feature branches off `main`

---

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](docs/contributing.md) for guidelines.

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Write tests for your changes
4. Ensure all tests pass: `cargo test --workspace && pytest`
5. Submit a pull request

---

## License

MIT — see [LICENSE](LICENSE) for details.

---

<p align="center">
  Built with 🦀 Rust + 🐍 Python
</p>