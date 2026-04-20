.PHONY: build test lint fmt clean install dev release docker help

# Default target
help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

# ── Build ─────────────────────────────────────────────────────

build: ## Build the Rust CLI (debug)
	cargo build

release: ## Build optimized release binary
	cargo build --release
	@echo "\n  Binary: target/release/testforge"

# ── Test ──────────────────────────────────────────────────────

test: test-rust test-python ## Run all tests

test-rust: ## Run Rust tests
	cargo test --workspace

test-python: ## Run Python tests
	PYTHONPATH=python:$$PYTHONPATH python -m pytest tests/ -v

test-python-slow: ## Run Python tests including slow ML tests
	PYTHONPATH=python:$$PYTHONPATH python -m pytest tests/ -v --run-slow

# ── Lint ──────────────────────────────────────────────────────

lint: lint-rust lint-python ## Run all linters

lint-rust: ## Lint Rust code
	cargo clippy --workspace --all-targets -- -D warnings
	cargo fmt --all -- --check

lint-python: ## Lint Python code
	ruff check python/
	mypy python/testforge_ai/ --ignore-missing-imports || true

# ── Format ────────────────────────────────────────────────────

fmt: ## Format all code
	cargo fmt --all
	ruff format python/

# ── Install ───────────────────────────────────────────────────

install: release ## Install the CLI binary
	cargo install --path crates/testforge-cli

install-python: ## Install the Python AI layer
	pip install maturin
	maturin develop --release
	pip install -e ".[dev]"

dev: ## Set up full development environment
	cargo build
	pip install -e ".[dev]" || pip install pytest numpy ruff --break-system-packages
	@echo "\n  Development environment ready!"
	@echo "  Run 'make test' to verify."

# ── Docker ────────────────────────────────────────────────────

docker: ## Build Docker image
	docker build -t testforge:latest -f docker/Dockerfile .

docker-run: docker ## Run TestForge in Docker
	docker run --rm -v $$(pwd):/project testforge:latest index .

# ── VS Code Extension ────────────────────────────────────────

vscode-build: ## Build the VS Code extension
	cd vscode-extension && npm install && npm run compile

vscode-package: ## Package the VS Code extension (.vsix)
	cd vscode-extension && npm run package

# ── CI ────────────────────────────────────────────────────────

ci: lint test ## Run full CI pipeline locally
	@echo "\n  ✓ All CI checks passed"

ci-report: build ## Generate CI coverage report
	./target/debug/testforge ci --format json --output testforge-report.json
	@echo "\n  Report: testforge-report.json"

# ── Clean ─────────────────────────────────────────────────────

clean: ## Remove build artifacts
	cargo clean
	rm -rf .pytest_cache __pycache__ tests/__pycache__
	rm -rf python/testforge_ai/__pycache__ python/testforge_ai/**/__pycache__
	rm -rf vscode-extension/out vscode-extension/node_modules
	@echo "  Cleaned."
