# Registry

A **Registry** resolves external `@...` references to Lemma source text. The default registry is [LemmaBase.com](https://lemmabase.com).

You can compile Lemma without a registry for complete isolation, or implement your own private registry. Authentication and authorization are not part of the Registry API yet.

---

## The engine never fetches

The `Engine` does not hold a registry and never performs network calls. External `@...` references must be resolved before loading into the engine:

- **CLI:** `lemma fetch` resolves `@` references and caches them in `.deps/` inside the workspace directory. All other commands (`run`, `server`, `schema`, `show`, `list`, `mcp`) load cached deps via `load_from_paths` or `load` in a loop. Since there is no lock file, `.deps/` should be checked into version control.
- **Crate users:** Call `resolve_registry_references` on a [`Context`](https://docs.rs/lemma-engine/latest/lemma/engine/struct.Context.html), then build an [`Engine`](https://docs.rs/lemma-engine/latest/lemma/engine/struct.Engine.html) and load each `(path, code)` pair with [`SourceType::Labeled`](https://docs.rs/lemma-engine/latest/lemma/engine/enum.SourceType.html) (or pass paths via `load_from_paths`).
- **WASM:** Resolve deps via `resolve_registry_references` with the browser `fetch()` fetcher, then load each resolved buffer with `engine.load(&code, SourceType::Labeled(attribute))` (or equivalent) in a loop.

If `@...` references are not resolved before loading, planning will report them as missing specs.

---

## The Registry trait

Implement `lemma::Registry`. All methods receive the full repository name as it appears in source (e.g. `"@org/project"` when Lemma references `@org/project` in a `uses` or `from` clause).

### Methods

| Method | Purpose |
|--------|---------|
| `get(&self, name) -> Result<RegistryBundle, RegistryError>` | Download all temporal versions for a repository identifier. `name` is the full name including `@` (e.g. `"@org/project"`). |
| `url_for_id(&self, name, effective) -> Option<String>` | Optional: return a URL for editor navigation. `name` is the full name including `@`. |

The trait is **async** and requires `Send + Sync`. On WASM the future is `?Send`.

### Types

- **`RegistryBundle`** -- returned on success:
  - `lemma_source: String` -- raw Lemma source (one or more top-level `spec ...` declarations). See [Bundle requirements](#bundle-requirements).
  - `attribute: String` -- source identifier for diagnostics (e.g. `"@lemma.std.finance"`).

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
use lemma::{resolve_registry_references, Context, Engine, ResourceLimits, SourceType};
use std::collections::HashMap;

let mut context = Context::new();
let mut sources = HashMap::new();
// ... insert local workspace specs into `context`, mirror their text in `sources` ...

let registry = my_registry_impl;
resolve_registry_references(&mut context, &mut sources, &registry, &ResourceLimits::default())
    .await?;

// Typical pattern: rebuild or extend an `Engine` from the merged `sources` map.
let mut engine = Engine::new();
for (path, code) in sources {
    engine.load(&code, SourceType::Labeled(path.as_str()))?;
}
```

---

## Bundle requirements

A registry bundle is ordinary Lemma source. Bundles should declare their repository with `repo @org/name` so the engine knows which repository the specs belong to.

After loading, the engine enforces **dependency isolation**: repos loaded as a dependency cannot merge with workspace repos or other dependencies' repos. All specs in a repository must share the same provenance (workspace or specific dependency ID).

1. **Spec names are normal identifiers.** Write `spec billing`, `spec rates`, and so on — the same surface syntax as local files.

2. **Cross-bundle references use `@` in `uses` / `from`.** To depend on another registry identifier, qualify it (`uses x: @org/rates rates`, `data t from @lemma/std finance`, etc.). Unqualified references (`uses x: rates`) resolve only within **the same** repository as the importing spec.

3. **Transitive loads.** The resolver fetches every unresolved repository reference until all qualifiers are satisfied. A single `.lemma` response per identifier is enough; you do not need to paste transitive dependencies into one megabundle unless your registry chooses to.

Registries may let authors edit friendlier forms on the server side, but what the engine parses must follow the rules above.

---

## LemmaBase (default registry)

When the `registry` feature is enabled, **LemmaBase** is available. It resolves identifiers via `GET https://lemmabase.com/{identifier}.lemma` (identifier already contains the `@` prefix). The LSP uses `url_for_id` for clickable links.

---

## Summary

| Goal | What to do |
|------|------------|
| Implement a registry | Implement the `Registry` trait. |
| Resolve dependencies | Call `resolve_registry_references`, or use `engine.load_dependency()` / `engine.load_dependency_from_paths()` for pre-fetched bundles. |
| Use no registry | Pass all files to `engine.load()` (in a loop) or `load_from_paths`. Unresolved `@...` refs fail during planning. |
