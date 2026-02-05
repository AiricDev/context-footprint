import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
import pLimit from "p-limit";
import {
  DocumentSymbol,
  Location,
  Position,
  Range,
  SymbolInformation,
  SymbolKind as LspSymbolKind,
  TextDocumentItem,
  Hover
} from "vscode-languageserver-protocol";
import { ExtractorBase, ExtractOptions } from "../../core/extractor-base";
import { discoverFiles, relativePath, toUri } from "../../core/utils";
import { LspClientOptions } from "../../core/lsp-client";
import {
  FunctionDetails,
  FunctionModifiers,
  Parameter,
  ReferenceRole,
  SemanticData,
  SymbolDefinition,
  SymbolKind,
  SymbolReference,
  TypeDetails,
  TypeKind,
  VariableDetails,
  VariableScope
} from "../../core/types";
import { LSP_CONCURRENCY, FILE_READ_CONCURRENCY, pyrightLangServer } from "./constants";
import type { SymbolRecord } from "./types";
import {
  splitParams,
  rangeContains,
  rangeSize,
  inferVisibility,
  inferMutability,
  isBuiltinType,
  simplifyLiteralType
} from "./symbol-utils";
import { extractHoverInfo, parseSignature } from "./hover";
import {
  inferReferenceRole,
  extractReceiver,
  findDefLine,
  findClassLine,
  isDecoratorForNestedDef
} from "./reference-utils";

export class PythonExtractor extends ExtractorBase {
  private fileContents = new Map<string, string>();
  private fileLines = new Map<string, string[]>(); // Cached line arrays
  private symbolIndex: SymbolRecord[] = [];
  private symbolIndexByUri = new Map<string, SymbolRecord[]>(); // Index by URI for fast lookup
  private definitions: SymbolDefinition[] = [];
  private references: SymbolReference[] = [];
  private externalSymbols: SymbolDefinition[] = [];
  private externalSymbolIds = new Set<string>();
  private hoverCache = new Map<string, Hover>();
  private protocolClasses = new Set<string>();
  // Map from function symbol ID to set of local type names defined within that function
  private localTypesByFunction = new Map<string, Set<string>>();
  /** During collectReferences: only use these symbol IDs for enclosing lookup. */
  private definitionSymbolIdsForEnclosing: Set<string> | null = null;

  constructor(options: ExtractOptions) {
    super(options);
  }

  private log(message: string, alwaysShow = false): void {
    if (alwaysShow || this.options.verbose) {
      console.error(message);
    }
  }

  private progress(current: number, total: number, label: string): void {
    const percent = Math.round((current / total) * 100);
    const bar = "█".repeat(Math.floor(percent / 5)) + "░".repeat(20 - Math.floor(percent / 5));
    process.stderr.write(`\r[${bar}] ${percent}% ${label} (${current}/${total})`);
    if (current === total) {
      process.stderr.write("\n");
    }
  }

  protected getLspOptions(): LspClientOptions {
    return {
      command: process.execPath,
      args: [pyrightLangServer, "--stdio"],
      rootUri: toUri(this.options.projectRoot)
    };
  }

  protected async collectSemanticData(): Promise<SemanticData> {
    this.log("Discovering files...", true);
    const files = await discoverFiles({
      cwd: this.options.projectRoot,
      patterns: ["**/*.py"],
      ignore: ["**/tests/**", "**/__pycache__/**", "**/.venv/**", "**/venv/**", ...(this.options.exclude ?? [])]
    });
    this.log(`Found ${files.length} Python files`, true);

    this.log("Reading files...", true);
    await this.readFiles(files);

    this.log("Opening documents in LSP...", true);
    await this.openDocuments(files);

    this.log("Collecting symbols...", true);
    await this.collectSymbols(files);

    this.log("Collecting references...", true);
    await this.collectReferences();

    const documents = this.groupByDocument();
    this.log(`Extraction complete: ${this.definitions.length} definitions, ${this.references.length} references`, true);

    return {
      project_root: this.options.projectRoot,
      documents,
      external_symbols: this.externalSymbols
    };
  }

  private async readFiles(files: string[]): Promise<void> {
    const limit = pLimit(FILE_READ_CONCURRENCY);
    let completed = 0;

    await Promise.all(
      files.map((file) =>
        limit(async () => {
          const content = await fs.promises.readFile(file, "utf8");
          this.fileContents.set(file, content);
          this.fileLines.set(file, content.split(/\r?\n/));
          this.parseProtocolClasses(file, content);
          completed++;
          this.progress(completed, files.length, "Reading files");
        })
      )
    );
  }

  private async collectSymbols(files: string[]): Promise<void> {
    const limit = pLimit(LSP_CONCURRENCY);
    let completed = 0;

    // First pass: fetch document symbols for all files
    const symbolsByFile = new Map<string, DocumentSymbol[]>();
    await Promise.all(
      files.map((file) =>
        limit(async () => {
          const uri = toUri(file);
          const docSymbols = await this.fetchDocumentSymbols(uri);
          const hierarchy = this.buildHierarchyFromFlatList(docSymbols);
          symbolsByFile.set(file, hierarchy);
          completed++;
          this.progress(completed, files.length, "Fetching symbols");
        })
      )
    );

    // Second pass: fetch hover info in parallel
    this.log("Fetching hover info...", true);
    const hoverTasks: Array<() => Promise<void>> = [];
    for (const [file, hierarchy] of symbolsByFile) {
      const uri = toUri(file);
      this.collectHoverTasks(uri, hierarchy, [], hoverTasks);
    }

    completed = 0;
    const totalHovers = hoverTasks.length;
    await Promise.all(
      hoverTasks.map((task) =>
        limit(async () => {
          await task();
          completed++;
          if (completed % 100 === 0 || completed === totalHovers) {
            this.progress(completed, totalHovers, "Fetching hover info");
          }
        })
      )
    );

    // Third pass: process symbols - first create all Type definitions
    // This ensures type symbol IDs exist before resolving type references
    this.log("Processing type definitions...", true);
    completed = 0;
    for (const [file, hierarchy] of symbolsByFile) {
      const uri = toUri(file);
      this.processTypeDefinitions(file, uri, hierarchy, []);
      completed++;
      if (completed % 10 === 0 || completed === symbolsByFile.size) {
        this.progress(completed, symbolsByFile.size, "Processing type definitions");
      }
    }

    // Fourth pass: process Function and Variable definitions
    // Now type references can be resolved to existing type symbol IDs
    this.log("Processing function and variable definitions...", true);
    completed = 0;
    for (const [file, hierarchy] of symbolsByFile) {
      const uri = toUri(file);
      await this.processNonTypeDefinitions(file, uri, hierarchy, []);
      completed++;
      if (completed % 10 === 0 || completed === symbolsByFile.size) {
        this.progress(completed, symbolsByFile.size, "Processing function and variable definitions");
      }
    }

    // Build URI index for fast enclosing symbol lookup
    for (const record of this.symbolIndex) {
      const records = this.symbolIndexByUri.get(record.uri) ?? [];
      records.push(record);
      this.symbolIndexByUri.set(record.uri, records);
    }
  }

