"""
Disk-backed embedding cache.

Stores computed embeddings keyed by a content hash so that unchanged
code symbols don't need to be re-embedded. Uses a simple directory of
`.npy` files for maximum portability.
"""

from __future__ import annotations

import hashlib
import logging
from pathlib import Path
from typing import Optional

import numpy as np

from testforge_ai.embeddings.provider import EmbeddingProvider, EmbeddingResult

logger = logging.getLogger(__name__)


class EmbeddingCache:
    """
    Wraps an :class:`EmbeddingProvider` with a persistent on-disk cache.

    Cache keys are SHA-256 hashes of the input text, stored as individual
    ``.npy`` files in the cache directory.

    Parameters
    ----------
    provider : EmbeddingProvider
        The underlying provider to delegate to on cache misses.
    cache_dir : Path
        Directory to store cached embeddings.
    """

    def __init__(self, provider: EmbeddingProvider, cache_dir: Path):
        self._provider = provider
        self._cache_dir = cache_dir
        self._cache_dir.mkdir(parents=True, exist_ok=True)
        self._hits = 0
        self._misses = 0

    def embed_texts(self, texts: list[str]) -> list[EmbeddingResult]:
        """
        Embed texts, serving from cache where possible.

        Texts with a cached embedding are returned immediately;
        only cache-missing texts are sent to the underlying provider.
        """
        results: list[Optional[EmbeddingResult]] = [None] * len(texts)
        to_compute: list[tuple[int, str]] = []

        for i, text in enumerate(texts):
            cached = self._get_cached(text)
            if cached is not None:
                results[i] = EmbeddingResult(
                    vector=cached,
                    text=text,
                    token_count=len(text.split()),
                )
                self._hits += 1
            else:
                to_compute.append((i, text))
                self._misses += 1

        if to_compute:
            missing_texts = [t for _, t in to_compute]
            computed = self._provider.embed_texts(missing_texts)

            for (original_idx, text), result in zip(to_compute, computed):
                self._put_cached(text, result.vector)
                results[original_idx] = result

        return [r for r in results if r is not None]

    def embed_single(self, text: str) -> EmbeddingResult:
        """Embed a single text with caching."""
        return self.embed_texts([text])[0]

    def invalidate(self, text: str) -> bool:
        """Remove a specific entry from the cache."""
        path = self._cache_path(text)
        if path.exists():
            path.unlink()
            return True
        return False

    def clear(self) -> int:
        """Clear all cached embeddings. Returns number of entries removed."""
        count = 0
        for f in self._cache_dir.glob("*.npy"):
            f.unlink()
            count += 1
        self._hits = 0
        self._misses = 0
        return count

    @property
    def stats(self) -> dict[str, int]:
        """Cache hit/miss statistics."""
        total = self._hits + self._misses
        return {
            "hits": self._hits,
            "misses": self._misses,
            "total": total,
            "hit_rate_pct": round(self._hits / total * 100) if total > 0 else 0,
            "cached_entries": sum(1 for _ in self._cache_dir.glob("*.npy")),
        }

    # ── Internal ──────────────────────────────────────────────────

    def _cache_path(self, text: str) -> Path:
        key = hashlib.sha256(text.encode("utf-8")).hexdigest()
        return self._cache_dir / f"{key}.npy"

    def _get_cached(self, text: str) -> Optional[np.ndarray]:
        path = self._cache_path(text)
        if path.exists():
            try:
                return np.load(path)
            except Exception:
                logger.warning("Corrupt cache entry %s, removing", path.name)
                path.unlink(missing_ok=True)
        return None

    def _put_cached(self, text: str, vector: np.ndarray) -> None:
        path = self._cache_path(text)
        try:
            np.save(path, vector)
        except Exception as e:
            logger.warning("Failed to cache embedding: %s", e)
