# Lemma Language – VS Code / Cursor extension

Syntax highlighting, inline diagnostics, and **format on save** for `.lemma` files. The extension starts the Lemma LSP binary; formatting and diagnostics come from the LSP.

## Install from this repo (development)

1. **Build the LSP** (from the **Lemma repo root**):
   ```bash
   cargo build --release -p lsp
   ```
   Binary: `target/release/lsp`.

2. **Install the extension** in VS Code / Cursor:
   - **Option A:** Open this folder (`engine/lsp/editors/vscode`) in VS Code and press F5 to run the Extension Development Host; or
   - **Option B:** From this folder run `npm run package`, then install the generated `.vsix` via **Extensions** → **...** → **Install from VSIX**.

3. **LSP discovery:**
   - If your **workspace root is the Lemma repo**, the extension uses `target/release/lsp` automatically.
   - Otherwise, set the path in settings: **Lemma: Lsp Server Path** (`lemma.lspServerPath`) to the full path of the `lsp` binary, or ensure `lsp` is on your `PATH`.

## Format on save not working

Format on save uses the LSP. If the LSP does not start, formatting (and diagnostics) will not work.

1. **Check that the LSP is running**
   - Open a `.lemma` file, then **View** → **Output** and select **Lemma Language Server**.
   - You should see a line like “Lemma LSP server initialized”. If you see a spawn/ENOENT error, the binary was not found.

2. **Fix the LSP path**
   - When **not** in the Lemma repo: set `lemma.lspServerPath` to the full path to the `lsp` binary (e.g. `/path/to/lemma/target/release/lsp`).
   - Or build the LSP and add the directory containing `lsp` to your `PATH`.

3. **Confirm formatter for Lemma**
   - In a `.lemma` file, open the Command Palette and run **Format Document**. If it works, format-on-save should work once **Editor: Format On Save** is on and the default formatter for `[lemma]` is this extension (both are set by the extension’s default config).

## Marketplace install

The extension is published under the **Lemma** publisher. Search for **Lemma Language** or **lemma-language** in the Extensions view.

- **After first publish:** The marketplace can take from **about 10 minutes up to several hours** to index a new or updated extension. If you don’t see it, wait and try again, or search by publisher: `@lemma`.
- **LSP when installed from marketplace:** The extension does **not** bundle the LSP binary. You must either have the Lemma repo open (with `target/release/lsp` built) or set `lemma.lspServerPath` to your `lsp` binary. Installing from the marketplace is mainly for users who also build Lemma from source or have `lsp` on their system.
