"""
Code context assembly for test generation.

Builds a rich context bundle from the index, including the target symbol's
dependency graph, sibling functions, existing tests, import statements,
and detected project conventions. This context is what makes TestForge's
generated tests superior to naive per-function generation.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from pathlib import Path

from testforge_ai.bridge import SymbolInfo

logger = logging.getLogger(__name__)


@dataclass
class CodeContext:
    """
    Rich context bundle for a target symbol.

    Assembled from the index and passed to the prompt builder so the LLM
    understands the broader codebase, not just the target function.
    """

    target: SymbolInfo
    dependencies: list[SymbolInfo] = field(default_factory=list)
    siblings: list[SymbolInfo] = field(default_factory=list)
    related_tests: list[SymbolInfo] = field(default_factory=list)
    imports: list[str] = field(default_factory=list)
    conventions: ProjectConventions = field(default_factory=lambda: ProjectConventions())

    @property
    def total_context_lines(self) -> int:
        """Total lines of code in the context (for token budgeting)."""
        total = self.target.line_count
        total += sum(d.line_count for d in self.dependencies)
        total += sum(s.line_count for s in self.siblings)
        total += sum(t.line_count for t in self.related_tests)
        return total

    def trim_to_budget(self, max_lines: int = 500) -> None:
        """
        Trim context to fit within a token budget.

        Prioritizes: target > dependencies > existing tests > siblings.
        Each category is truncated from the end if it exceeds its allocation.
        """
        remaining = max_lines - self.target.line_count

        # Allocate: 40% deps, 30% tests, 30% siblings
        dep_budget = int(remaining * 0.4)
        test_budget = int(remaining * 0.3)
        sib_budget = remaining - dep_budget - test_budget

        self.dependencies = _trim_symbols(self.dependencies, dep_budget)
        self.related_tests = _trim_symbols(self.related_tests, test_budget)
        self.siblings = _trim_symbols(self.siblings, sib_budget)


@dataclass
class ProjectConventions:
    """Detected patterns in the project's test suite."""

    test_file_pattern: str | None = None
    assertion_style: str | None = None
    uses_fixtures: bool = False
    mock_library: str | None = None
    docstring_style: str | None = None


