/** Resolved path to Pyright LSP entry. */
export const pyrightLangServer = require.resolve("pyright/langserver.index.js");

/** Concurrency limit for LSP requests (hover, definition, etc.). */
export const LSP_CONCURRENCY = 10;

/** Concurrency limit for file reads. */
export const FILE_READ_CONCURRENCY = 50;

/** Python builtin type names (no resolution to symbol ID). */
export const PYTHON_BUILTIN_TYPES = new Set([
  "int", "float", "complex", "bool", "str", "bytes", "bytearray",
  "list", "tuple", "set", "frozenset", "dict", "None", "NoneType",
  "object", "type", "range", "slice", "memoryview", "property",
  "callable", "iterable", "iterator", "generator", "coroutine",
  "any", "union", "optional", "literal", "final", "typeddict",
  "self", "cls", "exception", "BaseException"
]);
