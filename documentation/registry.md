# Registry

A **Registry** resolves external `@...` references to Lemma source text. The default registry is [LemmaBase.com](https://lemmabase.com).

You can compile Lemma without a registry for complete isolation, or implement your own private registry. Authentication and authorization are not part of the Registry API yet.

---

## The engine never fetches

The `Engine` does not hold a registry and never performs network calls. External `@...` references must be resolved before loading into the engine:

- **CLI:** `lemma get` resolves `@` references and caches them in `.deps/` inside the workspace directory. All other commands (`run`, `server`, `schema`, `show`, `list`, `mcp`) load cached deps via `load_from_paths` or `load` in a loop. Since there is no lock file, `.deps/` should be checked into version control.
- **Crate users:** Call `resolve_registry_references` to resolve deps, then pass the resulting source to `engine.load(code, attribute?)` (e.g. in a loop) or use `load_from_paths`.
- **WASM:** Resolve deps via `resolve_registry_references` with the browser `fetch()` fetcher, then pass each bundle to `engine.load()` in a loop.

If `@...` references are not resolved before loading, planning will report them as missing specs.

---

## The Registry trait

Implement `lemma::Registry`. All methods receive the identifier **without** the leading `@` (e.g. `"user/workspace/somespec"` for `spec @user/workspace/somespec`).

### Methods

| Method | Purpose |
|--------|---------|
| `get(&self, name) -> Result<RegistryBundle, RegistryError>` | Download all temporal versions for an `@...` identifier (both spec refs and data imports). |
| `url_for_id(&self, name, effective) -> Option<String>` | Optional: return a URL for editor navigation. |

The trait is **async** and requires `Send + Sync`. On WASM the future is `?Send`.

### Types

- **`RegistryBundle`** -- returned on success:
  - `lemma_source: String` -- raw Lemma source (one or more `spec ...` blocks). All spec names and references **must** use `@`-prefixed names (see [Bundle requirements](#bundle-requirements)).
  - `attribute: String` -- source identifier for diagnostics (e.g. `"@lemma/std/finance"`).

- **`RegistryError`** -- returned on failure:
  - `message: String` -- human-readable description.
  - `kind: RegistryErrorKind`:
    - `NotFound` -- spec or type not found.
    - `Unauthorized` -- access denied.
    - `NetworkError` -- transport failure.
    - `ServerError` -- server-side error.
    - `Other` -- anything else.

---

## Resolving dependencies

Call `resolve_registry_references` with a `Context`, sources map, and your registry:

```rust
use lemma::{resolve_registry_references, Engine, ResourceLimits};
use lemma::engine::Context;
use std::collections::HashMap;

let mut context = Context::new();
let mut sources = HashMap::new();
// ... parse and insert specs into context ...

let registry = my_registry_impl;
resolve_registry_references(&mut context, &mut sources, &registry, &ResourceLimits::default())
    .await?;

let mut engine = Engine::new();
for (path, code) in sources {
    engine.load(&code, Some(&path))?;
}
```

---

## Bundle requirements

The engine enforces strict isolation between local specs and registry specs. A `RegistryBundle` must satisfy all of the following:

1. **All spec declarations must use `@`-prefixed names.** A bundle must not contain bare-named specs like `spec billing` — only `spec @org/project/billing`.

2. **All references must use `@`-prefixed names.** This includes `with` references, `data x from ...`, and inline type annotations with `from`. A registry spec must not reference a bare name like `spec local_rates`.

3. **All dependencies must be inlined.** If `spec @org/billing` references `spec @org/rates`, the bundle must include both specs. The engine resolves transitive `@` references automatically, but the bundle should be self-contained when possible.

The registry is responsible for rewriting names. Authors may write bare names on the registry platform — the registry adds the `@` prefix when serving the bundle. The engine rejects bundles that violate these rules.

---

## LemmaBase (default registry)

When the `registry` feature is enabled, **LemmaBase** is available. It resolves identifiers via `GET https://lemmabase.com/@{identifier}.lemma`. The LSP uses `url_for_id` for clickable links.

---

## Summary

| Goal | What to do |
|------|------------|
| Implement a registry | Implement the `Registry` trait. |
| Resolve dependencies | Call `resolve_registry_references`, then pass resolved sources to `engine.load()` (in a loop) or `load_from_paths`. |
| Use no registry | Pass all files to `engine.load()` (in a loop) or `load_from_paths`. Unresolved `@...` refs fail during planning. |
