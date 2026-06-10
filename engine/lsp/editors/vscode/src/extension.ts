import * as fs from "fs";
import * as path from "path";
import { ExtensionContext, window, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

interface ServerCommand {
  command: string;
  args: string[];
}

function resolveServerCommand(raw: string): ServerCommand {
  const folder = workspace.workspaceFolders?.[0]?.uri.fsPath ?? "";
  const expanded = raw.replace(/\$\{workspaceFolder\}/g, folder);
  if (expanded !== "lemma") {
    return { command: expanded, args: ["lsp"] };
  }
  if (folder) {
    const release = path.join(folder, "target", "release", "lemma");
    if (fs.existsSync(release)) {
      return { command: release, args: ["lsp"] };
    }
    const debug = path.join(folder, "target", "debug", "lemma");
    if (fs.existsSync(debug)) {
      return { command: debug, args: ["lsp"] };
    }
  }
  return { command: "lemma", args: ["lsp"] };
}

export function activate(context: ExtensionContext): void {
  const config = workspace.getConfiguration("lemma");
  const rawPath: string = config.get<string>("lspServerPath", "lemma");
  const server = resolveServerCommand(rawPath);

  const serverOptions: ServerOptions = {
    run: {
      command: server.command,
      args: server.args,
      transport: TransportKind.stdio,
    },
    debug: {
      command: server.command,
      args: server.args,
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
  context.subscriptions.push(client);

  void client.start().catch((err: unknown) => {
    void window.showErrorMessage(
      "Lemma LSP requires the `lemma` binary. Install via: npm install -g lemma or cargo install lemma"
    );
    console.error(err);
  });
}

export function deactivate(): Thenable<void> | undefined {
  if (client) {
    return client.stop();
  }
  return undefined;
}
