#!/usr/bin/env bash
# TestForge installer — builds and installs from source.
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/testforge/testforge/main/install.sh | bash
#   # or
#   ./install.sh

set -euo pipefail

BOLD='\033[1m'
GREEN='\033[32m'
CYAN='\033[36m'
YELLOW='\033[33m'
RED='\033[31m'
RESET='\033[0m'

log()  { echo -e "  ${GREEN}✓${RESET} $1"; }
warn() { echo -e "  ${YELLOW}△${RESET} $1"; }
err()  { echo -e "  ${RED}✗${RESET} $1" >&2; }
info() { echo -e "  ${CYAN}→${RESET} $1"; }

echo ""
echo -e "  ${BOLD}🔥 TestForge Installer${RESET}"
echo ""

# ── Check prerequisites ──────────────────────────────────────

check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        err "$1 not found. Please install it first."
        echo "    $2"
        exit 1
    fi
    log "$1 found: $(command -v "$1")"
}

check_cmd "rustc" "Install from https://rustup.rs"
check_cmd "cargo" "Install from https://rustup.rs"
check_cmd "python3" "Install Python 3.10+"
echo ""

# ── Check Rust version ───────────────────────────────────────

RUST_VERSION=$(rustc --version | grep -oP '\d+\.\d+')
RUST_MAJOR=$(echo "$RUST_VERSION" | cut -d. -f1)
RUST_MINOR=$(echo "$RUST_VERSION" | cut -d. -f2)

if [ "$RUST_MAJOR" -lt 1 ] || ([ "$RUST_MAJOR" -eq 1 ] && [ "$RUST_MINOR" -lt 75 ]); then
    err "Rust >= 1.75 required (found $RUST_VERSION)"
    echo "    Run: rustup update stable"
    exit 1
fi
log "Rust version $RUST_VERSION OK"

# ── Check Python version ─────────────────────────────────────

PY_VERSION=$(python3 -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')")
PY_MAJOR=$(echo "$PY_VERSION" | cut -d. -f1)
PY_MINOR=$(echo "$PY_VERSION" | cut -d. -f2)

if [ "$PY_MAJOR" -lt 3 ] || ([ "$PY_MAJOR" -eq 3 ] && [ "$PY_MINOR" -lt 10 ]); then
    err "Python >= 3.10 required (found $PY_VERSION)"
    exit 1
fi
log "Python version $PY_VERSION OK"
echo ""

# ── Build Rust ────────────────────────────────────────────────

info "Building Rust CLI (release mode)..."
cargo build --release --bin testforge 2>&1 | tail -3

BINARY="target/release/testforge"
if [ ! -f "$BINARY" ]; then
    err "Build failed — binary not found"
    exit 1
fi
log "Built: $BINARY"

# ── Install binary ────────────────────────────────────────────

INSTALL_DIR="${HOME}/.local/bin"
mkdir -p "$INSTALL_DIR"

cp "$BINARY" "$INSTALL_DIR/testforge"
chmod +x "$INSTALL_DIR/testforge"
log "Installed to $INSTALL_DIR/testforge"

# Check PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    warn "$INSTALL_DIR is not in PATH"
    echo "    Add this to your shell config:"
    echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

# ── Install Python layer (optional) ──────────────────────────

echo ""
read -p "  Install Python AI layer? (requires ~80MB model download) [y/N] " -n 1 -r
echo ""

if [[ $REPLY =~ ^[Yy]$ ]]; then
    info "Installing Python dependencies..."

    if command -v pip3 &>/dev/null; then
        PIP="pip3"
    else
        PIP="python3 -m pip"
    fi

    $PIP install --quiet sentence-transformers anthropic pydantic rich numpy 2>&1 | tail -2
    log "Python AI layer installed"
    echo ""
    info "Pre-downloading embedding model..."
    python3 -c "from sentence_transformers import SentenceTransformer; SentenceTransformer('all-MiniLM-L6-v2')" 2>&1 | tail -1
    log "Embedding model ready"
fi

# ── Done ──────────────────────────────────────────────────────

echo ""
echo -e "  ${GREEN}${BOLD}✓ TestForge installed successfully!${RESET}"
echo ""
echo "  Quick start:"
echo -e "    ${CYAN}cd your-project${RESET}"
echo -e "    ${CYAN}testforge init${RESET}"
echo -e "    ${CYAN}testforge index .${RESET}"
echo -e "    ${CYAN}testforge search \"authentication\"${RESET}"
echo ""
