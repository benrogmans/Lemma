# @benrogmans/lemma-engine

Embeddable Lemma engine (WebAssembly).

npm `description` / `keywords` / `homepage`: edit **`NPM_BRANDING`** in `wasm/build.js` (not Cargo).

## Install

```bash
npm install @benrogmans/lemma-engine
```

## Browser / bundler

```javascript
import { init, Engine } from '@benrogmans/lemma-engine';

await init();
const engine = new Engine();
```

`lemma_bg.wasm` loads from the package URL. Serve over **http(s)** (not `file://`).

## Node

```javascript
import { readFileSync } from 'fs';
import { initSync, Engine } from '@benrogmans/lemma-engine';

initSync({ module: readFileSync('node_modules/@benrogmans/lemma-engine/lemma_bg.wasm') });
const engine = new Engine();
```

## LSP (browser streams)

```javascript
import { init, Engine } from '@benrogmans/lemma-engine';
import { serve, ServerConfig } from '@benrogmans/lemma-engine/lsp';

await init();
await serve(new ServerConfig(intoServer, fromServer));
```

## API (`Engine`)

| Method | |
|--------|--|
| `load(code, attribute)` | Promise; reject → `string[]` |
| `list()` | Spec entries |
| `show(spec, effective?)` | `SpecSchema` |
| `run(spec, rules, facts, effective?)` | `Response` |
| `format(code, attribute?)` | string or throw |
| `invert(...)` | throws (N/A) |

## Build (maintainers)

`node wasm/build.js`: wasm-pack → `lemma.bindings.js` + `lemma_bg.wasm` → copy checked-in `lemma-entry.js` / `lsp-entry.js` / `*.d.ts` into `pkg/`. Do not edit generated bindings by hand.

## License

Apache-2.0
