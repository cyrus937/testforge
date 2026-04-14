"""
Code complexity analysis for prioritizing test generation.

Computes cyclomatic complexity and other metrics to help identify
which functions need the most thorough testing.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Optional

from testforge_ai.bridge import SymbolInfo


@dataclass
class ComplexityReport:
    """Complexity metrics for a single symbol."""

    symbol_name: str
    cyclomatic_complexity: int
    line_count: int
    parameter_count: int
    has_error_handling: bool
    has_recursion: bool
    has_side_effects: bool
    risk_score: float  # 0.0 (trivial) to 1.0 (high risk)

    @property
    def risk_level(self) -> str:
        if self.risk_score >= 0.7:
            return "high"
        elif self.risk_score >= 0.4:
            return "medium"
        return "low"


def analyze_complexity(symbol: SymbolInfo) -> ComplexityReport:
    """
    Compute complexity metrics for a symbol.

    Uses heuristic analysis of the source code to estimate complexity
    without requiring a full AST (the AST is in Rust; here we do
    lightweight text analysis).

    Parameters
    ----------
    symbol : SymbolInfo
        The symbol to analyze.

    Returns
    -------
    ComplexityReport
        Computed metrics including a composite risk score.
    """
    source = symbol.source
    source_lower = source.lower()

    cc = _cyclomatic_complexity(source, symbol.language)
    param_count = _count_parameters(symbol.signature or "")
    has_error = _has_error_handling(source, symbol.language)
    has_recursion = _detect_recursion(symbol)
    has_side_effects = _detect_side_effects(source_lower)

    # Composite risk score
    risk = 0.0
    risk += min(cc / 15.0, 0.4)                    # max 0.4 from complexity
    risk += min(param_count / 8.0, 0.15)            # max 0.15 from parameters
    risk += 0.1 if has_error else 0.0               # error handling = more paths
    risk += 0.1 if has_recursion else 0.0           # recursion = edge cases
    risk += 0.15 if has_side_effects else 0.0       # side effects = harder to test
    risk += min(symbol.line_count / 100.0, 0.1)     # long functions = risky
    risk = min(risk, 1.0)

    return ComplexityReport(
        symbol_name=symbol.qualified_name,
        cyclomatic_complexity=cc,
        line_count=symbol.line_count,
        parameter_count=param_count,
        has_error_handling=has_error,
        has_recursion=has_recursion,
        has_side_effects=has_side_effects,
        risk_score=round(risk, 2),
    )


def prioritize_symbols(symbols: list[SymbolInfo]) -> list[tuple[SymbolInfo, ComplexityReport]]:
    """
    Rank symbols by testing priority (highest risk first).

    Useful for suggesting which functions to generate tests for first.
    """
    analyzed = [(sym, analyze_complexity(sym)) for sym in symbols]
    analyzed.sort(key=lambda x: x[1].risk_score, reverse=True)
    return analyzed


def _cyclomatic_complexity(source: str, language: str) -> int:
    """
    Estimate cyclomatic complexity by counting decision points.

    CC = 1 + (number of decision points)
    """
    cc = 1  # base path

    # Language-agnostic decision keywords
    decision_patterns = [
        r'\bif\b', r'\belif\b', r'\belse\s+if\b',
        r'\bfor\b', r'\bwhile\b',
        r'\band\b', r'\bor\b',
        r'\b\?\b',           # ternary
        r'\bcatch\b', r'\bexcept\b',
        r'\bcase\b',
    ]

    # Language-specific additions
    if language in ("rust",):
        decision_patterns.extend([r'\bmatch\b', r'=>'])
    if language in ("python",):
        decision_patterns.extend([r'\bwith\b', r'\byield\b'])

    for pattern in decision_patterns:
        cc += len(re.findall(pattern, source))

    return cc


def _count_parameters(signature: str) -> int:
    """Count parameters in a function signature."""
    if not signature:
        return 0

    # Extract content between parentheses
    match = re.search(r'\(([^)]*)\)', signature)
    if not match:
        return 0

    params_str = match.group(1).strip()
    if not params_str:
        return 0

    # Split by commas, excluding 'self' and 'cls'
    params = [
        p.strip() for p in params_str.split(",")
        if p.strip() and p.strip() not in ("self", "cls", "&self", "&mut self")
    ]

    return len(params)


def _has_error_handling(source: str, language: str) -> bool:
    """Detect presence of error handling constructs."""
    patterns = {
        "python": [r'\btry\b', r'\braise\b'],
        "javascript": [r'\btry\b', r'\bthrow\b'],
        "typescript": [r'\btry\b', r'\bthrow\b'],
        "rust": [r'\b\?\b', r'\.unwrap\(', r'Result<', r'\.expect\('],
        "java": [r'\btry\b', r'\bthrow\b'],
    }

    lang_patterns = patterns.get(language, [r'\btry\b', r'\braise\b', r'\bthrow\b'])
    return any(re.search(p, source) for p in lang_patterns)


def _detect_recursion(symbol: SymbolInfo) -> bool:
    """Check if a function calls itself."""
    # Remove the definition line to avoid false positives
    body = symbol.source
    lines = body.splitlines()
    if lines:
        body = "\n".join(lines[1:])

    return symbol.name in body


def _detect_side_effects(source_lower: str) -> bool:
    """Detect likely side effects (I/O, mutation, network)."""
    side_effect_markers = [
        "open(", "write(", "print(", "logging.",
        "request", "fetch(", "http",
        "insert", "update", "delete", "commit",
        "send(", "post(", "put(",
        "os.remove", "os.rename", "shutil.",
        "global ", "nonlocal ",
    ]
    return any(marker in source_lower for marker in side_effect_markers)
