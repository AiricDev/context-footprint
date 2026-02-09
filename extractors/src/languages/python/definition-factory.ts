import type { DocumentSymbol, Hover, Range } from "vscode-languageserver-protocol";
import { SymbolKind as LspSymbolKind } from "vscode-languageserver-protocol";
import { relativePath } from "../../core/utils";
import type {
  FunctionDetails,
  Parameter,
  SymbolDefinition,
  TypeDetails,
  VariableDetails
} from "../../core/types";
import {
  SymbolKind,
  TypeKind,
  VariableScope,
  Visibility
} from "../../core/types";
import { extractHoverInfo, parseSignature } from "./hover";
import {
  extractModuleName,
  inferMutability,
  inferVisibility,
  isBuiltinType,
  mapSymbolKind,
  simplifyLiteralType,
  splitParams
} from "./symbol-utils";
import type { SymbolRecord } from "./types";
import { extractAtomicTypes } from "./type-flattener";

/**
 * Extract a parameter's type annotation from source when hover reports "Unknown".
 * Looks for "paramName: Type" on the same line; if " = default" exists, stops before it.
 */
function getParamTypeFromSource(
  lines: string[],
  paramName: string,
  startLine: number,
  endLine: number
): string | null {
  const end = Math.min(endLine + 1, startLine + 30, lines.length);
  const paramBoundary = new RegExp(
    `\\b${paramName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*:\\s*(.+)`
  );
  for (let i = startLine; i < end; i++) {
    const line = lines[i];
    const colonMatch = line.match(paramBoundary);
    if (!colonMatch) continue;
    let typePart = colonMatch[1].trim();
    const eqIdx = typePart.indexOf(" = ");
    if (eqIdx >= 0) {
      typePart = typePart.slice(0, eqIdx).trim();
    }
    const commaIdx = typePart.indexOf(",");
    if (commaIdx >= 0) {
      typePart = typePart.slice(0, commaIdx).trim();
    }
    const parenIdx = typePart.indexOf(")");
    if (parenIdx >= 0) {
      typePart = typePart.slice(0, parenIdx).trim();
    }
    if (typePart) return typePart;
  }
  return null;
}

function isExplicitNoneReturn(typeExpr: string | null | undefined): boolean {
  if (!typeExpr) return false;
  const normalized = typeExpr.replace(/\s+/g, "").toLowerCase();
  return normalized === "none" || normalized === "nonetype";
}

/**
 * Extract the return type expression from source (explicit annotation only).
 * Supports multi-line, union (A | B), generics (List[T]), and nested brackets.
 * Returns null if no "->" is found (no explicit return type).
 */
function getReturnTypeFromSource(
  lines: string[],
  startLine: number,
  endLine: number
): string | null {
  const end = Math.min(endLine + 1, startLine + 40, lines.length);
  const fragment = lines.slice(startLine, end).join("\n");
  const arrowIdx = fragment.indexOf("->");
  if (arrowIdx === -1) return null;

  let pos = arrowIdx + 2;
  const n = fragment.length;
  let depth = 0; // count of unclosed ([{
  let start = pos;

  while (pos < n) {
    const ch = fragment[pos];

    if (ch === '"' || ch === "'") {
      const quote = ch;
      const isTriple = fragment.slice(pos, pos + 3) === quote + quote + quote;
      pos += isTriple ? 3 : 1;
      while (pos < n) {
        if (fragment[pos] === "\\") {
          pos += 2;
          continue;
        }
        if (isTriple && fragment.slice(pos, pos + 3) === quote + quote + quote) {
          pos += 3;
          break;
        }
        if (!isTriple && fragment[pos] === quote) {
          pos++;
          break;
        }
        pos++;
      }
      continue;
    }

    if (ch === "(" || ch === "[" || ch === "{") {
      depth++;
      pos++;
      continue;
    }
    if (ch === ")" || ch === "]" || ch === "}") {
      depth--;
      pos++;
      continue;
    }

    if (depth === 0 && ch === ":") {
      break;
    }
    pos++;
  }

  const raw = fragment.slice(start, pos).trim();
  if (!raw) return null;
  return raw.replace(/\s+/g, " ");
}

export interface DefinitionContext {
  projectRoot: string;
  getFileContent(filePath: string): string | undefined;
  getFileLines(filePath: string): string[];
  definitions: SymbolDefinition[];
  protocolClasses: Set<string>;
  localTypesByFunction: Map<string, Set<string>>;
  /** If provided, used to resolve "Unknown" / unresolved types from import (e.g. Request -> fastapi.Request). */
  getImportedModuleForName?(filePath: string, name: string): string | null;
}

