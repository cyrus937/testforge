"""
Test generation CLI — invoked by the Rust ``testforge gen-tests`` command.

This module is the bridge between the Rust CLI and the Python AI layer.
It receives symbol targets, builds rich context, generates tests via
the LLM, post-processes the output, and writes test files to disk.

Usage (called by Rust, not directly by users):
    python -m testforge_ai.cli_gen --project /path --target MyClass.method
"""

from __future__ import annotations

import argparse
import json
import logging
import sys
from pathlib import Path

logger = logging.getLogger("testforge_ai.gen")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="TestForge test generation backend",
    )
    parser.add_argument("--project", required=True, help="Project root")
    parser.add_argument("--target", required=True, help="Qualified symbol name")
    parser.add_argument("--framework", default="pytest", help="Test framework")
    parser.add_argument("--provider", default="claude", help="LLM provider")
    parser.add_argument("--model", default=None, help="LLM model override")
    parser.add_argument("--max-tokens", type=int, default=4096)
    parser.add_argument("--edge-cases", action="store_true")
    parser.add_argument("--mocks", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--output-dir", default=None)
    parser.add_argument("-v", "--verbose", action="count", default=0)

    args = parser.parse_args()

    level = {0: logging.WARNING, 1: logging.INFO}.get(args.verbose, logging.DEBUG)
    logging.basicConfig(
        level=level, format="%(levelname)s: %(message)s", stream=sys.stderr
    )

    try:
        result = generate_tests(args)

        if args.dry_run:
            # Print the generated test source to stdout
            print(result["source"])
        else:
            # Output JSON summary
            print(json.dumps(result))

        return 0

    except Exception as e:
        logger.error("Generation failed: %s", e)
        print(json.dumps({"error": str(e)}), file=sys.stderr)
        return 1


def generate_tests(args: argparse.Namespace) -> dict[str, object]:
    """Run the full test generation pipeline."""
    from testforge_ai.analysis.complexity import analyze_complexity
    from testforge_ai.analysis.context import ContextBuilder
    from testforge_ai.bridge import TestForgeBridge
    from testforge_ai.generator.post_process import PostProcessor
    from testforge_ai.generator.prompts.edge_cases import build_edge_case_prompt
    from testforge_ai.generator.prompts.integration import build_integration_prompt
    from testforge_ai.generator.prompts.mock_builder import build_mock_prompt
    from testforge_ai.generator.prompts.unit_test import build_unit_test_prompt

    project_root = Path(args.project).resolve()
    bridge = TestForgeBridge(project_root)

    # 1. Find target symbol
    all_symbols = bridge.get_all_symbols()
    target = next(
        (
            s
            for s in all_symbols
            if s.qualified_name == args.target or s.name == args.target
        ),
        None,
    )

    if target is None:
        raise ValueError(f"Symbol '{args.target}' not found in index")

    logger.info(
        "Target: %s (%s, %d lines)",
        target.qualified_name,
        target.kind,
        target.line_count,
    )

    # 2. Analyze complexity
    complexity = analyze_complexity(target)
    logger.info(
        "Complexity: CC=%d, risk=%s",
        complexity.cyclomatic_complexity,
        complexity.risk_level,
    )

    # 3. Build rich context
    ctx_builder = ContextBuilder(all_symbols, project_root)
    context = ctx_builder.build(target)

    # 4. Build prompt
    prompt_parts: list[str] = []

    # Main unit test prompt
    context_dict = {
        "dependencies": context.dependencies,
        "siblings": context.siblings,
        "existing_tests": context.related_tests,
    }

    prompt_parts.append(
        build_unit_test_prompt(
            target=target,
            context=context_dict,
            framework=args.framework,
            include_mocks=args.mocks,
        )
    )

    # Edge case analysis
    if args.edge_cases:
        edge_prompt = build_edge_case_prompt(target)
        if edge_prompt:
            prompt_parts.append(edge_prompt)

    # Mock builder (for symbols with external dependencies)
    if args.mocks and target.dependencies:
        mock_prompt = build_mock_prompt(target, context.dependencies)
        if mock_prompt:
            prompt_parts.append(mock_prompt)

    # Integration hint (for methods with many dependencies)
    if len(context.dependencies) >= 3:
        integration_prompt = build_integration_prompt(target, context.dependencies)
        prompt_parts.append(integration_prompt)

    full_prompt = "\n\n---\n\n".join(prompt_parts)

    logger.info("Prompt: %d chars, %d sections", len(full_prompt), len(prompt_parts))

    # 5. Call LLM
    source = _call_llm(
        prompt=full_prompt,
        provider=args.provider,
        model=args.model,
        max_tokens=args.max_tokens,
    )

    # 6. Post-process
    processor = PostProcessor(language=target.language, framework=args.framework)
    processed = processor.process(source, target)

    # 7. Write to file
    file_name = processor.output_filename(target)

    if not args.dry_run and args.output_dir:
        output_dir = Path(args.output_dir)
        output_dir.mkdir(parents=True, exist_ok=True)
        output_path = output_dir / file_name
        output_path.write_text(processed.source, encoding="utf-8")
        logger.info("Written to: %s", output_path)

    return {
        "source": processed.source,
        "file_name": file_name,
        "target_symbol": target.qualified_name,
        "test_count": processed.test_count,
        "framework": args.framework,
        "warnings": processed.warnings,
        "complexity": {
            "cyclomatic": complexity.cyclomatic_complexity,
            "risk": complexity.risk_level,
        },
    }


def _call_llm(
    prompt: str,
    provider: str,
    model: str | None,
    max_tokens: int,
) -> str:
    """Call the LLM and return raw output."""
    if provider == "claude":
        from testforge_ai.generator.providers.claude import ClaudeProvider

        client = ClaudeProvider(model=model) if model else ClaudeProvider()
        return client.generate(prompt=prompt, max_tokens=max_tokens, temperature=0.2)

    elif provider == "openai":
        raise NotImplementedError("OpenAI provider coming soon")

    elif provider == "local":
        raise NotImplementedError("Local model provider coming soon")

    elif provider == "mock":
        # For testing — returns a canned response
        return _mock_response(prompt)

    else:
        raise ValueError(f"Unknown provider: {provider}")


def _mock_response(prompt: str) -> str:
    """Generate a mock test response for testing the pipeline without an API key."""
    # Extract target name from prompt
    import re

    name_match = re.search(r"Target: `(\w+(?:\.\w+)*)`", prompt)
    name = name_match.group(1) if name_match else "target_function"

    framework = "pytest"
    if "jest" in prompt.lower():
        framework = "jest"
    elif "cargo-test" in prompt.lower():
        framework = "cargo-test"

    if framework == "pytest":
        return f'''```python
import pytest
from unittest.mock import MagicMock, patch


class Test{name.replace(".", "_").title()}:
    """Tests for {name}."""

    def test_happy_path(self):
        """Test normal execution with valid inputs."""
        # Arrange
        # TODO: Set up test inputs

        # Act
        # result = {name}(...)

        # Assert
        # assert result == expected
        pass

    def test_empty_input(self):
        """Test behavior with empty input."""
        pass

    def test_invalid_input_raises(self):
        """Test that invalid input raises appropriate exception."""
        # with pytest.raises(ValueError):
        #     {name}(invalid_input)
        pass

    def test_edge_case_none(self):
        """Test behavior with None input."""
        pass

    def test_with_mock_dependency(self):
        """Test with mocked external dependency."""
        # mock_dep = MagicMock()
        # result = {name}(mock_dep)
        # mock_dep.assert_called_once()
        pass
```'''
    return f"// Mock tests for {name}"


if __name__ == "__main__":
    sys.exit(main())
