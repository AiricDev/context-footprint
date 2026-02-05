import type { DocumentSymbol, Range } from "vscode-languageserver-protocol";
import type { SymbolKind } from "../../core/types";

export interface SymbolRecord {
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
