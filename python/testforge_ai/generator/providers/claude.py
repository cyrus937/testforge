"""
Anthropic Claude LLM provider for test generation.

Handles API authentication, request formatting, rate limiting,
and response parsing for the Claude Messages API.
"""

from __future__ import annotations

import logging
import os

logger = logging.getLogger(__name__)

# Default model for test generation — Sonnet offers the best
# balance of quality and speed for code generation tasks.
DEFAULT_MODEL = "claude-sonnet-4-20250514"


class ClaudeProvider:
    """
    LLM provider backed by the Anthropic Claude API.

    Parameters
    ----------
    api_key : str or None
        Anthropic API key. If None, reads from ``ANTHROPIC_API_KEY`` env var.
    model : str
        Model identifier. Defaults to Claude Sonnet.
    """

    def __init__(
        self,
        api_key: str | None = None,
        model: str = DEFAULT_MODEL,
    ):
        self._api_key = api_key or os.environ.get("ANTHROPIC_API_KEY")
        if not self._api_key:
            raise ValueError(
                "Anthropic API key required. Set ANTHROPIC_API_KEY environment "
                "variable or pass api_key parameter."
            )

        self._model = model
        self._client = None

    def _get_client(self):
        """Lazy-initialize the Anthropic client."""
        if self._client is None:
            try:
                import anthropic
            except ImportError as exc:
                raise ImportError(
                    "anthropic package required. Install with: pip install anthropic"
                ) from exc

            self._client = anthropic.Anthropic(api_key=self._api_key)
            logger.debug("Anthropic client initialized (model=%s)", self._model)

        return self._client

    def generate(
        self,
        prompt: str,
        max_tokens: int = 4096,
        temperature: float = 0.2,
        system: str | None = None,
    ) -> str:
        """
        Generate a completion from the Claude API.

        Parameters
        ----------
        prompt : str
            The user message / prompt.
        max_tokens : int
            Maximum tokens in the response.
        temperature : float
            Sampling temperature (0.0 = deterministic, 1.0 = creative).
        system : str or None
            Optional system prompt override.

        Returns
        -------
        str
            The generated text response.
        """
        client = self._get_client()

        system_prompt = system or (
            "You are an expert software engineer specializing in writing "
            "comprehensive, well-structured tests. Output only code."
        )

        logger.info(
            "Calling Claude API (model=%s, max_tokens=%d, temp=%.1f)",
            self._model,
            max_tokens,
            temperature,
        )

        message = client.messages.create(
            model=self._model,
            max_tokens=max_tokens,
            temperature=temperature,
            system=system_prompt,
            messages=[{"role": "user", "content": prompt}],
        )

        # Extract text from response
        text_parts = []
        for block in message.content:
            if block.type == "text":
                text_parts.append(block.text)

        result = "\n".join(text_parts)
        logger.info(
            "Response received (%d chars, %d input tokens, %d output tokens)",
            len(result),
            message.usage.input_tokens,
            message.usage.output_tokens,
        )

        return result

    @property
    def model_name(self) -> str:
        return self._model
