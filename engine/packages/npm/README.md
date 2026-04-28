# @lemmabase/lemma-engine

> [Lemma](https://github.com/lemma/lemma) is a declarative language for business rules. **This package is the engine, compiled to WebAssembly** - runs in the browser, on Node, Bun, Deno, Cloudflare Workers, Vercel Edge, etc.

Pricing tiers, tax brackets, leave entitlement, eligibility checks, discount stacks: the rules that change, that auditors ask about, that legal writes in PDFs and engineers re-implement in operational code... Lemma is a language built specifically for your business rules. It is readable by stakeholders, executable anywhere, and impossible to drift out of sync.

```lemma
spec pricing 2026-01-01

data money: scale
  -> unit eur 1.00
  -> decimals 2

data quantity : number
data is_vip   : false

rule unit_price:
  20 eur
  unless quantity >= 10 then 18 eur
  unless quantity >= 50 then 16 eur
  unless is_vip         then 15 eur

rule total:
  unit_price * quantity
```

```javascript
import { Lemma } from '@lemmabase/lemma-engine';

const engine = await Lemma();
await engine.load(pricing, 'pricing.lemma');

const response = engine.run('pricing', [], { quantity: 50, is_vip: false }, null);
// response.results.unit_price → 16 eur
// response.results.total      → 800 eur
```

The `Response` carries every rule's value (or `veto` if no result could be computed), the input snapshot, and the source location of every rule that fired, allowing you to render an audit trail in your UI.

## Why use it from JavaScript?

- **Deterministic.** `(spec, data, effective_date) → result`. No DB, no clock, no ambient state. Same inputs → same outputs, every time.
- **Explainable.** The `Response` tells you which rules contributed and why; pair it with the [CLI](https://github.com/lemma/lemma) for a full reasoning trace.
- **Time-aware.** Multiple versions of the same spec coexist. Pass an `effective` date and the engine resolves the version in force on that day.
- **Statically checked.** Type errors, missing data, cycles, scale-family mismatches - all caught at `load()` time. Bad specs never reach `run()`.
- **Runs anywhere V8 does.** ~2 MB WASM, no native binary, no postinstall script.
- **Editor in a tab.** Includes an in-process language server and a Monaco adapter, so you can build a real Lemma editor experience client-side - diagnostics, completion, formatting... even without setting up a server.

## Install

```bash
npm install @lemmabase/lemma-engine
```

## Browser

```javascript
import { Lemma } from '@lemmabase/lemma-engine';

const engine = await Lemma();
```

`Lemma()` initializes the WASM module once and returns an `Engine`. Serve over **http(s)**, not `file://`. For manual control: `init()` then `new Engine()`.

If your bundler emits IIFE, can't resolve `import.meta.url`, or refuses to ship `lemma_bg.wasm` as a separate asset, use the inlined entry - it embeds the wasm bytes in the JS bundle:

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
| `load(code, attribute?)` | Parse and validate a `.lemma` spec set. Resolves on success; rejects with `EngineError[]`. |
| `list()` | All loaded specs with metadata and an inlined `SpecSchema`. |
| `schema(spec, effective?)` | `SpecSchema` for the spec at the given effective date. |
| `run(spec, rules, data, effective?)` | Evaluate. `rules: []` runs everything; pass an array to filter. Returns a `Response`. |
| `format(code, attribute?)` | Canonical formatting; throws `EngineError` on parse error. |

Full TypeScript types are bundled - see `lemma.d.ts`.

## Status

Lemma is in early development. Expect breaking changes between minor versions; **don't put it in front of paying customers yet**. Production-readiness tracking lives in the [main repo](https://github.com/lemma/lemma).

## Related

- [`lemmabase.com`](https://lemmabase.com): public database for Lemma Specs
- [`lemma-cli`](https://crates.io/crates/lemma-cli): REPL, HTTP server, MCP server, formatter
- [`lemma-engine`](https://crates.io/crates/lemma-engine): same engine as a Rust crate
- [`lemma_engine` on Hex](https://hex.pm/packages/lemma_engine): Elixir bindings via Rustler
- VS Code / Cursor extension: search "Lemma Language" in the marketplace

## License

Apache-2.0
