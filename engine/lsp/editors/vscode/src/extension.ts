import { execFileSync } from "child_process";
import * as fs from "fs";
import * as path from "path";
import { ExtensionContext, window, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

interface ServerCommand {
  command: string;
  args: string[];
}

function lemmaBinaryVersion(command: string): string | null {
  try {
    const out = execFileSync(command, ["--version"], {
      encoding: "utf8",
      timeout: 3_000,
    });
    const line = out.trim().split("\n")[0] ?? "";
    const parts = line.split(/\s+/);
    return parts[parts.length - 1] ?? null;
  } catch {
    return null;
  }
}

function resolveWorkspaceLemmaBinary(
  folder: string,
  extensionVersion: string
): string | null {
  const release = path.join(folder, "target", "release", "lemma");
  const debug = path.join(folder, "target", "debug", "lemma");
  const candidates = [release, debug].filter((p) => fs.existsSync(p));
  const matching = candidates.find(
    (p) => lemmaBinaryVersion(p) === extensionVersion
  );
  if (matching) {
    return matching;
  }
  return candidates[0] ?? null;
}

function resolveServerCommand(
  raw: string,
  extensionVersion: string
): ServerCommand {
  const folder = workspace.workspaceFolders?.[0]?.uri.fsPath ?? "";
  const expanded = raw.replace(/\$\{workspaceFolder\}/g, folder);
  if (expanded !== "lemma") {
    return { command: expanded, args: ["lsp"] };
  }
  if (folder) {
    const workspaceBinary = resolveWorkspaceLemmaBinary(folder, extensionVersion);
    if (workspaceBinary) {
      return { command: workspaceBinary, args: ["lsp"] };
    }
  }
  return { command: "lemma", args: ["lsp"] };
}

export function activate(context: ExtensionContext): void {
  const config = workspace.getConfiguration("lemma");
  const rawPath: string = config.get<string>("lspServerPath", "lemma");
  const extensionVersion: string =
    context.extension.packageJSON.version ?? "0.0.0";
  const server = resolveServerCommand(rawPath, extensionVersion);

  // Omit transport: TransportKind.stdio — languageclient appends `--stdio` when set,
  // which older/unpatched CLIs reject. Undefined transport still uses stdio pipes.
  const serverOptions: ServerOptions = {
    run: {
      command: server.command,
      args: server.args,
    },
    debug: {
      command: server.command,
      args: server.args,
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "lemma" }],
    // Diagnostics: the LSP server sends textDocument/publishDiagnostics with an array of
    // Diagnostic per file. The client forwards them as-is; multiple diagnostics per file
    // (e.g. one per registry error) are all shown. No filtering or merging on the JS side.
    // Disk create/change sync (including lemma_deps after `lemma install`) is owned by the
    // Lemma CLI filesystem watch injected into `lemma lsp`, not by an editor file watcher.
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
