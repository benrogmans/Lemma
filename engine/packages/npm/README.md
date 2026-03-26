# @benrogmans/lemma-engine

Embeddable Lemma engine (WebAssembly).

npm `description` / `keywords` / `homepage`: edit **`NPM_BRANDING`** in `build.js` (not Cargo).

## Install

```bash
npm install @benrogmans/lemma-engine
```

## Browser / bundler

```javascript
import { Lemma } from '@benrogmans/lemma-engine';

const engine = await Lemma();
```

`Lemma()` initializes WASM once and returns an `Engine`. Serve over **http(s)** (not `file://`). For manual init: `init()` then `new Engine()`.

If your bundler outputs IIFE (or otherwise breaks `import.meta.url`), use:

```javascript
import { Lemma } from '@benrogmans/lemma-engine/iife';

const engine = await Lemma();
```

This entry embeds WASM bytes and avoids external wasm URL handling.

## esbuild auto-handler

If you use esbuild JS API, plugin rewrites root import to `/iife` automatically:

```javascript
import { lemmaEngineEsbuildPlugin } from '@benrogmans/lemma-engine/esbuild';
```

## Node

```javascript
import { Lemma } from '@benrogmans/lemma-engine';

const engine = await Lemma();
```

Or with a preloaded buffer (e.g. no async fetch): `initSync({ module })` then `new Engine()`.

## LSP (browser streams)

Call `init()` first. Use `LspClient`; `start()` uses the bundled LSP (no need to pass `serve`/`ServerConfig`). Optional: `start(serve, ServerConfig)` to override.

```javascript
import { init } from '@benrogmans/lemma-engine';
import { LspClient } from '@benrogmans/lemma-engine/lsp-client';

await init();
const client = new LspClient(monaco);
await client.start();
await client.initialize();
client.onDiagnostics((uri, diagnostics) => { /* ... */ });
client.didOpen(uri, 'lemma', 1, documentText);
```

## API (`Engine`)

| Method | |
|--------|--|
| `load(code, attribute)` | Promise; reject → `string[]` |
| `list()` | Spec entries |
| `schema(spec, effective?)` | `SpecSchema` |
| `run(spec, rules, facts, effective?)` | `Response` |
| `format(code, attribute?)` | string or throw |

## Build (maintainers)

`node build.js`: wasm-pack → `lemma.bindings.js` + `lemma_bg.wasm` → copy checked-in `lemma-entry.js` / `lsp-entry.js` / `*.d.ts` into `dist/`. Do not edit generated bindings by hand.

## License

Apache-2.0
