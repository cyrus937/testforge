"""
Language-specific prompt adaptations for Go test generation.

Go has its own testing conventions that differ significantly from
Python/JS. This module provides Go-specific prompt sections.
"""

from __future__ import annotations

from testforge_ai.bridge import SymbolInfo


def build_go_test_prompt(target: SymbolInfo, context: dict[str, list[SymbolInfo]]) -> str:
    """
    Build Go-specific testing instructions to append to the main prompt.

    Go conventions:
    - Tests are in the same package with `_test.go` suffix
    - Table-driven tests are idiomatic
    - `t.Helper()` for test utilities
    - `t.Parallel()` for concurrent tests
    - Subtests via `t.Run("name", func(t *testing.T) { ... })`
    """
    sections: list[str] = []

    sections.append("## Go Testing Conventions")
    sections.append(
        "Follow these Go-specific conventions when generating tests:"
    )

    # Table-driven tests
    sections.append(
        "\n### Table-Driven Tests\n"
        "Use table-driven tests for functions with multiple input scenarios:\n"
        "```go\n"
        "func TestFunctionName(t *testing.T) {\n"
        "    tests := []struct {\n"
        '        name     string\n'
        "        input    InputType\n"
        "        expected OutputType\n"
        "        wantErr  bool\n"
        "    }{\n"
        '        {"valid input", validInput, expectedOutput, false},\n'
        '        {"empty input", emptyInput, zeroValue, true},\n'
        "    }\n"
        "\n"
        "    for _, tt := range tests {\n"
        "        t.Run(tt.name, func(t *testing.T) {\n"
        "            got, err := FunctionName(tt.input)\n"
        "            if (err != nil) != tt.wantErr {\n"
        '                t.Errorf("FunctionName() error = %v, wantErr %v", err, tt.wantErr)\n'
        "            }\n"
        "            if got != tt.expected {\n"
        '                t.Errorf("FunctionName() = %v, want %v", got, tt.expected)\n'
        "            }\n"
        "        })\n"
        "    }\n"
        "}\n"
        "```"
    )

    # Interface mocking
    deps = context.get("dependencies", [])
    has_interface_deps = any(
        d.kind == "interface" or "interface" in d.source.lower()
        for d in deps
    )

    if has_interface_deps:
        sections.append(
            "\n### Interface Mocking\n"
            "Create mock implementations of interface dependencies:\n"
            "```go\n"
            "type mockStore struct {\n"
            "    saveFn   func(item *Item) error\n"
            "    findFn   func(id int) (*Item, error)\n"
            "}\n"
            "\n"
            "func (m *mockStore) Save(item *Item) error {\n"
            "    return m.saveFn(item)\n"
            "}\n"
            "```\n"
            "This gives full control over return values and error simulation."
        )

    # Error wrapping
    if "fmt.Errorf" in target.source or "%w" in target.source:
        sections.append(
            "\n### Error Wrapping\n"
            "Test that errors are properly wrapped with `errors.Is` and `errors.As`:\n"
            "```go\n"
            "if !errors.Is(err, expectedErr) {\n"
            '    t.Errorf("expected %v, got %v", expectedErr, err)\n'
            "}\n"
            "```"
        )

    # Exported vs unexported
    if target.visibility == "private":
        sections.append(
            "\n### Note: Unexported Function\n"
            f"`{target.name}` is unexported. Place the test file in the same "
            "package (not `_test` package) to access it directly."
        )

    return "\n".join(sections)


def build_java_test_prompt(target: SymbolInfo, context: dict[str, list[SymbolInfo]]) -> str:
    """
    Build Java/JUnit-specific testing instructions.

    JUnit conventions:
    - `@Test` annotation on each test method
    - `@BeforeEach` / `@AfterEach` for setup/teardown
    - `@DisplayName` for readable test names
    - AssertJ or Hamcrest for fluent assertions
    - Mockito for mocking
    """
    sections: list[str] = []

    sections.append("## JUnit Testing Conventions")
    sections.append(
        "Follow these Java/JUnit conventions:"
    )

    sections.append(
        "\n### Test Structure\n"
        "```java\n"
        "@ExtendWith(MockitoExtension.class)\n"
        f"class {target.parent or 'Target'}Test {{\n"
        "\n"
        "    @Mock\n"
        "    private DependencyType dependency;\n"
        "\n"
        "    @InjectMocks\n"
        f"    private {target.parent or 'Target'} underTest;\n"
        "\n"
        "    @Test\n"
        f'    @DisplayName("should do X when given Y")\n'
        f"    void {target.name}_givenValidInput_returnsExpected() {{\n"
        "        // Arrange\n"
        "        when(dependency.method()).thenReturn(value);\n"
        "\n"
        "        // Act\n"
        f"        var result = underTest.{target.name}(input);\n"
        "\n"
        "        // Assert\n"
        "        assertThat(result).isEqualTo(expected);\n"
        "        verify(dependency).method();\n"
        "    }}\n"
        "}}\n"
        "```"
    )

    # Exception testing
    if "throw" in target.source.lower():
        sections.append(
            "\n### Exception Testing\n"
            "```java\n"
            "@Test\n"
            f"void {target.name}_givenInvalidInput_throwsException() {{\n"
            f"    assertThrows(IllegalArgumentException.class, () -> {{\n"
            f"        underTest.{target.name}(invalidInput);\n"
            "    });\n"
            "}\n"
            "```"
        )

    return "\n".join(sections)
