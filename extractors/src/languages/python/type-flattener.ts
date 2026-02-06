const WRAPPER_TYPES = new Set([
  "Optional", "Union", "Final", "ClassVar", "Annotated", "Type",
]);

const FIRST_ARG_ONLY = new Set(["Final", "ClassVar", "Annotated"]);

const CONTAINER_TYPES = new Set([
  "List", "list", "Sequence", "Set", "set", "FrozenSet", "frozenset", "Deque",
  "Dict", "dict", "Mapping", "OrderedDict", "DefaultDict",
  "Tuple", "tuple",
  "Iterable", "Iterator", "Generator", "AsyncIterator", "AsyncIterable", "AsyncGenerator",
]);

const DROP_TOKENS = new Set(["Unknown", "None", "NoneType", "object", "...", "type[Self]", "Self"]);

function splitTopLevel(s: string, delimiter: string): string[] {
  const result: string[] = [];
  let current = "";
  let bracketDepth = 0;
  let parenDepth = 0;

  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (ch === "[") {
      bracketDepth++;
      current += ch;
    } else if (ch === "]") {
      bracketDepth--;
      current += ch;
    } else if (ch === "(") {
      parenDepth++;
      current += ch;
    } else if (ch === ")") {
      parenDepth--;
      current += ch;
    } else if (bracketDepth === 0 && parenDepth === 0 && delimiter === "|" && ch === "|") {
      result.push(current);
      current = "";
    } else if (bracketDepth === 0 && parenDepth === 0 && delimiter === "," && ch === ",") {
      result.push(current);
      current = "";
    } else {
      current += ch;
    }
  }

  if (current.trim()) {
    result.push(current);
  }

  return result.map((p) => p.trim()).filter((p) => p.length > 0);
}

function isCallableSignature(expr: string): boolean {
  if (!expr.startsWith("(")) return false;
  let depth = 0;
  for (let i = 0; i < expr.length; i++) {
    if (expr[i] === "(") depth++;
    else if (expr[i] === ")") depth--;
    if (depth === 0) {
      const rest = expr.slice(i + 1).trim();
      return rest.startsWith("->");
    }
  }
  return false;
}

function findBracketEnd(expr: string, start: number): number {
  let depth = 0;
  for (let i = start; i < expr.length; i++) {
    if (expr[i] === "[") depth++;
    else if (expr[i] === "]") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

function stripOuterParens(expr: string): string {
  if (!expr.startsWith("(") || !expr.endsWith(")")) return expr;
  let depth = 0;
  for (let i = 0; i < expr.length; i++) {
    if (expr[i] === "(") depth++;
    else if (expr[i] === ")") depth--;
    if (depth === 0 && i < expr.length - 1) return expr;
  }
  return expr.slice(1, -1).trim();
}

function extractInner(expr: string): string[] {
  let trimmed = expr.trim();

  if (!trimmed) return [];
  if (DROP_TOKENS.has(trimmed)) return [];

  trimmed = stripOuterParens(trimmed);
  if (!trimmed) return [];
  if (DROP_TOKENS.has(trimmed)) return [];

  if (isCallableSignature(trimmed)) return ["Callable"];

  const pipeparts = splitTopLevel(trimmed, "|");
  if (pipeparts.length > 1) {
    return pipeparts.flatMap((p) => extractInner(p));
  }

  const bracketIdx = trimmed.indexOf("[");
  if (bracketIdx > 0) {
    const name = trimmed.slice(0, bracketIdx).trim();
    const endIdx = findBracketEnd(trimmed, bracketIdx);
    if (endIdx === -1) return DROP_TOKENS.has(name) ? [] : [name];

    const argsStr = trimmed.slice(bracketIdx + 1, endIdx);

    if (name === "Callable") return ["Callable"];

    if (name === "Literal") return [];

    if (WRAPPER_TYPES.has(name) || CONTAINER_TYPES.has(name) || name.startsWith("_")) {
      const args = splitTopLevel(argsStr, ",");
      if (FIRST_ARG_ONLY.has(name) && args.length > 0) {
        return extractInner(args[0]);
      }
      return args.flatMap((a) => extractInner(a));
    }

    if (DROP_TOKENS.has(name)) return [];
    return [name];
  }

  if (DROP_TOKENS.has(trimmed)) return [];
  return [trimmed];
}

export function extractAtomicTypes(typeExpr: string): string[] {
  const results = extractInner(typeExpr);
  const seen = new Set<string>();
  const deduped: string[] = [];
  for (const r of results) {
    if (!seen.has(r)) {
      seen.add(r);
      deduped.push(r);
    }
  }
  return deduped;
}
