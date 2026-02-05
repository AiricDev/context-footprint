"""Simple Python module for testing semantic extraction."""

from typing import Protocol
from functools import wraps


def log_call(func):
    """Decorator that logs function calls."""
    @wraps(func)
    def wrapper(*args, **kwargs):
        print(f"Calling {func.__name__}")
        return func(*args, **kwargs)
    return wrapper


def retry(max_attempts: int = 3):
    """Decorator factory that retries function calls."""
    def decorator(func):
        def wrapper(*args, **kwargs):
            for i in range(max_attempts):
                try:
                    return func(*args, **kwargs)
                except Exception:
                    if i == max_attempts - 1:
                        raise
            return None
        return wrapper
    return decorator


def singleton(cls):
    """Class decorator that ensures only one instance exists."""
    instances = {}
    def get_instance(*args, **kwargs):
        if cls not in instances:
            instances[cls] = cls(*args, **kwargs)
        return instances[cls]
    return get_instance


class Reader(Protocol):
    """Abstract reader interface."""
    
    def read(self, path: str) -> str:
        """Read content from path."""
        ...


class FileReader(Reader):
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


@singleton
class ServiceManager:
    """Manages services - uses class decorator."""
    
    def __init__(self):
        self.services = {}
    
    def register(self, name: str, service):
        self.services[name] = service


@log_call
@retry(max_attempts=3)
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


# Test for scope-aware type resolution
class Config:
    """Global Config class."""
    value: str = "global"


def get_config() -> Config:
    """Return global Config instance.
    
    Returns:
        Config instance with global scope
    """
    return Config()


def use_aliased_type(data: Reader) -> Reader:
    """Function using aliased type.
    
    Args:
        data: A Reader instance (aliased as DataSource via import)
        
    Returns:
        Same Reader instance
    """
    return data