export function createSymbolId(
  projectRoot: string,
  filePath: string,
  parents: SymbolRecord[],
  symbol: DocumentSymbol,
  kind: SymbolKind
): string {
  const moduleName = extractModuleName(projectRoot, filePath);
  const nameChain: string[] = [moduleName];
  for (const parent of parents) {
    if (parent.kind === SymbolKind.Type || parent.kind === SymbolKind.Function) {
      nameChain.push(parent.name);
    }
  }
  const cleanName = symbol.name.replace(/\(.*/, "");
  nameChain.push(cleanName);
  return `${nameChain.join(".")}#${kind}`;
}

export function createFieldSymbolId(
  projectRoot: string,
  filePath: string,
  parents: SymbolRecord[],
  symbol: DocumentSymbol
): string {
  const moduleName = extractModuleName(projectRoot, filePath);
  const nameChain: string[] = [moduleName];
  for (const parent of parents) {
    if (parent.kind === SymbolKind.Type) {
      nameChain.push(parent.name);
    }
  }
  nameChain.push(symbol.name);
  return `${nameChain.join(".")}#Variable`;
}

export function resolveTypeRef(
  ctx: DefinitionContext,
  typeName: string,
  currentModule: string,
  excludedTypes: Set<string> = new Set(),
  filePath?: string
): string {
  const trimmed = typeName.trim();
  if (!trimmed) return typeName;
  if (isBuiltinType(trimmed)) {
    return trimmed;
  }

  const genericMatch = trimmed.match(/^(\w+)\[(.+)]$/);
  if (genericMatch) {
    const baseType = genericMatch[1];
    const paramType = genericMatch[2].trim();
    const resolvedParam = resolveTypeRef(
      ctx,
      paramType,
      currentModule,
      excludedTypes,
      filePath
    );
    return `${baseType}[${resolvedParam}]`;
  }

  if (trimmed.includes("|")) {
    const parts = trimmed.split(/\s*\|\s*/).map((p) => p.trim());
    const resolved = parts.map((p) =>
      resolveTypeRef(ctx, p, currentModule, excludedTypes, filePath)
    );
    return resolved.join(" | ");
  }

  if (trimmed.startsWith("Union[")) {
    return typeName;
  }

  if (excludedTypes.has(trimmed)) {
    return trimmed;
  }

  const possibleSymbolId = `${currentModule}.${trimmed}#Type`;
  const matchingType = ctx.definitions.find(
    (d) => d.symbol_id === possibleSymbolId && d.kind === SymbolKind.Type
  );
  if (matchingType) {
    return matchingType.symbol_id;
  }

  if (filePath && ctx.getImportedModuleForName) {
    const module = ctx.getImportedModuleForName(filePath, trimmed);
    if (module) {
      return `${module}.${trimmed}#Type`;
    }
  }
  return trimmed;
}

export function resolveTypeExpr(
  ctx: DefinitionContext,
  typeExpr: string,
  currentModule: string,
  excludedTypes: Set<string> = new Set(),
  filePath?: string
): string[] {
  const atoms = extractAtomicTypes(typeExpr);
  const resolved: string[] = [];
  const seen = new Set<string>();
  for (const atom of atoms) {
    if (excludedTypes.has(atom)) continue;
    const ref = resolveTypeRef(ctx, atom, currentModule, excludedTypes, filePath);
    if (ref && !seen.has(ref)) {
      seen.add(ref);
      resolved.push(ref);
    }
  }
  return resolved;
}

function pickPrimaryType(resolved: string[]): string | null {
  const nonBuiltin = resolved.find((r) => !isBuiltinType(r));
  return nonBuiltin ?? resolved[0] ?? null;
}

function isProtocolClass(ctx: DefinitionContext, symbolId: string): boolean {
  return ctx.protocolClasses.has(symbolId);
}

function inferTypeKind(
  ctx: DefinitionContext,
  symbolId: string,
  symbol: DocumentSymbol,
  hover?: Hover
): TypeKind {
  if (ctx.protocolClasses.has(symbolId)) {
    return TypeKind.Interface;
  }
  if (hover) {
    const { signature } = extractHoverInfo(hover);
    if (signature.includes("Protocol") || signature.includes("protocol")) {
      return TypeKind.Interface;
    }
  }
  if (
    symbol.detail?.includes("Protocol") ||
    (symbol as { kind?: number }).kind === LspSymbolKind.Interface
  ) {
    return TypeKind.Interface;
  }
  return TypeKind.Class;
}

function isAbstractType(symbol: DocumentSymbol): boolean {
  return symbol.detail?.includes("Protocol") ?? false;
}

function isAbstractFromHover(hover?: Hover): boolean {
  if (!hover) return false;
  const { signature } = extractHoverInfo(hover);
  return signature.includes("@abstract") || signature.includes("@abstractmethod");
}

function isMethodOfProtocol(ctx: DefinitionContext, enclosingSymbol: string | null): boolean {
  return enclosingSymbol !== null && ctx.protocolClasses.has(enclosingSymbol);
}

function inferAbstractFromDetail(detail?: string): boolean {
  return detail?.includes("abstract") ?? false;
}

/**
 * Detect if the function signature contains any parameter default with Depends(...).
 * Scans source lines of the function signature (def ... up to body).
 */
function hasDependsInSignature(
  lines: string[],
  startLine: number,
  endLine: number
): boolean {
  const end = Math.min(endLine + 1, startLine + 30, lines.length);
  for (let i = startLine; i < end; i++) {
    const line = lines[i] ?? "";
    if (line.includes("Depends(")) return true;
  }
  return false;
}

/**
 * Detect cf:di_wired pragma in documentation strings or nearby comments (# cf:di_wired).
 */
function hasCfDiWiredPragma(
  documentation: string[],
  lines: string[],
  startLine: number
): boolean {
  for (const doc of documentation) {
    if (doc.includes("cf:di_wired")) return true;
  }
  const end = Math.min(startLine + 5, lines.length);
  for (let i = Math.max(0, startLine - 1); i < end; i++) {
    const line = lines[i] ?? "";
    if (line.includes("# cf:di_wired") || line.includes("#cf:di_wired")) return true;
  }
  return false;
}

function inferFunctionModifiers(symbol: DocumentSymbol): {
  is_async: boolean;
  is_generator: boolean;
  is_static: boolean;
  is_abstract: boolean;
  visibility: Visibility;
} {
  const name = symbol.name;
  return {
    is_async: symbol.detail?.includes("async") ?? false,
    is_generator: false,
    is_static: name.startsWith("__") && name.endsWith("__"),
    is_abstract: false,
    visibility: inferVisibility(name)
  };
}

export function parseClassInheritance(
  ctx: DefinitionContext,
  filePath: string,
  symbol: DocumentSymbol,
  typeKind: TypeKind
): { inherits: string[]; implements: string[] } {
  const content = ctx.getFileContent(filePath);
  if (!content) {
    return { inherits: [], implements: [] };
  }

  const lines = content.split(/\r?\n/);
  const classLine = lines[symbol.range.start.line];
  if (!classLine) {
    return { inherits: [], implements: [] };
  }

  const classMatch = classLine.match(/class\s+\w+\s*\(([^)]*)\)/);
  if (!classMatch) {
    return { inherits: [], implements: [] };
  }

  const baseList = classMatch[1];
  if (!baseList.trim()) {
    return { inherits: [], implements: [] };
  }

  const bases = splitParams(baseList)
    .map((b) => b.trim())
    .filter((b) => b);
  const inherits: string[] = [];
  const implementsList: string[] = [];
  const moduleName = extractModuleName(ctx.projectRoot, filePath);

  for (const base of bases) {
    const baseNameMatch = base.match(/^(\w+)/);
    if (!baseNameMatch) continue;
    const baseName = baseNameMatch[1];
    const resolvedType = resolveTypeRef(ctx, baseName, moduleName, new Set(), filePath);
    const typeRef = resolvedType.includes("#") ? resolvedType : baseName;
    if (baseName === "Protocol" || isProtocolClass(ctx, typeRef)) {
      implementsList.push(typeRef);
    } else {
      inherits.push(typeRef);
    }
  }

  return { inherits, implements: implementsList };
}

