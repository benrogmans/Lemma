# Registry

A **Registry** resolves external `@...` references to Lemma source text. The default registry is [LemmaBase.com](https://lemmabase.com).

You can compile Lemma without a registry for complete isolation, or implement your own private registry. Authentication and authorization are not part of the Registry API yet.

---

## The engine never fetches

The `Engine` does not hold a registry and never performs network calls. External `@...` references must be resolved before calling `add_lemma_files`:

- **CLI:** `lemma get` resolves `@` references and caches them in the global deps directory. All other commands (`run`, `server`, `hash`, `show`, `list`, `mcp`) load cached deps as regular `.lemma` files.
- **Crate users:** Call `resolve_registry_references` to resolve deps, then include the resulting source in the file map passed to `add_lemma_files`.
- **WASM:** Resolve deps via `resolve_registry_references` with the browser `fetch()` fetcher, then pass everything to `add_lemma_files`.

If `@...` references are not resolved before `add_lemma_files`, planning will report them as missing specs.

---

## The Registry trait

Implement `lemma::Registry`. All methods receive the identifier **without** the leading `@` (e.g. `"user/workspace/somespec"` for `spec @user/workspace/somespec`).

### Methods

| Method | Purpose |
|--------|---------|
| `fetch_specs(&self, name) -> Result<RegistryBundle, RegistryError>` | Fetch all temporal versions for a `spec @...` reference. |
| `fetch_types(&self, name) -> Result<RegistryBundle, RegistryError>` | Fetch all temporal versions for a `type ... from @...` reference. |
| `url_for_id(&self, name, effective) -> Option<String>` | Optional: return a URL for editor navigation. |

The trait is **async** and requires `Send + Sync`. On WASM the future is `?Send`.

### Types

- **`RegistryBundle`** -- returned on success:
  - `lemma_source: String` -- raw Lemma source (one or more `spec ...` blocks). Spec names should be fully qualified (e.g. `spec @lemma/std/finance`).
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
engine.add_lemma_files(sources)?;
```

---

## LemmaBase (default registry)

When the `registry` feature is enabled, **LemmaBase** is available. It resolves identifiers via `GET https://lemmabase.com/@{identifier}.lemma`. The LSP uses `url_for_id` for clickable links.

---

## Summary

| Goal | What to do |
|------|------------|
| Implement a registry | Implement the `Registry` trait. |
| Resolve dependencies | Call `resolve_registry_references`, then pass resolved sources to `engine.add_lemma_files`. |
| Use no registry | Pass all files (including deps) directly to `add_lemma_files`. Unresolved `@...` refs fail during planning. |
