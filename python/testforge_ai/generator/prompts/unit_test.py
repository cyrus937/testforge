"""
Prompt template for unit test generation.

Constructs a detailed prompt that includes the target function,
its dependencies, sibling context, and any existing tests — giving
the LLM enough information to produce contextually-aware tests.
"""

from __future__ import annotations

from testforge_ai.bridge import SymbolInfo


def build_unit_test_prompt(
    target: SymbolInfo,
    context: dict[str, list[SymbolInfo]],
    framework: str = "pytest",
    include_mocks: bool = True,
) -> str:
    """
    Build the system + user prompt for unit test generation.

    Parameters
    ----------
    target : SymbolInfo
        The function/method to generate tests for.
    context : dict
        Context dict with keys: dependencies, siblings, existing_tests.
    framework : str
        Target test framework (pytest, jest, cargo-test, etc.).
    include_mocks : bool
        Whether to include mock/stub generation instructions.
    """
    sections: list[str] = []

    # System preamble
    sections.append(_system_preamble(framework, target.language))

    # Target function
    sections.append(_target_section(target))

    # Dependencies
    deps = context.get("dependencies", [])
    if deps:
        sections.append(_dependencies_section(deps))

    # Siblings (other functions in the same file)
    siblings = context.get("siblings", [])
    if siblings:
        sections.append(_siblings_section(siblings))

    # Existing tests
    existing = context.get("existing_tests", [])
    if existing:
        sections.append(_existing_tests_section(existing))

    # Generation instructions
    sections.append(_generation_instructions(target, framework, include_mocks))

    return "\n\n".join(sections)


def _system_preamble(framework: str, language: str) -> str:
    return f"""You are an expert {language} developer specializing in writing \
thorough, maintainable tests using {framework}.

Your task is to generate comprehensive unit tests for the function provided below.
The tests should:
- Cover the happy path, edge cases, and error conditions
- Use descriptive test names that explain what is being tested
- Be self-contained and runnable without modification
- Follow {framework} conventions and best practices
- Use proper assertions with clear failure messages"""


def _target_section(target: SymbolInfo) -> str:
    parts = [f"## Target: `{target.qualified_name}`"]

    if target.signature:
        parts.append(f"**Signature:** `{target.signature}`")

    if target.docstring:
        parts.append(f"**Documentation:** {target.docstring}")

    parts.append(f"**Source ({target.file_path}, lines {target.start_line}-{target.end_line}):**")
    parts.append(f"```{target.language}\n{target.source}\n```")

    if target.dependencies:
        parts.append(f"**Calls:** {', '.join(f'`{d}`' for d in target.dependencies)}")

    return "\n\n".join(parts)


def _dependencies_section(deps: list[SymbolInfo]) -> str:
    parts = ["## Dependencies\n\nThese functions are called by the target:"]
    for dep in deps[:5]:  # Limit to avoid context overflow
        parts.append(f"### `{dep.qualified_name}`\n```{dep.language}\n{dep.source}\n```")
    return "\n\n".join(parts)


def _siblings_section(siblings: list[SymbolInfo]) -> str:
    parts = ["## Other functions in the same module\n\nFor context about coding style:"]
    for sib in siblings[:3]:
        sig = sib.signature or sib.name
        doc = f" — {sib.docstring}" if sib.docstring else ""
        parts.append(f"- `{sig}`{doc}")
    return "\n".join(parts)


def _existing_tests_section(tests: list[SymbolInfo]) -> str:
    parts = [
        "## Existing tests\n\nFollow the same style and conventions as these existing tests:"
    ]
    for test in tests[:3]:
        parts.append(f"```{test.language}\n{test.source}\n```")
    return "\n\n".join(parts)


def _generation_instructions(
    target: SymbolInfo,
    framework: str,
    include_mocks: bool,
) -> str:
    instructions = f"""## Instructions

Generate a complete, runnable test file for `{target.qualified_name}` using {framework}.

Requirements:
1. Include all necessary imports
2. Test the happy path with typical inputs
3. Test boundary conditions and edge cases
4. Test error handling (invalid inputs, exceptions)
5. Use descriptive test function names: `test_<what>_<condition>_<expected_result>`
6. Add brief docstrings to each test explaining the scenario"""

    if include_mocks:
        instructions += """
7. Mock external dependencies (database calls, API requests, file I/O)
8. Use the project's existing mock patterns where detected"""

    instructions += """

Output ONLY the test code inside a single fenced code block.
Do not include explanations outside the code block."""

    return instructions
