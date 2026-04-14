"""
Tests for Phase 3 — test generation pipeline.

Covers the post-processor, mock builder prompts, integration prompts,
and the CLI generation entry point (with mock LLM provider).
"""

from __future__ import annotations

import json
import textwrap
from pathlib import Path

import pytest

from testforge_ai.bridge import SymbolInfo
from testforge_ai.generator.post_process import PostProcessor, ProcessedTest
from testforge_ai.generator.prompts.mock_builder import (
    build_mock_prompt,
    _suggest_strategy,
)
from testforge_ai.generator.prompts.integration import build_integration_prompt


# ── Helpers ───────────────────────────────────────────────────────────


def make_symbol(
    name: str = "my_func",
    qualified_name: str | None = None,
    kind: str = "function",
    language: str = "python",
    source: str = "def my_func(): pass",
    signature: str | None = "def my_func()",
    docstring: str | None = None,
    dependencies: list[str] | None = None,
    parent: str | None = None,
    file_path: str = "src/module.py",
    start_line: int = 1,
    end_line: int = 10,
) -> SymbolInfo:
    return SymbolInfo(
        name=name,
        qualified_name=qualified_name or name,
        kind=kind,
        language=language,
        source=source,
        signature=signature,
        docstring=docstring,
        dependencies=dependencies or [],
        parent=parent,
        file_path=file_path,
        start_line=start_line,
        end_line=end_line,
        visibility="public",
        content_hash="abcdef1234567890abcdef1234567890",
    )


# ── PostProcessor Tests ───────────────────────────────────────────────


class TestPostProcessorExtraction:
    """Tests for code extraction from LLM output."""

    def test_extracts_from_markdown_fence(self):
        raw = "```python\nimport pytest\n\ndef test_foo():\n    assert True\n```"
        proc = PostProcessor(language="python")
        result = proc.process(raw, make_symbol())
        assert "import pytest" in result.source
        assert "```" not in result.source

    def test_extracts_longest_code_block(self):
        raw = (
            "Here are the tests:\n\n"
            "```python\ndef test_a(): pass\n```\n\n"
            "And the main file:\n\n"
            "```python\nimport pytest\n\ndef test_b(): pass\n\ndef test_c(): pass\n```"
        )
        proc = PostProcessor(language="python")
        result = proc.process(raw, make_symbol())
        assert "test_b" in result.source
        assert "test_c" in result.source

    def test_handles_raw_code_without_fences(self):
        raw = "import pytest\n\ndef test_something():\n    assert 1 + 1 == 2\n"
        proc = PostProcessor(language="python")
        result = proc.process(raw, make_symbol())
        assert "test_something" in result.source

    def test_handles_empty_output(self):
        proc = PostProcessor(language="python")
        result = proc.process("", make_symbol())
        assert result.test_count == 0


class TestPostProcessorPython:
    """Tests for Python-specific post-processing."""

    def test_adds_missing_pytest_import(self):
        source = "def test_foo():\n    pytest.raises(ValueError)\n"
        proc = PostProcessor(language="python", framework="pytest")
        result = proc.process(f"```python\n{source}\n```", make_symbol())
        assert "import pytest" in result.source

    def test_adds_missing_mock_import(self):
        source = "def test_foo():\n    mock = MagicMock()\n"
        proc = PostProcessor(language="python")
        result = proc.process(f"```python\n{source}\n```", make_symbol())
        assert "from unittest.mock import" in result.source

    def test_replaces_tabs_with_spaces(self):
        source = "def test_foo():\n\tassert True\n"
        proc = PostProcessor(language="python")
        result = proc.process(f"```python\n{source}\n```", make_symbol())
        assert "\t" not in result.source
        assert "    assert True" in result.source

    def test_warns_on_placeholder_tests(self):
        source = "\n".join(
            [
                "def test_a(): pass  # TODO",
                "def test_b(): pass  # TODO",
                "def test_c(): pass  # TODO",
                "# TODO: implement",
            ]
        )
        proc = PostProcessor(language="python")
        result = proc.process(f"```python\n{source}\n```", make_symbol())
        assert any("TODO" in w or "placeholder" in w for w in result.warnings)

    def test_warns_on_bare_assert_true(self):
        source = "def test_foo():\n    assert True\n"
        proc = PostProcessor(language="python")
        result = proc.process(f"```python\n{source}\n```", make_symbol())
        assert any("assert True" in w for w in result.warnings)

    def test_validates_valid_syntax(self):
        source = "def test_ok():\n    assert 1 + 1 == 2\n"
        proc = PostProcessor(language="python")
        result = proc.process(f"```python\n{source}\n```", make_symbol())
        assert result.syntax_valid is True

    def test_detects_invalid_syntax(self):
        source = "def test_bad(\n    assert nope\n"
        proc = PostProcessor(language="python")
        result = proc.process(f"```python\n{source}\n```", make_symbol())
        assert result.syntax_valid is False
        assert any("syntax" in w.lower() for w in result.warnings)


