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
await engine.load({ 'pricing.lemma': pricing });

const response = engine.run(null, 'pricing', null, { quantity: 50, is_vip: false }, null, false);
// response.results.unit_price → 16 eur
// response.results.total      → 800 eur
```

`Lemma()` initializes the engine once and returns an `Engine`. The response carries each rule's value (or veto), per-rule `missing_data` when inputs are still unbound, and optional explanation trees when the last `run` argument is `true` ([api.v1.json](../schemas/api.v1.json)). Types and suggestions are on `engine.show(...)` (`Show.data` values are `ShowData`). Non-veto results flatten `RuleResultValue` (`display` + typed field) onto each `RuleResult`.

## Browser

```javascript
import { Lemma } from '@lemmabase/lemma-engine';

const engine = await Lemma();
```

Serve over http(s), not `file://`. For manual control: `init()` then `new Engine()`.

If your bundler emits IIFE, can't resolve `import.meta.url`, or refuses to ship the engine module as a separate asset, use the inlined entry:

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

For zero-fetch startup: `initSync({ module })` then `new Engine()`.

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
| `load(code)` | Load inline Lemma source as a volatile workspace source |
| `load(sources)` | Load multiple sources in one planning pass (object or `[label, code][]`; object keys keep insertion order, array form is the explicit ordered API; `@owner/name` keys tag registry dependencies) |
| `fetch(name)` | Download registry source only; resolves with `{ source, id }`. Does not load. |
| `list()` | JSON array of `ResolvedRepository`: each has `repository` and `specs`. |
| `show(repo?, spec, effective?)` | `Show`: interface + temporal window (no Lemma text) |
| `source(repo?, spec?, effective?)` | Formatted Lemma source (omit `spec` for whole repo) |
| `run(repo?, spec, effective?, data?, ruleNames?, explain?)` | Evaluate. Omit/`null` `ruleNames` for all rules. Returns a `Response`. With `explain: true`, per-rule `explanation` matches [api.v1.json](../schemas/api.v1.json). |
| `remove(repo?, name, effective?)` | Remove a temporal spec slice. |
| `limits()` | Resource limits for this engine. |
| `format(code, attribute?)` | Canonical formatting; throws `EngineError` on parse error. |

Full TypeScript types are bundled (see `lemma.d.ts`).

**API values (`RuleResultValue`):** when present, always `display`, plus exactly one typed field (`measure` / `ratio` / `number` / …) or `range` instead. Same shape on `ShowData.prefilled` / `ShowData.suggestion`; non-veto rule results flatten those fields onto `RuleResult` (no `value` wrapper). Measure and ratio maps hold every declared unit name → magnitude string so interactive prompts can switch units.

## Registry dependencies

Specs that `uses` a registry id such as `@iso/countries` need that dependency available. `fetch` only downloads; call `load` with the dependency id as the source label, then load your workspace:

```javascript
import { Lemma } from '@lemmabase/lemma-engine';

const engine = await Lemma();
const { source, id } = await engine.fetch('@iso/countries');
await engine.load({ [id]: source, 'app.lemma': sourceThatUsesStd });
```

In the browser, the registry must allow your origin (CORS).
