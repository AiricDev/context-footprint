"""Port interfaces for dependency injection."""

from typing import Protocol


class StoragePort(Protocol):
    """Abstract storage interface."""

    def save(self, key: str, data: str) -> bool:
        """Save data with given key."""
        ...

    def load(self, key: str) -> str:
        """Load data by key."""
        ...


class LoggerPort(Protocol):
    """Abstract logger interface."""

    def info(self, message: str) -> None:
        """Log info message."""
        ...
