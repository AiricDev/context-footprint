/**
 * SemanticData - Language-agnostic semantic model for CF graph construction.
 *
 * Design principles:
 * - Graph-centric: Designed from CF algorithm needs, not indexer format.
 * - Language-agnostic: Abstracts away language differences.
 * - Three symbol types: Function, Variable, Type (no locals, no standalone parameters).
 * - Declared types only: No type inference required from adapter.
 */

/** Globally unique symbol identifier within project. Use hierarchical format (e.g. "pkg.module.Class.method") or indexer format. For builtins use language-standard names. */
export type SymbolId = string;

/** Type reference: SymbolId of a Type definition or standardized name for builtin types (int, str, List, etc.). For generics include params (e.g. "List[int]"). */
export type TypeRef = string;

/**
 * Top-level container for project semantic data.
 *
 * Adapter contract:
 * - project_root: Absolute path; all document relative_path are relative to it.
 * - documents: All project source files; each has definitions and references.
 */
export interface SemanticData {
  project_root: string;
  documents: DocumentSemantics[];
}

/**
 * Semantic information for a single source file.
 *
 * Adapter contract:
 * - relative_path: From project root; use forward slashes.
 * - language: Lowercase (e.g. "python", "typescript").
 * - definitions: Functions, methods, globals, fields, types; exclude locals and standalone params.
 * - references: All calls, reads/writes, type annotations; set receiver for member access.
 */
export interface DocumentSemantics {
  relative_path: string;
  language: string;
  definitions: SymbolDefinition[];
  references: SymbolReference[];
}

export type SymbolDefinition = FunctionSymbol | VariableSymbol | TypeSymbol;

/**
 * Base fields for all symbol definitions.
 *
 * Adapter contract:
 * - symbol_id: Globally unique, deterministic.
 * - kind: Function | Variable | Type by primary purpose.
 * - name: Short name (e.g. method name only, not "Class.method").
 * - display_name: Human-readable, may include signature.
 * - location: Start of definition; file_path matches document relative_path; 0-based line/column.
 * - span: Full extent for context_size; functions = entire body (or signature only if abstract);
 *   end positions are exclusive.
 * - enclosing_symbol: Methods/fields point to their Type; top-level null.
 * - documentation: All doc comments; order most relevant first; empty if none.
 */
interface BaseSymbolDefinition {
  symbol_id: SymbolId;
  kind: SymbolKind;
  name: string;
  display_name: string;
  location: SourceLocation;
  span: SourceSpan;
  enclosing_symbol: SymbolId | null;
  documentation: string[];
}

export interface FunctionSymbol extends BaseSymbolDefinition {
  kind: SymbolKind.Function;
  details: { Function: FunctionDetails };
}

export interface VariableSymbol extends BaseSymbolDefinition {
  kind: SymbolKind.Variable;
  details: { Variable: VariableDetails };
}

export interface TypeSymbol extends BaseSymbolDefinition {
  kind: SymbolKind.Type;
  details: { Type: TypeDetails };
}

/** Symbol classification: Function (methods, constructors), Variable (globals, fields), Type (classes, interfaces, etc.). Methods are separate Function symbols. */
export enum SymbolKind {
  Function = "Function",
  Variable = "Variable",
  Type = "Type"
}

/**
 * Function/method details.
 *
 * Adapter contract:
 * - parameters: All except self/this/cls; declaration order; param_type from annotation only (null if untyped); no inference.
 * - return_types: From explicit return annotation; empty if none or void.
 * - type_params: Generic params with bounds.
 */
export interface FunctionDetails {
  parameters: Parameter[];
  return_types: TypeRef[];
  type_params: TypeParam[];
  modifiers: FunctionModifiers;
}

/**
 * Parameter of a function.
 *
 * Adapter contract:
 * - param_type: From explicit annotation only; null if untyped (do not infer from call sites).
 * - has_default: true if parameter has default value.
 * - is_variadic: true for *args, ...rest, etc.
 */
export interface Parameter {
  name: string;
  param_type?: TypeRef | null;
  has_default: boolean;
  is_variadic: boolean;
}

/** Generic type parameter; bounds are type constraints (e.g. T extends Base). */
export interface TypeParam {
  name: string;
  bounds: TypeRef[];
}

/**
 * Function modifiers.
 *
 * Adapter contract:
 * - is_abstract: true for interface/protocol/trait methods (signature only). Critical for CF boundary.
 * - is_constructor: true for type constructors (e.g. Python __init__). Language-specific; extractor sets this.
 * - visibility: Public/Private/Protected/Internal per language.
 */
