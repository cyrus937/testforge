"""
Sample Flask application for testing TestForge indexing and test generation.
This fixture simulates a small but realistic Python codebase.
"""

from dataclasses import dataclass
from typing import Optional


@dataclass
class User:
    """Represents an application user."""
    id: int
    username: str
    email: str
    is_active: bool = True


class UserService:
    """Service layer for user management operations."""

    def __init__(self, db_connection):
        """Initialize with a database connection."""
        self.db = db_connection
        self._cache: dict[int, User] = {}

    def get_user(self, user_id: int) -> Optional[User]:
        """
        Retrieve a user by ID.

        Checks the in-memory cache first, then falls back to the database.
        Returns None if the user is not found.
        """
        if user_id in self._cache:
            return self._cache[user_id]

        row = self.db.execute(
            "SELECT id, username, email, is_active FROM users WHERE id = ?",
            (user_id,)
        ).fetchone()

        if row is None:
            return None

        user = User(id=row[0], username=row[1], email=row[2], is_active=bool(row[3]))
        self._cache[user_id] = user
        return user

    def create_user(self, username: str, email: str) -> User:
        """
        Create a new user.

        Validates the email format and checks for duplicate usernames
        before inserting into the database.

        Raises:
            ValueError: If the email format is invalid.
            RuntimeError: If the username already exists.
        """
        if not self._validate_email(email):
            raise ValueError(f"Invalid email format: {email}")

        existing = self.find_by_username(username)
        if existing is not None:
            raise RuntimeError(f"Username '{username}' already taken")

        cursor = self.db.execute(
            "INSERT INTO users (username, email, is_active) VALUES (?, ?, ?)",
            (username, email, True)
        )
        self.db.commit()

        user = User(id=cursor.lastrowid, username=username, email=email)
        self._cache[user.id] = user
        return user

    def deactivate_user(self, user_id: int) -> bool:
        """Deactivate a user account. Returns True if the user was found."""
        user = self.get_user(user_id)
        if user is None:
            return False

        self.db.execute(
            "UPDATE users SET is_active = 0 WHERE id = ?",
            (user_id,)
        )
        self.db.commit()

        user.is_active = False
        self._cache[user_id] = user
        return True

    def find_by_username(self, username: str) -> Optional[User]:
        """Find a user by their username."""
        row = self.db.execute(
            "SELECT id, username, email, is_active FROM users WHERE username = ?",
            (username,)
        ).fetchone()

        if row is None:
            return None

        return User(id=row[0], username=row[1], email=row[2], is_active=bool(row[3]))

    def list_active_users(self, limit: int = 100) -> list[User]:
        """List all active users, up to a maximum limit."""
        rows = self.db.execute(
            "SELECT id, username, email, is_active FROM users WHERE is_active = 1 LIMIT ?",
            (limit,)
        ).fetchall()

        return [
            User(id=r[0], username=r[1], email=r[2], is_active=True)
            for r in rows
        ]

    @staticmethod
    def _validate_email(email: str) -> bool:
        """Basic email format validation."""
        if not email or "@" not in email:
            return False
        local, domain = email.rsplit("@", 1)
        return len(local) > 0 and "." in domain and len(domain) > 2


def compute_user_stats(users: list[User]) -> dict:
    """
    Compute aggregate statistics for a list of users.

    Returns a dict with keys: total, active, inactive, active_rate.
    """
    total = len(users)
    if total == 0:
        return {"total": 0, "active": 0, "inactive": 0, "active_rate": 0.0}

    active = sum(1 for u in users if u.is_active)
    inactive = total - active

    return {
        "total": total,
        "active": active,
        "inactive": inactive,
        "active_rate": round(active / total, 2),
    }