  private collectHoverTasks(
    uri: string,
    symbols: DocumentSymbol[],
    parents: DocumentSymbol[],
    tasks: Array<() => Promise<void>>
  ): void {
    const filePath = fileURLToPath(uri);
    const lines = this.fileLines.get(filePath) ?? [];

    for (const symbol of symbols) {
      const nameChain = [...parents.map((p) => p.name), symbol.name];
      const cacheKey = `${uri}#${nameChain.join(".")}`;

      if (!this.hoverCache.has(cacheKey)) {
        tasks.push(async () => {
          try {
            let position = symbol.selectionRange?.start ?? symbol.range.start;

            // If selectionRange equals range start, we need to find the actual name position
            // This is especially important for decorated functions where range starts at decorator
            if (
              symbol.selectionRange?.start.line === symbol.range.start.line &&
              symbol.selectionRange?.start.character === symbol.range.start.character
            ) {
              // For functions, find the "def" line first (handles decorators)
              const isFunction = symbol.kind === LspSymbolKind.Function || symbol.kind === LspSymbolKind.Method;
              let searchLine = symbol.range.start.line;
              
              if (isFunction) {
                // Find the actual "def" line
                for (let i = symbol.range.start.line; i < lines.length; i++) {
                  if (lines[i].match(/^\s*(async\s+)?def\s+/)) {
                    searchLine = i;
                    break;
                  }
                }
              }
              
              const lineText = lines[searchLine] ?? "";
              const namePos = lineText.indexOf(symbol.name);
              if (namePos >= 0) {
                position = { line: searchLine, character: namePos };
              }
            }

            const hover = await this.client!.sendRequest<Hover>("textDocument/hover", {
              textDocument: { uri },
              position
            });
            if (hover) {
              this.hoverCache.set(cacheKey, hover);
            }
          } catch {
            // Ignore hover errors
          }
        });
      }

      if (symbol.children) {
        this.collectHoverTasks(uri, symbol.children, [...parents, symbol], tasks);
      }
    }
  }

  private buildHierarchyFromFlatList(symbols: DocumentSymbol[]): DocumentSymbol[] {
    // If symbols already have children, return as-is
    if (symbols.some(s => s.children && s.children.length > 0)) {
      return symbols;
    }

    // Sort by range start position
    const sorted = [...symbols].sort((a, b) => {
      if (a.range.start.line !== b.range.start.line) {
        return a.range.start.line - b.range.start.line;
      }
      return a.range.start.character - b.range.start.character;
    });

    const root: DocumentSymbol[] = [];
    const stack: { symbol: DocumentSymbol; children: DocumentSymbol[] }[] = [];

    for (const symbol of sorted) {
      // Pop stack until we find a parent that contains this symbol
      while (stack.length > 0) {
        const top = stack[stack.length - 1];
        if (rangeContains(top.symbol.range, symbol.range.start)) {
          break;
        }
        stack.pop();
      }

      const newSymbol: DocumentSymbol = { ...symbol, children: [] };

      if (stack.length === 0) {
        // Top-level symbol
        root.push(newSymbol);
      } else {
        // Child of the top of stack
        stack[stack.length - 1].children.push(newSymbol);
      }

      stack.push({ symbol: newSymbol, children: newSymbol.children! });
    }

    return root;
  }

  private async openDocuments(files: string[]): Promise<void> {
    const limit = pLimit(LSP_CONCURRENCY);
    let completed = 0;

    await Promise.all(
      files.map((file) =>
        limit(async () => {
          const content = this.fileContents.get(file)!;
          const uri = toUri(file);
          const item: TextDocumentItem = {
            uri,
            languageId: "python",
            version: 1,
            text: content
          };
          await this.client!.sendNotification("textDocument/didOpen", { textDocument: item });
          completed++;
          this.progress(completed, files.length, "Opening documents");
        })
      )
    );
  }

  private parseProtocolClasses(filePath: string, content: string): void {
    // Match class definitions that inherit from Protocol
    const classRegex = /^class\s+(\w+)\s*\(\s*Protocol\s*\)/gm;
    let match;
    while ((match = classRegex.exec(content)) !== null) {
      const moduleName = relativePath(this.options.projectRoot, filePath)
        .replace(/\.py$/, "")
        .replace(/[\\/]/g, ".");
      const symbolId = `${moduleName}.${match[1]}#Type`;
      this.protocolClasses.add(symbolId);
    }
  }

  private async fetchDocumentSymbols(uri: string): Promise<DocumentSymbol[]> {
    const response = await this.client!.sendRequest<
      { documentSymbols: DocumentSymbol[] } | DocumentSymbol[] | SymbolInformation[]
    >(
      "textDocument/documentSymbol",
      { textDocument: { uri } }
    );

    if (Array.isArray(response)) {
      if (response.length === 0) return [];
      if ("range" in response[0]) {
        return response as DocumentSymbol[];
      }
      return (response as SymbolInformation[]).map((info) => ({
        name: info.name,
        detail: undefined,
        kind: info.kind,
        range: info.location.range,
        selectionRange: info.location.range,
        children: []
      }));
    }
    return response.documentSymbols ?? [];
  }

  private getHoverInfo(uri: string, nameChain: string[]): Hover | undefined {
    const cacheKey = `${uri}#${nameChain.join(".")}`;
    return this.hoverCache.get(cacheKey);
  }

