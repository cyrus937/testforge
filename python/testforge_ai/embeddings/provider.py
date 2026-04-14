"""Abstract base class for embedding providers."""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class EmbeddingResult:
    """Single embedding result with metadata."""

    vector: np.ndarray
    text: str
    token_count: int

    @property
    def dimension(self) -> int:
        return len(self.vector)

    def cosine_similarity(self, other: EmbeddingResult) -> float:
        """Compute cosine similarity with another embedding."""
        dot = np.dot(self.vector, other.vector)
        norm_a = np.linalg.norm(self.vector)
        norm_b = np.linalg.norm(other.vector)
        if norm_a == 0 or norm_b == 0:
            return 0.0
        return float(dot / (norm_a * norm_b))


class EmbeddingProvider(ABC):
    """
    Interface that all embedding providers must implement.

    Providers are responsible for converting text (code snippets,
    docstrings, natural language queries) into dense vector representations.
    """

    @abstractmethod
    def embed_texts(self, texts: list[str]) -> list[EmbeddingResult]:
        """
        Generate embeddings for a batch of texts.

        Parameters
        ----------
        texts : list[str]
            Input texts to embed. Can be code snippets, docstrings,
            or natural language queries.

        Returns
        -------
        list[EmbeddingResult]
            One embedding per input text, in the same order.
        """
        ...

    def embed_single(self, text: str) -> EmbeddingResult:
        """Convenience method for embedding a single text."""
        results = self.embed_texts([text])
        return results[0]

    @abstractmethod
    def dimension(self) -> int:
        """Return the dimensionality of the embedding vectors."""
        ...

    @abstractmethod
    def model_name(self) -> str:
        """Return the name/identifier of the underlying model."""
        ...

    def embed_code(self, code: str, language: str = "") -> EmbeddingResult:
        """
        Embed a code snippet with optional language prefix.

        Prepending the language helps the model disambiguate syntax
        across programming languages.
        """
        if language:
            prefixed = f"# Language: {language}\n{code}"
        else:
            prefixed = code
        return self.embed_single(prefixed)

    def embed_query(self, query: str) -> EmbeddingResult:
        """
        Embed a natural language search query.

        Some models (e.g., E5) require query prefixes for asymmetric
        retrieval. Override this method if your model needs special
        query formatting.
        """
        return self.embed_single(query)
