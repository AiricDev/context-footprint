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
  TextDocumentItem
} from "vscode-languageserver-protocol";
import { ExtractorBase, ExtractOptions } from "../../core/extractor-base";
import { discoverFiles, relativePath, toUri } from "../../core/utils";
import { LspClientOptions } from "../../core/lsp-client";
import {
  FunctionDetails,
  FunctionModifiers,
  Mutability,
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
}

export class PythonExtractor extends ExtractorBase {
  private fileContents = new Map<string, string>();
  private symbolIndex: SymbolRecord[] = [];
  private definitions: SymbolDefinition[] = [];
  private references: SymbolReference[] = [];

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
      this.processDocumentSymbols(file, uri, docSymbols);
    }

    await this.collectReferences();

    const documents = this.groupByDocument();

    return {
      project_root: this.options.projectRoot,
      documents,
      external_symbols: []
    };
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

  private processDocumentSymbols(filePath: string, uri: string, symbols: DocumentSymbol[], parents: string[] = []): void {
    for (const symbol of symbols) {
      const symbolId = this.createSymbolId(filePath, parents, symbol);
      const enclosing = parents.at(-1) ?? null;
      const kind = this.mapSymbolKind(symbol.kind);

      if (!kind) {
        // skip unsupported kinds
        continue;
      }

      this.symbolIndex.push({
        symbolId,
        uri,
        range: symbol.range,
        selectionRange: symbol.selectionRange ?? symbol.range,
        kind,
        enclosingSymbol: enclosing
      });

      const definition = this.createDefinition(filePath, symbolId, symbol, kind, enclosing);
      this.definitions.push(definition);

      const childParents = [...parents, symbolId];
      if (symbol.children) {
        this.processDocumentSymbols(filePath, uri, symbol.children, childParents);
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

  private createSymbolId(filePath: string, parents: string[], symbol: DocumentSymbol): string {
    const moduleName = relativePath(this.options.projectRoot, filePath)
      .replace(/\.py$/, "")
      .replace(/[\\/]/g, ".");
    const nameChain = parents.map((p) => p.split("#")[0].split(".").pop()).filter(Boolean);
    nameChain.push(symbol.name.replace(/\(.*/, ""));
    const baseName = [moduleName, ...nameChain].filter(Boolean).join(".");
    const suffix = this.mapSymbolKind(symbol.kind) ?? SymbolKind.Variable;
    return `${baseName}#${suffix}`;
  }

  private createDefinition(
    filePath: string,
    symbolId: string,
    symbol: DocumentSymbol,
    kind: SymbolKind,
    enclosingSymbol: string | null
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
    const documentation: string[] = [];

    if (kind === SymbolKind.Function) {
      const details: FunctionDetails = {
        parameters: this.extractParameters(symbol.detail ?? ""),
        return_types: this.extractReturnTypes(symbol.detail ?? ""),
        type_params: [],
        modifiers: this.inferFunctionModifiers(symbol)
      };
      return {
        symbol_id: symbolId,
        kind,
        name: symbol.name,
        display_name: symbol.detail ?? symbol.name,
        location,
        span,
        enclosing_symbol: enclosingSymbol,
        is_external: false,
        documentation,
        details: { Function: details }
      };
    }

    if (kind === SymbolKind.Type) {
      const details: TypeDetails = {
        kind: this.inferTypeKind(symbol),
        is_abstract: this.isAbstractType(symbol),
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
        display_name: symbol.detail ?? symbol.name,
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
      display_name: symbol.detail ?? symbol.name,
      location,
      span,
      enclosing_symbol: enclosingSymbol,
      is_external: false,
      documentation,
      details: { Variable: varDetails }
    };
  }

  private extractParameters(detail: string) {
    const match = detail.match(/\((.*)\)/);
    if (!match) return [];
    const params = match[1]
      .split(",")
      .map((p) => p.trim())
      .filter((p) => p && p !== "self" && p !== "cls");
    return params.map((name) => ({
      name: name.split(":")[0].trim(),
      param_type: undefined,
      has_default: name.includes("="),
      is_variadic: name.startsWith("*")
    }));
  }

  private extractReturnTypes(detail: string) {
    const match = detail.match(/->\s*([^:]+)$/);
    if (!match) return [];
    return [match[1].trim()];
  }

  private inferFunctionModifiers(symbol: DocumentSymbol): FunctionModifiers {
    const name = symbol.name;
    return {
      is_async: symbol.detail?.includes("async") ?? false,
      is_generator: false,
      is_static: name.startsWith("__") && name.endsWith("__"),
      is_abstract: symbol.detail?.includes("abstract") ?? false,
      visibility: this.inferVisibility(name)
    };
  }

  private inferTypeKind(symbol: DocumentSymbol): TypeKind {
    if (symbol.detail?.includes("Protocol")) {
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
