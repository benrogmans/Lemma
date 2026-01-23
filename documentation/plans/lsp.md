# Lemma editor support (MVP)

We want two things to start:

1. **Syntax highlighting** for `.lemma`
2. **Inline errors** in the editor by surfacing any `LemmaError`s as LSP diagnostics

Everything else (completion, hover, go-to-definition, rename, formatting, etc.) can wait until the basics are solid.

## 1) Syntax highlighting (no LSP required)

Use the existing TextMate grammar assets already in the repo under `lemma/syntax/`:

- `lemma.tmLanguage.json`
- `language-configuration.json`
- `README.md` (docs for consumers)

MVP plan:

- Ensure `.lemma` files are associated with the Lemma language in the editor (VS Code extension or equivalent).
- Package the existing TextMate grammar and language configuration into a minimal editor extension.

This gets us highlighting immediately, without implementing any language intelligence.

## 2) Diagnostics-only LSP (surface `LemmaError`s)

### Scope (v0)

Implement only:

- LSP lifecycle: `initialize`, `initialized`, `shutdown`
- Text sync: `textDocument/didOpen`, `textDocument/didChange`, `textDocument/didClose`
- Diagnostics: `textDocument/publishDiagnostics`

No workspace indexing. No navigation. No refactors.

### Extra: clickable external references

Even in a diagnostics-first LSP, we should support **clickable Registry references**:

- Ctrl/Cmd+clicking on `doc @external/space/doc` or `type ... from @external/space/type` should open the **Registry-defined URL** for that identifier.
- The LSP should not hardcode URL formats. It should delegate URL construction to the configured Registry implementation.

### What “diagnostics” means

Whenever a `.lemma` document is opened or changed:

- Run the same parsing/validation path we use in the engine/CLI for that input.
- Collect **any `LemmaError`s** (parse errors, semantic/validation errors, etc.).
- Convert each error into an LSP `Diagnostic`:
  - **message**: the error text
  - **severity**: `Error`
  - **range**: always provided (derived from the error’s `Source.span` when available; otherwise anchored to a safe default range in the current file)

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

### Suggested implementation approach (minimal)

- Add a new crate `lemma-lsp` using `tower-lsp`.
- Run as an stdio server (works for most editors).
- Keep an in-memory map of open documents: `Url -> String` (latest text).
- On change:
  - parse/validate the current buffer text
  - publish diagnostics for that `Url`

No caching needed yet; just debounce changes if it’s too chatty.

## Milestone / definition of done

MVP is done when:

- `.lemma` files have syntax highlighting via the TextMate grammar
- editing a `.lemma` file shows **all `LemmaError`s** as inline diagnostics in the editor, updating on each change

