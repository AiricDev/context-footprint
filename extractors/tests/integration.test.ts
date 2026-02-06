/**
 * Integration tests for context-footprint-extractors
 * Tests the CLI tool against Python test cases in tests/ directory
 * 
 * NOTE: These tests use caching to avoid re-running LSP extraction for each test.
 * The extraction takes ~15-20 seconds, so we cache the result per test file.
 */

import { describe, it, beforeAll } from "bun:test";
import assert from "node:assert";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runExtractionWithCache, clearExtractionCache } from "./test-helper";
import type { SourceSpan } from "../src/core/types";
import type { 
  SemanticData, 
  DocumentSemantics,
  SymbolDefinition,
  FunctionSymbol,
  VariableSymbol,
  TypeSymbol,
  SymbolReference 
} from "../src/core/types";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const TEST_CASES_DIR = path.resolve(__dirname, ".");

// Type guards
function isFunctionSymbol(def: SymbolDefinition): def is FunctionSymbol {
  return def.kind === "Function";
}

function isVariableSymbol(def: SymbolDefinition): def is VariableSymbol {
  return def.kind === "Variable";
}

function isTypeSymbol(def: SymbolDefinition): def is TypeSymbol {
  return def.kind === "Type";
}

/** Extract text from source file at given span. Span uses 0-based line/col; end is exclusive. */
function extractSpanText(sourceLines: string[], span: SourceSpan): string {
  if (span.start_line === span.end_line) {
    return sourceLines[span.start_line]!.slice(span.start_column, span.end_column);
  }
  const parts: string[] = [];
  parts.push(sourceLines[span.start_line]!.slice(span.start_column));
  for (let i = span.start_line + 1; i < span.end_line; i++) {
    parts.push(sourceLines[i]!);
  }
  parts.push(sourceLines[span.end_line]!.slice(0, span.end_column));
  return parts.join("\n");
}