  // First pass: process only Type definitions to establish type symbol IDs
  private processTypeDefinitions(
    filePath: string, 
    uri: string, 
    symbols: DocumentSymbol[], 
    parents: SymbolRecord[] = []
  ): void {
    for (const symbol of symbols) {
      const kind = this.mapSymbolKind(symbol.kind);

      if (!kind) {
        // Still recurse into children even if skipping this symbol
        if (symbol.children) {
          this.processTypeDefinitions(filePath, uri, symbol.children, parents);
        }
        continue;
      }

      // Only process Type definitions in this pass
      if (kind === SymbolKind.Type) {
        // Build proper symbol ID with full hierarchy
        const symbolId = this.createSymbolId(filePath, parents, symbol, kind);
        
        // Find enclosing Type symbol (for nested classes)
        let enclosing: string | null = null;
        for (let i = parents.length - 1; i >= 0; i--) {
          if (parents[i].kind === SymbolKind.Type) {
            enclosing = parents[i].symbolId;
            break;
          }
        }
        
        // Check if this type is defined inside a function (local type)
        const enclosingFunction = parents.find(p => p.kind === SymbolKind.Function);
        if (enclosingFunction) {
          // Record this local type to exclude it from type resolution in this function
          const localTypes = this.localTypesByFunction.get(enclosingFunction.symbolId) ?? new Set();
          localTypes.add(symbol.name);
          this.localTypesByFunction.set(enclosingFunction.symbolId, localTypes);
        }
        
        const record: SymbolRecord = {
          symbolId,
          uri,
          range: symbol.range,
          selectionRange: symbol.selectionRange ?? symbol.range,
          kind,
          enclosingSymbol: enclosing,
          name: symbol.name,
          detail: symbol.detail,
          children: symbol.children
        };

        this.symbolIndex.push(record);

        // Get hover info for this symbol
        const nameChain = [...parents.map(p => p.name), symbol.name].filter(Boolean);
        const hover = this.getHoverInfo(uri, nameChain);
        
        const definition = this.createTypeDefinition(filePath, symbolId, symbol, enclosing, hover);
        this.definitions.push(definition);

        // Collect fields for Type definitions
        if (symbol.children) {
          this.collectTypeFields(
            definition as SymbolDefinition & { kind: typeof SymbolKind.Type }, 
            symbol.children, 
            filePath, 
            record
          );
          
          // Recurse into children with updated parent chain
          const childParents = [...parents, record];
          this.processTypeDefinitions(filePath, uri, symbol.children, childParents);
        }
      } else if (symbol.children) {
        // For non-Type symbols, still recurse to find nested Type definitions
        // (e.g., a class defined inside a function)
        const symbolId = this.createSymbolId(filePath, parents, symbol, kind);
        const record: SymbolRecord = {
          symbolId,
          uri,
          range: symbol.range,
          selectionRange: symbol.selectionRange ?? symbol.range,
          kind,
          enclosingSymbol: null,
          name: symbol.name,
          detail: symbol.detail,
          children: symbol.children
        };
        const childParents = [...parents, record];
        this.processTypeDefinitions(filePath, uri, symbol.children, childParents);
      }
    }
  }

  // Second pass: process Function and Variable definitions
  // Type references are resolved using LSP typeDefinition
  private async processNonTypeDefinitions(
    filePath: string, 
    uri: string, 
    symbols: DocumentSymbol[], 
    parents: SymbolRecord[] = []
  ): Promise<void> {
    // Collect parameter info from Function parents to filter them out
    const parentFunction = parents.find(p => p.kind === SymbolKind.Function);
    const isInInit = parentFunction?.name === "__init__";
    
    // Build a set of (name, line) pairs for parameters
    const paramKeys = new Set<string>();
    if (parentFunction) {
      const funcLine = parentFunction.selectionRange.start.line;
      for (const child of symbols) {
        if (this.mapSymbolKind(child.kind) === SymbolKind.Variable && 
            child.range.start.line === funcLine) {
          paramKeys.add(`${child.name}@${child.range.start.line}`);
        }
      }
    }

    for (const symbol of symbols) {
      const kind = this.mapSymbolKind(symbol.kind);

      if (!kind) {
        if (symbol.children) {
          await this.processNonTypeDefinitions(filePath, uri, symbol.children, parents);
        }
        continue;
      }

      // Skip Type definitions - they were processed in the first pass
      if (kind === SymbolKind.Type) {
        // Find the existing record for this type
        const symbolId = this.createSymbolId(filePath, parents, symbol, kind);
        const record = this.symbolIndex.find(r => r.symbolId === symbolId);
        if (record && symbol.children) {
          const childParents = [...parents, record];
          await this.processNonTypeDefinitions(filePath, uri, symbol.children, childParents);
        }
        continue;
      }

      // Skip local variables and parameters
      if (kind === SymbolKind.Variable && parentFunction) {
        const paramKey = `${symbol.name}@${symbol.range.start.line}`;
        if (isInInit) {
          if (paramKeys.has(paramKey)) {
            continue;
          }
          // For class fields in __init__, use parents without the Function parent
          // so the symbol ID is main.Class.field#Variable not main.Class.__init__.field#Variable
        } else {
          continue;
        }
      }

      // Skip nested functions inside another function (e.g. wrapper inside log_call)
      // Annotation semantics: only top-level and method definitions are emitted
      if (kind === SymbolKind.Function && parentFunction) {
        const hasEnclosingType = parents.some((p) => p.kind === SymbolKind.Type);
        if (!hasEnclosingType) continue;
      }

      // Build proper symbol ID with full hierarchy
      // For class fields in __init__, exclude Function parents from the chain
      const symbolId = (kind === SymbolKind.Variable && isInInit)
        ? this.createFieldSymbolId(filePath, parents, symbol)
        : this.createSymbolId(filePath, parents, symbol, kind);
      
      // Find enclosing Type symbol (for methods and fields)
      let enclosing: string | null = null;
      for (let i = parents.length - 1; i >= 0; i--) {
        if (parents[i].kind === SymbolKind.Type) {
          enclosing = parents[i].symbolId;
          break;
        }
      }
      
      const record: SymbolRecord = {
        symbolId,
        uri,
        range: symbol.range,
        selectionRange: symbol.selectionRange ?? symbol.range,
        kind,
        enclosingSymbol: enclosing,
        name: symbol.name,
        detail: symbol.detail,
        children: symbol.children
      };

      this.symbolIndex.push(record);

      // Get hover info for this symbol
      const nameChain = [...parents.map(p => p.name), symbol.name].filter(Boolean);
      const hover = this.getHoverInfo(uri, nameChain);
      
      // For Function definitions, resolve type references using LSP
      let definition: SymbolDefinition;
      if (kind === SymbolKind.Function) {
        definition = await this.createFunctionDefinition(
          filePath, uri, symbolId, symbol, enclosing, hover
        );
      } else {
        definition = this.createVariableDefinition(filePath, symbolId, symbol, enclosing, hover);
      }
      this.definitions.push(definition);

      // Process children with updated parent chain
      if (symbol.children) {
        const childParents = [...parents, record];
        await this.processNonTypeDefinitions(filePath, uri, symbol.children, childParents);
      }
    }
  }

