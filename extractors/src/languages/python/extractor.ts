import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";
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
  private symbolIndex: SymbolRecord[] = [];
  private definitions: SymbolDefinition[] = [];
  private references: SymbolReference[] = [];
  private fileContents = new Map<string, string>();
  private symbolIndex: SymbolRecord[] = [];
  private definitions: SymbolDefinition[] = [];
  private references: SymbolReference[] = [];
  // Cache hover by "uri#line,col" for reliable lookup
  private hoverCache = new Map<string, Hover>();

  constructor(options: ExtractOptions) {
    super(options);
  }

  protected getLspOptions(): LspClientOptions {
    return {
      command: process.execPath,
      args: [pyrightLangServer, "--stdio"],
      rootUri: toUri(this.options.projectRoot)
    };
  }

  protected async collectSemanticData(): Promise<SemanticData> {
    const files = await discoverFiles({
      cwd: this.options.projectRoot,
      patterns: ["**/*.py"],
      ignore: ["**/tests/**", "**/__pycache__/**", "**/.venv/**", "**/venv/**", ...(this.options.exclude ?? [])]
    });

    await this.openDocuments(files);

    for (const file of files) {
      const uri = toUri(file);
      const docSymbols = await this.fetchDocumentSymbols(uri);
      // Build hierarchy from flat list using ranges
      const hierarchy = this.buildHierarchyFromFlatList(docSymbols);
      // Pre-fetch hover info for all symbols in the file
      await this.prefetchHoverInfo(uri, hierarchy);
      this.processDocumentSymbols(file, uri, hierarchy, []);
    }

    await this.collectReferences();

    const documents = this.groupByDocument();

    return {
      project_root: this.options.projectRoot,
      documents,
      external_symbols: []
    };
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
    for (const file of files) {
      const content = fs.readFileSync(file, "utf8");
      this.fileContents.set(file, content);
      const uri = toUri(file);
      const item: TextDocumentItem = {
        uri,
        languageId: "python",
        version: 1,
        text: content
      };
      await this.client!.sendNotification("textDocument/didOpen", { textDocument: item });
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

  private async prefetchHoverInfo(uri: string, symbols: DocumentSymbol[], parents: DocumentSymbol[] = []): Promise<void> {
    for (const symbol of symbols) {
      // Build full path for this symbol
      const nameChain = [...parents.map(p => p.name), symbol.name];
      const cacheKey = `${uri}#${nameChain.join(".")}`;
      
      if (!this.hoverCache.has(cacheKey)) {
        try {
          const hover = await this.client!.sendRequest<Hover>("textDocument/hover", {
            textDocument: { uri },
            position: symbol.selectionRange?.start ?? symbol.range.start
          });
          if (hover) {
            this.hoverCache.set(cacheKey, hover);
          }
        } catch {
          // Ignore hover errors
        }
      }
      
      if (symbol.children) {
        await this.prefetchHoverInfo(uri, symbol.children, [...parents, symbol]);
      }
    }
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
    
    for (const child of children) {
      const kind = this.mapSymbolKind(child.kind);
      if (kind === SymbolKind.Variable) {
        // This is a field - create FieldInfo
        const fieldSymbolId = this.createSymbolId(filePath, [typeRecord], child, kind);
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
      const isAbstract = this.isAbstractFromHover(hover) || this.inferAbstractFromDetail(symbol.detail);
      
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
      const typeKind = this.inferTypeKind(symbol, hover);
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
        // MarkupContent
        const content = hover.contents as { kind: string; value: string };
        const value = content.value.trim();
        
        // Pyright usually returns signature first, then docstring separated by newline
        const parts = value.split(/\n\n+/);
        if (parts.length > 0) {
          signature = parts[0].trim();
          if (parts.length > 1) {
            documentation.push(parts.slice(1).join("\n\n").trim());
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

    // Parse signature like: (reader: Reader, path: str) -> int
    // or: def process_file(reader: Reader, path: str) -> int
    
    // Remove "def " prefix if present
    const cleanSig = signature.replace(/^def\s+/, "");
    
    // Extract parameters from parentheses
    const paramMatch = cleanSig.match(/\((.*?)\)(?:\s*->\s*(.+))?$/s);
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
        if (!trimmed || trimmed === "self" || trimmed === "cls") {
          continue;
        }

        // Parse parameter: name: type = default or name: type or name = default or name
        const match = trimmed.match(/^([a-zA-Z_][a-zA-Z0-9_]*)\s*(?::\s*([^=]+))?\s*(?:=\s*(.+))?$/);
        if (match) {
          const name = match[1];
          const paramType = match[2]?.trim() || null;
          const hasDefault = !!match[3];
          const isVariadic = name.startsWith("*") || name.startsWith("**");

          parameters.push({
            name: name.replace(/^\*+/, ""), // Remove * or ** prefix from name
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
    return signature.includes("@abstract") || signature.includes("Protocol");
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

  private inferTypeKind(symbol: DocumentSymbol, hover?: Hover): TypeKind {
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
    for (const record of this.symbolIndex) {
      const position = record.selectionRange.start;
      const locations = await this.fetchReferences(record.uri, position);
      locations.forEach((loc) => {
        const reference = this.createReference(record.symbolId, loc);
        if (reference) {
          this.references.push(reference);
        }
      });
    }
  }

  private async fetchReferences(uri: string, position: Position): Promise<Location[]> {
    try {
      const result = await this.client!.sendRequest<Location[]>("textDocument/references", {
        textDocument: { uri },
        position,
        context: { includeDeclaration: false }
      });
      return result ?? [];
    } catch {
      return [];
    }
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
    // Find deepest symbol whose range contains location
    return this.symbolIndex
      .filter((record) => record.uri === location.uri && this.rangeContains(record.range, location.range.start))
      .sort((a, b) => this.rangeSize(a.range) - this.rangeSize(b.range))[0];
  }

  private getFileLines(filePath: string): string[] {
    const content = this.fileContents.get(filePath);
    if (!content) return [];
    return content.split(/\r?\n/);
  }

  private inferReferenceRole(lines: string[], range: Range): ReferenceRole {
    const line = lines[range.start.line] ?? "";
    const after = line.slice(range.end.character).trimStart();
    if (after.startsWith("(")) {
      return ReferenceRole.Call;
    }
    const before = line.slice(0, range.start.character).trimEnd();
    if (before.endsWith("=") || before.endsWith("+=") || before.endsWith("-=") || before.endsWith("*=") || before.endsWith("/=")) {
      return ReferenceRole.Write;
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
