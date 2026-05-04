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
import sqlite3
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
    logger.debug("Rust extension not available, falling back to SQLite mode")


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
            # SQLite mode — read directly from the index database
            self._db_path = (
                self.project_root / ".testforge" / "index" / "testforge.db"
            )
            if not self._db_path.exists():
                raise FileNotFoundError(
                    f"Index database not found at {self._db_path}. "
                    "Run `testforge index .` first."
                )
            logger.info("Bridge initialized in SQLite mode (db: %s)", self._db_path)

    @property
    def mode(self) -> str:
        """Return the current execution mode: 'native' or 'sqlite'."""
        return "native" if self._native else "sqlite"

    def get_all_symbols(self) -> list[SymbolInfo]:
        """Retrieve all indexed symbols."""
        if self._native:
            raw = self._engine.all_symbols()
            return [_parse_symbol(s) for s in raw]

        return self._query_symbols_from_db()

    def search_symbols(self, query: str, limit: int = 10) -> list[SymbolInfo]:
        """Search symbols by name (case-insensitive substring match)."""
        if self._native:
            raw = self._engine.search(query, limit)
            return [_parse_symbol(s) for s in raw]

        return self._search_symbols_in_db(query, limit)

    def get_status(self) -> IndexStatusInfo:
        """Get the current index status."""
        if self._native:
            raw = self._engine.status()
            return _parse_status(raw)

        return self._status_from_db()

    def index_project(self, clean: bool = False) -> dict:
        """
        Trigger a full index of the project.

        In SQLite mode, delegates to the testforge CLI if available.
        """
        if self._native:
            return self._engine.index(clean)

        cli = shutil.which("testforge")
        if cli is None:
            raise RuntimeError("testforge CLI not found. Cannot trigger indexing.")

        import subprocess
        args = [cli, "index", "."]
        if clean:
            args.append("--clean")

        result = subprocess.run(
            args, capture_output=True, text=True,
            cwd=str(self.project_root), timeout=120,
        )

        if result.returncode != 0:
            raise RuntimeError(f"Indexing failed: {result.stderr.strip()}")

        return {"output": result.stdout}

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

    # ── SQLite direct access ──────────────────────────────────────

    def _get_db(self) -> sqlite3.Connection:
        """Open a read-only connection to the index database."""
        conn = sqlite3.connect(f"file:{self._db_path}?mode=ro", uri=True)
        conn.row_factory = sqlite3.Row
        return conn

    def _query_symbols_from_db(self) -> list[SymbolInfo]:
        """Read all symbols directly from SQLite."""
        conn = self._get_db()
        try:
            rows = conn.execute(
                "SELECT id, name, qualified_name, kind, language, file_path, "
                "start_line, end_line, source, signature, docstring, "
                "dependencies, parent, visibility, content_hash "
                "FROM symbols ORDER BY file_path, start_line"
            ).fetchall()
            return [self._row_to_symbol(row) for row in rows]
        finally:
            conn.close()

    def _search_symbols_in_db(self, query: str, limit: int) -> list[SymbolInfo]:
        """Search symbols in SQLite by name substring."""
        conn = self._get_db()
        try:
            pattern = f"%{query}%"
            rows = conn.execute(
                "SELECT id, name, qualified_name, kind, language, file_path, "
                "start_line, end_line, source, signature, docstring, "
                "dependencies, parent, visibility, content_hash "
                "FROM symbols "
                "WHERE name LIKE ?1 OR qualified_name LIKE ?1 "
                "   OR docstring LIKE ?1 OR source LIKE ?1 "
                "ORDER BY CASE "
                "  WHEN name = ?2 THEN 0 "
                "  WHEN name LIKE ?2 || '%' THEN 1 "
                "  ELSE 2 "
                "END, name "
                "LIMIT ?3",
                (pattern, query, limit),
            ).fetchall()
            return [self._row_to_symbol(row) for row in rows]
        finally:
            conn.close()

    def _status_from_db(self) -> IndexStatusInfo:
        """Read index status directly from SQLite."""
        conn = self._get_db()
        try:
            file_count = conn.execute("SELECT COUNT(*) FROM files").fetchone()[0]
            symbol_count = conn.execute("SELECT COUNT(*) FROM symbols").fetchone()[0]
            embedding_count = conn.execute(
                "SELECT COUNT(*) FROM symbols WHERE embedding IS NOT NULL"
            ).fetchone()[0]

            lang_rows = conn.execute("SELECT DISTINCT language FROM files").fetchall()
            languages = []
            for row in lang_rows:
                try:
                    languages.append(json.loads(row[0]))
                except (json.JSONDecodeError, TypeError):
                    languages.append(row[0])

            last_row = conn.execute("SELECT MAX(indexed_at) FROM files").fetchone()
            last_indexed = last_row[0] if last_row else None

            return IndexStatusInfo(
                file_count=file_count,
                symbol_count=symbol_count,
                embedding_count=embedding_count,
                languages=languages,
                last_indexed=last_indexed,
            )
        finally:
            conn.close()

    def _row_to_symbol(self, row: sqlite3.Row) -> SymbolInfo:
        """Convert a SQLite row to a SymbolInfo."""
        # Parse JSON fields
        try:
            kind = json.loads(row["kind"]).replace('"', '')
        except (json.JSONDecodeError, TypeError):
            kind = str(row["kind"]).strip('"')

        try:
            language = json.loads(row["language"]).replace('"', '')
        except (json.JSONDecodeError, TypeError):
            language = str(row["language"]).strip('"')

        try:
            deps = json.loads(row["dependencies"])
        except (json.JSONDecodeError, TypeError):
            deps = []

        try:
            visibility = json.loads(row["visibility"]).replace('"', '')
        except (json.JSONDecodeError, TypeError):
            visibility = str(row["visibility"]).strip('"')

        return SymbolInfo(
            name=row["name"],
            qualified_name=row["qualified_name"],
            kind=kind,
            language=language,
            file_path=row["file_path"],
            start_line=row["start_line"],
            end_line=row["end_line"],
            source=row["source"],
            signature=row["signature"],
            docstring=row["docstring"],
            dependencies=deps if isinstance(deps, list) else [],
            parent=row["parent"],
            visibility=visibility,
            content_hash=row["content_hash"],
        )


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
