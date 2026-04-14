"""
Local embedding provider using sentence-transformers.

Runs entirely offline — no API keys or network access required.
Uses the `all-MiniLM-L6-v2` model by default (384-dimensional vectors,
~80 MB download on first use).
"""

from __future__ import annotations

import logging
from typing import Optional

import numpy as np

from testforge_ai.embeddings.provider import EmbeddingProvider, EmbeddingResult

logger = logging.getLogger(__name__)

# Default model — good balance of speed, size, and quality for code search
DEFAULT_MODEL = "all-MiniLM-L6-v2"


class LocalEmbeddingProvider(EmbeddingProvider):
    """
    Embedding provider backed by a local sentence-transformers model.

    Parameters
    ----------
    model_name : str
        HuggingFace model name or path. Defaults to ``all-MiniLM-L6-v2``.
    device : str or None
        Torch device (``"cpu"``, ``"cuda"``, ``"mps"``). ``None`` = auto-detect.
    batch_size : int
        Batch size for encoding. Larger batches are faster on GPU.
    normalize : bool
        Whether to L2-normalize output vectors (recommended for cosine similarity).

    Examples
    --------
    >>> provider = LocalEmbeddingProvider()
    >>> result = provider.embed_single("def add(a, b): return a + b")
    >>> result.dimension
    384
    """

    def __init__(
        self,
        model_name: str = DEFAULT_MODEL,
        device: Optional[str] = None,
        batch_size: int = 64,
        normalize: bool = True,
    ):
        self._model_name = model_name
        self._batch_size = batch_size
        self._normalize = normalize
        self._model = None
        self._device = device

    def _load_model(self):
        """Lazy-load the model on first use."""
        if self._model is not None:
            return

        try:
            from sentence_transformers import SentenceTransformer
        except ImportError:
            raise ImportError(
                "sentence-transformers is required for local embeddings. "
                "Install with: pip install sentence-transformers"
            )

        logger.info("Loading embedding model '%s'...", self._model_name)
        self._model = SentenceTransformer(
            self._model_name,
            device=self._device,
        )
        logger.info(
            "Model loaded (dimension=%d, device=%s)",
            self._model.get_sentence_embedding_dimension(),
            self._model.device,
        )

    def embed_texts(self, texts: list[str]) -> list[EmbeddingResult]:
        """Generate embeddings for a batch of texts."""
        self._load_model()
        assert self._model is not None

        # Encode in batches
        vectors = self._model.encode(
            texts,
            batch_size=self._batch_size,
            show_progress_bar=len(texts) > 100,
            normalize_embeddings=self._normalize,
            convert_to_numpy=True,
        )

        results = []
        for text, vector in zip(texts, vectors):
            results.append(
                EmbeddingResult(
                    vector=np.array(vector, dtype=np.float32),
                    text=text,
                    token_count=len(text.split()),  # rough estimate
                )
            )

        return results

    def embed_query(self, query: str) -> EmbeddingResult:
        """
        Embed a search query.

        For symmetric models like MiniLM, no special prefix is needed.
        For asymmetric models (e.g., E5), override to add "query: " prefix.
        """
        return self.embed_single(query)

    def dimension(self) -> int:
        """Return embedding dimensionality."""
        self._load_model()
        assert self._model is not None
        dim = self._model.get_sentence_embedding_dimension()
        assert dim is not None
        return int(dim)

    def model_name(self) -> str:
        return self._model_name
