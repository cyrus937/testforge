"""Pytest configuration and shared fixtures."""

from pathlib import Path

import pytest


def pytest_addoption(parser):
    """Add custom CLI options."""
    parser.addoption(
        "--run-slow",
        action="store_true",
        default=False,
        help="Run slow tests (e.g., model loading)",
    )


def pytest_configure(config):
    """Register custom markers."""
    config.addinivalue_line("markers", "slow: marks tests as slow (requires --run-slow)")


def pytest_collection_modifyitems(config, items):
    """Skip slow tests unless --run-slow is passed."""
    if config.getoption("--run-slow"):
        return
    skip_slow = pytest.mark.skip(reason="Need --run-slow option to run")
    for item in items:
        if "slow" in item.keywords:
            item.add_marker(skip_slow)


@pytest.fixture
def fixtures_dir() -> Path:
    """Path to the test fixtures directory."""
    return Path(__file__).parent / "fixtures"


@pytest.fixture
def flask_app_dir(fixtures_dir: Path) -> Path:
    """Path to the sample Flask app fixture."""
    return fixtures_dir / "python-flask-app"
