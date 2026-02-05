import type { Hover } from "vscode-languageserver-protocol";
import type { Parameter } from "../../core/types";
import { splitParams } from "./symbol-utils";

export function extractHoverInfo(hover?: Hover): { documentation: string[]; signature: string } {
  if (!hover) {
    return { documentation: [], signature: "" };
  }

  const documentation: string[] = [];
  let signature = "";

  if (hover.contents) {
    if (typeof hover.contents === "string") {
      signature = hover.contents.trim();
    } else if (Array.isArray(hover.contents)) {
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
      const content = hover.contents as { kind: string; value: string };
      const value = content.value.trim();
      const parts = value.split(/\n\n/);
      if (parts.length > 0) {
        signature = parts[0].trim();
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

/**
 * Parse function signature to extract parameters and return types.
 * Returns raw type names; resolution is done separately.
 */
export function parseSignature(signature: string): { parameters: Parameter[]; returnTypes: string[] } {
  if (!signature) {
    return { parameters: [], returnTypes: [] };
  }

  const normalizedSig = signature
    .replace(/\r?\n/g, " ")
    .replace(/\s+/g, " ")
    .trim();

  const cleanSig = normalizedSig.replace(/^\([^)]+\)\s*/, "");
  const withoutDef = cleanSig.replace(/^def\s+/, "");
  const paramMatch = withoutDef.match(/\(([^)]*)\)(?:\s*->\s*(.+))?$/);
  if (!paramMatch) {
    return { parameters: [], returnTypes: [] };
  }

  const paramString = paramMatch[1];
  const returnTypeStr = paramMatch[2]?.trim();

  const parameters: Parameter[] = [];
  if (paramString.trim()) {
    const paramParts = splitParams(paramString);

    for (const param of paramParts) {
      const trimmed = param.trim();
      if (!trimmed) continue;

      const isVariadic = trimmed.startsWith("*") || trimmed.startsWith("**");
      const paramWithoutStars = trimmed.replace(/^\*+/, "");
      const match = paramWithoutStars.match(/^([a-zA-Z_][a-zA-Z0-9_]*)\s*(?::\s*([^=]+))?\s*(?:=\s*(.+))?$/);
      if (match) {
        const name = match[1];
        if (name === "self" || name === "cls") continue;

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

  const returnTypes: string[] = [];
  if (returnTypeStr) {
    returnTypes.push(returnTypeStr);
  }

  return { parameters, returnTypes };
}
