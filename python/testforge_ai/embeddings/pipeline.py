"""
Embedding pipeline — orchestrates the full symbol → vector flow.

Reads symbols from the Rust index, computes embeddings in batches
via a configurable provider, caches results, and writes vectors
back for the search engine to use.

This is the bridge between Phase 1 (indexing) and Phase 2 (semantic search).
"""

from __future__ import annotations

import logging
import time
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from testforge_ai.bridge import SymbolInfo, TestForgeBridge
from testforge_ai.embeddings.cache import EmbeddingCache
from testforge_ai.embeddings.local import LocalEmbeddingProvider
from testforge_ai.embeddings.provider import EmbeddingProvider

logger = logging.getLogger(__name__)


@dataclass
class EmbeddingPipelineConfig:
    """Configuration for the embedding pipeline."""

    provider: str = "local"
    model: str = "all-MiniLM-L6-v2"
    batch_size: int = 64
    cache_enabled: bool = True
    max_chunk_tokens: int = 512
    include_docstrings: bool = True
    include_signatures: bool = True
    language_prefix: bool = True


@dataclass
class PipelineReport:
    """Results of an embedding pipeline run."""

    total_symbols: int = 0
    embedded: int = 0
    cached: int = 0
    skipped: int = 0
    errors: int = 0
    duration_seconds: float = 0.0
    dimension: int = 0

    @property
    def summary(self) -> str:
        return (
            f"Embedded {self.embedded} symbols ({self.cached} cached, "
            f"{self.skipped} skipped, {self.errors} errors) "
            f"in {self.duration_seconds:.1f}s "
            f"[dim={self.dimension}]"
        )


