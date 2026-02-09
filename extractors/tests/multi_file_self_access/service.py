"""Service that uses ports via self attribute access."""

from ports import StoragePort, LoggerPort


class DataService:
    """Service demonstrating self.port.method() patterns."""

    def __init__(self, storage: StoragePort, logger: LoggerPort):
        """Initialize with injected ports."""
        self.storage = storage
        self.logger = logger
        self._counter = 0

    def process(self, key: str, data: str) -> bool:
        """Process data - calls self.storage.save() and self.logger.info()."""
        self.logger.info(f"Processing {key}")
        result = self.storage.save(key, data)
        self._counter += 1
        return result

    def retrieve(self, key: str) -> str:
        """Retrieve data - calls self.storage.load()."""
        self.logger.info(f"Retrieving {key}")
        return self.storage.load(key)

    def get_count(self) -> int:
        """Get counter - reads self._counter."""
        return self._counter
