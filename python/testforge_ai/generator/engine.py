"""
Test generation engine.

Orchestrates the full pipeline: context assembly → prompt construction →
LLM call → post-processing → output. This is the main entry point
that the CLI and API use to generate tests.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from testforge_ai.bridge import SymbolInfo, TestForgeBridge
from testforge_ai.generator.prompts.edge_cases import build_edge_case_prompt
from testforge_ai.generator.prompts.unit_test import build_unit_test_prompt
from testforge_ai.generator.providers.claude import ClaudeProvider

logger = logging.getLogger(__name__)


@dataclass
class GenerationRequest:
    """Parameters for a test generation request."""

    target: SymbolInfo
    test_framework: str = "pytest"
    include_edge_cases: bool = True
    include_mocks: bool = True
    max_tokens: int = 4096
    temperature: float = 0.2


@dataclass
class GeneratedTest:
    """Output of the test generation pipeline."""

    source: str
    file_name: str
    target_symbol: str
    test_count: int
    framework: str
    warnings: list[str] = field(default_factory=list)


class TestGenerator:
    """
    AI-powered test generator.

    Combines code context from the Rust index with LLM generation
    to produce high-quality, contextually-aware tests.

    Parameters
    ----------
    project_root : Path
        Root of the project containing ``.testforge/``.
    provider : str
        LLM provider name: ``"claude"``, ``"openai"``, or ``"local"``.
    api_key : str or None
        API key for the LLM provider. If None, reads from environment.

    Examples
    --------
    >>> gen = TestGenerator(Path("."), provider="claude")
    >>> tests = gen.generate_for_symbol("AuthService.login")
    >>> print(tests.source)
    """

    def __init__(
        self,
        project_root: Path,
        provider: str = "claude",
        api_key: str | None = None,
    ):
        self.bridge = TestForgeBridge(project_root)
        self.provider = self._init_provider(provider, api_key)
        self.project_root = project_root

    def generate_for_symbol(
        self,
        qualified_name: str,
        **kwargs: Any,
    ) -> GeneratedTest:
        """
        Generate tests for a symbol identified by its qualified name.

        Parameters
        ----------
        qualified_name : str
            e.g., ``"AuthService.login"`` or ``"compute_tax"``.
        **kwargs
            Additional parameters forwarded to :class:`GenerationRequest`.
        """
        # Find the target symbol
        all_symbols = self.bridge.get_all_symbols()
        target = next(
            (s for s in all_symbols if s.qualified_name == qualified_name),
            None,
        )

        if target is None:
            raise ValueError(
                f"Symbol '{qualified_name}' not found in index. "
                "Run `testforge index .` to rebuild."
            )

        request = GenerationRequest(target=target, **kwargs)
        return self._generate(request, all_symbols)

    def generate_for_file(
        self,
        file_path: str,
        **kwargs: Any,
    ) -> list[GeneratedTest]:
        """Generate tests for all public functions/methods in a file."""
        symbols = self.bridge.get_symbols_in_file(file_path)
        public_symbols = [
            s for s in symbols
            if s.visibility == "public"
            and s.kind in ("function", "method")
        ]

        results = []
        for sym in public_symbols:
            try:
                all_symbols = self.bridge.get_all_symbols()
                request = GenerationRequest(target=sym, **kwargs)
                result = self._generate(request, all_symbols)
                results.append(result)
            except Exception as e:
                logger.warning("Failed to generate tests for %s: %s", sym.qualified_name, e)

        return results

    def _generate(
        self,
        request: GenerationRequest,
        all_symbols: list[SymbolInfo],
    ) -> GeneratedTest:
        """Internal generation pipeline."""
        target = request.target

        # 1. Assemble context
        context = self._build_context(target, all_symbols)

        # 2. Build prompt
        prompt = build_unit_test_prompt(
            target=target,
            context=context,
            framework=request.test_framework,
            include_mocks=request.include_mocks,
        )

        # 3. Optionally add edge case analysis
        if request.include_edge_cases:
            edge_prompt = build_edge_case_prompt(target)
            prompt += "\n\n" + edge_prompt

        # 4. Call LLM
        logger.info("Generating tests for %s...", target.qualified_name)
        raw_output = self.provider.generate(
            prompt=prompt,
            max_tokens=request.max_tokens,
            temperature=request.temperature,
        )

        # 5. Post-process
        source = self._extract_code(raw_output)
        test_count = source.count("def test_") + source.count("fn test_")

        # 6. Build output filename
        file_stem = Path(target.file_path).stem
        if target.language == "python":
            file_name = f"test_{file_stem}.py"
        elif target.language == "rust":
            file_name = f"{file_stem}_test.rs"
        else:
            file_name = f"{file_stem}.test.{_lang_ext(target.language)}"

        return GeneratedTest(
            source=source,
            file_name=file_name,
            target_symbol=target.qualified_name,
            test_count=test_count,
            framework=request.test_framework,
        )

    def _build_context(
        self,
        target: SymbolInfo,
        all_symbols: list[SymbolInfo],
    ) -> dict[str, list[SymbolInfo]]:
        """Assemble rich context for the LLM prompt."""
        # Find dependencies
        deps = [
            s for s in all_symbols
            if s.qualified_name in target.dependencies
        ]

        # Find siblings (other symbols in the same file)
        siblings = [
            s for s in all_symbols
            if s.file_path == target.file_path
            and s.qualified_name != target.qualified_name
        ]

        # Find existing tests
        existing_tests = [
            s for s in all_symbols
            if "test" in s.file_path.lower()
            and any(dep in s.source for dep in [target.name])
        ]

        return {
            "dependencies": deps,
            "siblings": siblings,
            "existing_tests": existing_tests,
        }

    def _extract_code(self, raw: str) -> str:
        """Extract code blocks from LLM output."""
        # Look for fenced code blocks
        lines = raw.split("\n")
        in_block = False
        code_lines: list[str] = []

        for line in lines:
            if line.strip().startswith("```"):
                if in_block:
                    break  # end of first code block
                in_block = True
                continue
            if in_block:
                code_lines.append(line)

        if code_lines:
            return "\n".join(code_lines)

        # No code block found — return raw output
        return raw

    def _init_provider(self, name: str, api_key: str | None) -> ClaudeProvider:
        """Initialize the LLM provider."""
        if name == "claude":
            return ClaudeProvider(api_key=api_key)
        else:
            raise ValueError(f"Unsupported LLM provider: {name}")


def _lang_ext(language: str) -> str:
    """Map language name to file extension."""
    return {
        "python": "py",
        "javascript": "js",
        "typescript": "ts",
        "rust": "rs",
        "java": "java",
        "go": "go",
    }.get(language, "txt")