export function createTypeDefinition(
  ctx: DefinitionContext,
  filePath: string,
  symbolId: string,
  symbol: DocumentSymbol,
  enclosingSymbol: string | null,
  hover?: Hover
): SymbolDefinition {
  const relPath = relativePath(ctx.projectRoot, filePath);
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
  const typeKind = inferTypeKind(ctx, symbolId, symbol, hover);
  const inheritance = parseClassInheritance(ctx, filePath, symbol, typeKind);

  const details: TypeDetails = {
    kind: typeKind,
    is_abstract: typeKind === TypeKind.Interface || isAbstractType(symbol),
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
    documentation,
    details: { Type: details }
  };
}

export async function createFunctionDefinition(
  ctx: DefinitionContext,
  filePath: string,
  symbolId: string,
  symbol: DocumentSymbol,
  enclosingSymbol: string | null,
  hover?: Hover
): Promise<SymbolDefinition> {
  const relPath = relativePath(ctx.projectRoot, filePath);
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
  const { parameters: rawParameters, returnTypes: rawReturnTypes } = parseSignature(
    signature || symbol.detail || ""
  );

  const moduleName = extractModuleName(ctx.projectRoot, filePath);
  const excludedTypes = ctx.localTypesByFunction.get(symbolId) ?? new Set();
  const lines = ctx.getFileLines(filePath);
  const startLine = symbol.range.start.line;
  const endLine = symbol.range.end.line;

  const parameters: Parameter[] = [];
  for (const param of rawParameters) {
    let typeToResolve = param.param_type;
    if (
      typeToResolve === "Unknown" ||
      (typeToResolve && typeToResolve.includes("Unknown"))
    ) {
      const fromSource = getParamTypeFromSource(lines, param.name, startLine, endLine);
      if (fromSource) typeToResolve = fromSource;
    }
    const resolvedType = typeToResolve
      ? pickPrimaryType(resolveTypeExpr(ctx, typeToResolve, moduleName, excludedTypes, filePath))
      : null;
    parameters.push({ ...param, param_type: resolvedType });
  }

  const funcName = symbol.name.replace(/\(.*/, "");
  const isConstructor = funcName === "__init__";

  const returnTypes: string[] = [];
  const explicitReturn = getReturnTypeFromSource(lines, startLine, endLine);
  if (explicitReturn) {
    if (isExplicitNoneReturn(explicitReturn)) {
      // Preserve explicit `-> None` as a declared return type.
      returnTypes.push("None");
    } else {
      returnTypes.push(
        ...resolveTypeExpr(ctx, explicitReturn, moduleName, excludedTypes, filePath)
      );
    }
  }
  // Python __init__ implicitly returns None; treat as signature-complete for CF.
  if (isConstructor && returnTypes.length === 0) {
    returnTypes.push("None");
  }

  const isAbstract =
    isAbstractFromHover(hover) ||
    inferAbstractFromDetail(symbol.detail) ||
    isMethodOfProtocol(ctx, enclosingSymbol);

  const isDiWired =
    hasDependsInSignature(lines, startLine, endLine) ||
    hasCfDiWiredPragma(documentation, lines, startLine);

  const details: FunctionDetails = {
    parameters,
    return_types: returnTypes,
    type_params: [],
    modifiers: {
      ...inferFunctionModifiers(symbol),
      is_abstract: isAbstract,
      is_constructor: isConstructor,
      is_di_wired: isDiWired
    }
  };

  return {
    symbol_id: symbolId,
    kind: SymbolKind.Function,
    name: funcName,
    display_name: signature || symbol.detail || symbol.name,
    location,
    span,
    enclosing_symbol: enclosingSymbol,
    documentation,
    details: { Function: details }
  };
}

export function createVariableDefinition(
  ctx: DefinitionContext,
  filePath: string,
  symbolId: string,
  symbol: DocumentSymbol,
  enclosingSymbol: string | null,
  hover?: Hover
): SymbolDefinition {
  const relPath = relativePath(ctx.projectRoot, filePath);
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
  const currentModule = extractModuleName(ctx.projectRoot, filePath);
  const lines = ctx.getFileLines(filePath);
  const declaredType = extractDeclaredType(ctx, lines, symbol.selectionRange, currentModule, filePath);
  const inferredType = extractVariableType(ctx, signature, currentModule, filePath);
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
    documentation,
    details: { Variable: varDetails }
  };
}

