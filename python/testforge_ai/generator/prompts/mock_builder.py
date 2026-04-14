"""
Prompt template for mock/stub generation.

Analyzes a function's dependencies and produces instructions for
the LLM to generate appropriate mocks, stubs, and fakes.
"""

from __future__ import annotations

from testforge_ai.bridge import SymbolInfo


def build_mock_prompt(
    target: SymbolInfo,
    dependencies: list[SymbolInfo],
) -> str:
    """
    Build a supplementary prompt for mock generation.

    Analyzes each dependency to determine the best mocking strategy:
    - Simple functions → `MagicMock` / `jest.fn()`
    - Database calls → in-memory fake or fixture
    - HTTP clients → response stubs
    - File I/O → `tmp_path` / `StringIO`

    Parameters
    ----------
    target : SymbolInfo
        The function being tested.
    dependencies : list[SymbolInfo]
        Resolved dependency symbols.
    """
    if not dependencies:
        return ""

    sections: list[str] = []
    sections.append("## Mocking Strategy")
    sections.append(
        "Generate mocks for these external dependencies. "
        "Choose the simplest approach that allows isolated testing:"
    )

    for dep in dependencies:
        strategy = _suggest_strategy(dep)
        sections.append(
            f"\n### `{dep.qualified_name}` ({dep.kind})\n"
            f"**Strategy:** {strategy['approach']}\n"
            f"**Reason:** {strategy['reason']}"
        )
        if dep.signature:
            sections.append(f"**Signature:** `{dep.signature}`")
        if strategy.get("example"):
            sections.append(
                f"**Example:**\n```{target.language}\n{strategy['example']}\n```"
            )

    # General mock guidelines
    sections.append(
        "\n## Mock Guidelines\n"
        "- Mock at the boundary (database, network, filesystem), not internal logic\n"
        "- Verify mock calls with `assert_called_with` / `toHaveBeenCalledWith`\n"
        "- Use `side_effect` for simulating errors and exceptions\n"
        "- Prefer dependency injection over patching when possible\n"
        "- Name mocks descriptively: `mock_user_repository`, not `mock1`"
    )

    return "\n".join(sections)


def _suggest_strategy(dep: SymbolInfo) -> dict:
    """Suggest the best mocking strategy for a dependency."""
    source_lower = dep.source.lower()
    name_lower = dep.name.lower()

    # Database operations
    if any(
        kw in source_lower
        for kw in ["execute", "query", "cursor", "commit", "fetchone", "fetchall"]
    ):
        return {
            "approach": "In-memory fake or MagicMock with return_value",
            "reason": "Database operation — isolate from real DB",
            "example": _db_mock_example(dep),
        }

    # HTTP / API calls
    if any(
        kw in source_lower
        for kw in ["request", "fetch", "http", "response", "url", "api"]
    ):
        return {
            "approach": "Response stub with `unittest.mock.patch` or `responses` library",
            "reason": "Network call — avoid external dependency in tests",
            "example": _http_mock_example(dep),
        }

    # File I/O
    if any(kw in source_lower for kw in ["open(", "read(", "write(", "path", "file"]):
        return {
            "approach": "Use `tmp_path` fixture or `StringIO`",
            "reason": "File operation — use temporary files for isolation",
        }

    # Caching
    if any(kw in name_lower for kw in ["cache", "redis", "memcache"]):
        return {
            "approach": "Simple dict-based fake cache",
            "reason": "Cache dependency — use in-memory replacement",
        }

    # Logging / metrics
    if any(kw in name_lower for kw in ["log", "metric", "track", "emit"]):
        return {
            "approach": "MagicMock (fire and forget)",
            "reason": "Side-effect only — just verify it was called",
        }

    # Default: simple mock
    return {
        "approach": "MagicMock with configured return_value",
        "reason": "Standard dependency — mock return value for isolation",
    }


def _db_mock_example(dep: SymbolInfo) -> str:
    """Generate a database mock example."""
    if dep.language == "python":
        return (
            "mock_db = MagicMock()\n"
            "mock_db.execute.return_value.fetchone.return_value = (\n"
            '    1, "test_user", "test@example.com", True\n'
            ")\n"
            "mock_db.execute.return_value.fetchall.return_value = []"
        )
    return ""


def _http_mock_example(dep: SymbolInfo) -> str:
    """Generate an HTTP mock example."""
    if dep.language == "python":
        return (
            '@patch("module.requests.get")\n'
            "def test_with_mock_api(self, mock_get):\n"
            "    mock_get.return_value.status_code = 200\n"
            '    mock_get.return_value.json.return_value = {"key": "value"}'
        )
    return ""
