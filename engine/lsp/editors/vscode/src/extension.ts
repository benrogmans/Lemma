import * as fs from "fs";
import * as path from "path";
import { ExtensionContext, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

function resolveServerPath(raw: string): string {
  const folder = workspace.workspaceFolders?.[0]?.uri.fsPath ?? "";
  const expanded = raw.replace(/\$\{workspaceFolder\}/g, folder);
  if (expanded !== "lsp") {
    return expanded;
  }
  if (folder) {
    const inRepo = path.join(folder, "target", "release", "lsp");
    if (fs.existsSync(inRepo)) {
      return inRepo;
    }
  }
  return "lsp";
}

export function activate(context: ExtensionContext): void {
  const config = workspace.getConfiguration("lemma");
  const rawPath: string = config.get<string>("lspServerPath", "lsp");
  const serverPath = resolveServerPath(rawPath);

  const serverOptions: ServerOptions = {
    run: {
      command: serverPath,
      transport: TransportKind.stdio,
    },
    debug: {
      command: serverPath,
      transport: TransportKind.stdio,
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "lemma" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.lemma"),
    },
    // Diagnostics: the LSP server sends textDocument/publishDiagnostics with an array of
    // Diagnostic per file. The client forwards them as-is; multiple diagnostics per file
    // (e.g. one per registry error) are all shown. No filtering or merging on the JS side.
  };

  client = new LanguageClient(
    "lemmaLanguageServer",
    "Lemma Language Server",
    serverOptions,
    clientOptions
  );

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (client) {
    return client.stop();
  }
  return undefined;
}