class ContextBuilder:
    """
    Assembles a :class:`CodeContext` from the full symbol index.

    Parameters
    ----------
    all_symbols : list[SymbolInfo]
        Complete list of symbols from the project index.
    project_root : Path
        Root directory of the project.
    """

    def __init__(self, all_symbols: list[SymbolInfo], project_root: Path):
        self._symbols = all_symbols
        self._project_root = project_root
        self._by_name: dict[str, SymbolInfo] = {
            s.qualified_name: s for s in all_symbols
        }
        self._by_file: dict[str, list[SymbolInfo]] = {}
        for s in all_symbols:
            self._by_file.setdefault(s.file_path, []).append(s)

    def build(self, target: SymbolInfo) -> CodeContext:
        """Build the full context for a target symbol."""
        ctx = CodeContext(target=target)

        # Resolve dependencies
        ctx.dependencies = self._resolve_dependencies(target)

        # Collect siblings
        ctx.siblings = [
            s for s in self._by_file.get(target.file_path, [])
            if s.qualified_name != target.qualified_name
        ]

        # Find related tests
        ctx.related_tests = self._find_related_tests(target)

        # Extract imports from the target file
        ctx.imports = self._extract_imports(target.file_path)

        # Detect project conventions
        ctx.conventions = self._detect_conventions()

        # Trim to stay within token budget
        ctx.trim_to_budget()

        logger.info(
            "Context for %s: %d deps, %d siblings, %d tests, %d total lines",
            target.qualified_name,
            len(ctx.dependencies),
            len(ctx.siblings),
            len(ctx.related_tests),
            ctx.total_context_lines,
        )

        return ctx

    def _resolve_dependencies(self, target: SymbolInfo) -> list[SymbolInfo]:
        """Resolve the target's dependency names to actual symbols."""
        deps = []
        for dep_name in target.dependencies:
            # Try exact match first
            if dep_name in self._by_name:
                deps.append(self._by_name[dep_name])
                continue

            # Try qualified match (e.g., "validate" → "Validator.validate")
            matches = [
                s for s in self._symbols
                if s.name == dep_name
                and s.file_path == target.file_path
            ]
            if matches:
                deps.append(matches[0])
                continue

            # Try cross-file match
            matches = [s for s in self._symbols if s.name == dep_name]
            if len(matches) == 1:
                deps.append(matches[0])

        return deps

    def _find_related_tests(self, target: SymbolInfo) -> list[SymbolInfo]:
        """Find existing tests that reference the target symbol."""
        related = []
        for sym in self._symbols:
            # Heuristic: symbol is in a test file and references target
            is_test_file = any(
                marker in sym.file_path.lower()
                for marker in ["test_", "_test.", ".test.", "tests/", "spec/"]
            )
            if is_test_file and target.name in sym.source:
                related.append(sym)

        return related

    def _extract_imports(self, file_path: str) -> list[str]:
        """
        Extract import statements from a source file.

        Reads the actual file from disk since imports aren't stored
        as symbols in the index.
        """
        full_path = self._project_root / file_path
        if not full_path.exists():
            return []

        try:
            source = full_path.read_text(encoding="utf-8")
        except Exception:
            return []

        imports = []
        for line in source.splitlines():
            stripped = line.strip()
            if stripped.startswith(("import ", "from ", "use ", "require(")):
                imports.append(stripped)

        return imports

    def _detect_conventions(self) -> ProjectConventions:
        """
        Detect testing conventions from the project's existing tests.

        Scans test files to identify patterns like assertion style,
        fixture usage, and mock libraries.
        """
        conventions = ProjectConventions()

        test_symbols = [
            s for s in self._symbols
            if any(
                marker in s.file_path.lower()
                for marker in ["test_", "_test.", ".test.", "tests/"]
            )
        ]

        if not test_symbols:
            return conventions

        # Detect test file naming pattern
        test_files = set(s.file_path for s in test_symbols)
        if any("test_" in Path(f).name for f in test_files):
            conventions.test_file_pattern = "test_{name}.py"
        elif any("_test." in f for f in test_files):
            conventions.test_file_pattern = "{name}_test.py"
        elif any(".test." in f for f in test_files):
            conventions.test_file_pattern = "{name}.test.js"

        # Sample test sources for pattern detection
        sample_sources = " ".join(s.source for s in test_symbols[:20])

        # Assertion style
        if "assert " in sample_sources:
            conventions.assertion_style = "assert"
        elif "self.assert" in sample_sources:
            conventions.assertion_style = "unittest"
        elif "expect(" in sample_sources:
            conventions.assertion_style = "expect"

        # Fixtures
        conventions.uses_fixtures = (
            "@pytest.fixture" in sample_sources
            or "def fixture" in sample_sources
            or "@fixture" in sample_sources
        )

        # Mock library
        if "unittest.mock" in sample_sources or "from mock" in sample_sources:
            conventions.mock_library = "unittest.mock"
        elif "pytest-mock" in sample_sources or "mocker" in sample_sources:
            conventions.mock_library = "pytest-mock"
        elif "jest.fn" in sample_sources or "jest.mock" in sample_sources:
            conventions.mock_library = "jest"

        # Docstring style
        if '"""' in sample_sources:
            if ":param" in sample_sources:
                conventions.docstring_style = "sphinx"
            elif "Parameters" in sample_sources:
                conventions.docstring_style = "numpy"
            else:
                conventions.docstring_style = "google"

        return conventions


def _trim_symbols(symbols: list[SymbolInfo], max_lines: int) -> list[SymbolInfo]:
    """Keep as many symbols as fit within the line budget."""
    result = []
    total = 0
    for sym in symbols:
        if total + sym.line_count > max_lines:
            break
        result.append(sym)
        total += sym.line_count
    return result
