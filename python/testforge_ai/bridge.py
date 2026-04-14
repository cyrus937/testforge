"""
Bridge between the Rust core engine and the Python AI layer.

This module provides two execution modes:

1. **Native mode** (production): Uses PyO3 to call compiled Rust code
   directly from Python, with near-zero overhead.

2. **Subprocess mode** (fallback): Invokes the ``testforge`` CLI binary
   as a subprocess, parsing its JSON output. Used when the Rust extension
   module is not available (e.g., during development or on unsupported platforms).

The bridge auto-detects which mode to use on import.
"""

from __future__ import annotations

import json
import logging
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

logger = logging.getLogger(__name__)

# Try importing the compiled Rust extension
_NATIVE_AVAILABLE = False
try:
    from testforge_ai import _rust  # type: ignore[attr-defined]

    _NATIVE_AVAILABLE = True
    logger.debug("Rust extension module loaded (native mode)")
except ImportError:
    logger.debug("Rust extension not available, falling back to subprocess mode")


@dataclass
class SymbolInfo:
    """Python-side representation of a code symbol from the Rust index."""

    name: str
    qualified_name: str
    kind: str
    language: str
    file_path: str
    start_line: int
    end_line: int
    source: str
    signature: str | None = None
    docstring: str | None = None
    dependencies: list[str] = field(default_factory=list)
    parent: str | None = None
    visibility: str = "public"
    content_hash: str = ""

    @property
    def line_count(self) -> int:
        return self.end_line - self.start_line + 1


@dataclass
class IndexStatusInfo:
    """Python-side representation of the index status."""

    file_count: int
    symbol_count: int
    embedding_count: int
    languages: list[str]
    last_indexed: str | None = None
    watcher_active: bool = False


class TestForgeBridge:
    """
    High-level Python interface to the TestForge Rust engine.

    Parameters
    ----------
    project_root : Path
        Root directory of the project (must contain ``.testforge/``).

    Examples
    --------
    >>> bridge = TestForgeBridge(Path("/path/to/project"))
    >>> symbols = bridge.get_all_symbols()
    >>> status = bridge.get_status()
    """

    def __init__(self, project_root: Path):
        self.project_root = project_root.resolve()

        if not (self.project_root / ".testforge").is_dir():
            raise FileNotFoundError(
                f"No .testforge directory found at {self.project_root}. "
                "Run `testforge init` first."
            )

        self._native = _NATIVE_AVAILABLE
        if self._native:
            self._engine = _rust.Engine(str(self.project_root))
            logger.info("Bridge initialized in native mode")
        else:
            self._cli_path = shutil.which("testforge")
            if self._cli_path is None:
                raise RuntimeError(
                    "Neither the Rust extension module nor the `testforge` CLI "
                    "binary could be found. Install TestForge with: cargo install testforge-cli"
                )
            logger.info(
                "Bridge initialized in subprocess mode (CLI: %s)", self._cli_path
            )

    @property
    def mode(self) -> str:
        """Return the current execution mode: 'native' or 'subprocess'."""
        return "native" if self._native else "subprocess"

    def get_all_symbols(self) -> list[SymbolInfo]:
        """Retrieve all indexed symbols."""
        if self._native:
            raw = self._engine.all_symbols()
            return [_parse_symbol(s) for s in raw]

        output = self._run_cli(["search", "", "--format", "json"])
        return [_parse_symbol(s) for s in json.loads(output)]

    def search_symbols(self, query: str, limit: int = 10) -> list[SymbolInfo]:
        """Search symbols by name (keyword match)."""
        if self._native:
            raw = self._engine.search(query, limit)
            return [_parse_symbol(s) for s in raw]

        output = self._run_cli(
            ["search", query, "--limit", str(limit), "--format", "json"]
        )
        return [_parse_symbol(s) for s in json.loads(output)]

    def get_status(self) -> IndexStatusInfo:
        """Get the current index status."""
        if self._native:
            raw = self._engine.status()
            return _parse_status(raw)

        output = self._run_cli(["status", "--json"])
        return _parse_status(json.loads(output))

    def index_project(self, clean: bool = False) -> dict:
        """
        Trigger a full index of the project.

        Parameters
        ----------
        clean : bool
            If True, clear the existing index first.

        Returns
        -------
        dict
            Indexing report with file/symbol counts.
        """
        if self._native:
            return self._engine.index(clean)

        args = ["index", "."]
        if clean:
            args.append("--clean")
        output = self._run_cli(args)
        return {"output": output}

    def get_symbol_source(self, qualified_name: str) -> str | None:
        """Get the source code of a specific symbol by qualified name."""
        symbols = self.get_all_symbols()
        for sym in symbols:
            if sym.qualified_name == qualified_name:
                return sym.source
        return None

    def get_symbols_in_file(self, file_path: str) -> list[SymbolInfo]:
        """Get all symbols from a specific file."""
        all_symbols = self.get_all_symbols()
        return [s for s in all_symbols if s.file_path == file_path]

    # ── Internal ──────────────────────────────────────────────────

    def _run_cli(self, args: list[str]) -> str:
        """Run a testforge CLI command and return stdout."""
        cmd = [self._cli_path, *args]
        logger.debug("Running: %s", " ".join(cmd))

        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            cwd=str(self.project_root),
            timeout=120,
        )

        if result.returncode != 0:
            raise RuntimeError(
                f"testforge command failed (exit {result.returncode}): "
                f"{result.stderr.strip()}"
            )

        return result.stdout


def _parse_symbol(data: dict) -> SymbolInfo:
    """Parse a symbol dict (from JSON or PyO3) into SymbolInfo."""
    return SymbolInfo(
        name=data.get("name", ""),
        qualified_name=data.get("qualified_name", ""),
        kind=data.get("kind", "function"),
        language=data.get("language", "unknown"),
        file_path=data.get("file_path", ""),
        start_line=data.get("start_line", 0),
        end_line=data.get("end_line", 0),
        source=data.get("source", ""),
        signature=data.get("signature"),
        docstring=data.get("docstring"),
        dependencies=data.get("dependencies", []),
        parent=data.get("parent"),
        visibility=data.get("visibility", "public"),
        content_hash=data.get("content_hash", ""),
    )


def _parse_status(data: dict) -> IndexStatusInfo:
    """Parse a status dict into IndexStatusInfo."""
    return IndexStatusInfo(
        file_count=data.get("file_count", 0),
        symbol_count=data.get("symbol_count", 0),
        embedding_count=data.get("embedding_count", 0),
        languages=data.get("languages", []),
        last_indexed=data.get("last_indexed"),
        watcher_active=data.get("watcher_active", False),
    )
