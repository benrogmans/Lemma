# Lemma editor support

Editor integrations for the Lemma LSP (this directory lives under `engine/lsp/`).

- **vscode/** — VS Code / Cursor extension: syntax highlighting, language configuration, and LSP client (runs `lemma lsp`).

The VS Code extension runs `lemma lsp` and requires the `lemma` CLI (`npm install -g lemma` or `cargo install lemma`). When the workspace root is the Lemma repo, it auto-detects `target/release/lemma` or `target/debug/lemma`. Override with **Lemma: Lsp Server Path** (`lemma.lspServerPath`).

Other editors: point your LSP client at `lemma` with argument `lsp` and stdio transport.
