# Getting Started

This guide walks you through installing TestForge, indexing your first
project, and running your first search and test generation.

## Prerequisites

Before installing TestForge, make sure you have:

- **Rust** ≥ 1.75 — Install from [rustup.rs](https://rustup.rs)
- **Python** ≥ 3.10 — Check with `python3 --version`
- **Git** — For cloning the repo and `.gitignore` support

Optional (for AI features):

- An **Anthropic API key** for test generation with Claude
- **maturin** for building the PyO3 bridge (`pip install maturin`)

## Installation

### From Source (Recommended)

```bash
# Clone the repository
git clone https://github.com/testforge/testforge.git
cd testforge

# Build the Rust CLI
cargo build --release

# The binary is at target/release/testforge
# Add it to your PATH or create a symlink:
sudo ln -s $(pwd)/target/release/testforge /usr/local/bin/testforge

# Verify the installation
testforge --version
```

### Python AI Layer

The AI layer is optional but required for embedding computation and
test generation:

```bash
# Install maturin (builds the Rust-Python bridge)
pip install maturin

# Build and install the Python package
maturin develop --release

# Or install in development mode
pip install -e ".[dev]"
```

### Docker

If you prefer not to install Rust and Python locally:

```bash
cd docker
docker compose build
docker compose run testforge --help
```

## Your First Project

### Step 1: Initialize

Navigate to your project's root directory and initialize TestForge:

```bash
cd ~/my-project
testforge init
```

This creates a `.testforge/` directory with a default configuration file.
You'll see:

```
→ Initializing TestForge in /home/user/my-project

  ✓ Created .testforge/config.toml
  ✓ Created .testforge/index/
  ✓ Created .testforge/cache/

  Next: Run testforge index . to build the search index.
```

### Step 2: Configure (Optional)

Edit `.testforge/config.toml` to customize behavior:

```toml
[project]
name = "my-project"
languages = ["python", "javascript"]  # Empty = auto-detect all

[indexer]
max_file_size_kb = 500  # Skip files larger than this

[llm]
provider = "claude"
api_key_env = "ANTHROPIC_API_KEY"  # Read API key from this env var
```

See [configuration.md](configuration.md) for all available options.

### Step 3: Index

Build the search index:

```bash
testforge index .
```

Output:

```
  ✓ Indexing complete in 0.8s

  Files indexed:  147
  Symbols found:  892
  Files skipped:  0 (unchanged)
```

The index is stored in `.testforge/index/testforge.db` (SQLite).
Subsequent runs only re-index changed files.

### Step 4: Search

Search your codebase with keywords:

```bash
testforge search "authenticate"
```

```
  Found 5 results for "authenticate"

   1.  function  authenticate_user
      ↳ src/auth/service.py:45–78
      def authenticate_user(self, username: str, password: str) -> Optional[Token]
      Authenticate a user with username and password.

   2.  method  AuthMiddleware.authenticate_request
      ↳ src/middleware/auth.py:12–34
      def authenticate_request(self, request: Request) -> bool
      Validate the authentication token in the request header.
```

You can filter by language or symbol kind:

```bash
testforge search "validate" --language python --kind function
```

Or output as JSON for scripting:

```bash
testforge search "payment" --format json | jq '.[].qualified_name'
```

### Step 5: Generate Tests

Set your API key and generate tests for a function:

```bash
export ANTHROPIC_API_KEY=sk-ant-...

testforge gen-tests src/auth/service.py::authenticate_user
```

TestForge will:
1. Look up `authenticate_user` in the index
2. Resolve its dependencies (functions it calls)
3. Find existing tests in your project
4. Detect your testing conventions (pytest? fixtures? mock library?)
5. Build a rich prompt with all this context
6. Call the Claude API to generate tests
7. Post-process and write the test file

The generated tests land in `tests/generated/` by default.

### Step 6: Watch Mode (Optional)

Keep the index up to date automatically:

```bash
testforge index . --watch
```

This starts a file watcher that re-indexes changed files in real-time.
Press Ctrl+C to stop.

## Check Index Status

```bash
testforge status
```

```
  ◆ TestForge Index — /home/user/my-project

  Files indexed:    147
  Symbols extracted: 892
  Embeddings:        0 (not computed yet)
  Languages:         python, javascript
  Last indexed:      2m ago
  Watcher:           inactive
```

## Common Workflows

### "What functions touch the database?"

```bash
testforge search "database" --kind function
```

### "Generate tests for an entire module"

```bash
testforge gen-tests src/payments/ --recursive
```

### "Find code similar to this function"

```bash
testforge search "input validation and sanitization"
```

### "Re-index after a big refactor"

```bash
testforge index . --clean   # Clears index first
```

## Troubleshooting

**"Configuration file not found"**
Run `testforge init` in your project root first.

**"Unsupported language: xyz"**
TestForge currently supports Python, JavaScript/TypeScript, and Rust.
More languages are coming in Phase 5.

**"File too large"**
Increase `indexer.max_file_size_kb` in `.testforge/config.toml`.
The default is 500 KB.

**Slow indexing on large repos**
Narrow the scope with `project.languages` and `project.exclude`
in your config file.

## Next Steps

- Read the [Architecture](architecture.md) guide to understand how TestForge works
- Explore [Configuration](configuration.md) for all tuning options
- Check the [API Reference](api-reference.md) if you want to integrate TestForge
  into your CI/CD pipeline
- See [Contributing](contributing.md) to help improve TestForge