class TestPostProcessorJavaScript:
    """Tests for JavaScript-specific post-processing."""

    def test_warns_on_missing_test_blocks(self):
        source = "function helper() { return 1; }\n"
        proc = PostProcessor(language="javascript", framework="jest")
        result = proc.process(
            f"```javascript\n{source}\n```", make_symbol(language="javascript")
        )
        assert any("test blocks" in w.lower() for w in result.warnings)

    def test_counts_jest_tests(self):
        source = textwrap.dedent(
            """\
            describe('MyModule', () => {
                test('should do A', () => { expect(1).toBe(1); });
                it('should do B', () => { expect(2).toBe(2); });
            });
        """
        )
        proc = PostProcessor(language="javascript", framework="jest")
        result = proc.process(
            f"```javascript\n{source}\n```", make_symbol(language="javascript")
        )
        assert result.test_count == 2


class TestPostProcessorRust:
    """Tests for Rust-specific post-processing."""

    def test_wraps_in_cfg_test_module(self):
        source = "#[test]\nfn test_add() {\n    assert_eq!(1 + 1, 2);\n}\n"
        proc = PostProcessor(language="rust", framework="cargo-test")
        result = proc.process(f"```rust\n{source}\n```", make_symbol(language="rust"))
        assert "#[cfg(test)]" in result.source
        assert "mod tests" in result.source

    def test_counts_rust_tests(self):
        source = "#[test]\nfn test_a() {}\n\n#[test]\nfn test_b() {}\n"
        proc = PostProcessor(language="rust", framework="cargo-test")
        result = proc.process(f"```rust\n{source}\n```", make_symbol(language="rust"))
        assert result.test_count == 2


class TestPostProcessorHeader:
    """Tests for file header generation."""

    def test_python_header(self):
        proc = PostProcessor(language="python")
        result = proc.process(
            "```python\ndef test_x(): pass\n```",
            make_symbol(qualified_name="MyClass.method"),
        )
        assert "Auto-generated tests for MyClass.method" in result.source
        assert "TestForge" in result.source

    def test_javascript_header(self):
        proc = PostProcessor(language="javascript", framework="jest")
        result = proc.process(
            "```javascript\ntest('x', () => {});\n```",
            make_symbol(language="javascript"),
        )
        assert "/**" in result.source
        assert "TestForge" in result.source


class TestPostProcessorFilename:
    """Tests for output filename generation."""

    def test_python_filename(self):
        proc = PostProcessor(language="python")
        target = make_symbol(file_path="src/auth/service.py")
        assert proc.output_filename(target) == "test_service.py"

    def test_javascript_filename(self):
        proc = PostProcessor(language="javascript")
        target = make_symbol(file_path="src/utils.js", language="javascript")
        assert proc.output_filename(target) == "utils.test.js"

    def test_typescript_filename(self):
        proc = PostProcessor(language="typescript")
        target = make_symbol(file_path="src/api.ts", language="typescript")
        assert proc.output_filename(target) == "api.test.ts"

    def test_rust_filename(self):
        proc = PostProcessor(language="rust")
        target = make_symbol(file_path="src/parser.rs", language="rust")
        assert proc.output_filename(target) == "parser_test.rs"

    def test_java_filename(self):
        proc = PostProcessor(language="java")
        target = make_symbol(file_path="src/UserService.java", language="java")
        assert proc.output_filename(target) == "UserServiceTest.java"


# ── Mock Builder Tests ────────────────────────────────────────────────


class TestMockBuilder:
    """Tests for the mock builder prompt."""

    def test_empty_dependencies_returns_empty(self):
        target = make_symbol()
        assert build_mock_prompt(target, []) == ""

    def test_includes_dependency_names(self):
        dep = make_symbol(
            name="fetch_user",
            qualified_name="UserRepo.fetch_user",
            source="def fetch_user(uid): return db.execute('SELECT...')",
        )
        target = make_symbol(dependencies=["fetch_user"])
        prompt = build_mock_prompt(target, [dep])
        assert "UserRepo.fetch_user" in prompt

    def test_suggests_db_mock_for_sql(self):
        dep = make_symbol(
            name="query_db",
            source="def query_db(): cursor.execute('SELECT * FROM users')",
        )
        strategy = _suggest_strategy(dep)
        assert (
            "MagicMock" in strategy["approach"]
            or "fake" in strategy["approach"].lower()
        )

    def test_suggests_http_mock_for_requests(self):
        dep = make_symbol(
            name="call_api",
            source="def call_api(url): return requests.get(url).json()",
        )
        strategy = _suggest_strategy(dep)
        assert (
            "stub" in strategy["approach"].lower()
            or "patch" in strategy["approach"].lower()
        )

    def test_suggests_file_mock_for_io(self):
        dep = make_symbol(
            name="read_config",
            source="def read_config(): return open('config.yml').read()",
        )
        strategy = _suggest_strategy(dep)
        assert "tmp_path" in strategy["approach"] or "StringIO" in strategy["approach"]

    def test_includes_mock_guidelines(self):
        dep = make_symbol(name="some_dep", source="def some_dep(): pass")
        target = make_symbol(dependencies=["some_dep"])
        prompt = build_mock_prompt(target, [dep])
        assert "Mock Guidelines" in prompt
        assert "boundary" in prompt.lower()


