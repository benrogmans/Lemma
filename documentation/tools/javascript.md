---
nav_title: JavaScript / TypeScript
nav_order: 30
---

# JavaScript / TypeScript

`@lemmabase/lemma-engine` runs in the browser, Node, Bun, Deno, and edge runtimes.

## Install

```bash
npm install @lemmabase/lemma-engine
```

## Usage

```javascript
import { Lemma } from '@lemmabase/lemma-engine';

const engine = await Lemma();
await engine.load(pricing, 'pricing.lemma');

const response = engine.run(null, 'pricing', null, { quantity: 50, is_vip: false }, null);
// response.results.unit_price → 16 eur
// response.results.total      → 800 eur
```

`Lemma()` initializes the WASM module once and returns an `Engine`. The Response carries every Rule's value (or Veto if no result could be computed), the input snapshot, and the source location of every Rule that fired.

## Browser

```javascript
import { Lemma } from '@lemmabase/lemma-engine';

const engine = await Lemma();
```

Serve over http(s), not `file://`. For manual control: `init()` then `new Engine()`.

If your bundler emits IIFE, can't resolve `import.meta.url`, or refuses to ship `lemma_bg.wasm` as a separate asset, use the inlined entry:

```javascript
import { Lemma } from '@lemmabase/lemma-engine/iife';
```

esbuild users get an auto-rewriting plugin:

```javascript
import { lemmaEngineEsbuildPlugin } from '@lemmabase/lemma-engine/esbuild';

esbuild.build({ /* ... */ plugins: [lemmaEngineEsbuildPlugin()] });
```

## Node

Identical to the browser path:

```javascript
import { Lemma } from '@lemmabase/lemma-engine';

const engine = await Lemma();
```

For zero-fetch startup with a preloaded module: `initSync({ module })` then `new Engine()`.

## In-process LSP + Monaco

```javascript
import { init } from '@lemmabase/lemma-engine';
import { LspClient } from '@lemmabase/lemma-engine/lsp-client';

await init();
const client = new LspClient(monaco);
await client.start();
await client.initialize();

client.onDiagnostics((uri, diagnostics) => { /* render */ });
client.didOpen('file:///pricing.lemma', 'lemma', 1, source);
```

A pre-wired Monaco adapter ships at `@lemmabase/lemma-engine/monaco`.

## API

`Engine` (returned by `Lemma()` or `new Engine()`):

| Method | Description |
|--------|-------------|
| `load(code, attribute?)` | Parse and validate a `.lemma` Spec set. Resolves on success; rejects with `EngineError[]`. |
| `load_batch(sources, dependency?)` | Load many sources in one planning pass. |
| `fetch(name)` | Download registry source only; resolves with `{ source, id }`. Does not load. |
| `list()` | JSON array of `ResolvedRepository`: each has `repository` and `specs`. |
| `format_repository(repo)` | Canonical Lemma source for a loaded repository, formatted from the in-engine AST. |
| `schema(repo, name, effective?)` | `SpecSchema`; `repo` null for workspace. |
| `run(repo, name, ruleNames, data, effective?, explain?)` | Evaluate. Omit/`null` `ruleNames` for all Rules; pass a non-empty array to scope. Returns a `Response`. |
| `format(code, attribute?)` | Canonical formatting; throws `EngineError` on parse error. |

Full TypeScript types are bundled (see `lemma.d.ts`).

## Registry dependencies

Specs that reference `uses … @org/pkg` need that package available. `fetch` only downloads; call `load_batch` to load the dependency, then load your workspace:

```javascript
import { Lemma } from '@lemmabase/lemma-engine';

const engine = await Lemma();
const { source, id } = await engine.fetch('@iso/countries');
await engine.load_batch({ '': source }, id);
await engine.load(sourceThatUsesStd, 'app.lemma');
```

In the browser, the registry must allow your origin (CORS).
