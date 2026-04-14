"""
Prompt template for edge case detection and test generation.

Analyzes function signatures and bodies to identify potential
edge cases that should be covered by tests.
"""

from __future__ import annotations

from testforge_ai.bridge import SymbolInfo


def build_edge_case_prompt(target: SymbolInfo) -> str:
    """
    Build a supplementary prompt focused on edge case detection.

    This is appended to the main unit test prompt to encourage
    the LLM to think about boundary conditions.
    """
    edge_cases = _detect_potential_edge_cases(target)

    if not edge_cases:
        return ""

    lines = [
        "## Edge Cases to Consider",
        "",
        "Based on the function's signature and body, pay special attention to:",
    ]

    for case in edge_cases:
        lines.append(f"- {case}")

    return "\n".join(lines)


def _detect_potential_edge_cases(target: SymbolInfo) -> list[str]:
    """
    Static analysis of the function to detect likely edge cases.

    This is a heuristic analysis — it looks for patterns in the source
    code that commonly require edge case testing.
    """
    cases: list[str] = []
    source = target.source.lower()
    sig = (target.signature or "").lower()

    # String parameters
    if "str" in sig or "string" in sig:
        cases.append("Empty string input")
        cases.append("Very long string input")
        if "name" in sig or "email" in sig:
            cases.append("String with special characters (unicode, emoji)")

    # Numeric parameters
    if any(t in sig for t in ["int", "float", "number", "i32", "f64"]):
        cases.append("Zero value")
        cases.append("Negative values")
        cases.append("Very large numbers (overflow boundary)")
        if "float" in sig or "f64" in sig or "f32" in sig:
            cases.append("NaN and infinity values")

    # Collection parameters
    if any(t in sig for t in ["list", "vec", "array", "dict", "map", "set"]):
        cases.append("Empty collection")
        cases.append("Single-element collection")
        cases.append("Very large collection")
        if "dict" in sig or "map" in sig:
            cases.append("Missing keys")

    # Optional/nullable parameters
    if any(t in sig for t in ["optional", "none", "null", "option<"]):
        cases.append("None/null input for optional parameters")

    # Division or modulo in source
    if any(op in source for op in [" / ", " // ", " % ", ".div("]):
        cases.append("Division by zero")

    # File operations
    if any(op in source for op in ["open(", "read(", "write(", "path"]):
        cases.append("File not found / permission denied")
        cases.append("Empty file")

    # Network/API calls
    if any(op in source for op in ["request", "fetch", "http", "url", "api"]):
        cases.append("Network timeout / connection error")
        cases.append("Invalid response format")
        cases.append("HTTP error status codes (4xx, 5xx)")

    # Index access
    if "[" in source and "]" in source:
        cases.append("Index out of bounds")

    # Boolean logic
    if source.count("if ") > 2:
        cases.append("All branches of conditional logic")

    # Recursion
    source_lines = source.splitlines()
    body_text = "\n".join(source_lines[1:]) if len(source_lines) > 1 else ""
    if target.name + "(" in body_text:
        cases.append("Recursive base case")
        cases.append("Deep recursion (stack overflow)")

    # Async
    if "async" in source:
        cases.append("Concurrent execution / race conditions")
        cases.append("Cancelled or timed-out coroutines")

    return cases
