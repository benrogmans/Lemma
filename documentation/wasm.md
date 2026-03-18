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
import { init, Engine } from '@benrogmans/lemma-engine';

await init();
const engine = new Engine();
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
| `lemma.js` | Public entry: `init`, `initSync`, `Engine` |
| `lemma.bindings.js` | wasm-pack glue (do not import directly) |
| `lsp.js` | `serve`, `ServerConfig` for browser LSP |

## API

- **`init()`** — await once (browser).
- **`initSync({ module })`** — Node + `readFileSync('…/lemma_bg.wasm')`.
- **`Engine`** — `load`, `list`, `show`, `run`, `format`; `invert` throws.
- **`@benrogmans/lemma-engine/lsp`** — LSP streams after `init()`.

**Spec id** (for `show` / `run`): `name` or `name~` + 8 hex chars (same as CLI).

See [engine/wasm/README.md](../engine/wasm/README.md).

## Build from source

```bash
cd lemma/engine
node wasm/build.js
```

Output: `engine/pkg/`.