  private collectTypeFields(
    typeDef: SymbolDefinition & { kind: typeof SymbolKind.Type }, 
    children: DocumentSymbol[], 
    filePath: string,
    typeRecord: SymbolRecord
  ): void {
    const typeDetails = (typeDef.details as { Type: TypeDetails }).Type;
    const uri = toUri(filePath);
    const currentModule = this.extractModuleName(filePath);
    const lines = this.getFileLines(filePath);
    
    // Helper to get field type - prefers declared type annotation over inferred
    const getFieldType = (field: DocumentSymbol, nameChain: string[]): string | null => {
      // First try to get declared type from source
      const declaredType = this.extractDeclaredType(lines, field.selectionRange, currentModule);
      if (declaredType) return declaredType;
      
      // Fall back to hover-based inference
      const hover = this.getHoverInfo(uri, nameChain);
      if (hover) {
        const { signature } = extractHoverInfo(hover);
        return this.extractVariableType(signature, currentModule);
      }
      return null;
    };
    
    // Only recurse into __init__ to find fields - other methods have parameters, not fields
    const collectFieldsFromInit = (symbols: DocumentSymbol[]) => {
      for (const child of symbols) {
        const childKind = this.mapSymbolKind(child.kind);
        
        if (childKind === SymbolKind.Function && child.name === "__init__") {
          // Look for fields inside __init__
          if (child.children) {
            for (const field of child.children) {
              const fieldKind = this.mapSymbolKind(field.kind);
              if (fieldKind === SymbolKind.Variable) {
                // Skip parameters (those on the same line as the function)
                if (field.range.start.line === child.selectionRange.start.line) {
                  continue;
                }
                const fieldSymbolId = this.createSymbolId(filePath, [typeRecord], field, fieldKind);
                if (!typeDetails.fields.some(f => f.symbol_id === fieldSymbolId)) {
                  const nameChain = [typeRecord.name, child.name, field.name];
                  const fieldType = getFieldType(field, nameChain);
                  typeDetails.fields.push({
                    name: field.name,
                    field_type: fieldType || null,
                    mutability: inferMutability(field),
                    visibility: inferVisibility(field.name),
                    symbol_id: fieldSymbolId
                  });
                }
              }
            }
          }
        } else if (childKind === SymbolKind.Variable) {
          // Direct class-level variables (rare in Python but possible)
          const fieldSymbolId = this.createSymbolId(filePath, [typeRecord], child, childKind);
          if (!typeDetails.fields.some(f => f.symbol_id === fieldSymbolId)) {
            const nameChain = [typeRecord.name, child.name];
            const fieldType = getFieldType(child, nameChain);
            typeDetails.fields.push({
              name: child.name,
              field_type: fieldType || null,
              mutability: inferMutability(child),
              visibility: inferVisibility(child.name),
              symbol_id: fieldSymbolId
            });
          }
        }
      }
    };
    
    collectFieldsFromInit(children);
  }

  private mapSymbolKind(kind: LspSymbolKind): SymbolKind | null {
    switch (kind) {
      case LspSymbolKind.Function:
      case LspSymbolKind.Method:
      case LspSymbolKind.Constructor:
        return SymbolKind.Function;
      case LspSymbolKind.Class:
      case LspSymbolKind.Interface:
        return SymbolKind.Type;
      case LspSymbolKind.Variable:
      case LspSymbolKind.Field:
      case LspSymbolKind.Constant:
      case LspSymbolKind.Property:
        return SymbolKind.Variable;
      default:
        return null;
    }
  }

  private extractModuleName(filePath: string): string {
    return relativePath(this.options.projectRoot, filePath)
      .replace(/\.py$/, "")
      .replace(/[\\/]/g, ".");
  }

  private createSymbolId(filePath: string, parents: SymbolRecord[], symbol: DocumentSymbol, kind: SymbolKind): string {
    const moduleName = this.extractModuleName(filePath);
    
    // Build the name chain: module.ParentClass.method
    const nameChain: string[] = [moduleName];
    
    // Add parent names (both Types and Functions for nested definitions)
    // This ensures local classes inside functions get unique IDs like main.func.LocalClass#Type
    for (const parent of parents) {
      if (parent.kind === SymbolKind.Type || parent.kind === SymbolKind.Function) {
        nameChain.push(parent.name);
      }
    }
    
    // Add the current symbol name (remove parentheses for functions)
    const cleanName = symbol.name.replace(/\(.*/, "");
    nameChain.push(cleanName);
    
    const baseName = nameChain.join(".");
    const suffix = kind;
    return `${baseName}#${suffix}`;
  }

  // Create symbol ID for class fields (in __init__), excludes Function parents
  private createFieldSymbolId(filePath: string, parents: SymbolRecord[], symbol: DocumentSymbol): string {
    const moduleName = this.extractModuleName(filePath);
    
    // Build the name chain: module.ParentClass (only Types, not Functions)
    const nameChain: string[] = [moduleName];
    
    // Only add Type parents, skip Function parents (like __init__)
    for (const parent of parents) {
      if (parent.kind === SymbolKind.Type) {
        nameChain.push(parent.name);
      }
    }
    
    // Add the field name
    nameChain.push(symbol.name);
    
    const baseName = nameChain.join(".");
    return `${baseName}#Variable`;
  }

  // Parse class inheritance from source code
  // Returns { inherits: [...], implements: [...] }
  private parseClassInheritance(
    filePath: string,
    symbol: DocumentSymbol,
    typeKind: TypeKind
  ): { inherits: string[]; implements: string[] } {
    const content = this.fileContents.get(filePath);
    if (!content) {
      return { inherits: [], implements: [] };
    }

    const lines = content.split(/\r?\n/);
    const classLine = lines[symbol.range.start.line];
    if (!classLine) {
      return { inherits: [], implements: [] };
    }

    // Match class definition: class ClassName(Base1, Base2):
    // Handle multi-line class definitions by looking at the start line
    const classMatch = classLine.match(/class\s+\w+\s*\(([^)]*)\)/);
    if (!classMatch) {
      // No explicit bases - inherit from object in Python 3
      return { inherits: [], implements: [] };
    }

    const baseList = classMatch[1];
    if (!baseList.trim()) {
      return { inherits: [], implements: [] };
    }

    // Parse base classes
    // Split by comma, handling potential generic types like List[str]
    const bases = splitParams(baseList).map(b => b.trim()).filter(b => b);
    
    const inherits: string[] = [];
    const implementsList: string[] = [];

    for (const base of bases) {
      // Extract just the type name (handle generics like Reader[T])
      const baseNameMatch = base.match(/^(\w+)/);
      if (!baseNameMatch) continue;
      
      const baseName = baseNameMatch[1];
      
      // Resolve to full symbol ID if it's a user-defined type in current module
      const moduleName = this.extractModuleName(filePath);
      const resolvedType = this.resolveTypeRef(baseName, moduleName);
      
      // If resolved to a symbol ID (contains #), use it; otherwise keep original
      const typeRef = resolvedType.includes("#") ? resolvedType : baseName;
      
      // In Python, Protocol is treated as Interface (implements relationship)
      // Regular classes go to inherits, Protocol classes go to implements
      if (baseName === "Protocol" || this.isProtocolClass(typeRef)) {
        implementsList.push(typeRef);
      } else {
        inherits.push(typeRef);
      }
    }

    return { inherits, implements: implementsList };
  }

  // Check if a type is a Protocol class
  private isProtocolClass(symbolId: string): boolean {
    return this.protocolClasses.has(symbolId);
  }

