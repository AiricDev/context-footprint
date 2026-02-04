"""Simple Python module for testing semantic extraction."""

from typing import Protocol


class Reader(Protocol):
    """Abstract reader interface."""
    
    def read(self, path: str) -> str:
        """Read content from path."""
        ...


class FileReader:
    """Concrete file reader implementation."""
    
    def __init__(self, encoding: str = "utf-8"):
        """Initialize file reader.
        
        Args:
            encoding: File encoding to use
        """
        self.encoding = encoding
        self._cache = {}
    
    def read(self, path: str) -> str:
        """Read file content.
        
        Args:
            path: File path to read
            
        Returns:
            File content as string
        """
        if path in self._cache:
            return self._cache[path]
        
        with open(path, 'r', encoding=self.encoding) as f:
            content = f.read()
            self._cache[path] = content
            return content


def process_file(reader: Reader, path: str) -> int:
    """Process a file using given reader.
    
    Args:
        reader: Reader instance (interface type)
        path: File path
        
    Returns:
        Number of lines processed
    """
    content = reader.read(path)
    lines = content.split('\n')
    return len(lines)


# Global configuration
MAX_SIZE = 1024 * 1024
_debug_mode = False
