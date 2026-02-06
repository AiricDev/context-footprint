import type { Range, Position } from "vscode-languageserver-protocol";
import { SymbolKind as LspSymbolKind } from "vscode-languageserver-protocol";
import type { DocumentSymbol } from "vscode-languageserver-protocol";
import { Mutability, Visibility } from "../../core/types";
import { SymbolKind } from "../../core/types";
import { relativePath } from "../../core/utils";
import { PYTHON_BUILTIN_TYPES } from "./types";

/**
 * Extract Python module name from file path (relative to project root).
 */
export function extractModuleName(projectRoot: string, filePath: string): string {
  return relativePath(projectRoot, filePath)
    .replace(/\.py$/, "")
    .replace(/[\\/]/g, ".");
}

/**
 * Map LSP SymbolKind to our SymbolKind (Function, Variable, Type).
 */
export function mapSymbolKind(kind: LspSymbolKind): SymbolKind | null {
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

/**
 * Build a hierarchy of DocumentSymbols from a flat list by containment (range).
 * If symbols already have children, returns as-is.
 */
export function buildHierarchyFromFlatList(symbols: DocumentSymbol[]): DocumentSymbol[] {
  if (symbols.some((s) => s.children && s.children.length > 0)) {
    return symbols;
  }

  const sorted = [...symbols].sort((a, b) => {
    if (a.range.start.line !== b.range.start.line) {
      return a.range.start.line - b.range.start.line;
    }
    return a.range.start.character - b.range.start.character;
  });

  const root: DocumentSymbol[] = [];
  const stack: { symbol: DocumentSymbol; children: DocumentSymbol[] }[] = [];

  for (const symbol of sorted) {
    while (stack.length > 0) {
      const top = stack[stack.length - 1];
      if (rangeContains(top.symbol.range, symbol.range.start)) {
        break;
      }
      stack.pop();
    }

    const newSymbol: DocumentSymbol = { ...symbol, children: [] };

    if (stack.length === 0) {
      root.push(newSymbol);
    } else {
      stack[stack.length - 1].children.push(newSymbol);
    }

    stack.push({ symbol: newSymbol, children: newSymbol.children! });
  }

  return root;
}

/**
 * Split a parameter/argument string by top-level commas (respects brackets).
 */
export function splitParams(paramString: string): string[] {
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

export function rangeContains(range: Range, position: Position): boolean {
  if (position.line < range.start.line || position.line > range.end.line) return false;
  if (position.line === range.start.line && position.character < range.start.character) return false;
  if (position.line === range.end.line && position.character > range.end.character) return false;
  return true;
}

export function rangeSize(range: Range): number {
  return (range.end.line - range.start.line) * 1000 + (range.end.character - range.start.character);
}

export function inferVisibility(name: string): Visibility {
  if (name.startsWith("__") && !name.endsWith("__")) return Visibility.Private;
  if (name.startsWith("_")) return Visibility.Private;
  return Visibility.Public;
}

/** Pyright marks all-caps variables as Constant (PEP 8). */
export function inferMutability(symbol: DocumentSymbol): Mutability {
  if (symbol.kind === LspSymbolKind.Constant) {
    return Mutability.Const;
  }
  return Mutability.Mutable;
}

export function isBuiltinType(typeName: string): boolean {
  const baseType = typeName.split("[")[0].trim();
  return PYTHON_BUILTIN_TYPES.has(baseType) || PYTHON_BUILTIN_TYPES.has(baseType.toLowerCase());
}

/**
 * Simplify Literal types to base types.
 * E.g. Literal[1048576] -> int, Literal['hello'] -> str
 */
export function simplifyLiteralType(typeStr: string): string {
  const literalMatch = typeStr.match(/^Literal\[(.+)\]$/);
  if (!literalMatch) return typeStr;

  const value = literalMatch[1].trim();

  if (value === "True" || value === "False") return "bool";
  if ((value.startsWith("'") && value.endsWith("'")) || (value.startsWith('"') && value.endsWith('"'))) {
    return "str";
  }
  if (/^-?\d+$/.test(value)) return "int";
  if (/^-?\d+\.\d+$/.test(value)) return "float";

  return typeStr;
}
