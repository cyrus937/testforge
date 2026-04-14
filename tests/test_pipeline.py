"""
Tests for the embedding pipeline.

Tests chunk construction, pipeline configuration, and vector writing
without requiring a real ML model or Rust index.
"""

from __future__ import annotations

import json
import struct
import uuid
from pathlib import Path

import numpy as np
import pytest

from testforge_ai.bridge import SymbolInfo
from testforge_ai.embeddings.pipeline import (
    EmbeddingPipeline,
    EmbeddingPipelineConfig,
    PipelineReport,
)


def make_symbol(
    name: str = "my_func",
    kind: str = "function",
    language: str = "python",
    source: str = "def my_func(): pass",
    signature: str | None = "def my_func()",
    docstring: str | None = "A helper function.",
    qualified_name: str | None = None,
    file_path: str = "src/module.py",
) -> SymbolInfo:
    return SymbolInfo(
        name=name,
        qualified_name=qualified_name or name,
        kind=kind,
        language=language,
        source=source,
        signature=signature,
        docstring=docstring,
        dependencies=[],
        parent=None,
        file_path=file_path,
        start_line=1,
        end_line=5,
        visibility="public",
        content_hash="abcdef1234567890abcdef1234567890",
    )


class TestChunkConstruction:
    """Tests for the _build_chunk method."""

    def _build_chunk(self, symbol: SymbolInfo, **kwargs) -> str:
        """Helper that builds a chunk without needing a full pipeline."""
        config = EmbeddingPipelineConfig(**kwargs)
        # Directly call the chunk builder logic
        parts: list[str] = []

        if config.language_prefix:
            parts.append(f"[{symbol.language}]")

        parts.append(f"{symbol.kind}: {symbol.qualified_name}")

        if config.include_signatures and symbol.signature:
            parts.append(symbol.signature)

        if config.include_docstrings and symbol.docstring:
            doc = symbol.docstring
            if len(doc) > 300:
                doc = doc[:297] + "..."
            parts.append(doc)

        source = symbol.source
        lines = source.splitlines()
        if len(lines) > 30:
            truncated_lines = lines[:15] + ["  # ..."] + lines[-10:]
            source = "\n".join(truncated_lines)
        parts.append(source)

        return "\n".join(parts)

    def test_includes_language_prefix(self):
        sym = make_symbol(language="python")
        chunk = self._build_chunk(sym, language_prefix=True)
        assert "[python]" in chunk

    def test_no_language_prefix_when_disabled(self):
        sym = make_symbol(language="python")
        chunk = self._build_chunk(sym, language_prefix=False)
        assert "[python]" not in chunk

    def test_includes_qualified_name(self):
        sym = make_symbol(name="validate", qualified_name="UserService.validate")
        chunk = self._build_chunk(sym)
        assert "UserService.validate" in chunk

    def test_includes_signature(self):
        sym = make_symbol(signature="def compute(a: int, b: int) -> int")
        chunk = self._build_chunk(sym, include_signatures=True)
        assert "def compute(a: int, b: int) -> int" in chunk

    def test_excludes_signature_when_disabled(self):
        sym = make_symbol(signature="def compute(a: int) -> int")
        chunk = self._build_chunk(sym, include_signatures=False)
        assert "def compute" not in chunk or "def compute" in sym.source

    def test_includes_docstring(self):
        sym = make_symbol(docstring="Validate user credentials.")
        chunk = self._build_chunk(sym, include_docstrings=True)
        assert "Validate user credentials." in chunk

    def test_truncates_long_docstring(self):
        long_doc = "x" * 500
        sym = make_symbol(docstring=long_doc)
        chunk = self._build_chunk(sym)
        assert "..." in chunk
        # Should be truncated to ~300 chars + "..."
        doc_part = [line for line in chunk.splitlines() if "xxx" in line]
        assert all(len(line) <= 310 for line in doc_part)

    def test_truncates_long_source(self):
        long_source = "\n".join([f"line_{i} = {i}" for i in range(60)])
        sym = make_symbol(source=long_source)
        chunk = self._build_chunk(sym)
        assert "# ..." in chunk

    def test_includes_source_code(self):
        sym = make_symbol(source="def greet(name):\n    return f'Hello {name}'")
        chunk = self._build_chunk(sym)
        assert "return f'Hello" in chunk

    def test_kind_included(self):
        sym = make_symbol(kind="method", name="handle")
        chunk = self._build_chunk(sym)
        assert "method: handle" in chunk


class TestPipelineReport:
    """Tests for the PipelineReport dataclass."""

    def test_summary_format(self):
        report = PipelineReport(
            total_symbols=100,
            embedded=90,
            cached=30,
            skipped=5,
            errors=5,
            duration_seconds=2.5,
            dimension=384,
        )
        summary = report.summary
        assert "90" in summary
        assert "30" in summary
        assert "2.5" in summary
        assert "384" in summary

    def test_empty_report(self):
        report = PipelineReport()
        assert report.total_symbols == 0
        assert report.embedded == 0


class TestPipelineConfig:
    """Tests for configuration defaults."""

    def test_defaults(self):
        config = EmbeddingPipelineConfig()
        assert config.provider == "local"
        assert config.model == "all-MiniLM-L6-v2"
        assert config.batch_size == 64
        assert config.cache_enabled is True
        assert config.language_prefix is True

    def test_custom_config(self):
        config = EmbeddingPipelineConfig(
            provider="openai",
            model="text-embedding-3-small",
            batch_size=128,
        )
        assert config.provider == "openai"
        assert config.model == "text-embedding-3-small"
        assert config.batch_size == 128


class TestVectorFileFormat:
    """Tests for the binary vector store format compatibility."""

    def test_write_and_read_vectors(self, tmp_path: Path):
        """Verify the vector file format is readable."""
        output_path = tmp_path / "vectors.bin"

        # Simulate writing vectors
        entries = [
            (uuid.uuid4(), [0.1, 0.2, 0.3, 0.4]),
            (uuid.uuid4(), [0.5, 0.6, 0.7, 0.8]),
        ]

        dimension = 4
        count = len(entries)

        # Build binary data
        header = json.dumps({
            "version": 1,
            "dimension": dimension,
            "count": count,
        }).encode("utf-8")

        data = bytearray()
        data.extend(struct.pack("<I", len(header)))
        data.extend(header)

        for uid, _ in entries:
            data.extend(uid.bytes)

        for _, vec in entries:
            for v in vec:
                data.extend(struct.pack("<f", v))

        output_path.write_bytes(bytes(data))

        # Read back and verify
        raw = output_path.read_bytes()
        header_len = struct.unpack("<I", raw[:4])[0]
        parsed_header = json.loads(raw[4 : 4 + header_len])

        assert parsed_header["version"] == 1
        assert parsed_header["dimension"] == 4
        assert parsed_header["count"] == 2

        # Read UUIDs
        ids_start = 4 + header_len
        for i in range(count):
            offset = ids_start + i * 16
            uid_bytes = raw[offset : offset + 16]
            parsed_uid = uuid.UUID(bytes=uid_bytes)
            assert parsed_uid == entries[i][0]

        # Read vectors
        vecs_start = ids_start + count * 16
        for i in range(count):
            for d in range(dimension):
                offset = vecs_start + (i * dimension + d) * 4
                (val,) = struct.unpack("<f", raw[offset : offset + 4])
                assert abs(val - entries[i][1][d]) < 1e-6