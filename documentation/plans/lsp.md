# Lemma editor support (MVP)

We want three things to start:

1. **Syntax highlighting** for `.lemma`
2. **Inline errors** in the editor by surfacing any `LemmaError`s as LSP diagnostics
3. **Clickable external references** so users can navigate to Registry-hosted documents

Everything else (completion, hover, go-to-definition, rename, formatting, etc.) can wait until the basics are solid.

## 1) Syntax highlighting (no LSP required)

Use the existing TextMate grammar assets already in the repo under `lemma/syntax/`:

- `lemma.tmLanguage.json`
- `language-configuration.json`
- `README.md` (docs for consumers)

MVP plan:

- Ensure `.lemma` files are associated with the Lemma language in the editor (VS Code extension or equivalent).
- Package the existing TextMate grammar and language configuration into a minimal VS Code extension.
  - The extension needs a `package.json` with `contributes.languages` and `contributes.grammars` entries. The existing `package.json` in `lemma/syntax/` may be reusable as a starting point.
  - The same extension will also launch the LSP server (see below), so the extension scaffold should be set up with that in mind.

This gets us highlighting immediately, without implementing any language intelligence.

## 2) Diagnostics-only LSP (surface `LemmaError`s)

### Scope (v0)

Implement only:

- LSP lifecycle: `initialize`, `initialized`, `shutdown`
- Text sync: `textDocument/didOpen`, `textDocument/didChange`, `textDocument/didClose`
- Diagnostics: `textDocument/publishDiagnostics`
- Document links: `textDocument/documentLink` (for clickable Registry references)

No navigation. No refactors.

### Clickable external references

Even in a diagnostics-first LSP, we support **clickable Registry references**:

- Ctrl/Cmd+clicking on `doc @external/space/doc` or `type ... from @external/space/type` should open the **Registry-defined URL** for that identifier.
- The LSP delegates URL construction to a Registry implementation behind a trait/interface. It should not hardcode URL formats.
- **For the MVP, a stub Registry implementation is used** since the full Registry does not exist yet. The stub can return a placeholder URL (e.g. a "not yet available" page or a configured base URL with the identifier appended). The important thing is that the trait boundary exists from day one so the real Registry can be plugged in later without changing the LSP.

### What "diagnostics" means

Whenever a `.lemma` document is opened or changed:

- Run the same parsing/validation path we use in the engine/CLI for that input.
- Collect **any `LemmaError`s** (parse errors, semantic/validation errors, etc.).
- Convert each error into an LSP `Diagnostic`:
  - **message**: the error text
  - **severity**: `Error`
  - **range**: always provided (derived from the error's `Source.span` when available; otherwise anchored to a safe default range in the current file)

### Reality check: `LemmaError` already has locations/spans

Lemma already ships what the LSP needs for diagnostics:

- `LemmaError::location() -> Option<&Source>`
- `Source` contains:
  - `attribute`: source identifier (typically the filename or `"<input>"`)
  - `doc_name`: Lemma doc name (useful for display, not required for LSP)
  - `span: Span`
- `Span` contains **byte offsets** (`start`, `end`) plus a **start** `line`/`col`

For LSP diagnostics, the reliable approach is to build the `Range` from `Span.start..Span.end`
against the **current editor buffer text** (so offsets match what the user is seeing). This also
avoids relying on whether `Span.col` is 0-based or 1-based.

All `LemmaError`s should still become diagnostics:

- `LemmaError::MultipleErrors`: flatten and publish diagnostics for each contained error.
- `LemmaError::ResourceLimitExceeded`: publish a single diagnostic anchored at a safe default range (e.g. the first character of the currently edited document / `textDocument` being analyzed), since it is not tied to a specific span.

### Workspace document model (cross-document validation)

Full diagnostics (semantic errors, type mismatches, unresolved references, circular dependencies) require `plan()`, which needs **all documents** in the workspace. The LSP therefore maintains an in-memory workspace model:

- On startup, discover all `.lemma` files in the workspace root and parse them.
- Keep an in-memory map of **all** documents: `Url -> (String, Option<Vec<LemmaDoc>>)` — the latest text and its parsed AST (if parsing succeeded).
- When a file is opened or changed:
  1. Update its entry in the map with the new buffer text.
  2. Re-parse the changed file.
  3. Re-run `plan()` using all successfully parsed documents.
  4. Publish diagnostics for the changed file (and any other files whose diagnostics changed as a result).
- When a file is created/deleted (via `workspace/didChangeWatchedFiles` or filesystem events), update the map accordingly.

**Error cascading:** A parse error in one file means that file produces no `LemmaDoc`, so other files that reference it will get planning errors (e.g. unresolved cross-document references). This is acceptable and expected — fixing the parse error in the broken file will clear both its own diagnostics and the cascading ones on the next re-plan.

**Debouncing:** Re-parsing and re-planning on every keystroke is expensive. Changes should be debounced with a ~200–300ms window after the last edit before triggering a re-plan. Parse-only errors for the active file can be published immediately (they're cheap); the full workspace re-plan runs after the debounce settles.

### Suggested implementation approach

- Add a new crate `lemma-lsp` to the workspace using `tower-lsp`.
- Add `lemma-lsp` to the workspace members in the root `Cargo.toml`.
- Run as an stdio server (works for VS Code and most other editors).
- The VS Code extension launches the `lemma-lsp` binary via stdio on activation.
- Workspace document map as described above.

## Milestone / definition of done

MVP is done when:

- `.lemma` files have syntax highlighting via the TextMate grammar
- Editing a `.lemma` file shows **all `LemmaError`s** as inline diagnostics in the editor, updating on each change
- Cross-document errors (unresolved references, type mismatches) surface correctly when multiple `.lemma` files are in the workspace
- Ctrl/Cmd+clicking on an `@external/...` reference produces a document link (via a stub Registry that returns a placeholder URL)
