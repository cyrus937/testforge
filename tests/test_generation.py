"""
Tests for the test generation pipeline.

These tests verify prompt construction, edge case detection, and
context assembly without making actual LLM API calls.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from testforge_ai.bridge import SymbolInfo
from testforge_ai.generator.prompts.unit_test import build_unit_test_prompt
from testforge_ai.generator.prompts.edge_cases import (
    build_edge_case_prompt,
    _detect_potential_edge_cases,
)
from testforge_ai.analysis.complexity import analyze_complexity, prioritize_symbols
from testforge_ai.analysis.context import ContextBuilder, ProjectConventions


# ── Fixtures ──────────────────────────────────────────────────────────


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
    end_line: int = 5,
) -> SymbolInfo:
    """Create a test SymbolInfo with sensible defaults."""
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
        content_hash="abc123",
    )


# ── Prompt Construction Tests ─────────────────────────────────────────


class TestUnitTestPrompt:
    """Tests for the unit test prompt builder."""

    def test_prompt_includes_target_source(self):
        target = make_symbol(
            name="compute_tax",
            source="def compute_tax(amount, rate):\n    return amount * rate",
            signature="def compute_tax(amount, rate)",
        )
        prompt = build_unit_test_prompt(target, context={})

        assert "compute_tax" in prompt
        assert "amount * rate" in prompt

    def test_prompt_includes_docstring(self):
        target = make_symbol(
            name="validate",
            docstring="Validate user input against schema.",
        )
        prompt = build_unit_test_prompt(target, context={})
        assert "Validate user input" in prompt

    def test_prompt_includes_dependencies(self):
        dep = make_symbol(
            name="check_format",
            source="def check_format(data): return True",
        )
        target = make_symbol(
            name="process",
            dependencies=["check_format"],
        )
        context = {"dependencies": [dep], "siblings": [], "existing_tests": []}
        prompt = build_unit_test_prompt(target, context=context)

        assert "check_format" in prompt
        assert "Dependencies" in prompt

    def test_prompt_includes_existing_tests(self):
        existing = make_symbol(
            name="test_validate",
            source="def test_validate():\n    assert validate('ok') is True",
            file_path="tests/test_module.py",
        )
        target = make_symbol(name="validate")
        context = {"dependencies": [], "siblings": [], "existing_tests": [existing]}
        prompt = build_unit_test_prompt(target, context=context)

        assert "Existing tests" in prompt

    def test_prompt_specifies_framework(self):
        target = make_symbol(name="func")
        prompt = build_unit_test_prompt(target, context={}, framework="jest")
        assert "jest" in prompt.lower()

    def test_prompt_includes_mock_instructions(self):
        target = make_symbol(name="func")
        prompt = build_unit_test_prompt(target, context={}, include_mocks=True)
        assert "mock" in prompt.lower() or "Mock" in prompt

    def test_prompt_omits_mock_instructions_when_disabled(self):
        target = make_symbol(name="func")
        prompt = build_unit_test_prompt(target, context={}, include_mocks=False)
        # The mock instruction line should not appear
        assert "Mock external dependencies" not in prompt


# ── Edge Case Detection Tests ─────────────────────────────────────────


class TestEdgeCaseDetection:
    """Tests for heuristic edge case analysis."""

    def test_detects_string_edge_cases(self):
        target = make_symbol(
            name="greet",
            signature="def greet(name: str) -> str",
            source="def greet(name: str) -> str:\n    return f'Hello, {name}!'",
        )
        cases = _detect_potential_edge_cases(target)
        assert any("empty string" in c.lower() for c in cases)

    def test_detects_numeric_edge_cases(self):
        target = make_symbol(
            name="divide",
            signature="def divide(a: float, b: float) -> float",
            source="def divide(a: float, b: float) -> float:\n    return a / b",
        )
        cases = _detect_potential_edge_cases(target)
        assert any("zero" in c.lower() for c in cases)
        assert any("nan" in c.lower() or "infinity" in c.lower() for c in cases)

    def test_detects_collection_edge_cases(self):
        target = make_symbol(
            name="average",
            signature="def average(values: list[float]) -> float",
            source="def average(values: list[float]) -> float:\n    return sum(values) / len(values)",
        )
        cases = _detect_potential_edge_cases(target)
        assert any("empty collection" in c.lower() for c in cases)

    def test_detects_division_by_zero(self):
        target = make_symbol(
            name="ratio",
            signature="def ratio(a: int, b: int) -> float",
            source="def ratio(a: int, b: int) -> float:\n    return a / b",
        )
        cases = _detect_potential_edge_cases(target)
        assert any("division by zero" in c.lower() for c in cases)

    def test_detects_file_operations(self):
        target = make_symbol(
            name="read_config",
            source="def read_config(path):\n    with open(path) as f:\n        return f.read()",
        )
        cases = _detect_potential_edge_cases(target)
        assert any("file not found" in c.lower() for c in cases)

    def test_detects_network_operations(self):
        target = make_symbol(
            name="fetch_data",
            source="def fetch_data(url):\n    response = requests.get(url)\n    return response.json()",
        )
        cases = _detect_potential_edge_cases(target)
        assert any("timeout" in c.lower() or "connection" in c.lower() for c in cases)

    def test_detects_async_edge_cases(self):
        target = make_symbol(
            name="fetch",
            source="async def fetch(url):\n    async with session.get(url) as resp:\n        return await resp.json()",
        )
        cases = _detect_potential_edge_cases(target)
        assert any("concurrent" in c.lower() or "race" in c.lower() for c in cases)

    def test_prompt_includes_detected_cases(self):
        target = make_symbol(
            name="process",
            signature="def process(items: list[str]) -> dict",
            source="def process(items: list[str]) -> dict:\n    result = {}\n    for item in items:\n        result[item] = len(item)\n    return result",
        )
        prompt = build_edge_case_prompt(target)
        assert "Edge Cases" in prompt
        assert "empty collection" in prompt.lower()

    def test_no_edge_cases_returns_empty(self):
        target = make_symbol(
            name="noop",
            signature="def noop()",
            source="def noop():\n    pass",
        )
        prompt = build_edge_case_prompt(target)
        assert prompt == ""


# ── Complexity Analysis Tests ─────────────────────────────────────────


class TestComplexityAnalysis:
    """Tests for the complexity analyzer."""

    def test_simple_function_low_complexity(self):
        target = make_symbol(
            name="add",
            signature="def add(a: int, b: int) -> int",
            source="def add(a: int, b: int) -> int:\n    return a + b",
        )
        report = analyze_complexity(target)
        assert report.cyclomatic_complexity <= 2
        assert report.risk_level == "low"

    def test_complex_function_high_complexity(self):
        source = """def process(data, mode):
    if mode == 'a':
        if data > 0:
            for item in range(data):
                if item % 2 == 0:
                    try:
                        result = compute(item)
                    except ValueError:
                        pass
        elif data < 0:
            while data < 0:
                data += 1
    elif mode == 'b':
        if data and len(data) > 0:
            return True
    else:
        raise ValueError("Unknown mode")
    return False"""

        target = make_symbol(
            name="process",
            signature="def process(data, mode)",
            source=source,
            start_line=1,
            end_line=18,
        )
        report = analyze_complexity(target)
        assert report.cyclomatic_complexity >= 8
        assert report.risk_level in ("medium", "high")

    def test_detects_error_handling(self):
        target = make_symbol(
            name="safe_divide",
            source="def safe_divide(a, b):\n    try:\n        return a / b\n    except ZeroDivisionError:\n        return 0",
        )
        report = analyze_complexity(target)
        assert report.has_error_handling is True

    def test_detects_recursion(self):
        target = make_symbol(
            name="factorial",
            source="def factorial(n):\n    if n <= 1:\n        return 1\n    return n * factorial(n - 1)",
        )
        report = analyze_complexity(target)
        assert report.has_recursion is True

    def test_detects_side_effects(self):
        target = make_symbol(
            name="save",
            source="def save(data):\n    with open('output.txt', 'w') as f:\n        f.write(data)",
        )
        report = analyze_complexity(target)
        assert report.has_side_effects is True

    def test_parameter_count(self):
        target = make_symbol(
            name="func",
            signature="def func(self, a: int, b: str, c: float, d: bool)",
        )
        report = analyze_complexity(target)
        assert report.parameter_count == 4  # self excluded

    def test_prioritize_sorts_by_risk(self):
        simple = make_symbol(
            name="simple",
            source="def simple(): return 1",
            signature="def simple()",
        )
        complex_sym = make_symbol(
            name="complex",
            source="def complex(a, b, c):\n    if a:\n        if b:\n            if c:\n                try:\n                    return open('f').read()\n                except:\n                    pass",
            signature="def complex(a, b, c)",
            start_line=1,
            end_line=8,
        )

        ranked = prioritize_symbols([simple, complex_sym])
        assert ranked[0][0].name == "complex"
        assert ranked[0][1].risk_score > ranked[1][1].risk_score


# ── Context Builder Tests ─────────────────────────────────────────────


class TestContextBuilder:
    """Tests for the context assembly logic."""

    def test_resolves_direct_dependencies(self):
        validate = make_symbol(name="validate", qualified_name="validate")
        process = make_symbol(
            name="process",
            qualified_name="process",
            dependencies=["validate"],
        )

        builder = ContextBuilder([validate, process], project_root=Path("/project"))
        ctx = builder.build(process)

        assert len(ctx.dependencies) == 1
        assert ctx.dependencies[0].name == "validate"

    def test_finds_siblings_in_same_file(self):
        func_a = make_symbol(name="func_a", file_path="src/utils.py")
        func_b = make_symbol(name="func_b", file_path="src/utils.py")
        func_c = make_symbol(name="func_c", file_path="src/other.py")

        builder = ContextBuilder([func_a, func_b, func_c], project_root=Path("/project"))
        ctx = builder.build(func_a)

        sibling_names = [s.name for s in ctx.siblings]
        assert "func_b" in sibling_names
        assert "func_c" not in sibling_names

    def test_finds_related_tests(self):
        target = make_symbol(name="validate", file_path="src/validator.py")
        test_sym = make_symbol(
            name="test_validate_input",
            file_path="tests/test_validator.py",
            source="def test_validate_input():\n    assert validate('ok')",
        )
        other_test = make_symbol(
            name="test_unrelated",
            file_path="tests/test_other.py",
            source="def test_unrelated():\n    assert True",
        )

        builder = ContextBuilder(
            [target, test_sym, other_test], project_root=Path("/project")
        )
        ctx = builder.build(target)

        test_names = [t.name for t in ctx.related_tests]
        assert "test_validate_input" in test_names
        assert "test_unrelated" not in test_names

    def test_trims_context_to_budget(self):
        target = make_symbol(name="target", start_line=1, end_line=10)
        # Create many large dependencies
        deps = [
            make_symbol(
                name=f"dep_{i}",
                qualified_name=f"dep_{i}",
                start_line=1,
                end_line=100,
            )
            for i in range(20)
        ]
        target_with_deps = make_symbol(
            name="target",
            dependencies=[f"dep_{i}" for i in range(20)],
            start_line=1,
            end_line=10,
        )

        builder = ContextBuilder(
            [target_with_deps] + deps, project_root=Path("/project")
        )
        ctx = builder.build(target_with_deps)

        assert ctx.total_context_lines <= 600  # budget + some slack

    def test_detects_pytest_conventions(self):
        test_with_fixture = make_symbol(
            name="test_with_fixture",
            file_path="tests/test_something.py",
            source="@pytest.fixture\ndef db():\n    return MockDB()\n\ndef test_create(db):\n    assert db.create('item')",
        )

        builder = ContextBuilder([test_with_fixture], project_root=Path("/project"))
        conventions = builder._detect_conventions()

        assert conventions.uses_fixtures is True
        assert conventions.assertion_style == "assert"
        assert conventions.test_file_pattern == "test_{name}.py"