  // Create Type definition (first pass)
  private createTypeDefinition(
    filePath: string,
    symbolId: string,
    symbol: DocumentSymbol,
    enclosingSymbol: string | null,
    hover?: Hover
  ): SymbolDefinition {
    const relPath = relativePath(this.options.projectRoot, filePath);
    const location = {
      file_path: relPath,
      line: symbol.range.start.line,
      column: symbol.range.start.character
    };
    const span = {
      start_line: symbol.range.start.line,
      start_column: symbol.range.start.character,
      end_line: symbol.range.end.line,
      end_column: symbol.range.end.character
    };

    const { documentation } = extractHoverInfo(hover);
    const typeKind = this.inferTypeKind(symbolId, symbol, hover);
    
    // Parse inheritance from source code
    const inheritance = this.parseClassInheritance(filePath, symbol, typeKind);
    
    const details: TypeDetails = {
      kind: typeKind,
      is_abstract: typeKind === TypeKind.Interface || this.isAbstractType(symbol),
      is_final: false,
      visibility: inferVisibility(symbol.name),
      type_params: [],
      fields: [],
      inherits: inheritance.inherits,
      implements: inheritance.implements
    };
    
    return {
      symbol_id: symbolId,
      kind: SymbolKind.Type,
      name: symbol.name,
      display_name: symbol.detail || symbol.name,
      location,
      span,
      enclosing_symbol: enclosingSymbol,
      is_external: false,
      documentation,
      details: { Type: details }
    };
  }

