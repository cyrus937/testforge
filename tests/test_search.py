"""
Tests for the embedding subsystem.

Tests the provider interface, local embedding provider, and disk cache
without requiring a GPU or external API.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

from testforge_ai.embeddings.provider import EmbeddingResult
from testforge_ai.embeddings.cache import EmbeddingCache


class TestEmbeddingResult:
    """Tests for the EmbeddingResult dataclass."""

    def test_dimension_property(self):
        result = EmbeddingResult(
            vector=np.array([1.0, 2.0, 3.0], dtype=np.float32),
            text="test",
            token_count=1,
        )
        assert result.dimension == 3

    def test_cosine_similarity_identical(self):
        vec = np.array([1.0, 0.0, 0.0], dtype=np.float32)
        a = EmbeddingResult(vector=vec, text="a", token_count=1)
        b = EmbeddingResult(vector=vec, text="b", token_count=1)
        assert abs(a.cosine_similarity(b) - 1.0) < 1e-6

    def test_cosine_similarity_orthogonal(self):
        a = EmbeddingResult(
            vector=np.array([1.0, 0.0], dtype=np.float32), text="a", token_count=1
        )
        b = EmbeddingResult(
            vector=np.array([0.0, 1.0], dtype=np.float32), text="b", token_count=1
        )
        assert abs(a.cosine_similarity(b)) < 1e-6

    def test_cosine_similarity_opposite(self):
        a = EmbeddingResult(
            vector=np.array([1.0, 0.0], dtype=np.float32), text="a", token_count=1
        )
        b = EmbeddingResult(
            vector=np.array([-1.0, 0.0], dtype=np.float32), text="b", token_count=1
        )
        assert abs(a.cosine_similarity(b) - (-1.0)) < 1e-6

    def test_cosine_similarity_zero_vector(self):
        a = EmbeddingResult(
            vector=np.array([0.0, 0.0], dtype=np.float32), text="a", token_count=1
        )
        b = EmbeddingResult(
            vector=np.array([1.0, 0.0], dtype=np.float32), text="b", token_count=1
        )
        assert a.cosine_similarity(b) == 0.0


class FakeProvider:
    """Deterministic embedding provider for testing the cache."""

    def __init__(self, dimension: int = 4):
        self._dimension = dimension
        self.call_count = 0

    def embed_texts(self, texts: list[str]) -> list[EmbeddingResult]:
        self.call_count += 1
        results = []
        for text in texts:
            # Deterministic vector from text hash
            seed = hash(text) % (2**31)
            rng = np.random.RandomState(seed)
            vec = rng.randn(self._dimension).astype(np.float32)
            vec /= np.linalg.norm(vec)
            results.append(
                EmbeddingResult(vector=vec, text=text, token_count=len(text.split()))
            )
        return results

    def dimension(self) -> int:
        return self._dimension

    def model_name(self) -> str:
        return "fake-test-model"


class TestEmbeddingCache:
    """Tests for the disk-backed embedding cache."""

    @pytest.fixture
    def cache(self, tmp_path: Path) -> EmbeddingCache:
        provider = FakeProvider(dimension=4)
        return EmbeddingCache(provider, tmp_path / "cache")

    def test_cache_miss_calls_provider(self, cache: EmbeddingCache):
        results = cache.embed_texts(["hello world"])
        assert len(results) == 1
        assert results[0].dimension == 4
        assert cache.stats["misses"] == 1

    def test_cache_hit_avoids_provider(self, cache: EmbeddingCache):
        # First call — miss
        cache.embed_texts(["hello world"])
        # Second call — hit
        cache.embed_texts(["hello world"])
        assert cache.stats["hits"] == 1
        assert cache.stats["misses"] == 1

    def test_mixed_hits_and_misses(self, cache: EmbeddingCache):
        cache.embed_texts(["a", "b"])
        cache.embed_texts(["a", "c"])

        # "a" should be a hit, "c" should be a miss
        assert cache.stats["hits"] == 1
        assert cache.stats["misses"] == 3  # a, b from first call + c from second

    def test_cached_values_match_originals(self, cache: EmbeddingCache):
        original = cache.embed_texts(["test input"])[0]
        cached = cache.embed_texts(["test input"])[0]

        np.testing.assert_array_almost_equal(original.vector, cached.vector)

    def test_invalidate_entry(self, cache: EmbeddingCache):
        cache.embed_texts(["to be removed"])
        assert cache.invalidate("to be removed") is True
        assert cache.invalidate("nonexistent") is False

        # After invalidation, next call should be a miss
        cache.embed_texts(["to be removed"])
        assert cache.stats["misses"] == 2  # initial + after invalidation

    def test_clear_cache(self, cache: EmbeddingCache):
        cache.embed_texts(["a", "b", "c"])
        count = cache.clear()
        assert count == 3
        assert cache.stats["cached_entries"] == 0

    def test_cache_directory_created(self, tmp_path: Path):
        cache_dir = tmp_path / "deep" / "nested" / "cache"
        provider = FakeProvider()
        cache = EmbeddingCache(provider, cache_dir)
        cache.embed_texts(["test"])
        assert cache_dir.exists()

    def test_hit_rate_percentage(self, cache: EmbeddingCache):
        cache.embed_texts(["a"])
        cache.embed_texts(["a"])
        cache.embed_texts(["a"])

        # 1 miss + 2 hits = 67% hit rate
        assert cache.stats["hit_rate_pct"] == 67


class TestLocalEmbeddingProvider:
    """
    Tests for the local sentence-transformers provider.

    These tests are marked as slow because they download and load
    the ML model on first run (~80 MB).
    """

    @pytest.fixture
    def provider(self):
        try:
            from testforge_ai.embeddings.local import LocalEmbeddingProvider
            return LocalEmbeddingProvider(model_name="all-MiniLM-L6-v2")
        except ImportError:
            pytest.skip("sentence-transformers not installed")

    @pytest.mark.slow
    def test_embed_single_text(self, provider):
        result = provider.embed_single("def add(a, b): return a + b")
        assert result.dimension == 384
        assert result.text == "def add(a, b): return a + b"

    @pytest.mark.slow
    def test_embed_batch(self, provider):
        texts = [
            "def add(a, b): return a + b",
            "def subtract(a, b): return a - b",
            "class Calculator: pass",
        ]
        results = provider.embed_texts(texts)
        assert len(results) == 3
        for r in results:
            assert r.dimension == 384

    @pytest.mark.slow
    def test_similar_code_has_high_similarity(self, provider):
        a = provider.embed_single("def add(x, y): return x + y")
        b = provider.embed_single("def sum(a, b): return a + b")
        c = provider.embed_single("class DatabaseConnection: pass")

        sim_ab = a.cosine_similarity(b)
        sim_ac = a.cosine_similarity(c)

        assert sim_ab > sim_ac, (
            f"Similar functions should have higher similarity "
            f"({sim_ab:.3f}) than dissimilar ones ({sim_ac:.3f})"
        )

    @pytest.mark.slow
    def test_embed_code_with_language_prefix(self, provider):
        result = provider.embed_code("x = 42", language="python")
        assert result.dimension == 384
        assert "python" in result.text.lower()
