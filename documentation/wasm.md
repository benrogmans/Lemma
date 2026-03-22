---
title: WebAssembly
---

# Lemma engine in the browser

JS API mirrors Rust types (`Response`, `SpecSchema`, spec list entries) as plain objects.

## Install

```bash
npm install @benrogmans/lemma-engine
```

## Usage

```javascript
import { Lemma } from '@benrogmans/lemma-engine';

const engine = await Lemma();
```

`lemma_bg.wasm` loads from the package URL (via generated `lemma.bindings.js`). Use **http(s)** — not `file://`.

```javascript
try {
  await engine.load(`
  spec example
  fact price: 100
  rule total: price * 2
`, 'example.lemma');
} catch (errs) {
  console.error(Array.isArray(errs) ? errs.join('\n') : errs);
}

const response = engine.run('example', [], {}, null);
console.log(response.results);
```

## Package layout

| Artifact | Role |
|----------|------|
| `lemma.js` | Public entry: `Lemma()`, `init`, `initSync`, `Engine` |
| `lemma.bindings.js` | wasm-pack glue (do not import directly) |
| `lsp-client.js` | `LspClient`: `start()`, `initialize()`, `didOpen`, diagnostics, formatting, semantic tokens |
| `lsp.js` | Low-level `serve`, `ServerConfig` (used by LspClient; override via `start(serve, ServerConfig)` if needed) |

## API

- **`Lemma()`** — async; initializes WASM once, returns an `Engine` (recommended).
- **`init()`** — await once (browser).
- **`initSync({ module })`** — Node + `readFileSync('…/lemma_bg.wasm')`.
- **`Engine`** — `load`, `list`, `schema`, `run`, `format`; `invert` throws.
- **`@benrogmans/lemma-engine/lsp-client`** — `LspClient`: `start()` (no args), `initialize()`, `didOpen`, `onDiagnostics`, `formatting`, `semanticTokensFull`. Call `init()` first.

**Spec id** (for `show` / `run`): `name` or `name~` + 8 hex chars (same as CLI).

See [engine/packages/npm/README.md](../engine/packages/npm/README.md).

## Build from source

```bash
cd lemma/engine/packages/npm
node build.js
```

Output: `engine/packages/npm/dist/`.