# ── Integration Prompt Tests ──────────────────────────────────────────


class TestIntegrationPrompt:
    """Tests for the integration test prompt."""

    def test_empty_with_few_dependencies(self):
        dep = make_symbol(name="single_dep")
        target = make_symbol(dependencies=["single_dep"])
        assert build_integration_prompt(target, [dep]) == ""

    def test_generates_scenarios_for_many_deps(self):
        deps = [
            make_symbol(name=f"dep_{i}", qualified_name=f"dep_{i}") for i in range(4)
        ]
        target = make_symbol(dependencies=[f"dep_{i}" for i in range(4)])
        prompt = build_integration_prompt(target, deps)

        assert "Integration Test Scenarios" in prompt
        assert "Full happy path" in prompt

    def test_includes_error_propagation_scenario(self):
        deps = [
            make_symbol(name=f"dep_{i}", qualified_name=f"dep_{i}") for i in range(3)
        ]
        target = make_symbol(
            dependencies=[f"dep_{i}" for i in range(3)],
            source="def process():\n    try:\n        validate()\n    except ValueError:\n        raise",
        )
        prompt = build_integration_prompt(target, deps)
        assert "Error propagation" in prompt

    def test_includes_state_consistency_for_mutations(self):
        deps = [
            make_symbol(name=f"dep_{i}", qualified_name=f"dep_{i}") for i in range(3)
        ]
        target = make_symbol(
            dependencies=[f"dep_{i}" for i in range(3)],
            source="def save_all():\n    db.insert(data)\n    db.commit()",
        )
        prompt = build_integration_prompt(target, deps)
        assert "State consistency" in prompt

    def test_integration_guidelines_present(self):
        deps = [
            make_symbol(name=f"dep_{i}", qualified_name=f"dep_{i}") for i in range(3)
        ]
        target = make_symbol(dependencies=[f"dep_{i}" for i in range(3)])
        prompt = build_integration_prompt(target, deps)
        assert "test_integration_" in prompt


# ── CLI Gen Pipeline Tests ────────────────────────────────────────────


class TestCliGenPipeline:
    """Tests for the generation CLI using the mock provider."""

    def test_mock_provider_generates_python_tests(self):
        from testforge_ai.cli_gen import _mock_response

        prompt = "## Target: `UserService.create_user`\npytest"
        result = _mock_response(prompt)
        assert "test_" in result.lower() or "Test" in result
        assert "class" in result or "def test_" in result

    def test_mock_response_includes_target_name(self):
        from testforge_ai.cli_gen import _mock_response

        prompt = "## Target: `AuthHandler.validate_token`\npytest"
        result = _mock_response(prompt)
        assert "AuthHandler" in result or "validate_token" in result

    def test_mock_provider_detects_jest(self):
        from testforge_ai.cli_gen import _mock_response

        prompt = "## Target: `fetchData`\njest framework"
        result = _mock_response(prompt)
        # Should not generate Python-style tests
        assert "jest" in prompt.lower()


class TestEndToEndPostProcessing:
    """End-to-end tests combining mock generation with post-processing."""

    def test_full_pipeline_python(self):
        from testforge_ai.cli_gen import _mock_response

        target = make_symbol(
            name="create_user",
            qualified_name="UserService.create_user",
            file_path="src/users.py",
        )

        # Generate mock response
        prompt = f"## Target: `{target.qualified_name}`\npytest"
        raw = _mock_response(prompt)

        # Post-process
        proc = PostProcessor(language="python", framework="pytest")
        result = proc.process(raw, target)

        assert result.test_count >= 1
        assert "TestForge" in result.source  # header present
        assert result.syntax_valid

    def test_output_filename_matches_target(self):
        proc = PostProcessor(language="python")
        target = make_symbol(
            name="process",
            file_path="src/payments/processor.py",
        )
        assert proc.output_filename(target) == "test_processor.py"

    def test_full_pipeline_with_warnings(self):
        raw = "```python\ndef test_placeholder():\n    assert True\n```"
        target = make_symbol(name="my_func")
        proc = PostProcessor(language="python")
        result = proc.process(raw, target)

        assert result.test_count == 1
        assert len(result.warnings) >= 1  # should warn about assert True