class EmbeddingPipeline:
    """
    Full pipeline: symbols → text chunks → embeddings → vector store.

    Parameters
    ----------
    project_root : Path
        Project root with ``.testforge/`` directory.
    config : EmbeddingPipelineConfig
        Pipeline configuration.
    """

    def __init__(
        self,
        project_root: Path,
        config: EmbeddingPipelineConfig | None = None,
    ):
        self.project_root = project_root.resolve()
        self.config = config or EmbeddingPipelineConfig()
        self.bridge = TestForgeBridge(project_root)

        # Initialize provider
        self._provider = self._init_provider()

        # Initialize cache
        cache_dir = self.project_root / ".testforge" / "cache" / "embeddings"
        self._cache: EmbeddingCache | None
        if self.config.cache_enabled:
            self._cache = EmbeddingCache(self._provider, cache_dir)
        else:
            self._cache = None

    def run(self) -> PipelineReport:
        """
        Run the full embedding pipeline.

        1. Load all symbols from the Rust index
        2. Build text chunks for each symbol
        3. Compute embeddings (with caching)
        4. Write vectors to the vector store file

        Returns
        -------
        PipelineReport
            Summary of the pipeline run.
        """
        start = time.time()
        report = PipelineReport()

        # 1. Load symbols
        logger.info("Loading symbols from index...")
        symbols = self.bridge.get_all_symbols()
        report.total_symbols = len(symbols)
        logger.info("Found %d symbols", len(symbols))

        if not symbols:
            report.duration_seconds = time.time() - start
            return report

        # 2. Build text chunks
        chunks = [self._build_chunk(sym) for sym in symbols]

        # 3. Compute embeddings in batches
        vectors: list[list[float] | None] = [None] * len(symbols)
        batch_size = self.config.batch_size

        for batch_start in range(0, len(chunks), batch_size):
            batch_end = min(batch_start + batch_size, len(chunks))
            batch_texts = chunks[batch_start:batch_end]
            batch_indices = list(range(batch_start, batch_end))

            try:
                if self._cache:
                    results = self._cache.embed_texts(batch_texts)
                else:
                    results = self._provider.embed_texts(batch_texts)

                for idx, result in zip(batch_indices, results, strict=False):
                    vectors[idx] = result.vector.tolist()
                    report.embedded += 1

            except Exception as e:
                logger.error(
                    "Failed to embed batch [%d:%d]: %s",
                    batch_start, batch_end, e,
                )
                report.errors += len(batch_indices)

            # Log progress
            done = min(batch_end, len(chunks))
            if done % (batch_size * 5) == 0 or done == len(chunks):
                logger.info("Progress: %d/%d symbols", done, len(chunks))

        # 4. Write vectors to disk
        report.dimension = self._provider.dimension()
        self._write_vectors(symbols, vectors)

        report.duration_seconds = time.time() - start

        if self._cache:
            cache_stats = self._cache.stats
            report.cached = cache_stats["hits"]

        logger.info(report.summary)
        return report

    def embed_query(self, query: str) -> list[float]:
        """
        Embed a search query for vector retrieval.

        Parameters
        ----------
        query : str
            Natural language search query.

        Returns
        -------
        list[float]
            Query embedding vector.
        """
        result = self._provider.embed_query(query)
        return cast(list[float], result.vector.tolist())

    def embed_symbol(self, symbol: SymbolInfo) -> list[float]:
        """Embed a single symbol."""
        chunk = self._build_chunk(symbol)
        if self._cache:
            result = self._cache.embed_single(chunk)
        else:
            result = self._provider.embed_single(chunk)
        return cast(list[float], result.vector.tolist())

    def _build_chunk(self, symbol: SymbolInfo) -> str:
        """
        Build a text chunk for embedding from a symbol.

        Combines the symbol's name, signature, docstring, and source
        into a single text that captures its semantic meaning.

        The chunk format is optimized for sentence-transformers:
        short, focused text that captures intent.
        """
        parts: list[str] = []

        # Language prefix helps disambiguate across languages
        if self.config.language_prefix:
            parts.append(f"[{symbol.language}]")

        # Symbol kind + name
        parts.append(f"{symbol.kind}: {symbol.qualified_name}")

        # Signature (most informative single line)
        if self.config.include_signatures and symbol.signature:
            parts.append(symbol.signature)

        # Docstring (semantic intent)
        if self.config.include_docstrings and symbol.docstring:
            doc = symbol.docstring
            # Truncate long docstrings
            if len(doc) > 300:
                doc = doc[:297] + "..."
            parts.append(doc)

        # Source code (truncated for embedding efficiency)
        source = symbol.source
        lines = source.splitlines()
        if len(lines) > 30:
            # Keep first 15 + last 10 lines (signature + body end)
            truncated_lines = [*lines[:15], "  # ...", *lines[-10:]]
            source = "\n".join(truncated_lines)

        parts.append(source)

        return "\n".join(parts)

    def _init_provider(self) -> EmbeddingProvider:
        """Initialize the embedding provider based on config."""
        if self.config.provider == "local":
            return LocalEmbeddingProvider(
                model_name=self.config.model,
                batch_size=self.config.batch_size,
            )
        elif self.config.provider == "openai":
            raise NotImplementedError("OpenAI embedding provider coming soon")
        else:
            raise ValueError(f"Unknown provider: {self.config.provider}")

    def _write_vectors(
        self,
        symbols: list[SymbolInfo],
        vectors: list[list[float] | None],
    ) -> None:
        """
        Write computed vectors to the binary vector store file.

        Format compatible with the Rust VectorStore::load_from_disk().
        """
        import json
        import struct
        import uuid as uuid_mod

        output_dir = self.project_root / ".testforge" / "search" / "vectors"
        output_dir.mkdir(parents=True, exist_ok=True)
        output_path = output_dir / "vectors.bin"

        # Filter to only symbols with valid vectors
        entries: list[tuple[str, list[float]]] = []
        for sym, vec in zip(symbols, vectors, strict=False):
            if vec is not None:
                # Use content_hash as a stable ID proxy, or generate from name
                # The Rust side uses UUID, so we need to parse/generate one
                try:
                    sym_id = uuid_mod.UUID(sym.content_hash[:32].ljust(32, '0'))
                except ValueError:
                    sym_id = uuid_mod.uuid5(uuid_mod.NAMESPACE_DNS, sym.qualified_name)
                entries.append((str(sym_id), vec))

        if not entries:
            logger.warning("No vectors to write")
            return

        dimension = len(entries[0][1])

        # Build binary format matching Rust VectorStore
        header = json.dumps({
            "version": 1,
            "dimension": dimension,
            "count": len(entries),
        }).encode("utf-8")

        data = bytearray()

        # Header length (4 bytes LE)
        data.extend(struct.pack("<I", len(header)))
        # Header JSON
        data.extend(header)

        # UUIDs (16 bytes each)
        for id_str, _ in entries:
            uid = uuid_mod.UUID(id_str)
            data.extend(uid.bytes)

        # Vectors (f32 LE)
        for _, vec in entries:
            for v in vec:
                data.extend(struct.pack("<f", v))

        output_path.write_bytes(bytes(data))
        logger.info(
            "Wrote %d vectors (%d dimensions) to %s",
            len(entries), dimension, output_path,
        )
