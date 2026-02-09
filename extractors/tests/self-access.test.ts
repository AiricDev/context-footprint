/**
 * Tests for self.attribute access in multi-file Python projects.
 * 
 * Verifies that when a class has attributes whose types are defined in
 * other modules (e.g., port interfaces), the extractor correctly emits
 * references for:
 *   - self.field reads (Read role)
 *   - self.field writes (Write role) 
 *   - self.field.method() calls (Call role targeting the method on the port type)
 */

import { describe, it, beforeAll } from "bun:test";
import assert from "node:assert";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { runExtractionWithCache } from "./test-helper";
import type { SemanticData, DocumentSemantics, SymbolReference } from "../src/core/types";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

describe("Self access references in multi-file project", () => {
  const testProjectPath = path.resolve(__dirname, "multi_file_self_access");
  let data: SemanticData;
  let serviceDoc: DocumentSemantics;
  let portsDoc: DocumentSemantics;

  beforeAll(async () => {
    data = await runExtractionWithCache(testProjectPath);
    serviceDoc = data.documents.find((d) => d.relative_path === "service.py")!;
    portsDoc = data.documents.find((d) => d.relative_path === "ports.py")!;
  }, 30000);

  it("should extract both documents", () => {
    assert.ok(serviceDoc, "service.py document should be extracted");
    assert.ok(portsDoc, "ports.py document should be extracted");
  });

  it("should extract DataService type definition", () => {
    const dataService = serviceDoc.definitions.find(
      (d) => d.kind === "Type" && d.name === "DataService"
    );
    assert.ok(dataService, "DataService class should be extracted");
  });

  it("should extract field definitions for DataService", () => {
    const storageField = serviceDoc.definitions.find(
      (d) => d.kind === "Variable" && d.name === "storage"
    );
    const loggerField = serviceDoc.definitions.find(
      (d) => d.kind === "Variable" && d.name === "logger"
    );
    const counterField = serviceDoc.definitions.find(
      (d) => d.kind === "Variable" && d.name === "_counter"
    );

    assert.ok(storageField, "storage field should be extracted");
    assert.ok(loggerField, "logger field should be extracted");
    assert.ok(counterField, "_counter field should be extracted");
  });

  describe("self.field Write references in __init__", () => {
    it("should have Write references for self.storage and self.logger assignments", () => {
      const initSymbol = serviceDoc.definitions.find(
        (d) => d.kind === "Function" && d.name === "__init__"
      );
      assert.ok(initSymbol, "__init__ should be extracted");

      const initRefs = serviceDoc.references.filter(
        (r) => r.enclosing_symbol === initSymbol!.symbol_id
      );

      const storageWrite = initRefs.find(
        (r) => r.role === "Write" && r.receiver === "self" &&
               r.target_symbol.includes("storage") && r.target_symbol.endsWith("#Variable")
      );
      assert.ok(storageWrite,
        `Should have Write reference for self.storage assignment. ` +
        `Found refs: ${JSON.stringify(initRefs.map(r => ({ target: r.target_symbol, role: r.role, receiver: r.receiver })))}`
      );

      const loggerWrite = initRefs.find(
        (r) => r.role === "Write" && r.receiver === "self" &&
               r.target_symbol.includes("logger") && r.target_symbol.endsWith("#Variable")
      );
      assert.ok(loggerWrite,
        `Should have Write reference for self.logger assignment. ` +
        `Found refs: ${JSON.stringify(initRefs.map(r => ({ target: r.target_symbol, role: r.role, receiver: r.receiver })))}`
      );
    });
  });

  describe("self.field Read references in method bodies", () => {
    it("should have Read reference for self._counter in get_count", () => {
      const getCountSymbol = serviceDoc.definitions.find(
        (d) => d.kind === "Function" && d.name === "get_count"
      );
      assert.ok(getCountSymbol, "get_count should be extracted");

      const getRefs = serviceDoc.references.filter(
        (r) => r.enclosing_symbol === getCountSymbol!.symbol_id
      );

      const counterRead = getRefs.find(
        (r) => r.role === "Read" && r.receiver === "self" &&
               r.target_symbol.includes("_counter") && r.target_symbol.endsWith("#Variable")
      );
      assert.ok(counterRead,
        `Should have Read reference for self._counter in get_count. ` +
        `Found refs: ${JSON.stringify(getRefs.map(r => ({ target: r.target_symbol, role: r.role, receiver: r.receiver })))}`
      );
    });
  });

  describe("self.port.method() Call references", () => {
    it("should have Call reference for self.storage.save() in process", () => {
      const processSymbol = serviceDoc.definitions.find(
        (d) => d.kind === "Function" && d.name === "process"
      );
      assert.ok(processSymbol, "process method should be extracted");

      const processRefs = serviceDoc.references.filter(
        (r) => r.enclosing_symbol === processSymbol!.symbol_id
      );

      // self.storage.save() should produce a Call to StoragePort.save
      const saveCall = processRefs.find(
        (r) => r.role === "Call" && r.target_symbol.includes("save") &&
               r.target_symbol.endsWith("#Function")
      );
      assert.ok(saveCall,
        `Should have Call reference for self.storage.save() in process. ` +
        `Found refs: ${JSON.stringify(processRefs.map(r => ({ target: r.target_symbol, role: r.role, receiver: r.receiver })))}`
      );
    });

    it("should have Call reference for self.logger.info() in process", () => {
      const processSymbol = serviceDoc.definitions.find(
        (d) => d.kind === "Function" && d.name === "process"
      );

      const processRefs = serviceDoc.references.filter(
        (r) => r.enclosing_symbol === processSymbol!.symbol_id
      );

      // self.logger.info() should produce a Call to LoggerPort.info
      const infoCall = processRefs.find(
        (r) => r.role === "Call" && r.target_symbol.includes("info") &&
               r.target_symbol.endsWith("#Function")
      );
      assert.ok(infoCall,
        `Should have Call reference for self.logger.info() in process. ` +
        `Found refs: ${JSON.stringify(processRefs.map(r => ({ target: r.target_symbol, role: r.role, receiver: r.receiver })))}`
      );
    });

    it("should have Call reference for self.storage.load() in retrieve", () => {
      const retrieveSymbol = serviceDoc.definitions.find(
        (d) => d.kind === "Function" && d.name === "retrieve"
      );
      assert.ok(retrieveSymbol, "retrieve method should be extracted");

      const retrieveRefs = serviceDoc.references.filter(
        (r) => r.enclosing_symbol === retrieveSymbol!.symbol_id
      );

      const loadCall = retrieveRefs.find(
        (r) => r.role === "Call" && r.target_symbol.includes("load") &&
               r.target_symbol.endsWith("#Function")
      );
      assert.ok(loadCall,
        `Should have Call reference for self.storage.load() in retrieve. ` +
        `Found refs: ${JSON.stringify(retrieveRefs.map(r => ({ target: r.target_symbol, role: r.role, receiver: r.receiver })))}`
      );
    });
  });

  describe("self.field Write in method body (non-init)", () => {
    it("should have Write reference for self._counter += 1 in process", () => {
      const processSymbol = serviceDoc.definitions.find(
        (d) => d.kind === "Function" && d.name === "process"
      );

      const processRefs = serviceDoc.references.filter(
        (r) => r.enclosing_symbol === processSymbol!.symbol_id
      );

      const counterWrite = processRefs.find(
        (r) => r.role === "Write" && r.receiver === "self" &&
               r.target_symbol.includes("_counter") && r.target_symbol.endsWith("#Variable")
      );
      assert.ok(counterWrite,
        `Should have Write reference for self._counter += 1 in process. ` +
        `Found refs: ${JSON.stringify(processRefs.map(r => ({ target: r.target_symbol, role: r.role, receiver: r.receiver })))}`
      );
    });
  });
});
