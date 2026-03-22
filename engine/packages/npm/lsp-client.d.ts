/**
 * LSP client for the Lemma WASM playground. Talks JSON-RPC over streams to the WASM LSP server.
 */

export interface LspDiagnostic {
  range: { start: { line: number; character: number }; end: { line: number; character: number } };
  message: string;
  severity?: number;
}

export class LspClient {
  constructor(monacoInstance: unknown);

  /** Launch the WASM LSP server. Optional serve/ServerConfig override for testing. */
  start(serveFn?: (config: unknown) => Promise<void>, ServerConfigCls?: new (into: AsyncIterable<Uint8Array>, from: WritableStream<Uint8Array>) => unknown): Promise<void>;

  initialize(): Promise<void>;
  didOpen(uri: string, languageId: string, version: number, text: string): void;
  didChange(uri: string, version: number, text: string): void;
  formatting(uri: string, tabSize: number, insertSpaces: boolean): Promise<unknown>;
  semanticTokensFull(uri: string): Promise<{ data: number[] } | null>;
  onDiagnostics(callback: (uri: string, diagnostics: LspDiagnostic[]) => void): void;
  sendRequest(method: string, params: unknown): Promise<unknown>;
  sendNotification(method: string, params: unknown): void;
  stop(): void;
  setMonacoMarkers(model: unknown, diagnostics: LspDiagnostic[]): void;
}
