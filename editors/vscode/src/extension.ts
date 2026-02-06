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
  if (expanded !== "lemma-lsp") {
    return expanded;
  }
  if (folder) {
    const inRepo = path.join(folder, "target", "release", "lemma-lsp");
    if (fs.existsSync(inRepo)) {
      return inRepo;
    }
  }
  return "lemma-lsp";
}

export function activate(context: ExtensionContext): void {
  const config = workspace.getConfiguration("lemma");
  const rawPath: string = config.get<string>("lspServerPath", "lemma-lsp");
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