export interface FunctionModifiers {
  is_async: boolean;
  is_generator: boolean;
  is_static: boolean;
  is_abstract: boolean;
  /** True if this function is a type constructor (e.g. Python __init__). Set by language extractor. */
  is_constructor?: boolean;
  visibility: Visibility;
}

/**
 * Variable/field details.
 *
 * Adapter contract:
 * - var_type: From explicit annotation only; null if untyped; do not infer from assignment.
 * - mutability: Const | Immutable | Mutable; only Mutable triggers SharedStateWrite expansion.
 * - scope: Global (module-level) or Field (class field; must have enclosing_symbol → Type).
 *   Local variables must NOT be extracted.
 */
export interface VariableDetails {
  var_type?: TypeRef | null;
  mutability: Mutability;
  scope: VariableScope;
  visibility: Visibility;
}

/**
 * Type definition details.
 *
 * Adapter contract:
 * - kind: Map to Class, Interface (Protocol/trait), Struct, Enum, etc.
 * - is_abstract: true for Protocol, interface, trait, abstract class. Critical for CF boundary.
 * - is_final: true if type cannot be inherited.
 * - fields: All data members; each should have a matching Variable definition with scope=Field.
 *   Methods are NOT fields (separate Function definitions).
 * - inherits: Base classes; implements: interfaces/protocols/traits (language-specific).
 */
export interface TypeDetails {
  kind: TypeKind;
  is_abstract: boolean;
  is_final: boolean;
  visibility: Visibility;
  type_params: TypeParam[];
  fields: FieldInfo[];
  inherits: TypeRef[];
  implements: TypeRef[];
}

/**
 * Field view at type level. Each field also has a Variable definition (scope=Field) with same symbol_id.
 *
 * Adapter contract:
 * - symbol_id: Must match the Variable definition for this field.
 */
export interface FieldInfo {
  name: string;
  field_type?: TypeRef | null;
  mutability: Mutability;
  visibility: Visibility;
  symbol_id: SymbolId;
}

/** Visibility: Public, Private, Protected, Internal. Map from language (e.g. leading underscore → Private, pub → Public). */
export enum Visibility {
  Public = "Public",
  Private = "Private",
  Protected = "Protected",
  Internal = "Internal"
}

/** Const = compile-time constant; Immutable = runtime immutable; Mutable = can be reassigned (triggers SharedStateWrite in CF). When in doubt use Mutable. */
export enum Mutability {
  Const = "Const",
  Immutable = "Immutable",
  Mutable = "Mutable"
}

/** Global = module/package-level; Field = class/struct field (must have enclosing_symbol → Type). Locals are not extracted. */
export enum VariableScope {
  Global = "Global",
  Field = "Field"
}

/** Class, Interface (Protocol/trait), Struct, Enum, TypeAlias, Union, Intersection. */
export enum TypeKind {
  Class = "Class",
  Interface = "Interface",
  Struct = "Struct",
  Enum = "Enum",
  TypeAlias = "TypeAlias",
  Union = "Union",
  Intersection = "Intersection"
}

/**
 * A reference to a symbol (for edge construction).
 *
 * Adapter contract:
 * - target_symbol: Must exist in definitions.
 * - location: Where the reference occurs.
 * - enclosing_symbol: Function or module containing this reference.
 * - role: Call/Read/Write/Decorate.
 * - receiver: undefined for direct access (foo(), GLOBAL_VAR); set to variable name for member access (self.method(), obj.field).
 */
export interface SymbolReference {
  target_symbol: SymbolId;
  location: SourceLocation;
  enclosing_symbol: SymbolId;
  role: ReferenceRole;
  receiver?: string | null;
}

/**
 * Reference role (determines edge kind in graph).
 *
 * - Call: function/method/constructor call.
 * - Read: variable read (not assignment).
 * - Write: assignment/mutation (mutable shared state → SharedStateWrite expansion).
 * - Decorate: decorator/annotation application; target_symbol = decorator function.
 */
export enum ReferenceRole {
  Call = "Call",
  Read = "Read",
  Write = "Write",
  Decorate = "Decorate",
}

/**
 * Single point in source. file_path = document relative_path; line and column are 0-based.
 */
export interface SourceLocation {
  file_path: string;
  line: number;
  column: number;
}

/**
 * Range in source. start_* inclusive; end_* exclusive (span does not include end position). 0-based.
 */
export interface SourceSpan {
  start_line: number;
  start_column: number;
  end_line: number;
  end_column: number;
}