  // Create Function definition with resolved type references (second pass)
  private async createFunctionDefinition(
    filePath: string,
    uri: string,
    symbolId: string,
    symbol: DocumentSymbol,
    enclosingSymbol: string | null,
    hover?: Hover
  ): Promise<SymbolDefinition> {
    const relPath = relativePath(this.options.projectRoot, filePath);
    const location = {
      file_path: relPath,
      line: symbol.range.start.line,
      column: symbol.range.start.character
    };
    const span = {
      start_line: symbol.range.start.line,
      start_column: symbol.range.start.character,
      end_line: symbol.range.end.line,
      end_column: symbol.range.end.character
    };

    const { documentation, signature } = extractHoverInfo(hover);
    
    // Parse signature to get raw type names
    const { parameters: rawParameters, returnTypes: rawReturnTypes } = 
      parseSignature(signature || symbol.detail || "");
    
    // Resolve type references to fully qualified symbol IDs
    // Pass the function's symbol ID to exclude locally shadowed types
    const moduleName = this.extractModuleName(filePath);
    const excludedTypes = this.localTypesByFunction.get(symbolId) ?? new Set();
    
    const parameters: Parameter[] = [];
    for (const param of rawParameters) {
      const resolvedType = param.param_type 
        ? this.resolveTypeRef(param.param_type, moduleName, excludedTypes)
        : null;
      parameters.push({
        ...param,
        param_type: resolvedType
      });
    }
    
    const returnTypes: string[] = [];
    for (const returnType of rawReturnTypes) {
      const resolvedType = this.resolveTypeRef(returnType, moduleName, excludedTypes);
      returnTypes.push(resolvedType);
    }

    const isAbstract = this.isAbstractFromHover(hover) || 
                       this.inferAbstractFromDetail(symbol.detail) ||
                       this.isMethodOfProtocol(enclosingSymbol);
    
    const details: FunctionDetails = {
      parameters,
      return_types: returnTypes,
      type_params: [],
      modifiers: {
        ...this.inferFunctionModifiers(symbol),
        is_abstract: isAbstract
      }
    };
    
    return {
      symbol_id: symbolId,
      kind: SymbolKind.Function,
      name: symbol.name.replace(/\(.*/, ""),
      display_name: signature || symbol.detail || symbol.name,
      location,
      span,
      enclosing_symbol: enclosingSymbol,
      is_external: false,
      documentation,
      details: { Function: details }
    };
  }

  // Create Variable definition (second pass)
  private createVariableDefinition(
    filePath: string,
    symbolId: string,
    symbol: DocumentSymbol,
    enclosingSymbol: string | null,
    hover?: Hover
  ): SymbolDefinition {
    const relPath = relativePath(this.options.projectRoot, filePath);
    const location = {
      file_path: relPath,
      line: symbol.range.start.line,
      column: symbol.range.start.character
    };
    const span = {
      start_line: symbol.range.start.line,
      start_column: symbol.range.start.character,
      end_line: symbol.range.end.line,
      end_column: symbol.range.end.character
    };

    const { documentation, signature } = extractHoverInfo(hover);
    const currentModule = this.extractModuleName(filePath);
    const lines = this.getFileLines(filePath);
    
    // Prefer declared type annotation from source code over inferred type from hover
    const declaredType = this.extractDeclaredType(lines, symbol.selectionRange, currentModule);
    const inferredType = this.extractVariableType(signature, currentModule);
    const varType = declaredType || inferredType;

    const varDetails: VariableDetails = {
      var_type: varType || undefined,
      mutability: inferMutability(symbol),
      scope: enclosingSymbol ? VariableScope.Field : VariableScope.Global,
      visibility: inferVisibility(symbol.name)
    };

    return {
      symbol_id: symbolId,
      kind: SymbolKind.Variable,
      name: symbol.name,
      display_name: symbol.detail || symbol.name,
      location,
      span,
      enclosing_symbol: enclosingSymbol,
      is_external: false,
      documentation,
      details: { Variable: varDetails }
    };
  }

  // Resolve a type reference to a fully qualified symbol ID
  // Uses heuristic matching against known types in the current module
  // excludedTypes: set of type names that should not be resolved (locally shadowed)
  private resolveTypeRef(
    typeName: string,
    currentModule: string,
    excludedTypes: Set<string> = new Set()
  ): string {
    // For builtin types, return as-is
    if (isBuiltinType(typeName)) {
      return typeName;
    }

    // For generic types like "List[Reader]", try to resolve the parameter
    const genericMatch = typeName.match(/^(\w+)\[(.+)]$/);
    if (genericMatch) {
      const baseType = genericMatch[1];
      const paramType = genericMatch[2].trim();
      const resolvedParam = this.resolveTypeRef(paramType, currentModule, excludedTypes);
      return `${baseType}[${resolvedParam}]`;
    }

    // For union types, return as-is (too complex to resolve each part)
    if (typeName.includes("|") || typeName.startsWith("Union[")) {
      return typeName;
    }

    // Check if this type is excluded (locally shadowed)
    if (excludedTypes.has(typeName)) {
      // Return original name - the type reference refers to the local type
      return typeName;
    }

    // Check if this type matches a known type definition in the current module
    // Build the expected symbol ID for a type in the current module
    const possibleSymbolId = `${currentModule}.${typeName}#Type`;
    
    // Check if we have a type definition with this symbol ID
    const matchingType = this.definitions.find(
      d => d.symbol_id === possibleSymbolId && d.kind === SymbolKind.Type
    );
    
    if (matchingType) {
      return matchingType.symbol_id;
    }

    // Not found in current module, return original name
    // (could be an import from another module, external type, etc.)
    return typeName;
  }

  /**
   * Extract declared type annotation from source code.
   * Patterns: "name: Type" or "name: Type = value"
   * Returns null if no explicit type annotation is found.
   */
  private extractDeclaredType(lines: string[], range: Range, currentModule: string): string | null {
    const line = lines[range.start.line] ?? "";
    // Look for pattern: name: Type or name: Type = value
    // Match from the variable name position to find ": Type"
    const afterName = line.slice(range.end.character);
    
    // Pattern: ": Type" possibly followed by "=" or end of statement
    const typeMatch = afterName.match(/^\s*:\s*([^=]+?)(?:\s*=|$)/);
    if (typeMatch) {
      const rawType = typeMatch[1].trim();
      if (rawType) {
        return this.resolveTypeRef(rawType, currentModule);
      }
    }
    return null;
  }

  /**
   * Extract variable type from hover signature.
   * Pyright format: "(variable) name: Type" or "(type alias) Type = ..."
   */
  private extractVariableType(signature: string, currentModule: string): string | null {
    if (!signature) return null;

    // Normalize to single line
    const normalized = signature.replace(/\r?\n/g, " ").replace(/\s+/g, " ").trim();

    // Pattern: "(variable) name: Type" or "(constant) name: Type"
    const varMatch = normalized.match(/^\((?:variable|constant)\)\s+\w+:\s*(.+)$/);
    if (varMatch) {
      const rawType = varMatch[1].trim();
      const resolved = this.resolveTypeRef(rawType, currentModule);
      return simplifyLiteralType(resolved);
    }

    // Pattern: just "name: Type" (simpler case)
    const simpleMatch = normalized.match(/^\w+:\s*(.+)$/);
    if (simpleMatch) {
      const rawType = simpleMatch[1].trim();
      const resolved = this.resolveTypeRef(rawType, currentModule);
      return simplifyLiteralType(resolved);
    }

    return null;
  }

  private isAbstractFromHover(hover?: Hover): boolean {
    if (!hover) return false;
    
    const { signature } = extractHoverInfo(hover);
    // In Pyright, abstract methods often show "@abstractmethod" or similar in hover
    return signature.includes("@abstract") || signature.includes("@abstractmethod");
  }

  private isMethodOfProtocol(enclosingSymbol: string | null): boolean {
    // If the enclosing symbol is a Protocol class, the method is abstract
    return enclosingSymbol !== null && this.protocolClasses.has(enclosingSymbol);
  }

  private inferAbstractFromDetail(detail?: string): boolean {
    return detail?.includes("abstract") ?? false;
  }

  private inferFunctionModifiers(symbol: DocumentSymbol): FunctionModifiers {
    const name = symbol.name;
    return {
      is_async: symbol.detail?.includes("async") ?? false,
      is_generator: false,
      is_static: name.startsWith("__") && name.endsWith("__"),
      is_abstract: false, // Will be set by caller
      visibility: inferVisibility(name)
    };
  }

  private inferTypeKind(symbolId: string, symbol: DocumentSymbol, hover?: Hover): TypeKind {
    // Check if this is a Protocol class (parsed from source)
    if (this.protocolClasses.has(symbolId)) {
      return TypeKind.Interface;
    }
    
    // Check hover info first
    if (hover) {
      const { signature } = extractHoverInfo(hover);
      if (signature.includes("Protocol") || signature.includes("protocol")) {
        return TypeKind.Interface;
      }
    }
    
    // Check both detail and the original LSP SymbolKind
    if (symbol.detail?.includes("Protocol") || 
        (symbol as any).kind === LspSymbolKind.Interface) {
      return TypeKind.Interface;
    }
    return TypeKind.Class;
  }

  private isAbstractType(symbol: DocumentSymbol): boolean {
    return symbol.detail?.includes("Protocol") ?? false;
  }

  private async collectReferences(): Promise<void> {
    // Only use symbols we actually emit as definitions for enclosing lookup
    const definitionSymbolIds = new Set(this.definitions.map((d) => d.symbol_id));
    this.definitionSymbolIdsForEnclosing = definitionSymbolIds;

    // Build symbol lookup: name -> symbolId -> SymbolRecord
    const symbolByName = new Map<string, SymbolRecord[]>();
    for (const record of this.symbolIndex) {
      if (!definitionSymbolIds.has(record.symbolId)) continue;
      const records = symbolByName.get(record.name) ?? [];
      records.push(record);
      symbolByName.set(record.name, records);
    }

    // Build definition site lookup for filtering out self-references
    // Map from uri -> list of records for fast range-based lookup
    const definitionsByUri = new Map<string, SymbolRecord[]>();
    for (const record of this.symbolIndex) {
      const records = definitionsByUri.get(record.uri) ?? [];
      records.push(record);
      definitionsByUri.set(record.uri, records);
    }

    // Build imported names per file (so we resolve refs to e.g. functools.wraps)
    const importedNamesByFile = new Map<string, Set<string>>();
    for (const [filePath, lines] of this.fileLines) {
      importedNamesByFile.set(filePath, this.collectImportedNames(lines));
    }

    // Collect all identifier positions from all files
    const identifierPositions: Array<{
      uri: string;
      filePath: string;
      name: string;
      position: Position;
      range: Range;
    }> = [];

    for (const [filePath, lines] of this.fileLines) {
      const uri = toUri(filePath);
      const importedNames = importedNamesByFile.get(filePath) ?? new Set();
      for (let lineNum = 0; lineNum < lines.length; lineNum++) {
        const line = lines[lineNum];
        const regex = /\b([a-zA-Z_][a-zA-Z0-9_]*)\b/g;
        let match;
        while ((match = regex.exec(line)) !== null) {
          const name = match[1];
          if (symbolByName.has(name) || importedNames.has(name)) {
            identifierPositions.push({
              uri,
              filePath,
              name,
              position: { line: lineNum, character: match.index },
              range: {
                start: { line: lineNum, character: match.index },
                end: { line: lineNum, character: match.index + name.length }
              }
            });
          }
        }
      }
    }

    this.log(`Found ${identifierPositions.length} potential references to resolve`, true);

    // Resolve each identifier via definition request
    const limit = pLimit(LSP_CONCURRENCY);
    let completed = 0;
    const total = identifierPositions.length;

    this.progress(0, total, "Resolving references");

    await Promise.all(
      identifierPositions.map((pos) =>
        limit(async () => {
          try {
            const definition = await this.fetchDefinition(pos.uri, pos.position);
            if (definition) {
              const targetRecord = this.findSymbolAtLocation(definition);

              if (targetRecord) {
                // Skip if this identifier is at the definition site (not a reference)
                // Exception: Field definitions (Variable with scope=Field) are also Write references
                // Check if the identifier position falls within the selectionRange of ANY definition
                // with the same name (not just the target - LSP may resolve method names to their class)
                const defsInFile = definitionsByUri.get(pos.uri) ?? [];
                const matchingDef = defsInFile.find((def) => {
                  // Only check definitions with the same name
                  if (def.name !== pos.name) return false;
                  // Check if the identifier is on the same line and within the selectionRange
                  const sel = def.selectionRange;
                  if (pos.range.start.line !== sel.start.line) return false;
                  // Check if identifier start is within selectionRange
                  return pos.range.start.character >= sel.start.character &&
                         pos.range.start.character < sel.end.character;
                });
                
                
                if (matchingDef) {
                  // For Field definitions, keep as Write reference (first assignment is both def and write)
                  // For Functions and Types, skip (these are pure definitions)
                  if (matchingDef.kind !== SymbolKind.Variable) {
                    completed++;
                    return;
                  }
                  // For Variables (fields), we continue but use matchingDef as the actual target
                  // since LSP may return the enclosing class type instead of the field
                }

                // Find enclosing symbol for this reference
                const enclosing = this.findEnclosingSymbol({
                  uri: pos.uri,
                  range: pos.range
                } as Location);

                if (enclosing) {
                  const lines = this.fileLines.get(pos.filePath) ?? [];
                  const receiver = extractReceiver(lines, pos.range);
                  
                  // Determine the actual target symbol
                  let actualTarget = targetRecord.symbolId;
                  let actualTargetKind = targetRecord.kind;
                  
                  // If this is a field definition site, use the field as target
                  if (matchingDef && matchingDef.kind === SymbolKind.Variable) {
                    actualTarget = matchingDef.symbolId;
                    actualTargetKind = matchingDef.kind;
                  }
                  // If there's a receiver (e.g., "self.encoding" or "reader.read"), 
                  // LSP may return the type definition instead of the member.
                  // We need to look up the actual member (field or method) on the type.
                  else if (receiver && targetRecord.kind === SymbolKind.Type) {
                    // Look for a member of this type with the name we're referencing
                    const memberSymbolId = this.findMemberSymbol(targetRecord.symbolId, pos.name);
                    if (memberSymbolId) {
                      actualTarget = memberSymbolId;
                      // Update the kind as well
                      const memberRecord = this.symbolIndex.find(r => r.symbolId === memberSymbolId);
                      if (memberRecord) actualTargetKind = memberRecord.kind;
                    }
                  }
                  
                  // Get the actual target record for role inference
                  const actualTargetRecord = this.symbolIndex.find(r => r.symbolId === actualTarget) ?? targetRecord;
                  const role = inferReferenceRole(lines, pos.range, actualTargetRecord.kind);

                  // Skip type-in-signature (param/return/inherits) - no reference emitted
                  if (role === null) {
                    completed++;
                    return;
                  }

                  // Skip Decorate refs when decorator is for nested def/class (we don't track those)
                  if (role === ReferenceRole.Decorate && isDecoratorForNestedDef(lines, pos.range.start.line)) {
                    completed++;
                    return;
                  }

                  // Skip type refs on def/class line (param types, return type, base classes)
                  if (enclosing.kind === SymbolKind.Function) {
                    const defLine = findDefLine(lines, enclosing.range.start.line);
                    if (pos.range.start.line === defLine && actualTargetRecord.kind === SymbolKind.Type) {
                      completed++;
                      return;
                    }
                  } else if (enclosing.kind === SymbolKind.Type) {
                    const classLine = findClassLine(lines, enclosing.range.start.line);
                    if (pos.range.start.line === classLine && actualTargetRecord.kind === SymbolKind.Type) {
                      completed++;
                      return;
                    }
                  }

                  // Class decorator: use __init__ as enclosing_symbol (Decorate semantics)
                  let enclosingSymbolId = enclosing.symbolId;
                  if (role === ReferenceRole.Decorate && enclosing.kind === SymbolKind.Type) {
                    const initId = this.findInitSymbol(enclosing.symbolId);
                    if (initId) enclosingSymbolId = initId;
                  }

                  this.references.push({
                    target_symbol: actualTarget,
                    enclosing_symbol: enclosingSymbolId,
                    role,
                    receiver,
                    location: {
                      file_path: relativePath(this.options.projectRoot, pos.filePath),
                      line: pos.range.start.line,
                      column: pos.range.start.character
                    }
                  });
                }
              } else {
                // Definition not in our symbol index: external (e.g. functools.wraps) or at import line
                try {
                  const defPath = fileURLToPath(definition.uri);
                  const defPathResolved = path.resolve(defPath);
                  const projectFilePaths = new Set(
                    Array.from(this.fileLines.keys()).map((f) => path.resolve(f))
                  );
                  const isOutsideProject = !projectFilePaths.has(defPathResolved);
                  let defLine = "";
                  if (projectFilePaths.has(defPathResolved)) {
                    const key = Array.from(this.fileLines.keys()).find(
                      (k) => path.resolve(k) === defPathResolved
                    );
                    if (key) {
                      defLine =
                        this.fileLines.get(key)?.[definition.range.start.line] ??
                        "";
                    }
                  }
                  const isImportLine =
                    /^\s*from\s+\S+\s+import/.test(defLine) ||
                    /^\s*import\s+/.test(defLine);
                  if (isOutsideProject || isImportLine) {
                    let externalSymbolId: string;
                    if (isImportLine) {
                      const fromMatch = defLine.match(/^\s*from\s+(\S+)\s+import/);
                      const module = fromMatch
                        ? fromMatch[1]
                        : path.basename(defPath, path.extname(defPath));
                      externalSymbolId = `${module}.${pos.name}#Function`;
                    } else {
                      externalSymbolId = this.inferExternalSymbolId(
                        definition.uri,
                        pos.name
                      );
                    }
                    this.ensureExternalSymbol(externalSymbolId, pos.name);
                    const enclosing = this.findEnclosingSymbol({
                      uri: pos.uri,
                      range: pos.range
                    } as Location);
                    if (enclosing) {
                      const lines = this.fileLines.get(pos.filePath) ?? [];
                      const role = inferReferenceRole(
                        lines,
                        pos.range,
                        SymbolKind.Function
                      );
                      if (role !== null &&
                          !(role === ReferenceRole.Decorate && isDecoratorForNestedDef(lines, pos.range.start.line))) {
                        this.references.push({
                          target_symbol: externalSymbolId,
                          enclosing_symbol: enclosing.symbolId,
                          role,
                          receiver: undefined,
                          location: {
                            file_path: relativePath(
                              this.options.projectRoot,
                              pos.filePath
                            ),
                            line: pos.range.start.line,
                            column: pos.range.start.character
                          }
                        });
                      }
                    }
                  }
                } catch {
                  // Ignore URI parse errors
                }
              }
            } else {
              // LSP returned no definition (e.g. stdlib); if name is imported, create external ref
              const importedNames = importedNamesByFile.get(pos.filePath);
              if (importedNames?.has(pos.name)) {
                const enclosing = this.findEnclosingSymbol({
                  uri: pos.uri,
                  range: pos.range
                } as Location);
                if (enclosing) {
                  const module = this.getModuleForImportedName(pos.filePath, pos.name);
                  if (module) {
                    const externalSymbolId = `${module}.${pos.name}#Function`;
                    this.ensureExternalSymbol(externalSymbolId, pos.name);
                    const lines = this.fileLines.get(pos.filePath) ?? [];
                    const role = inferReferenceRole(lines, pos.range, SymbolKind.Function);
                    if (role !== null &&
                        !(role === ReferenceRole.Decorate && isDecoratorForNestedDef(lines, pos.range.start.line))) {
                      this.references.push({
                        target_symbol: externalSymbolId,
                        enclosing_symbol: enclosing.symbolId,
                        role,
                        receiver: undefined,
                        location: {
                          file_path: relativePath(this.options.projectRoot, pos.filePath),
                          line: pos.range.start.line,
                          column: pos.range.start.character
                        }
                      });
                    }
                  }
                }
              }
            }
          } catch {
            // Ignore errors
          }

          completed++;
          const updateInterval = Math.max(1, Math.floor(total / 100));
          if (completed % updateInterval === 0 || completed === total || completed === 1) {
            this.progress(completed, total, "Resolving references");
          }
        })
      )
    );
    this.definitionSymbolIdsForEnclosing = null;
  }

  private async fetchDefinition(uri: string, position: Position): Promise<Location | null> {
    try {
      const result = await this.client!.sendRequest<Location | Location[] | null>(
        "textDocument/definition",
        { textDocument: { uri }, position }
      );
      if (Array.isArray(result)) {
        return result[0] ?? null;
      }
      return result;
    } catch {
      return null;
    }
  }

  private findSymbolAtLocation(location: Location): SymbolRecord | undefined {
    const candidates = this.symbolIndexByUri.get(location.uri);
    if (!candidates) return undefined;

    // Find symbol whose selectionRange contains the location
    for (const record of candidates) {
      if (
        rangeContains(record.selectionRange, location.range.start) ||
        (record.selectionRange.start.line === location.range.start.line &&
          record.selectionRange.start.character === location.range.start.character)
      ) {
        return record;
      }
    }
    return undefined;
  }

  private findEnclosingSymbol(location: Location): SymbolRecord | undefined {
    const candidates = this.symbolIndexByUri.get(location.uri);
    if (!candidates) return undefined;

    const allowedIds = this.definitionSymbolIdsForEnclosing;
    let best: SymbolRecord | undefined;
    let bestSize = Infinity;

    for (const record of candidates) {
      if (record.kind === SymbolKind.Variable) continue;
      if (allowedIds && !allowedIds.has(record.symbolId)) continue;

      if (rangeContains(record.range, location.range.start)) {
        const size = rangeSize(record.range);
        if (size < bestSize) {
          best = record;
          bestSize = size;
        }
      }
    }

    return best;
  }

  private getFileLines(filePath: string): string[] {
    // Use cached lines instead of splitting every time
    return this.fileLines.get(filePath) ?? [];
  }

  private findMemberSymbol(typeSymbolId: string, memberName: string): string | null {
    // Look for a member (method or field) of the given type with the given name
    // Symbol IDs follow pattern: module.Type.member#Kind
    // e.g., main.FileReader.encoding#Variable or main.Reader.read#Function
    for (const record of this.symbolIndex) {
      if (record.enclosingSymbol === typeSymbolId && record.name === memberName) {
        return record.symbolId;
      }
    }
    return null;
  }

  /** Find __init__ method symbol ID for a type (for class-decorator enclosing). */
  private findInitSymbol(typeSymbolId: string): string | null {
    return this.findMemberSymbol(typeSymbolId, "__init__");
  }

  /** Collect imported names from file lines (from X import Y, Z; import X as Y). */
  private collectImportedNames(lines: string[]): Set<string> {
    const names = new Set<string>();
    for (const line of lines) {
      const trimmed = line.trim();
      const fromMatch = trimmed.match(/^from\s+\S+\s+import\s+(.+)$/);
      if (fromMatch) {
        for (const part of fromMatch[1].split(",")) {
          const asMatch = part.trim().match(/^(\w+)(?:\s+as\s+(\w+))?$/);
          if (asMatch) names.add(asMatch[2] ?? asMatch[1]);
        }
        continue;
      }
      const importMatch = trimmed.match(/^import\s+(.+)$/);
      if (importMatch) {
        for (const part of importMatch[1].split(",")) {
          const asMatch = part.trim().match(/^(\w+)(?:\s+as\s+(\w+))?$/);
          if (asMatch) names.add(asMatch[2] ?? asMatch[1]);
        }
      }
    }
    return names;
  }

  /** Get module name for an imported name (from X import Y -> X for name Y). */
  private getModuleForImportedName(filePath: string, name: string): string | null {
    const lines = this.fileLines.get(filePath) ?? [];
    for (const line of lines) {
      const fromMatch = line.trim().match(/^from\s+(\S+)\s+import\s+(.+)$/);
      if (fromMatch) {
        const imports = fromMatch[2].split(",").map((p) => p.trim().match(/^(\w+)(?:\s+as\s+(\w+))?$/));
        for (const m of imports) {
          if (m && (m[2] ?? m[1]) === name) return fromMatch[1];
        }
      }
    }
    return null;
  }

  /** Infer external symbol ID from definition URI and name (e.g. functools.wraps#Function). */
  private inferExternalSymbolId(definitionUri: string, name: string): string {
    try {
      const defPath = fileURLToPath(definitionUri);
      const base = path.basename(defPath, path.extname(defPath));
      const module = base === "typing" || /^[a-z_]+$/.test(base) ? base : "external";
      return `${module}.${name}#Function`;
    } catch {
      return `external.${name}#Function`;
    }
  }

  /** Ensure external symbol exists in externalSymbols (minimal stub for reference target). */
  private ensureExternalSymbol(symbolId: string, name: string): void {
    if (this.externalSymbolIds.has(symbolId)) return;
    this.externalSymbolIds.add(symbolId);
    const [displayName] = symbolId.split("#");
    const lastDot = displayName.lastIndexOf(".");
    const shortName = lastDot >= 0 ? displayName.slice(lastDot + 1) : displayName;
    this.externalSymbols.push({
      symbol_id: symbolId,
      kind: SymbolKind.Function,
      name: shortName,
      display_name: displayName,
      location: { file_path: "", line: 0, column: 0 },
      span: { start_line: 0, start_column: 0, end_line: 0, end_column: 0 },
      enclosing_symbol: null,
      is_external: true,
      documentation: [],
      details: {
        Function: {
          parameters: [],
          return_types: [],
          type_params: [],
          modifiers: {
            is_async: false,
            is_generator: false,
            is_static: false,
            is_abstract: false,
            visibility: "Public" as const
          }
        }
      }
    });
  }

  private groupByDocument() {
    const docsMap = new Map<string, { definitions: SymbolDefinition[]; references: SymbolReference[] }>();
    for (const def of this.definitions) {
      const file = def.location.file_path;
      if (!docsMap.has(file)) {
        docsMap.set(file, { definitions: [], references: [] });
      }
      docsMap.get(file)!.definitions.push(def);
    }
    for (const ref of this.references) {
      const file = ref.location.file_path;
      if (!docsMap.has(file)) {
        docsMap.set(file, { definitions: [], references: [] });
      }
      docsMap.get(file)!.references.push(ref);
    }
    return Array.from(docsMap.entries()).map(([file, data]) => ({
      relative_path: file,
      language: "python",
      definitions: data.definitions,
      references: data.references
    }));
  }
}
