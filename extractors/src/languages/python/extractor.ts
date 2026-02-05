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
  Mutability,
  Parameter,
  ReferenceRole,
  SemanticData,
  SymbolDefinition,
  SymbolKind,
  SymbolReference,
  TypeDetails,
  TypeKind,
  VariableDetails,
  VariableScope,
  Visibility
} from "../../core/types";

const pyrightLangServer = require.resolve("pyright/langserver.index.js");

// Concurrency limits for LSP requests
const LSP_CONCURRENCY = 10;
const FILE_READ_CONCURRENCY = 50;

interface SymbolRecord {
  symbolId: string;
  uri: string;
  range: Range;
  selectionRange: Range;
  kind: SymbolKind;
  enclosingSymbol: string | null;
  name: string;
  detail?: string;
  children?: DocumentSymbol[];
}

interface SymbolInfo {
  name: string;
  kind: SymbolKind;
  detail?: string;
  documentation: string[];
  parameters: Parameter[];
  returnTypes: string[];
  isAbstract: boolean;
}

export class PythonExtractor extends ExtractorBase {
  private fileContents = new Map<string, string>();
  private fileLines = new Map<string, string[]>(); // Cached line arrays
  private symbolIndex: SymbolRecord[] = [];
  private symbolIndexByUri = new Map<string, SymbolRecord[]>(); // Index by URI for fast lookup
  private definitions: SymbolDefinition[] = [];
  private references: SymbolReference[] = [];
  private hoverCache = new Map<string, Hover>();
  private protocolClasses = new Set<string>();

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
      external_symbols: []
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