describe("Integration Tests for extract-semantics CLI", () => {
  const testProjectPath = path.join(TEST_CASES_DIR, "simple_python_for_extract");
  let cachedData: SemanticData;
  let mainDoc: DocumentSemantics;

  // Run extraction once before all tests and cache the result
  beforeAll(async () => {
    console.log(`\n[Integration Test] Starting extraction for ${testProjectPath}`);
    const startTime = Date.now();
    cachedData = await runExtractionWithCache(testProjectPath);
    mainDoc = cachedData.documents.find((d) => d.relative_path === "main.py")!;
    console.log(`[Integration Test] Setup completed in ${Date.now() - startTime}ms`);
  }, 30000); // 30 second timeout for setup

  describe("SemanticData schema compliance", () => {
    it("should have correct top-level SemanticData structure", () => {
      assert.strictEqual(typeof cachedData.project_root, "string");
      assert.ok(path.isAbsolute(cachedData.project_root), "project_root should be absolute path");
      assert.ok(Array.isArray(cachedData.documents), "documents should be an array");
      assert.ok(cachedData.documents.length > 0, "should have at least one document");
    });

    it("should have correct DocumentSemantics structure", () => {
      const doc = cachedData.documents[0];
      
      // relative_path: from project root, forward slashes
      assert.strictEqual(typeof doc.relative_path, "string");
      assert.ok(!doc.relative_path.startsWith("/"), "relative_path should not start with /");
      assert.ok(!doc.relative_path.includes("\\"), "relative_path should use forward slashes");
      
      // language: lowercase
      assert.strictEqual(doc.language, "python");
      
      // definitions and references arrays
      assert.ok(Array.isArray(doc.definitions));
      assert.ok(Array.isArray(doc.references));
    });
  });

  describe("Type definition validation (FileReader class)", () => {
    it("should extract FileReader class with correct hierarchical symbol_id", () => {
      const fileReader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "FileReader"
      );
      assert.ok(fileReader, "FileReader class should be extracted");
      // symbol_id should be: module.Class#Type
      assert.strictEqual(fileReader.symbol_id, "main.FileReader#Type");
      assert.strictEqual(fileReader.name, "FileReader");
      assert.strictEqual(fileReader.enclosing_symbol, null);
    });

    it("should have correct Type details for FileReader", () => {
      const fileReader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "FileReader"
      )!;

      const typeDetails = fileReader.details.Type;
      assert.strictEqual(typeDetails.kind, "Class");
      assert.strictEqual(typeDetails.is_abstract, false);
      assert.strictEqual(typeDetails.visibility, "Public");
    });

    it("should extract Reader Protocol as Interface kind", () => {
      const reader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "Reader"
      );
      assert.ok(reader, "Reader Protocol should be extracted");
      // Protocol should be mapped to Interface
      assert.strictEqual(reader.details.Type.kind, "Interface");
      assert.strictEqual(reader.details.Type.is_abstract, true);
    });

    it("should extract FileReader's implementation of Reader Protocol", () => {
      const fileReader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "FileReader"
      )!;

      // FileReader implements Reader (Protocol)
      const implementsList = fileReader.details.Type.implements;
      assert.ok(implementsList.length > 0, "FileReader should have implements array");
      assert.ok(
        implementsList.includes("main.Reader#Type"),
        "FileReader should implement Reader Protocol"
      );
    });
  });

  describe("Function/Method definition validation", () => {
    it("should extract module-level function has correct details", () => {
      const processFile = mainDoc.definitions.find(
        (d): d is FunctionSymbol => isFunctionSymbol(d) && d.name === "process_file"
      );

      assert.ok(processFile, "process_file function should be extracted");
      // Module-level functions should have null enclosing_symbol
      assert.strictEqual(processFile.enclosing_symbol, null,
        "Module-level function should have null enclosing_symbol");
      // symbol_id should be module.function#Function
      assert.strictEqual(processFile.symbol_id, "main.process_file#Function");

      const funcDetails = processFile.details.Function;
      assert.ok(Array.isArray(funcDetails.parameters));
      assert.ok(Array.isArray(funcDetails.return_types));
      assert.strictEqual(typeof funcDetails.modifiers.is_async, "boolean");
      assert.strictEqual(typeof funcDetails.modifiers.is_generator, "boolean");
      assert.strictEqual(typeof funcDetails.modifiers.is_static, "boolean");
      assert.strictEqual(typeof funcDetails.modifiers.is_abstract, "boolean");
      assert.strictEqual(funcDetails.modifiers.visibility, "Public");

      const params = funcDetails.parameters;
      // process_file(reader: Reader, path: str) -> int
      assert.strictEqual(params.length, 2, "process_file should have 2 parameters");
      
      // First parameter: reader - should use fully qualified symbol_id for user-defined types
      assert.strictEqual(params[0].name, "reader");
      assert.strictEqual(params[0].param_type, "main.Reader#Type", "reader parameter should have fully qualified type symbol_id");
      assert.strictEqual(params[0].has_default, false);
      assert.strictEqual(params[0].is_variadic, false);
      
      // Second parameter: path - builtin types can use simple names
      assert.strictEqual(params[1].name, "path");
      assert.strictEqual(params[1].param_type, "str", "path parameter should have type str");
      assert.strictEqual(params[1].has_default, false);
      assert.strictEqual(params[1].is_variadic, false);
    });

    it("should extract FileReader.__init__ with correct hierarchical symbol_id", () => {
      const fileReader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "FileReader"
      )!;

      const initMethod = mainDoc.definitions.find(
        (d): d is FunctionSymbol => 
          isFunctionSymbol(d) && 
          d.name === "__init__" &&
          d.enclosing_symbol === fileReader.symbol_id
      );

      assert.ok(initMethod, "FileReader.__init__ should be extracted with correct enclosing_symbol");
      // Method symbol_id should be: module.Class.method#Function
      assert.strictEqual(initMethod.symbol_id, "main.FileReader.__init__#Function");
      assert.strictEqual(initMethod.enclosing_symbol, "main.FileReader#Type");

      const params = initMethod.details.Function.parameters;
      // __init__(self, encoding: str = "utf-8")
      // self should be excluded, encoding should be included
      assert.strictEqual(params.length, 1, "__init__ should have 1 parameter (excluding self)");
      
      assert.strictEqual(params[0].name, "encoding");
      assert.strictEqual(params[0].param_type, "str", "encoding parameter should have type str (builtin)");
      assert.strictEqual(params[0].has_default, true, "encoding has default value");
      assert.strictEqual(params[0].is_variadic, false);
    });

    it("should distinguish Reader.read from FileReader.read with unique symbol_ids", () => {
      const readerRead = mainDoc.definitions.find(
        (d): d is FunctionSymbol => 
          isFunctionSymbol(d) && 
          d.symbol_id === "main.Reader.read#Function"
      );

      const fileReaderRead = mainDoc.definitions.find(
        (d): d is FunctionSymbol => 
          isFunctionSymbol(d) && 
          d.symbol_id === "main.FileReader.read#Function"
      );

      assert.ok(readerRead, "Reader.read should have symbol_id main.Reader.read#Function");
      assert.ok(fileReaderRead, "FileReader.read should have symbol_id main.FileReader.read#Function");
    });

    it("should extract Reader.read as abstract method", () => {
      const readerRead = mainDoc.definitions.find(
        (d): d is FunctionSymbol => 
          isFunctionSymbol(d) && 
          d.symbol_id === "main.Reader.read#Function"
      )!;

      assert.strictEqual(readerRead.details.Function.modifiers.is_abstract, true,
        "Protocol method should be marked as abstract");
    });

    it("should extract FileReader.read with correct return type", () => {
      const fileReaderRead = mainDoc.definitions.find(
        (d): d is FunctionSymbol => 
          isFunctionSymbol(d) && 
          d.symbol_id === "main.FileReader.read#Function"
      )!;

      const returnTypes = fileReaderRead.details.Function.return_types;
      assert.ok(returnTypes.includes("str"), "FileReader.read should return str");
    });

    it("should extract process_file with correct return type", () => {
      const processFile = mainDoc.definitions.find(
        (d): d is FunctionSymbol => isFunctionSymbol(d) && d.name === "process_file"
      )!;

      const returnTypes = processFile.details.Function.return_types;
      assert.ok(returnTypes.includes("int"), "process_file should return int");
    });
  });

  describe("Documentation extraction", () => {
    it("should extract docstring for FileReader class", () => {
      const fileReader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "FileReader"
      )!;

      assert.ok(fileReader.documentation.length > 0, "FileReader should have documentation");
      assert.ok(
        fileReader.documentation[0].includes("Concrete file reader implementation"),
        "FileReader docstring should contain expected text"
      );
    });

    it("should extract docstring for Reader Protocol", () => {
      const reader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "Reader"
      )!;

      assert.ok(reader.documentation.length > 0, "Reader should have documentation");
      assert.ok(
        reader.documentation[0].includes("Abstract reader interface"),
        "Reader docstring should contain expected text"
      );
    });

    it("should extract docstring for methods", () => {
      const processFile = mainDoc.definitions.find(
        (d): d is FunctionSymbol => isFunctionSymbol(d) && d.name === "process_file"
      )!;

      assert.ok(processFile.documentation.length > 0, "process_file should have documentation");
      assert.ok(
        processFile.documentation[0].includes("Process a file"),
        "process_file docstring should contain expected text"
      );
    });
  });

  describe("Variable/Field definition validation", () => {
    it("should extract module-level variables with null enclosing_symbol", () => {
      const maxSize = mainDoc.definitions.find(
        (d): d is VariableSymbol => isVariableSymbol(d) && d.name === "MAX_SIZE"
      );

      assert.ok(maxSize);
      assert.strictEqual(maxSize.enclosing_symbol, null,
        "Module-level variable should have null enclosing_symbol");
      assert.strictEqual(maxSize.details.Variable.scope, "Global");
    });

    it("should set correct visibility for private variables", () => {
      const debugMode = mainDoc.definitions.find(
        (d): d is VariableSymbol => isVariableSymbol(d) && d.name === "_debug_mode"
      );

      assert.ok(debugMode);
      // Leading underscore means Private in Python
      assert.strictEqual(debugMode.details.Variable.visibility, "Private");
    });

    it("should simplify Literal types to base types for variables without type annotation", () => {
      // MAX_SIZE = 1024 * 1024 -> should be "int", not "Literal[1048576]"
      const maxSize = mainDoc.definitions.find(
        (d): d is VariableSymbol => isVariableSymbol(d) && d.name === "MAX_SIZE"
      )!;
      assert.strictEqual(maxSize.details.Variable.var_type, "int",
        "Integer literal should be simplified to int");

      // _debug_mode = False -> should be "bool", not "Literal[False]"
      const debugMode = mainDoc.definitions.find(
        (d): d is VariableSymbol => isVariableSymbol(d) && d.name === "_debug_mode"
      )!;
      assert.strictEqual(debugMode.details.Variable.var_type, "bool",
        "Boolean literal should be simplified to bool");
    });

    it("should extract class fields with correct enclosing_symbol", () => {
      const fileReader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "FileReader"
      )!;

      const encodingField = mainDoc.definitions.find(
        (d): d is VariableSymbol => 
          isVariableSymbol(d) && 
          d.name === "encoding" &&
          d.enclosing_symbol === fileReader.symbol_id
      );

      assert.ok(encodingField, "encoding field should have FileReader as enclosing_symbol");
      assert.strictEqual(encodingField.details.Variable.scope, "Field");
    });

    it("should extract _cache field as private", () => {
      const fileReader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "FileReader"
      )!;

      const cacheField = mainDoc.definitions.find(
        (d): d is VariableSymbol => 
          isVariableSymbol(d) && 
          d.name === "_cache" &&
          d.enclosing_symbol === fileReader.symbol_id
      );

      assert.ok(cacheField, "_cache field should be extracted");
      assert.strictEqual(cacheField.details.Variable.visibility, "Private");
      assert.strictEqual(cacheField.details.Variable.scope, "Field");
    });
  });

  describe("Type fields validation", () => {
    it("should populate FileReader fields array", () => {
      const fileReader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "FileReader"
      )!;

      const fields = fileReader.details.Type.fields;
      assert.ok(fields.length >= 2, "FileReader should have at least 2 fields (encoding and _cache)");
      
      const fieldNames = fields.map(f => f.name);
      assert.ok(fieldNames.includes("encoding"), "FileReader fields should include encoding");
      assert.ok(fieldNames.includes("_cache"), "FileReader fields should include _cache");
    });

    it("should have field symbol_ids matching Variable definitions", () => {
      const fileReader = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "FileReader"
      )!;

      for (const field of fileReader.details.Type.fields) {
        const matchingVar = mainDoc.definitions.find(
          (d): d is VariableSymbol => 
            isVariableSymbol(d) && d.symbol_id === field.symbol_id
        );
        assert.ok(matchingVar, `Field ${field.name} should have matching Variable definition`);
      }
    });

    it("should use declared type annotation instead of inferred Literal type", () => {
      // Config.value has explicit type annotation: value: str = "global"
      // Should use declared type "str", not inferred "Literal['global']"
      const config = mainDoc.definitions.find(
        (d): d is TypeSymbol => isTypeSymbol(d) && d.name === "Config"
      )!;

      const valueField = config.details.Type.fields.find(f => f.name === "value");
      assert.ok(valueField, "Config should have a 'value' field");
      assert.strictEqual(valueField.field_type, "str",
        "Field with explicit type annotation should use declared type, not Literal");

      // Also check the Variable definition
      const valueVar = mainDoc.definitions.find(
        (d): d is VariableSymbol => 
          isVariableSymbol(d) && d.symbol_id === "main.Config.value#Variable"
      );
      assert.ok(valueVar, "Config.value Variable definition should exist");
      assert.strictEqual(valueVar.details.Variable.var_type, "str",
        "Variable with explicit type annotation should use declared type, not Literal");
    });
  });

  describe("SourceLocation and SourceSpan validation", () => {
    const mainPyPath = path.join(testProjectPath, "main.py");
    const sourceLines: string[] = fs.readFileSync(mainPyPath, "utf-8").split("\n");

    it("should point to source lines that contain the symbol name", () => {
      // Reader class: location should point to a line containing "class Reader"
      const reader = mainDoc.definitions.find(
        (d) => isTypeSymbol(d) && d.name === "Reader"
      )!;
      const readerLine = sourceLines[reader.location.line];
      assert.ok(readerLine?.includes("Reader"),
        `Line ${reader.location.line + 1} should contain "Reader", got: ${JSON.stringify(readerLine)}`);

      // FileReader class: location should point to a line containing "class FileReader"
      const fileReader = mainDoc.definitions.find(
        (d) => isTypeSymbol(d) && d.name === "FileReader"
      )!;
      const fileReaderLine = sourceLines[fileReader.location.line];
      assert.ok(fileReaderLine?.includes("FileReader"),
        `Line ${fileReader.location.line + 1} should contain "FileReader", got: ${JSON.stringify(fileReaderLine)}`);
    });

    it("should have span text that matches the symbol declaration", () => {
      const reader = mainDoc.definitions.find(
        (d) => isTypeSymbol(d) && d.name === "Reader"
      )!;
      const readerSpanText = extractSpanText(sourceLines, reader.span);
      assert.ok(readerSpanText.includes("class Reader"),
        `Span should cover "class Reader", got: ${JSON.stringify(readerSpanText.slice(0, 80))}...`);

      const fileReader = mainDoc.definitions.find(
        (d) => isTypeSymbol(d) && d.name === "FileReader"
      )!;
      const fileReaderSpanText = extractSpanText(sourceLines, fileReader.span);
      assert.ok(fileReaderSpanText.includes("class FileReader"),
        `Span should cover "class FileReader", got: ${JSON.stringify(fileReaderSpanText.slice(0, 80))}...`);
    });

    it("should have valid source spans (start <= end, in bounds)", () => {
      for (const def of mainDoc.definitions) {
        const span = def.span;
        assert.ok(span.start_line <= span.end_line,
          `${def.symbol_id}: start_line should be <= end_line`);
        assert.ok(span.start_line >= 0 && span.end_line < sourceLines.length,
          `${def.symbol_id}: span lines should be within file bounds`);

        if (span.start_line === span.end_line) {
          assert.ok(span.start_column <= span.end_column,
            `${def.symbol_id}: start_column should be <= end_column when on same line`);
        }
      }
    });

    it("should have file_path matching document relative_path", () => {
      for (const def of mainDoc.definitions) {
        assert.strictEqual(def.location.file_path, "main.py",
          "Definition file_path should match document relative_path");
      }
    });
  });

  describe("SymbolReference validation", () => {
    it("should have references with valid target_symbol", () => {
      assert.ok(mainDoc.references.length > 0, "Should have references");

      for (const ref of mainDoc.references) {
        assert.ok(ref.target_symbol, "Reference should have target_symbol");
        assert.strictEqual(typeof ref.target_symbol, "string");
      }
    });

    it("should have enclosing_symbol pointing to containing function/method", () => {
      const definedSymbols = new Set(mainDoc.definitions.map((d) => d.symbol_id));

      for (const ref of mainDoc.references) {
        assert.ok(ref.enclosing_symbol, "Reference should have enclosing_symbol");
        assert.ok(definedSymbols.has(ref.enclosing_symbol),
          `Reference enclosing_symbol ${ref.enclosing_symbol} should exist in definitions`);
      }
    });

    it("should correctly identify Read and Write roles", () => {
      // Only Call, Read, Write, and Decorate roles are valid
      const validRoles = ["Call", "Read", "Write", "Decorate"];

      for (const ref of mainDoc.references) {
        assert.ok(validRoles.includes(ref.role),
          `Reference role ${ref.role} should be valid`);
      }

      // Should have some write references (assignments)
      const writeRefs = mainDoc.references.filter((r) => r.role === "Write");
      assert.ok(writeRefs.length > 0, "Should have Write references for assignments");

      // Find references with receiver (like self.encoding, self._cache)
      const refsWithReceiver = mainDoc.references.filter(
        (r) => r.receiver === "self"
      );

      assert.ok(refsWithReceiver.length > 0,
        "Should have references with 'self' as receiver for member access");
    });

    it("should NOT have Call references at class/function definition sites", () => {
      // Class and function definitions should NOT generate Call references
      // The definition itself is not a "call" - it's a declaration
      const definitionLines = new Map<number, string>();
      for (const def of mainDoc.definitions) {
        definitionLines.set(def.location.line, def.symbol_id);
      }

      const badCallRefs = mainDoc.references.filter((ref) => {
        if (ref.role !== "Call") return false;
        // Check if this Call reference is at a definition site for that same symbol
        const defAtLine = definitionLines.get(ref.location.line);
        return defAtLine === ref.target_symbol;
      });

      assert.strictEqual(badCallRefs.length, 0,
        `Should not have Call references at definition sites: ${JSON.stringify(badCallRefs.map(r => ({
          target: r.target_symbol,
          line: r.location.line,
          col: r.location.column
        })))}`);
    });

    it("should have Call reference target method symbol, not the class type", () => {
      // reader.read(path) should reference main.Reader.read#Function, not main.Reader#Type
      const methodCallRef = mainDoc.references.find(
        (r) => r.role === "Call" && 
               r.receiver === "reader" &&
               r.enclosing_symbol === "main.process_file#Function"
      );

      if (methodCallRef) {
        // The target should be the method, not the type
        assert.ok(
          methodCallRef.target_symbol === "main.Reader.read#Function",
          `Method call should target the method symbol (got ${methodCallRef.target_symbol})`
        );
      }
    });

    it("should have field access references target the field Variable, not the class Type", () => {
      // self._cache, self.encoding should reference the field Variable symbols
      const selfFieldRefs = mainDoc.references.filter(
        (r) => r.receiver === "self" && 
               (r.role === "Read" || r.role === "Write")
      );

      for (const ref of selfFieldRefs) {
        // Field access should target Variable symbols, not Type symbols
        assert.ok(
          ref.target_symbol.endsWith("#Variable"),
          `Field access should target Variable symbol, not Type (got ${ref.target_symbol})`
        );
      }
    });

    it("should have Write references for field assignments in __init__", () => {
      // self.encoding = encoding and self._cache = {} should generate Write references
      const initRefs = mainDoc.references.filter(
        (r) => r.enclosing_symbol === "main.FileReader.__init__#Function"
      );
      
      const encodingWrite = initRefs.find(
        (r) => r.target_symbol === "main.FileReader.encoding#Variable" && r.role === "Write"
      );
      const cacheWrite = initRefs.find(
        (r) => r.target_symbol === "main.FileReader._cache#Variable" && r.role === "Write"
      );
      
      assert.ok(encodingWrite, "Should have Write reference for self.encoding assignment");
      assert.ok(cacheWrite, "Should have Write reference for self._cache assignment");
    });

    it("should have consistent symbol_id format across all symbols", () => {
      for (const doc of cachedData.documents) {
        for (const def of doc.definitions) {
          // symbol_id should end with #Type, #Function, or #Variable
          assert.ok(
            def.symbol_id.endsWith("#Type") ||
            def.symbol_id.endsWith("#Function") ||
            def.symbol_id.endsWith("#Variable"),
            `${def.symbol_id} should end with #Type, #Function, or #Variable`
          );
        }
      }
    });

    it("should have unique symbol_ids within document", () => {
      for (const doc of cachedData.documents) {
        const symbolIds = doc.definitions.map((d) => d.symbol_id);
        const uniqueIds = new Set(symbolIds);
        
        assert.strictEqual(symbolIds.length, uniqueIds.size,
          "All symbol_ids within a document should be unique");
      }
    });
  });

  describe("Annotation (Decorator) semantics", () => {
    // Specification: how Python decorators (annotations) should be expressed in SemanticData.
    //
    // 1. main.py:7-14 (log_call)
    //    - The inner "wrapper" function is inside a function; it is NOT a definition (we only emit top-level/method definitions).
    //    - Decorator used inside function are ignored.
    //
    // 2. main.py:16-28 (retry)
    //    - Inner "decorator" and "wrapper" are NOT definitions; ignore any annotations/references inside them.
    //    - No requirement for references from retry's inner scope.
    //
    // 3. main.py:79-87 (@singleton class ServiceManager)
    //    - Class is decorated with @singleton. Express as ONE Decorate reference:
    //      enclosing_symbol = main.ServiceManager.__init__#Function, target_symbol = main.singleton#Function, role = Decorate.
    //
    // 4. main.py:90-105 (@log_call @retry(max_attempts=3) def process_file(...))
    //    - Function has two decorators. Express as TWO references, both role = Decorate:
    //      (enclosing = main.process_file#Function, target = main.log_call#Function),
    //      (enclosing = main.process_file#Function, target = main.retry#Function).

    it("should NOT define nested functions inside decorators as top-level definitions", () => {
      const funcDefs = mainDoc.definitions.filter((d) => isFunctionSymbol(d));
      const logCallId = "main.log_call#Function";
      const retryId = "main.retry#Function";

      const nestedInLogCall = funcDefs.filter(
        (d) => d.enclosing_symbol === logCallId
      );
      const nestedInRetry = funcDefs.filter(
        (d) => d.enclosing_symbol === retryId
      );

      assert.strictEqual(
        nestedInLogCall.length,
        0,
        "log_call should have no nested function definitions (wrapper is not a definition)"
      );
      assert.strictEqual(
        nestedInRetry.length,
        0,
        "retry should have no nested function definitions (decorator/wrapper are not definitions)"
      );
    });

    it("should not have references from log_call to wraps (Call or Decorate)", () => {
      // Case 1 (main.py 7-14): inside log_call, @wraps(func) — express as one reference: enclosing = log_call, target = wraps.
      // Role may be Call (function call) or Decorate (decorator application); both match the spec.
      const refsInLogCall = mainDoc.references.filter(
        (r) => r.enclosing_symbol === "main.log_call#Function"
      );
      assert.strictEqual(refsInLogCall.length, 0, "Should not have references from log_call to wraps");

      // Case 2 (main.py 16-28): retry's inner decorator/wrapper are not definitions; ignore annotations inside.
      // We do not require any references from them. This test documents that inner functions need no refs.
      const refsInRetry = mainDoc.references.filter(
        (r) => r.enclosing_symbol === "main.retry#Function"
      );
      // retry body may have a reference to max_attempts (parameter) or range, etc.; no requirement for zero.
      // This test documents that inner functions are not definitions and need no refs.
      assert.strictEqual(refsInRetry.length, 0, "Should not have references from retry inner functions");
    });

    it("should have Decorate reference from ServiceManager.__init__ to singleton", () => {
      // Case 3 (main.py 79-87): @singleton on class ServiceManager.
      // SemanticData: one reference enclosing_symbol = main.ServiceManager.__init__#Function,
      // target_symbol = main.singleton#Function, role = Decorate.
      const annotationRefs = mainDoc.references.filter(
        (r) =>
          r.role === "Decorate" &&
          r.target_symbol === "main.singleton#Function"
      );
      const fromInit = annotationRefs.find(
        (r) => r.enclosing_symbol === "main.ServiceManager.__init__#Function"
      );
      assert.ok(
        fromInit,
        "Should have Decorate reference from ServiceManager.__init__ to singleton (class decorator expressed via __init__)"
      );
    });

    it("should have two Decorate references from process_file to log_call and retry", () => {
      // Case 4 (main.py 90-105): @log_call and @retry(max_attempts=3) on process_file.
      // SemanticData: two references, both role = Decorate:
      //   (enclosing = main.process_file#Function, target = main.log_call#Function),
      //   (enclosing = main.process_file#Function, target = main.retry#Function).
      const processFileAnnotationRefs = mainDoc.references.filter(
        (r) =>
          r.role === "Decorate" &&
          r.enclosing_symbol === "main.process_file#Function"
      );
      assert.strictEqual(
        processFileAnnotationRefs.length,
        2,
        "process_file should have exactly 2 Decorate references (log_call and retry)"
      );
      const targets = processFileAnnotationRefs.map((r) => r.target_symbol);
      assert.ok(
        targets.includes("main.log_call#Function"),
        "Should have Decorate reference from process_file to log_call"
      );
      assert.ok(
        targets.includes("main.retry#Function"),
        "Should have Decorate reference from process_file to retry"
      );
    });

    it("should treat decorator usage as reference (Decorate), not definition", () => {
      const defIds = new Set(mainDoc.definitions.map((d) => d.symbol_id));
      const annotationRefs = mainDoc.references.filter(
        (r) => r.role === "Decorate"
      );
      assert.ok(
        annotationRefs.length >= 3,
        "Should have at least 3 Decorate references (singleton, log_call, retry)"
      );
      for (const ref of annotationRefs) {
        assert.ok(
          defIds.has(ref.target_symbol),
          `Annotation reference should point to definition: ${ref.target_symbol}`
        );
      }
    });

    it("should include the expected Decorate references for cases 3 & 4", () => {
      // Required set of decorator application references for main.py (cases 3 & 4).
      // Case 1 (@wraps inside log_call) may also appear as Decorate; extra Decorate refs are allowed.
      const requiredAnnotationRefs: [string, string][] = [
        ["main.ServiceManager.__init__#Function", "main.singleton#Function"],
        ["main.process_file#Function", "main.log_call#Function"],
        ["main.process_file#Function", "main.retry#Function"],
      ];
      const annotationRefs = mainDoc.references.filter(
        (r) => r.role === "Decorate"
      );
      const actualPairs = annotationRefs.map((r) => [
        r.enclosing_symbol,
        r.target_symbol,
      ]) as [string, string][];
      for (const [enclosing, target] of requiredAnnotationRefs) {
        const found = actualPairs.some(
          ([e, t]) => e === enclosing && t === target
        );
        assert.ok(
          found,
          `Missing Decorate reference: enclosing=${enclosing}, target=${target}. Actual: ${JSON.stringify(actualPairs)}`
        );
      }
    });
  });

});
