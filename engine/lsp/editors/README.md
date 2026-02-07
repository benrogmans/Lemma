# Lemma editor support

Editor integrations for the Lemma LSP (this directory lives under `engine/lsp/`).

- **vscode/** — VS Code / Cursor extension: syntax highlighting, language configuration, and LSP client (connects to the LSP binary).

The LSP binary is built from the repo root with `cargo build --release -p lsp` and lives at **`target/release/lsp`**. The VS Code extension discovers it via `lemma.lspServerPath` or from that path when the workspace root is the Lemma repo.
