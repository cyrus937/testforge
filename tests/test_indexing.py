"""
Integration tests for the indexing pipeline.

These tests use the sample fixture projects to verify that the full
indexing pipeline — file walking, parsing, symbol extraction, and
storage — works end-to-end.
"""

from __future__ import annotations

import subprocess
import shutil
from pathlib import Path

import pytest

FIXTURES_DIR = Path(__file__).parent / "fixtures"
FLASK_APP = FIXTURES_DIR / "python-flask-app"


def testforge_cli(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess:
    """Run the testforge CLI and return the result."""
    cli = shutil.which("testforge")
    if cli is None:
        pytest.skip("testforge CLI not found in PATH")
    return subprocess.run(
        [cli, *args],
        capture_output=True,
        text=True,
        cwd=str(cwd or FLASK_APP),
        timeout=30,
    )


class TestIndexingPipeline:
    """End-to-end tests for the indexing pipeline."""

    @pytest.fixture(autouse=True)
    def setup_project(self, tmp_path: Path):
        """Copy fixture to a temp directory and initialize testforge."""
        self.project = tmp_path / "project"
        shutil.copytree(FLASK_APP, self.project)

        result = testforge_cli("init", cwd=self.project)
        assert result.returncode == 0, f"init failed: {result.stderr}"
        yield
        # Cleanup happens automatically via tmp_path

    def test_index_creates_database(self):
        """Indexing should create the SQLite database."""
        result = testforge_cli("index", ".", cwd=self.project)
        assert result.returncode == 0, f"index failed: {result.stderr}"

        db_path = self.project / ".testforge" / "index" / "testforge.db"
        assert db_path.exists(), "Database file should be created"

    def test_index_finds_python_symbols(self):
        """Indexing the Flask fixture should find all classes and functions."""
        testforge_cli("index", ".", cwd=self.project)

        result = testforge_cli("search", "User", "--format", "json", cwd=self.project)
        assert result.returncode == 0

        import json
        symbols = json.loads(result.stdout)
        names = [s["name"] for s in symbols]

        assert "User" in names, "Should find the User dataclass"
        assert "UserService" in names, "Should find the UserService class"

    def test_index_extracts_methods(self):
        """Methods inside classes should be extracted with correct parent."""
        testforge_cli("index", ".", cwd=self.project)

        result = testforge_cli("search", "create_user", "--format", "json", cwd=self.project)
        assert result.returncode == 0

        import json
        symbols = json.loads(result.stdout)
        create_user = next((s for s in symbols if s["name"] == "create_user"), None)

        assert create_user is not None, "Should find create_user method"
        assert create_user.get("parent") == "UserService"
        assert create_user.get("kind") in ("method",)

    def test_index_captures_docstrings(self):
        """Docstrings should be extracted from functions."""
        testforge_cli("index", ".", cwd=self.project)

        result = testforge_cli("search", "get_user", "--format", "json", cwd=self.project)
        assert result.returncode == 0

        import json
        symbols = json.loads(result.stdout)
        get_user = next((s for s in symbols if s["name"] == "get_user"), None)

        assert get_user is not None
        assert get_user.get("docstring") is not None
        assert "Retrieve" in get_user["docstring"]

    def test_incremental_reindex_skips_unchanged(self):
        """Running index twice should skip unchanged files."""
        testforge_cli("index", ".", cwd=self.project)
        result = testforge_cli("index", ".", cwd=self.project)
        assert result.returncode == 0
        # The output should mention skipped files
        assert "skipped" in result.stdout.lower() or "unchanged" in result.stdout.lower()

    def test_status_shows_counts(self):
        """Status should report correct file and symbol counts."""
        testforge_cli("index", ".", cwd=self.project)

        result = testforge_cli("status", cwd=self.project)
        assert result.returncode == 0
        assert "Files indexed" in result.stdout
        assert "Symbols" in result.stdout


class TestSearchFeatures:
    """Tests for the search command."""

    @pytest.fixture(autouse=True)
    def setup_indexed_project(self, tmp_path: Path):
        self.project = tmp_path / "project"
        shutil.copytree(FLASK_APP, self.project)
        testforge_cli("init", cwd=self.project)
        testforge_cli("index", ".", cwd=self.project)

    def test_search_by_function_name(self):
        result = testforge_cli("search", "authenticate", cwd=self.project)
        assert result.returncode == 0
        assert "authenticate" in result.stdout.lower()

    def test_search_no_results(self):
        result = testforge_cli("search", "xyznonexistent", cwd=self.project)
        assert result.returncode == 0
        assert "no results" in result.stdout.lower() or "0" in result.stdout

    def test_search_json_format(self):
        result = testforge_cli("search", "user", "--format", "json", cwd=self.project)
        assert result.returncode == 0

        import json
        data = json.loads(result.stdout)
        assert isinstance(data, list)

    def test_search_with_limit(self):
        result = testforge_cli(
            "search", "user", "--limit", "2", "--format", "json", cwd=self.project
        )
        assert result.returncode == 0

        import json
        data = json.loads(result.stdout)
        assert len(data) <= 2
