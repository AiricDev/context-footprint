/**
 * Integration tests for context-footprint-extractors
 * Tests the CLI tool against Python test cases in tests/ directory
 * 
 * NOTE: These tests use caching to avoid re-running LSP extraction for each test.
 * The extraction takes ~15-20 seconds, so we cache the result per test file.
 */

import { describe, it, beforeAll } from "bun:test";
import assert from "node:assert";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runExtractionWithCache, clearExtractionCache } from "./test-helper";
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
      assert.ok(Array.isArray(cachedData.external_symbols), "external_symbols should be an array");
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
      assert.strictEqual(fileReader.is_external, false);
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
    it("should extract module-level function with correct symbol_id", () => {
      const processFile = mainDoc.definitions.find(
        (d): d is FunctionSymbol => isFunctionSymbol(d) && d.name === "process_file"
      );

      assert.ok(processFile, "process_file function should be extracted");
      // Module-level functions should have null enclosing_symbol
      assert.strictEqual(processFile.enclosing_symbol, null,
        "Module-level function should have null enclosing_symbol");
      // symbol_id should be module.function#Function
      assert.strictEqual(processFile.symbol_id, "main.process_file#Function");
    });

    it("should have correct Function details with parameters", () => {
      const processFile = mainDoc.definitions.find(
        (d): d is FunctionSymbol => isFunctionSymbol(d) && d.name === "process_file"
      )!;

      const funcDetails = processFile.details.Function;
      assert.ok(Array.isArray(funcDetails.parameters));
      assert.ok(Array.isArray(funcDetails.return_types));
      assert.strictEqual(typeof funcDetails.modifiers.is_async, "boolean");
      assert.strictEqual(typeof funcDetails.modifiers.is_generator, "boolean");
      assert.strictEqual(typeof funcDetails.modifiers.is_static, "boolean");
      assert.strictEqual(typeof funcDetails.modifiers.is_abstract, "boolean");
      assert.ok(["Public", "Private", "Protected", "Internal"].includes(funcDetails.modifiers.visibility));
    });

    it("should extract process_file parameters correctly with fully qualified type symbol_ids", () => {
      const processFile = mainDoc.definitions.find(
        (d): d is FunctionSymbol => isFunctionSymbol(d) && d.name === "process_file"
      )!;

      const params = processFile.details.Function.parameters;
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
    });

    it("should extract FileReader.__init__ parameters correctly", () => {
      const initMethod = mainDoc.definitions.find(
        (d): d is FunctionSymbol => 
          isFunctionSymbol(d) && 
          d.name === "__init__" &&
          d.symbol_id === "main.FileReader.__init__#Function"
      )!;

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
  });

  describe("SourceLocation and SourceSpan validation", () => {
    it("should have 0-based line and column numbers", () => {
      // Reader class starts at line 6 (1-based) / 5 (0-based), column 0
      const reader = mainDoc.definitions.find(
        (d) => isTypeSymbol(d) && d.name === "Reader"
      )!;

      // LSP returns 0-based line numbers
      assert.strictEqual(reader.location.line, 5,
        "Line numbers should be 0-based (Reader is on line 6 in file)");
      assert.strictEqual(reader.location.column, 0);
      
      // FileReader class starts at line 14 (1-based) / 13 (0-based), column 0
      const fileReader = mainDoc.definitions.find(
        (d) => isTypeSymbol(d) && d.name === "FileReader"
      )!;
      assert.strictEqual(fileReader.location.line, 13,
        "Line numbers should be 0-based (FileReader is on line 14 in file)");
    });

    it("should have valid source spans (start <= end)", () => {
      for (const def of mainDoc.definitions) {
        const span = def.span;
        assert.ok(span.start_line <= span.end_line,
          `${def.symbol_id}: start_line should be <= end_line`);
        
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
      const validRoles = ["Call", "Read", "Write", "TypeAnnotation", "TypeInstantiation", "Import", "Decorator"];

      for (const ref of mainDoc.references) {
        assert.ok(validRoles.includes(ref.role),
          `Reference role ${ref.role} should be valid`);
      }

      // Should have some write references (assignments)
      const writeRefs = mainDoc.references.filter((r) => r.role === "Write");
      assert.ok(writeRefs.length > 0, "Should have Write references for assignments");
    });

    it("should identify receiver for member access", () => {
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

    it("should use TypeAnnotation role for type annotations in function signatures", () => {
      // process_file(reader: Reader, path: str) -> int
      // The "Reader" type annotation should be TypeAnnotation, not Read
      const processFileRefs = mainDoc.references.filter(
        (r) => r.enclosing_symbol === "main.process_file#Function"
      );

      // Find reference to Reader type in process_file
      const readerTypeRef = processFileRefs.find(
        (r) => r.target_symbol === "main.Reader#Type" && 
               r.location.line === 43 // def process_file line
      );

      if (readerTypeRef) {
        assert.strictEqual(readerTypeRef.role, "TypeAnnotation",
          "Type annotation in function parameter should have TypeAnnotation role, not Read");
      }
    });

    it("should use TypeAnnotation role for return type annotations", () => {
      // def get_config() -> Config:
      const getConfigRefs = mainDoc.references.filter(
        (r) => r.enclosing_symbol === "main.get_config#Function" &&
               r.target_symbol === "main.Config#Type"
      );

      // The return type annotation reference
      const returnTypeRef = getConfigRefs.find(
        (r) => r.location.line === 69 // def get_config line
      );

      if (returnTypeRef) {
        assert.strictEqual(returnTypeRef.role, "TypeAnnotation",
          "Return type annotation should have TypeAnnotation role");
      }
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
          methodCallRef.target_symbol.includes("read#Function"),
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

    it("should NOT have TypeAnnotation references for parameter/return types (already in definitions)", () => {
      // Parameter types and return types are already captured in FunctionDetails
      // They should NOT appear as TypeAnnotation references
      const typeAnnotationRefs = mainDoc.references.filter(
        (r) => r.role === "TypeAnnotation"
      );
      
      // Type annotations should only be for inheritance (class Foo(Bar)) 
      // not for function signatures
      for (const ref of typeAnnotationRefs) {
        // Check that enclosing_symbol is a Type (class inheritance context), not a Function
        const enclosingDef = mainDoc.definitions.find(d => d.symbol_id === ref.enclosing_symbol);
        assert.ok(
          enclosingDef?.kind === "Type",
          `TypeAnnotation should only be for class inheritance, not function signatures. ` +
          `Found TypeAnnotation in ${ref.enclosing_symbol} at line ${ref.location.line}`
        );
      }
    });
  });

  describe("Cross-reference validation", () => {
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
});
