# Configuration

TestForge is configured via `.testforge/config.toml` at the project root.
Every option has a sensible default, so a minimal (or even empty) config
file works out of the box.

## Configuration File Location

TestForge discovers its configuration by walking up from the current
directory until it finds a `.testforge/` directory. This means you can
run `testforge` commands from any subdirectory of your project.

```
my-project/
├── .testforge/
│   ├── config.toml      ← configuration lives here
│   ├── index/           ← SQLite database
│   └── cache/           ← embedding cache
├── src/
└── tests/
```

## Full Reference

Below is a complete `config.toml` with all options set to their defaults
and annotated.

```toml
# ─────────────────────────────────────────────────────────────
# Project metadata
# ─────────────────────────────────────────────────────────────

[project]
# Human-readable project name. Defaults to the directory name.
name = "my-project"

# Languages to index. Leave empty to auto-detect from file extensions.
# Valid values: "python", "javascript", "typescript", "rust", "java", "go", "csharp"
languages = []

# Glob patterns and directory names to exclude from indexing.
# These are applied on top of .gitignore rules.
exclude = [
    "node_modules",
    ".venv",
    "__pycache__",
    "target",
    "dist",
    ".git",
    "*.min.js",
    "*.lock",
]


# ─────────────────────────────────────────────────────────────
# Indexer settings
# ─────────────────────────────────────────────────────────────

[indexer]
# Maximum file size (in KB) to index. Files above this are skipped.
# Raise this if you have large generated files you want indexed.
max_file_size_kb = 500

# Whether to auto-start the file watcher when running `testforge index`.
watch = false

# Number of parallel threads for parsing. 0 = auto-detect (uses all cores).
parallelism = 0


# ─────────────────────────────────────────────────────────────
# Embedding model
# ─────────────────────────────────────────────────────────────

[embeddings]
# Provider for computing embeddings.
#   "local"  — sentence-transformers (offline, ~80 MB model download)
#   "openai" — OpenAI Embeddings API (requires API key)
provider = "local"

# Model name or HuggingFace path.
# For "local": any sentence-transformers model
# For "openai": e.g. "text-embedding-3-small"
model = "all-MiniLM-L6-v2"

# Batch size for embedding computation. Larger batches are faster on GPU.
batch_size = 64

# Environment variable containing the API key (only for "openai" provider).
# api_key_env = "OPENAI_API_KEY"


# ─────────────────────────────────────────────────────────────
# LLM provider (for test generation)
# ─────────────────────────────────────────────────────────────

[llm]
# Which LLM to use for generating tests.
#   "claude" — Anthropic Claude API
#   "openai" — OpenAI Chat API
#   "local"  — Local model via Ollama (coming soon)
provider = "claude"

# Model identifier.
model = "claude-sonnet-4-20250514"

# Environment variable containing the API key.
api_key_env = "ANTHROPIC_API_KEY"

# Maximum tokens per generation request.
max_tokens = 4096

# Sampling temperature. Lower = more deterministic.
# 0.0–0.3 recommended for code generation.
temperature = 0.2


# ─────────────────────────────────────────────────────────────
# Test generation preferences
# ─────────────────────────────────────────────────────────────

[generation]
# Target test framework. TestForge uses this to format generated tests.
#   Python:     "pytest", "unittest"
#   JavaScript: "jest", "mocha", "vitest"
#   Rust:       "cargo-test"
#   Java:       "junit"
test_framework = "pytest"

# Whether to include edge case analysis in the prompt.
# When true, TestForge analyzes the function signature and body for
# potential edge cases (empty strings, zero values, null inputs, etc.)
# and adds them to the LLM prompt.
include_edge_cases = true

# Whether to generate mock/stub setups for external dependencies.
include_mocks = true

# Automatically run generated tests after creation.
# Requires the test framework to be installed in the project.
auto_run = false

# Output directory for generated test files (relative to project root).
output_dir = "tests/generated"


# ─────────────────────────────────────────────────────────────
# API server (for VS Code extension and CI/CD integration)
# ─────────────────────────────────────────────────────────────

[server]
# Host to bind the API server to.
host = "127.0.0.1"

# Port number.
port = 7654

# Enable CORS headers (required for the VS Code extension webviews).
cors = true
```

## Environment Variables

TestForge reads API keys from environment variables for security
(never hardcode keys in `config.toml`):

| Variable | Used by | Description |
|----------|---------|-------------|
| `ANTHROPIC_API_KEY` | `llm.provider = "claude"` | Anthropic API key |
| `OPENAI_API_KEY` | `llm.provider = "openai"` or `embeddings.provider = "openai"` | OpenAI API key |
| `TESTFORGE_LOG` | All | Log level override (e.g., `debug`, `trace`) |
| `RUST_LOG` | Rust crates | Fine-grained Rust logging (e.g., `testforge_indexer=debug`) |

## CLI Overrides

Some options can be overridden via CLI flags:

```bash
# Override verbosity
testforge -vvv index .

# Override output format
testforge search "auth" --format json

# Override watch mode
testforge index . --watch

# Clean index before re-indexing
testforge index . --clean
```

## Recommended Configurations

### Small Python Project

```toml
[project]
languages = ["python"]

[generation]
test_framework = "pytest"
```

### Large Monorepo

```toml
[project]
languages = ["python", "typescript"]
exclude = ["node_modules", ".venv", "dist", "build", "vendor", "*.generated.*"]

[indexer]
max_file_size_kb = 1000
parallelism = 8

[embeddings]
batch_size = 128
```

### Offline / Air-Gapped Environment

```toml
[embeddings]
provider = "local"
model = "all-MiniLM-L6-v2"

[llm]
provider = "local"
model = "codellama:13b"
```

### CI/CD Integration

```toml
[server]
host = "0.0.0.0"
port = 7654

[generation]
auto_run = true
output_dir = "tests/generated"
```

## Data Directories

TestForge stores its runtime data under `.testforge/`:

| Path | Contents | Size |
|------|----------|------|
| `.testforge/config.toml` | Configuration | ~1 KB |
| `.testforge/index/testforge.db` | SQLite index (files + symbols) | ~1 MB per 1000 files |
| `.testforge/cache/embeddings/` | Cached embedding vectors | ~1 KB per symbol |

The SQLite database uses WAL journal mode, so you may also see
`testforge.db-wal` and `testforge.db-shm` files during operation.
These are safe to delete when TestForge is not running.

## Ignoring TestForge Data

Add to your `.gitignore`:

```
.testforge/index/
.testforge/cache/
```

Keep `.testforge/config.toml` in version control so your team shares
the same configuration.
