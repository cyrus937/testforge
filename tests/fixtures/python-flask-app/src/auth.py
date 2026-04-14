"""
Authentication utilities for the sample Flask app.
"""

import hashlib
import secrets
import time
from dataclasses import dataclass
from typing import Optional


@dataclass
class AuthToken:
    """An authentication token with expiry."""
    token: str
    user_id: int
    expires_at: float
    scopes: list[str]

    @property
    def is_expired(self) -> bool:
        return time.time() > self.expires_at


class AuthService:
    """Handles authentication and token management."""

    TOKEN_TTL_SECONDS = 3600  # 1 hour

    def __init__(self, user_service, secret_key: str):
        self.user_service = user_service
        self.secret_key = secret_key
        self._tokens: dict[str, AuthToken] = {}

    def authenticate(self, username: str, password: str) -> Optional[AuthToken]:
        """
        Authenticate a user with username and password.

        Returns an AuthToken on success, None on failure.
        """
        user = self.user_service.find_by_username(username)
        if user is None:
            return None

        if not user.is_active:
            return None

        password_hash = self._hash_password(password)
        # In a real app, compare against stored hash
        if not self._verify_password(password_hash, user.id):
            return None

        token = self._create_token(user.id, scopes=["read", "write"])
        return token

    def validate_token(self, token_str: str) -> Optional[AuthToken]:
        """
        Validate a token string and return the AuthToken if valid.

        Returns None if the token is unknown or expired.
        """
        token = self._tokens.get(token_str)
        if token is None:
            return None

        if token.is_expired:
            del self._tokens[token_str]
            return None

        return token

    def revoke_token(self, token_str: str) -> bool:
        """Revoke a token. Returns True if the token existed."""
        if token_str in self._tokens:
            del self._tokens[token_str]
            return True
        return False

    def refresh_token(self, token_str: str) -> Optional[AuthToken]:
        """
        Refresh an existing token, extending its expiry.

        The old token is revoked and a new one is issued.
        """
        old_token = self.validate_token(token_str)
        if old_token is None:
            return None

        self.revoke_token(token_str)
        return self._create_token(old_token.user_id, scopes=old_token.scopes)

    def _create_token(self, user_id: int, scopes: list[str]) -> AuthToken:
        """Generate a new auth token."""
        token_str = secrets.token_urlsafe(32)
        token = AuthToken(
            token=token_str,
            user_id=user_id,
            expires_at=time.time() + self.TOKEN_TTL_SECONDS,
            scopes=scopes,
        )
        self._tokens[token_str] = token
        return token

    def _hash_password(self, password: str) -> str:
        """Hash a password with the secret key."""
        salted = f"{self.secret_key}:{password}"
        return hashlib.sha256(salted.encode()).hexdigest()

    def _verify_password(self, password_hash: str, user_id: int) -> bool:
        """Verify a password hash against the stored hash."""
        # Stub: in a real app, query the database
        return True
