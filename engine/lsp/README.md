# Lemma LSP

Language Server Protocol implementation for [Lemma](https://github.com/lemma/lemma). Provides inline diagnostics and editor features for `.lemma` files.

## Features

- **Diagnostics** — Parse and planning (semantic) errors are published as you type. Parse errors are shown immediately; a debounced (250ms) full workspace validation adds planning errors. Errors use source spans where available; diagnostics are cleared when a file is closed.
- **Workspace validation** — On native, when the client provides a workspace root, the server scans for `.lemma` files under that root at startup (skips hidden directories only) and runs full planning so cross-spec errors (missing spec/type refs, circular dependencies, etc.) are reported per file. The client keeps the server in sync via file open/change/close. On WASM, only the open file is validated (no filesystem).
- **Registry links** — `@`-prefixed Registry references (e.g. `spec @user/workspace/somespec`, `type ... from @lemma/std/finance`) are turned into clickable links when the Registry (LemmaBase) provides a URL. Works even when the file has parse errors (text-based scan).
- **Text document sync** — Full file sync on open, change, and close; no incremental sync.

The server uses the Lemma engine with **registry** support (LemmaBase) for resolving `@...` identifiers and communicates over stdio (native) or browser streams (WASM).

## Build

From the **repository root**:

```bash
cargo build --release -p lsp
```

The binary is produced at `target/release/lsp`.

The crate also supports a **WASM** build for in-browser use; the library entry point is `lsp::browser::serve`. The Lemma WASM playground does not use the LSP for diagnostics; it uses the engine’s `getDiagnostics` API directly for inline errors.

## Usage

Run the binary with no arguments. It speaks LSP over stdio:

- **VS Code / Cursor** — Use the extension under [editors/vscode](editors/vscode). It starts the LSP automatically and looks for `target/release/lsp` when the workspace root is the Lemma repo, or uses the `lemma.lspServerPath` setting. Format-on-save and diagnostics only work when the LSP is running; see [editors/vscode/README.md](editors/vscode/README.md) if format on save does nothing.
- **Other editors** — Point your editor’s LSP client at the `lsp` binary with stdio transport (no extra arguments).

## Layout

- **`src/`** — LSP server (tower-lsp): server, diagnostics, registry links, workspace model, registry integration.
- **`editors/`** — Editor-specific clients and config (e.g. VS Code extension); see [editors/README.md](editors/README.md).
