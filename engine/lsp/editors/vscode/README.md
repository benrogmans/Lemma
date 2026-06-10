# Lemma Language – VS Code / Cursor extension

Syntax highlighting, inline diagnostics, and **format on save** for `.lemma` files. The extension runs `lemma lsp`; formatting and diagnostics come from the LSP.

## Prerequisites

Install the `lemma` CLI:

```bash
npm install -g lemma
# or: cargo install lemma
```

## Install from this repo (development)

1. **Build the CLI** (from the **Lemma repo root**):
   ```bash
   cargo build --release -p lemma
   ```
   Binary: `target/release/lemma`.

2. **Install the extension** in VS Code / Cursor:
   - **Option A:** Open this folder (`engine/lsp/editors/vscode`) in VS Code and press F5 to run the Extension Development Host; or
   - **Option B:** From the repo root run `cargo lsp vsix`, then install the generated `.vsix` via **Extensions** → **...** → **Install from VSIX**.

3. **LSP discovery:**
   - If your **workspace root is the Lemma repo**, the extension uses `target/release/lemma lsp` automatically.
   - Otherwise, set **Lemma: Lsp Server Path** (`lemma.lspServerPath`) to the full path of the `lemma` binary, or ensure `lemma` is on your `PATH`.

## Format on save not working

Format on save uses the LSP. If the LSP does not start, formatting (and diagnostics) will not work.

1. **Check that the LSP is running**
   - Open a `.lemma` file, then **View** → **Output** and select **Lemma Language Server**.
   - You should see a line like “Lemma LSP server initialized”. If you see a spawn/ENOENT error, the `lemma` binary was not found.

2. **Fix the lemma path**
   - When **not** in the Lemma repo: set `lemma.lspServerPath` to the full path to the `lemma` binary (e.g. `/path/to/lemma/target/release/lemma`).
   - Or install `lemma` globally: `npm install -g lemma` or `cargo install lemma`.

3. **Confirm formatter for Lemma**
   - In a `.lemma` file, open the Command Palette and run **Format Document**. If it works, format-on-save should work once **Editor: Format On Save** is on and the default formatter for `[lemma]` is this extension (both are set by the extension’s default config).

## Marketplace install

The extension is published under the **Lemma** publisher. Search for **Lemma Language** or **lemma-language** in the Extensions view.

- **After first publish:** The marketplace can take from **about 10 minutes up to several hours** to index a new or updated extension. If you don’t see it, wait and try again, or search by publisher: `@lemma`.
- **LSP when installed from marketplace:** The extension does **not** bundle the `lemma` binary. Install `lemma` via `npm install -g lemma` or `cargo install lemma`. When developing in the Lemma repo, the extension auto-detects `target/release/lemma`.

## Verify locally

Automated stdio tests cover the LSP server (`cargo nextest run -p lemma --test integration integrations::lsp`). To verify the VS Code extension client end-to-end:

1. `cargo build -p lemma` (or `npm install -g lemma@<version>` after CLI release)
2. `cargo lsp vsix` → install the `.vsix` via **Extensions** → **...** → **Install from VSIX**
3. Open a folder **outside** the Lemma repo that contains a `.lemma` file
4. **View** → **Output** → **Lemma Language Server** → expect “Lemma LSP server initialized”
5. Introduce a syntax error → inline diagnostic appears
6. **Format Document** → spec reformats
