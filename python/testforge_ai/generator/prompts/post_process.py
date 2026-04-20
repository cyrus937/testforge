"""
Post-processing pipeline for LLM-generated tests.

Takes raw LLM output and produces clean, runnable test files by:
1. Extracting code from markdown fences
2. Validating syntax (AST parse)
3. Fixing common LLM mistakes (missing imports, bad indentation)
4. Adding file headers and metadata
5. Formatting with standard tools (ruff/black style)
"""

from __future__ import annotations

import ast
import logging
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import cast

from testforge_ai.bridge import SymbolInfo

logger = logging.getLogger(__name__)


@dataclass
class ProcessedTest:
    """Result of post-processing a generated test."""

    source: str
    test_count: int
    warnings: list[str] = field(default_factory=list)
    syntax_valid: bool = True
    fixed_issues: list[str] = field(default_factory=list)


class PostProcessor:
    """
    Post-processes LLM-generated test code into clean, runnable files.

    Parameters
    ----------
    language : str
        Target programming language.
    framework : str
        Test framework (pytest, jest, cargo-test, etc.).
    """

    def __init__(self, language: str = "python", framework: str = "pytest"):
        self.language = language
        self.framework = framework

    def process(self, raw_output: str, target: SymbolInfo) -> ProcessedTest:
        """
        Run the full post-processing pipeline.

        Parameters
        ----------
        raw_output : str
            Raw LLM output (may contain markdown, explanations, etc.).
        target : SymbolInfo
            The symbol the tests were generated for.

        Returns
        -------
        ProcessedTest
            Clean, formatted test source with metadata.
        """
        warnings: list[str] = []
        fixed: list[str] = []

        # Step 1: Extract code from markdown fences
        source = self._extract_code(raw_output)
        if source != raw_output:
            fixed.append("Extracted code from markdown fence")

        # Step 2: Language-specific processing
        if self.language == "python":
            source, py_warnings, py_fixed = self._process_python(source, target)
            warnings.extend(py_warnings)
            fixed.extend(py_fixed)
        elif self.language in ("javascript", "typescript"):
            source, js_warnings, js_fixed = self._process_javascript(source, target)
            warnings.extend(js_warnings)
            fixed.extend(js_fixed)
        elif self.language == "rust":
            source, rs_warnings, rs_fixed = self._process_rust(source, target)
            warnings.extend(rs_warnings)
            fixed.extend(rs_fixed)

        # Step 3: Add file header
        source = self._add_header(source, target)

        # Step 4: Count tests
        test_count = self._count_tests(source)

        # Step 5: Validate syntax
        syntax_valid = self._validate_syntax(source)
        if not syntax_valid:
            warnings.append(
                "Generated code has syntax errors — manual review recommended"
            )

        if test_count == 0:
            warnings.append("No test functions detected in output")

        result = ProcessedTest(
            source=source,
            test_count=test_count,
            warnings=warnings,
            syntax_valid=syntax_valid,
            fixed_issues=fixed,
        )

        logger.info(
            "Post-processed: %d tests, %d warnings, %d fixes, syntax=%s",
            test_count,
            len(warnings),
            len(fixed),
            "OK" if syntax_valid else "ERROR",
        )

        return result

    def output_filename(self, target: SymbolInfo) -> str:
        """Generate the output filename for the test file."""
        stem = Path(target.file_path).stem

        if self.language == "python":
            return f"test_{stem}.py"
        elif self.language in ("javascript", "typescript"):
            ext = "ts" if self.language == "typescript" else "js"
            return f"{stem}.test.{ext}"
        elif self.language == "rust":
            return f"{stem}_test.rs"
        elif self.language == "java":
            # CamelCase convention
            camel = stem[0].upper() + stem[1:] if stem else "Test"
            return f"{camel}Test.java"
        else:
            return f"test_{stem}.txt"

    # ── Code extraction ───────────────────────────────────────────

    def _extract_code(self, raw: str) -> str:
        """Extract code from markdown fenced code blocks."""
        # Try to find fenced code blocks
        pattern = r"```(?:\w+)?\s*\n(.*?)```"
        matches = cast(list[str], re.findall(pattern, raw, re.DOTALL))

        if matches:
            # Return the longest code block (likely the main test file)
            return max(matches, key=len).strip()

        # No code blocks found — check if the raw output looks like code
        lines = raw.strip().splitlines()
        if lines and (
            lines[0].startswith(
                ("import ", "from ", "def ", "class ", "#!", "//", "use ")
            )
        ):
            return raw.strip()

        # Last resort: return as-is
        return raw.strip()

    # ── Python processing ─────────────────────────────────────────

    def _process_python(
        self, source: str, target: SymbolInfo
    ) -> tuple[str, list[str], list[str]]:
        """Python-specific post-processing."""
        warnings: list[str] = []
        fixed: list[str] = []

        # Fix common import issues
        source, import_fixes = self._fix_python_imports(source, target)
        fixed.extend(import_fixes)

        # Fix indentation (LLMs sometimes mix tabs and spaces)
        if "\t" in source:
            source = source.replace("\t", "    ")
            fixed.append("Replaced tabs with spaces")

        # Ensure trailing newline
        if not source.endswith("\n"):
            source += "\n"

        # Check for placeholder TODOs
        todo_count = source.count("# TODO") + source.count("pass  #")
        if todo_count > 3:
            warnings.append(
                f"Found {todo_count} TODO/placeholder comments — tests may need completion"
            )

        # Check for `assert True` antipattern
        if "assert True" in source or "assert False" in source:
            warnings.append(
                "Found bare `assert True/False` — these aren't real assertions"
            )

        return source, warnings, fixed

    def _fix_python_imports(
        self, source: str, target: SymbolInfo
    ) -> tuple[str, list[str]]:
        """Add missing imports for common testing patterns."""
        fixes: list[str] = []
        lines = source.splitlines()

        # Collect existing imports
        existing_imports = {
            line.strip()
            for line in lines
            if line.strip().startswith(("import ", "from "))
        }

        needed_imports: list[str] = []

        # pytest import
        if (
            self.framework == "pytest"
            and "pytest" not in " ".join(existing_imports)
            and (
                "pytest." in source or "@pytest" in source or "pytest.raises" in source
            )
        ):
            needed_imports.append("import pytest")
            fixes.append("Added missing `import pytest`")

        # unittest.mock
        if (
            "MagicMock" in source or "patch" in source or "Mock(" in source
        ) and "mock" not in " ".join(existing_imports).lower():
            needed_imports.append("from unittest.mock import MagicMock, patch, Mock")
            fixes.append("Added missing mock imports")

        # Target module import
        module_path = target.file_path.replace("/", ".").replace("\\", ".")
        if module_path.endswith(".py"):
            module_path = module_path[:-3]

        # Check if the target is imported
        target_imported = any(
            target.name in imp or module_path in imp for imp in existing_imports
        )
        if not target_imported and target.name in source:
            import_line = f"from {module_path} import {target.name}"
            if target.parent:
                import_line = f"from {module_path} import {target.parent}"
            needed_imports.append(import_line)
            fixes.append(f"Added import for `{target.name}`")

        if needed_imports:
            import_block = "\n".join(needed_imports)
            # Insert after any existing imports or at the top
            source = import_block + "\n\n" + source

        return source, fixes

    # ── JavaScript processing ─────────────────────────────────────

    def _process_javascript(
        self, source: str, target: SymbolInfo
    ) -> tuple[str, list[str], list[str]]:
        """JavaScript/TypeScript-specific post-processing."""
        warnings: list[str] = []
        fixed: list[str] = []

        # Ensure describe/it blocks exist
        if "describe(" not in source and "test(" not in source and "it(" not in source:
            warnings.append("No test blocks (describe/test/it) found")

        # Fix missing jest imports for TypeScript
        if (
            self.language == "typescript"
            and "import" not in source
            and "expect(" in source
        ):
            source = (
                "import { describe, it, expect } from '@jest/globals';\n\n" + source
            )
            fixed.append("Added jest globals import for TypeScript")

        if not source.endswith("\n"):
            source += "\n"

        return source, warnings, fixed

    # ── Rust processing ───────────────────────────────────────────

    def _process_rust(
        self, source: str, target: SymbolInfo
    ) -> tuple[str, list[str], list[str]]:
        """Rust-specific post-processing."""
        warnings: list[str] = []
        fixed: list[str] = []

        # Ensure cfg(test) attribute
        if "#[cfg(test)]" not in source:
            source = (
                "#[cfg(test)]\nmod tests {\n    use super::*;\n\n" + source + "\n}\n"
            )
            fixed.append("Wrapped in #[cfg(test)] mod tests")

        # Check for assert macros
        if "assert" not in source:
            warnings.append("No assert macros found")

        if not source.endswith("\n"):
            source += "\n"

        return source, warnings, fixed

    # ── Header generation ─────────────────────────────────────────

    def _add_header(self, source: str, target: SymbolInfo) -> str:
        """Add a file header comment with metadata."""
        timestamp = __import__("datetime").datetime.now().strftime("%Y-%m-%d %H:%M")

        if self.language == "python":
            header = (
                f'"""\n'
                f"Auto-generated tests for {target.qualified_name}\n"
                f"Generated by TestForge on {timestamp}\n"
                f"Framework: {self.framework}\n"
                f'"""\n\n'
            )
        elif self.language in ("javascript", "typescript"):
            header = (
                f"/**\n"
                f" * Auto-generated tests for {target.qualified_name}\n"
                f" * Generated by TestForge on {timestamp}\n"
                f" * Framework: {self.framework}\n"
                f" */\n\n"
            )
        elif self.language == "rust":
            header = (
                f"// Auto-generated tests for {target.qualified_name}\n"
                f"// Generated by TestForge on {timestamp}\n"
                f"// Framework: {self.framework}\n\n"
            )
        else:
            header = f"// Tests for {target.qualified_name} — TestForge {timestamp}\n\n"

        return header + source

    # ── Validation ────────────────────────────────────────────────

    def _validate_syntax(self, source: str) -> bool:
        """Validate that the generated code is syntactically correct."""
        if self.language == "python":
            try:
                ast.parse(source)
                return True
            except SyntaxError as e:
                logger.warning("Syntax error at line %d: %s", e.lineno or 0, e.msg)
                return False

        # For other languages, basic heuristic checks
        if self.language in ("javascript", "typescript"):
            # Check balanced braces
            return source.count("{") == source.count("}")

        if self.language == "rust":
            return source.count("{") == source.count("}")

        return True

    # ── Counting ──────────────────────────────────────────────────

    def _count_tests(self, source: str) -> int:
        """Count the number of test functions in the source."""
        count = 0

        if self.language == "python":
            count = len(re.findall(r"^\s*def test_", source, re.MULTILINE))
            count += len(re.findall(r"^\s*async def test_", source, re.MULTILINE))

        elif self.language in ("javascript", "typescript"):
            count = len(re.findall(r"""(?:test|it)\s*\(""", source))

        elif self.language == "rust":
            count = len(re.findall(r"#\[test\]", source))

        elif self.language == "java":
            count = len(re.findall(r"@Test", source))

        return count