/**
 * Extract declared type annotation from source code.
 */
export function extractDeclaredType(
  ctx: DefinitionContext,
  lines: string[],
  range: Range,
  currentModule: string,
  filePath?: string
): string | null {
  const line = lines[range.start.line] ?? "";
  const afterName = line.slice(range.end.character);
  const typeMatch = afterName.match(/^\s*:\s*([^=]+?)(?:\s*=|$)/);
  if (typeMatch) {
    const rawType = typeMatch[1].trim();
    if (rawType) {
      return pickPrimaryType(resolveTypeExpr(ctx, rawType, currentModule, new Set(), filePath));
    }
  }
  return null;
}

export function extractVariableType(
  ctx: DefinitionContext,
  signature: string,
  currentModule: string,
  filePath?: string
): string | null {
  if (!signature) return null;
  const normalized = signature.replace(/\r?\n/g, " ").replace(/\s+/g, " ").trim();
  const varMatch = normalized.match(/^\((?:variable|constant)\)\s+\w+:\s*(.+)$/);
  if (varMatch) {
    const rawType = varMatch[1].trim();
    const resolved = resolveTypeExpr(ctx, simplifyLiteralType(rawType), currentModule, new Set(), filePath);
    return pickPrimaryType(resolved);
  }
  const simpleMatch = normalized.match(/^\w+:\s*(.+)$/);
  if (simpleMatch) {
    const rawType = simpleMatch[1].trim();
    const resolved = resolveTypeExpr(ctx, simplifyLiteralType(rawType), currentModule, new Set(), filePath);
    return pickPrimaryType(resolved);
  }
  return null;
}

export { mapSymbolKind };
