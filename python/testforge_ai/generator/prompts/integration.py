"""
Prompt template for integration test suggestions.

When a function has multiple interacting dependencies, this prompt
guides the LLM to generate integration-level tests that verify
the collaboration between components.
"""

from __future__ import annotations

from testforge_ai.bridge import SymbolInfo


def build_integration_prompt(
    target: SymbolInfo,
    dependencies: list[SymbolInfo],
) -> str:
    """
    Build a supplementary prompt for integration test scenarios.

    This is added when the target has 3+ dependencies, suggesting
    the LLM also generates a few integration tests alongside unit tests.

    Parameters
    ----------
    target : SymbolInfo
        The function being tested.
    dependencies : list[SymbolInfo]
        Resolved dependency symbols (the collaboration partners).
    """
    if len(dependencies) < 2:
        return ""

    dep_names = [d.qualified_name for d in dependencies[:6]]
    dep_list = ", ".join(f"`{n}`" for n in dep_names)

    sections: list[str] = [
        "## Integration Test Scenarios",
        "",
        f"`{target.qualified_name}` collaborates with {dep_list}.",
        "In addition to the isolated unit tests above, generate **2-3 integration tests**",
        "that verify the collaboration between these components:",
        "",
    ]

    # Suggest specific integration scenarios
    scenarios = _suggest_scenarios(target, dependencies)
    for i, scenario in enumerate(scenarios, 1):
        sections.append(f"{i}. **{scenario['name']}** — {scenario['description']}")

    sections.extend(
        [
            "",
            "### Integration test guidelines:",
            "- Use real implementations where possible, mock only external boundaries",
            "- Test the data flow through the full call chain",
            "- Verify side effects (database writes, cache updates, event emission)",
            "- Name integration tests with `test_integration_` prefix",
            "- These can be longer than unit tests — clarity over brevity",
        ]
    )

    return "\n".join(sections)


def _suggest_scenarios(
    target: SymbolInfo,
    dependencies: list[SymbolInfo],
) -> list[dict[str, str]]:
    """Generate integration test scenario suggestions."""
    scenarios: list[dict[str, str]] = []
    source_lower = target.source.lower()

    # Happy path through all dependencies
    scenarios.append(
        {
            "name": "Full happy path",
            "description": (
                f"Call `{target.name}` with valid input and verify that all "
                f"dependencies are invoked in the correct order with correct data."
            ),
        }
    )

    # Error propagation
    has_error_handling = any(
        kw in source_lower
        for kw in ["try", "except", "catch", "error", "raise", "throw"]
    )
    if has_error_handling:
        scenarios.append(
            {
                "name": "Error propagation",
                "description": (
                    "Simulate a failure in one dependency and verify the error "
                    "propagates correctly (wrapped, logged, re-raised, or handled)."
                ),
            }
        )

    # State consistency
    has_mutation = any(
        kw in source_lower
        for kw in ["save", "update", "delete", "insert", "commit", "write", "set("]
    )
    if has_mutation:
        scenarios.append(
            {
                "name": "State consistency on partial failure",
                "description": (
                    "Start a multi-step operation, fail midway, and verify "
                    "the system state is consistent (rollback or partial success)."
                ),
            }
        )

    # Concurrent access
    has_shared_state = any(
        kw in source_lower for kw in ["cache", "lock", "mutex", "atomic", "global"]
    )
    if has_shared_state:
        scenarios.append(
            {
                "name": "Concurrent access",
                "description": (
                    "Verify behavior when multiple callers invoke the function "
                    "simultaneously (thread safety, cache consistency)."
                ),
            }
        )

    # Data transformation chain
    if len(dependencies) >= 3:
        dep_chain = " → ".join(f"`{d.name}`" for d in dependencies[:4])
        scenarios.append(
            {
                "name": "Data transformation pipeline",
                "description": (
                    f"Verify the data flows correctly through the chain: {dep_chain}. "
                    "Check that intermediate transformations produce expected shapes."
                ),
            }
        )

    return scenarios[:4]  # Cap at 4 scenarios
