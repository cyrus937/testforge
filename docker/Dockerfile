# ── Stage 1: Build Rust binaries ──────────────────────────────────────
FROM rust:1.82-bookworm AS rust-builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY crates/ crates/

# Build release binary
RUN cargo build --release --bin testforge \
    && strip target/release/testforge

# ── Stage 2: Python + final image ─────────────────────────────────────
FROM python:3.12-slim-bookworm

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy Rust binary
COPY --from=rust-builder /build/target/release/testforge /usr/local/bin/testforge

# Install Python AI layer
WORKDIR /app
COPY pyproject.toml README.md ./
COPY python/ python/

RUN pip install --no-cache-dir . \
    && pip install --no-cache-dir sentence-transformers

# Pre-download the default embedding model
RUN python -c "from sentence_transformers import SentenceTransformer; SentenceTransformer('all-MiniLM-L6-v2')"

# Default working directory for mounted projects
WORKDIR /project

ENTRYPOINT ["testforge"]
CMD ["--help"]