    // Third pass: process symbols (synchronous, builds definitions)
    this.log("Processing symbols...", true);
    completed = 0;
    const totalFiles = symbolsByFile.size;
    for (const [file, hierarchy] of symbolsByFile) {
      const uri = toUri(file);
      this.processDocumentSymbols(file, uri, hierarchy, []);
      completed++;
      if (completed % 10 === 0 || completed === totalFiles) {
        this.progress(completed, totalFiles, "Processing symbols");
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

            if (
              symbol.selectionRange?.start.line === symbol.range.start.line &&
              symbol.selectionRange?.start.character === symbol.range.start.character
            ) {
              const lineText = lines[symbol.range.start.line] ?? "";
              const namePos = lineText.indexOf(symbol.name, symbol.range.start.character);
              if (namePos >= 0) {
                position = { line: symbol.range.start.line, character: namePos };
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
        if (this.rangeContains(top.symbol.range, symbol.range.start)) {
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

  private processDocumentSymbols(
    filePath: string, 
    uri: string, 
    symbols: DocumentSymbol[], 
    parents: SymbolRecord[] = []
  ): void {
    // Collect parameter info from Function parents to filter them out
    const parentFunction = parents.find(p => p.kind === SymbolKind.Function);
    const isInInit = parentFunction?.name === "__init__";
    
    // Build a set of (name, line) pairs for parameters
    // Parameters are defined on the same line as the function signature
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
        // Skip unsupported kinds but still process children
        if (symbol.children) {
          this.processDocumentSymbols(filePath, uri, symbol.children, parents);
        }
        continue;
      }

      // Skip local variables and parameters according to types.ts
      if (kind === SymbolKind.Variable && parentFunction) {
        const paramKey = `${symbol.name}@${symbol.range.start.line}`;
        if (isInInit) {
          // In __init__, variables on the function signature line are parameters
          // Variables on other lines are class fields (keep them)
          if (paramKeys.has(paramKey)) {
            continue; // Skip parameters
          }
          // Keep class fields (will be processed below)
        } else {
          // In other functions, skip all variables (local variables)
          continue;
        }
      }

      // Build proper symbol ID with full hierarchy
      const symbolId = this.createSymbolId(filePath, parents, symbol, kind);
      
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
      
      const definition = this.createDefinition(filePath, symbolId, symbol, kind, enclosing, hover);
      this.definitions.push(definition);

      // Collect fields for Type definitions
      if (kind === SymbolKind.Type && symbol.children) {
        this.collectTypeFields(
          definition as SymbolDefinition & { kind: typeof SymbolKind.Type }, 
          symbol.children, 
          filePath, 
          record
        );
      }

      // Process children with updated parent chain
      if (symbol.children) {
        const childParents = [...parents, record];
        this.processDocumentSymbols(filePath, uri, symbol.children, childParents);
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
                  typeDetails.fields.push({
                    name: field.name,
                    field_type: null,
                    mutability: Mutability.Mutable,
                    visibility: this.inferVisibility(field.name),
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
            typeDetails.fields.push({
              name: child.name,
              field_type: null,
              mutability: Mutability.Mutable,
              visibility: this.inferVisibility(child.name),
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

  private createSymbolId(filePath: string, parents: SymbolRecord[], symbol: DocumentSymbol, kind: SymbolKind): string {
    const moduleName = relativePath(this.options.projectRoot, filePath)
      .replace(/\.py$/, "")
      .replace(/[\\/]/g, ".");
    
    // Build the name chain: module.ParentClass.method
    const nameChain: string[] = [moduleName];
    
    // Add parent type names (only Types, not Functions)
    for (const parent of parents) {
      if (parent.kind === SymbolKind.Type) {
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

  private createDefinition(
    filePath: string,
    symbolId: string,
    symbol: DocumentSymbol,
    kind: SymbolKind,
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

    // Extract documentation and type info from hover
    const { documentation, signature } = this.extractHoverInfo(hover);

    if (kind === SymbolKind.Function) {
      const { parameters, returnTypes } = this.parseSignature(signature || symbol.detail || "");
      // Method is abstract if: @abstractmethod decorator, or part of a Protocol class
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
        kind,
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

    if (kind === SymbolKind.Type) {
      const typeKind = this.inferTypeKind(symbolId, symbol, hover);
      const details: TypeDetails = {
        kind: typeKind,
        is_abstract: typeKind === TypeKind.Interface || this.isAbstractType(symbol),
        is_final: false,
        visibility: this.inferVisibility(symbol.name),
        type_params: [],
        fields: [],
        inherits: [],
        implements: []
      };
      return {
        symbol_id: symbolId,
        kind,
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

    const varDetails: VariableDetails = {
      var_type: undefined,
      mutability: Mutability.Mutable,
      scope: enclosingSymbol ? VariableScope.Field : VariableScope.Global,
      visibility: this.inferVisibility(symbol.name)
    };

    return {
      symbol_id: symbolId,
      kind,
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

  private extractHoverInfo(hover?: Hover): { documentation: string[]; signature: string } {
    if (!hover) {
      return { documentation: [], signature: "" };
    }

    const documentation: string[] = [];
    let signature = "";

    if (hover.contents) {
      if (typeof hover.contents === "string") {
        // Plain string content - usually just the signature
        signature = hover.contents.trim();
      } else if (Array.isArray(hover.contents)) {
        // Array of MarkedString
        for (const item of hover.contents) {
          if (typeof item === "string") {
            if (!signature) {
              signature = item.trim();
            } else {
              documentation.push(item.trim());
            }
          } else if (item && typeof item === "object" && "value" in item) {
            if (item.language === "python") {
              if (!signature) {
                signature = item.value.trim();
              }
            } else {
              documentation.push(item.value.trim());
            }
          }
        }
      } else if (typeof hover.contents === "object" && "kind" in hover.contents) {
        // MarkupContent (plaintext or markdown)
        const content = hover.contents as { kind: string; value: string };
        const value = content.value.trim();
        
        // Pyright format: "(type) signature\n\nDocstring"
        // or "(function) def name(...)\n\nDocstring"
        // Split on double newline to separate signature from docs
        const parts = value.split(/\n\n/);
        if (parts.length > 0) {
          signature = parts[0].trim();
          // Everything after the first double-newline is documentation
          if (parts.length > 1) {
            const docText = parts.slice(1).join("\n\n").trim();
            if (docText) {
              documentation.push(docText);
            }
          }
        }
      }
    }

    return { documentation, signature };
  }

  private parseSignature(signature: string): { parameters: Parameter[]; returnTypes: string[] } {
    if (!signature) {
      return { parameters: [], returnTypes: [] };
    }

    // Pyright format can be multiline:
    // "(function) def process_file(\n    reader: Reader,\n    path: str\n) -> int"
    // or "(class) ClassName"
    // or "(method) def read(self, path: str) -> str"
    
    // Normalize to single line by removing newlines and extra spaces
    const normalizedSig = signature
      .replace(/\r?\n/g, " ")
      .replace(/\s+/g, " ")
      .trim();
    
    // Remove type prefix like "(function) ", "(method) ", "(class) "
    const cleanSig = normalizedSig.replace(/^\([^)]+\)\s*/, "");
    
    // Remove "def " prefix if present
    const withoutDef = cleanSig.replace(/^def\s+/, "");
    
    // Extract parameters from parentheses - use greedy match for content
    const paramMatch = withoutDef.match(/\(([^)]*)\)(?:\s*->\s*(.+))?$/);
    if (!paramMatch) {
      return { parameters: [], returnTypes: [] };
    }

    const paramString = paramMatch[1];
    const returnTypeStr = paramMatch[2]?.trim();

    // Parse individual parameters
    const parameters: Parameter[] = [];
    if (paramString.trim()) {
      // Handle nested parentheses in type annotations by simple split on comma
      // This is a simplified parser - real Python signatures can be complex
      const paramParts = this.splitParams(paramString);
      
      for (const param of paramParts) {
        const trimmed = param.trim();
        if (!trimmed) {
          continue;
        }

        // Check for variadic parameters
        const isVariadic = trimmed.startsWith("*") || trimmed.startsWith("**");
        const paramWithoutStars = trimmed.replace(/^\*+/, "");

        // Parse parameter: name: type = default or name: type or name = default or name
        const match = paramWithoutStars.match(/^([a-zA-Z_][a-zA-Z0-9_]*)\s*(?::\s*([^=]+))?\s*(?:=\s*(.+))?$/);
        if (match) {
          const name = match[1];
          
          // Skip self/cls after extracting the name (handles typed forms like "self: Self@FileReader")
          if (name === "self" || name === "cls") {
            continue;
          }
          
          const paramType = match[2]?.trim() || null;
          const hasDefault = !!match[3];

          parameters.push({
            name,
            param_type: paramType,
            has_default: hasDefault,
            is_variadic: isVariadic
          });
        }
      }
    }

    // Parse return types
    const returnTypes: string[] = [];
    if (returnTypeStr) {
      returnTypes.push(returnTypeStr);
    }

    return { parameters, returnTypes };
  }

  private splitParams(paramString: string): string[] {
    const result: string[] = [];
    let current = "";
    let depth = 0;
    
    for (const char of paramString) {
      if (char === "(" || char === "[" || char === "{") {
        depth++;
        current += char;
      } else if (char === ")" || char === "]" || char === "}") {
        depth--;
        current += char;
      } else if (char === "," && depth === 0) {
        result.push(current);
        current = "";
      } else {
        current += char;
      }
    }
    
    if (current.trim()) {
      result.push(current);
    }
    
    return result;
  }

  private isAbstractFromHover(hover?: Hover): boolean {
    if (!hover) return false;
    
    const { signature } = this.extractHoverInfo(hover);
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
      visibility: this.inferVisibility(name)
    };
  }

  private inferTypeKind(symbolId: string, symbol: DocumentSymbol, hover?: Hover): TypeKind {
    // Check if this is a Protocol class (parsed from source)
    if (this.protocolClasses.has(symbolId)) {
      return TypeKind.Interface;
    }
    
    // Check hover info first
    if (hover) {
      const { signature } = this.extractHoverInfo(hover);
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

  private inferVisibility(name: string): Visibility {
    if (name.startsWith("__") && !name.endsWith("__")) return Visibility.Private;
    if (name.startsWith("_")) return Visibility.Private;
    return Visibility.Public;
  }

  private async collectReferences(): Promise<void> {
    // Build symbol lookup: name -> symbolId -> SymbolRecord
    const symbolByName = new Map<string, SymbolRecord[]>();
    for (const record of this.symbolIndex) {
      const records = symbolByName.get(record.name) ?? [];
      records.push(record);
      symbolByName.set(record.name, records);
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
      for (let lineNum = 0; lineNum < lines.length; lineNum++) {
        const line = lines[lineNum];
        // Match Python identifiers
        const regex = /\b([a-zA-Z_][a-zA-Z0-9_]*)\b/g;
        let match;
        while ((match = regex.exec(line)) !== null) {
          const name = match[1];
          // Only consider names that match known symbols
          if (symbolByName.has(name)) {
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
              // Check if definition matches a known symbol
              const targetRecord = this.findSymbolAtLocation(definition);
              if (targetRecord) {
                // Find enclosing symbol for this reference
                const enclosing = this.findEnclosingSymbol({
                  uri: pos.uri,
                  range: pos.range
                } as Location);

                if (enclosing) {
                  const lines = this.fileLines.get(pos.filePath) ?? [];
                  this.references.push({
                    target_symbol: targetRecord.symbolId,
                    enclosing_symbol: enclosing.symbolId,
                    role: this.inferReferenceRole(lines, pos.range),
                    receiver: this.extractReceiver(lines, pos.range),
                    location: {
                      file_path: relativePath(this.options.projectRoot, pos.filePath),
                      line: pos.range.start.line,
                      column: pos.range.start.character
                    }
                  });
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
        this.rangeContains(record.selectionRange, location.range.start) ||
        (record.selectionRange.start.line === location.range.start.line &&
          record.selectionRange.start.character === location.range.start.character)
      ) {
        return record;
      }
    }
    return undefined;
  }

  private createReference(targetSymbolId: string, location: Location): SymbolReference | null {
    const absPath = fileURLToPath(location.uri);
    const relPath = relativePath(this.options.projectRoot, absPath);
    const lines = this.getFileLines(absPath);
    const role = this.inferReferenceRole(lines, location.range);

    const enclosing = this.findEnclosingSymbol(location);
    if (!enclosing) return null;

    return {
      target_symbol: targetSymbolId,
      enclosing_symbol: enclosing.symbolId,
      role,
      receiver: this.extractReceiver(lines, location.range),
      location: {
        file_path: relPath,
        line: location.range.start.line,
        column: location.range.start.character
      }
    };
  }

  private findEnclosingSymbol(location: Location): SymbolRecord | undefined {
    // Use URI index for O(n) -> O(m) lookup where m << n
    const candidates = this.symbolIndexByUri.get(location.uri);
    if (!candidates) return undefined;

    // Find deepest symbol whose range contains location
    let best: SymbolRecord | undefined;
    let bestSize = Infinity;

    for (const record of candidates) {
      if (this.rangeContains(record.range, location.range.start)) {
        const size = this.rangeSize(record.range);
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

  private inferReferenceRole(lines: string[], range: Range): ReferenceRole {
    const line = lines[range.start.line] ?? "";
    const after = line.slice(range.end.character).trimStart();
    
    // Check for function call
    if (after.startsWith("(")) {
      return ReferenceRole.Call;
    }
    
    // Check for assignment - the reference is being assigned to
    // Patterns: "x = ", "x += ", "x[...] = ", "self.x = "
    // After the reference, check for assignment operators
    if (after.match(/^(?:\[[^\]]*\])?\s*(?:=|\+=|-=|\*=|\/=|%=|\|=|&=|\^=|>>=|<<=|\*\*=|\/\/=)/)) {
      return ReferenceRole.Write;
    }
    
    // Also check for augmented assignment where reference is before operator
    const before = line.slice(0, range.start.character).trimEnd();
    if (before.endsWith("=") || before.endsWith("+=") || before.endsWith("-=") || 
        before.endsWith("*=") || before.endsWith("/=")) {
      // This is the right-hand side of an assignment, so it's a Read
      return ReferenceRole.Read;
    }
    
    return ReferenceRole.Read;
  }

  private extractReceiver(lines: string[], range: Range): string | null {
    const line = lines[range.start.line] ?? "";
    const before = line.slice(0, range.start.character);
    const match = before.match(/([a-zA-Z0-9_]+)\.$/);
    return match ? match[1] : null;
  }

  private rangeContains(range: Range, position: Position): boolean {
    if (position.line < range.start.line || position.line > range.end.line) return false;
    if (position.line === range.start.line && position.character < range.start.character) return false;
    if (position.line === range.end.line && position.character > range.end.character) return false;
    return true;
  }

  private rangeSize(range: Range): number {
    return (range.end.line - range.start.line) * 1000 + (range.end.character - range.start.character);
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
