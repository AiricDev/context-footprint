import type { Range } from "vscode-languageserver-protocol";
import { ReferenceRole, SymbolKind } from "../../core/types";

/**
 * Infer reference role from source context (line, range, target kind).
 * Returns null for type-in-signature (param/return/inherits) - do not emit reference.
 */
export function inferReferenceRole(lines: string[], range: Range, targetKind: SymbolKind): ReferenceRole | null {
  const line = lines[range.start.line] ?? "";
  const before = line.slice(0, range.start.character);
  const after = line.slice(range.end.character).trimStart();

  // Python decorators: express as Decorate role
  if (before.match(/^\s*@\s*$/)) {
    return ReferenceRole.Decorate;
  }

  // Type in signature (param, return, inheritance): do not emit reference
  if (targetKind === SymbolKind.Type) {
    if (before.match(/(?::\s*|->?\s*)$/)) {
      return null;
    }
    if (after.match(/^[\]\[,\)=\s]/) || after === "") {
      if (before.match(/[\[\(,]\s*$/)) {
        return null;
      }
    }
    if (before.match(/\|\s*$/) || after.startsWith("|")) {
      return null;
    }
    if (before.match(/class\s+\w+\s*\(\s*$/) || before.match(/,\s*$/)) {
      const lineBeforeClass = line.match(/class\s+\w+\s*\(/);
      if (lineBeforeClass) {
        return null;
      }
    }
  }

  if (after.startsWith("(")) {
    return ReferenceRole.Call;
  }

  if (after.match(/^(?:\[[^\]]*\])?\s*(?:=(?!=)|\+=|-=|\*=|\/=|%=|\|=|&=|\^=|>>=|<<=|\*\*=|\/\/=)/)) {
    return ReferenceRole.Write;
  }

  return ReferenceRole.Read;
}

/**
 * True if this @ line decorates a nested def/class (inside a function body).
 * Refs from such decorators are skipped (we don't track nested definitions).
 * Top-level @ before def/class (indent 0) is NOT nested.
 */
export function isDecoratorForNestedDef(lines: string[], lineNum: number): boolean {
  const line = lines[lineNum] ?? "";
  if (!line.match(/^\s*@/)) return false;
  const indent = line.match(/^(\s*)/)?.[1]?.length ?? 0;
  if (indent === 0) return false; // module-level decorator, not nested
  for (let i = lineNum + 1; i < lines.length; i++) {
    const next = lines[i] ?? "";
    if (next.trim() === "") continue;
    const nextIndent = next.match(/^(\s*)/)?.[1]?.length ?? 0;
    if (nextIndent < indent) return false; // dedented
    if (nextIndent >= indent && next.match(/^\s*(def |class )/)) {
      return true; // nested def/class inside function body
    }
    return false; // other statement
  }
  return false;
}

/** Extract receiver from "receiver." before the reference position. */
export function extractReceiver(lines: string[], range: Range): string | null {
  const line = lines[range.start.line] ?? "";
  const before = line.slice(0, range.start.character);
  const match = before.match(/([a-zA-Z0-9_]+)\.$/);
  return match ? match[1] : null;
}

/** Line containing "def " starting from startLine (handles decorated functions). */
export function findDefLine(lines: string[], startLine: number): number {
  for (let i = startLine; i < lines.length; i++) {
    if (lines[i].match(/^\s*(async\s+)?def\s+/)) {
      return i;
    }
  }
  return startLine;
}

/** Line containing "class " starting from startLine (handles decorated classes). */
export function findClassLine(lines: string[], startLine: number): number {
  for (let i = startLine; i < lines.length; i++) {
    if (lines[i].match(/^\s*class\s+/)) {
      return i;
    }
  }
  return startLine;
}
