# Registry

A **Registry** in Lemma resolves external `@...` references to Lemma source text. The engine calls the registry when it encounters `doc @identifier` or `type ... from @identifier` so that documents and types can be loaded from outside the current workspace.

A Registry can fetch Lemma docs and types from anywhere: in-memory maps, filesystem lookups, databases, or remote APIs. The only contract is the **Registry trait**: given an identifier (the part after `@`), return a bundle of Lemma source or an error. By default Lemma uses the LemmaBase.com Registry, but you are invited to compile Lemma without a registry for complete isolation, or implement your own private Registry.

Note: authentication and authorization is not part of the Registry API yet but is to come. The API is not stable yet.

---

## When the engine uses a Registry

- **Document references:** `fact helper = doc @org/example/helper` — the engine needs the Lemma source for the document named by `@org/example/helper`.
- **Type imports:** `type money from @lemma/std/finance` — the engine needs the Lemma source for the document that defines the imported types.

Resolution happens during `add_lemma_files`, after parsing and before planning. The engine collects all unresolved `@...` identifiers, calls the registry for each, parses the returned source, and repeats until no unresolved references remain. If the registry returns an error, the engine reports a `Error::Registry` and does not proceed.

---

## The Registry trait

Implement the `lemma::Registry` trait. All methods receive the **identifier without** the leading `@` (e.g. `"user/workspace/somedoc"` for `doc @user/workspace/somedoc`).

### Methods

| Method | Purpose |
|--------|--------|
| `resolve_doc(&self, identifier: &str) -> Result<RegistryBundle, RegistryError>` | Resolve a `doc @...` reference. Return Lemma source for the requested document (and optionally its dependencies). |
| `resolve_type(&self, identifier: &str) -> Result<RegistryBundle, RegistryError>` | Resolve a `type ... from @...` reference. Return Lemma source for the doc(s) that define the imported types. |
| `url_for_id(&self, identifier: &str) -> Option<String>` | Optional: return a URL for editor navigation (e.g. "open in browser"). Return `None` if no URL is available. |

The trait is **async** and requires `Send + Sync` so the engine can use it across threads. On WASM the async future is `?Send` (e.g. for `fetch()`).

### Types you work with

- **`RegistryBundle`** — what you return on success:
  - `lemma_source: String` — raw Lemma source (one or more `doc ...` blocks). Doc names in the source are **without** `@` (e.g. `doc org/example/helper`).
  - `attribute: String` — source identifier for diagnostics and proofs, typically `"@"` + identifier (e.g. `"@org/example/helper"`).

- **`RegistryError`** — what you return on failure:
  - `message: String` — human-readable description.
  - `kind: RegistryErrorKind` — so the engine can give appropriate suggestions:
    - `NotFound` — document or type not found (e.g. HTTP 404).
    - `Unauthorized` — access denied (e.g. HTTP 401, 403).
    - `NetworkError` — transport failure (DNS, timeout, connection refused).
    - `ServerError` — server-side error (e.g. HTTP 5xx).
    - `Other` — anything else.

---

## Creating a custom Registry

You can implement the trait in any way. Examples:

- **In-memory:** Store `HashMap<String, RegistryBundle>` and look up by identifier. No I/O.
- **Filesystem:** Map identifiers to paths (e.g. `org/example/helper` → `./registry/org/example/helper.lemma`) and read files.
- **HTTP:** Call a REST or other API; map status codes to `RegistryErrorKind` (404 → `NotFound`, 401/403 → `Unauthorized`, 5xx → `ServerError`, transport failure → `NetworkError`).
- **Database or other backend:** Query by identifier and return Lemma source (or an error) however you like.

The engine does not care how you obtain the source; it only calls `resolve_doc` and `resolve_type` and uses the returned bundles or errors.

### Minimal in-memory example

```rust
use lemma::{Engine, Registry, RegistryBundle, RegistryError, RegistryErrorKind};
use std::collections::HashMap;
use std::sync::Arc;

struct InMemoryRegistry {
    bundles: HashMap<String, RegistryBundle>,
}

impl InMemoryRegistry {
    fn new() -> Self {
        Self { bundles: HashMap::new() }
    }

    fn add(&mut self, identifier: &str, lemma_source: &str) {
        self.bundles.insert(
            identifier.to_string(),
            RegistryBundle {
                lemma_source: lemma_source.to_string(),
                attribute: format!("@{}", identifier),
            },
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Registry for InMemoryRegistry {
    async fn resolve_doc(&self, identifier: &str) -> Result<RegistryBundle, RegistryError> {
        self.bundles.get(identifier).cloned().ok_or(RegistryError {
            message: format!("not found: {}", identifier),
            kind: RegistryErrorKind::NotFound,
        })
    }

    async fn resolve_type(&self, identifier: &str) -> Result<RegistryBundle, RegistryError> {
        self.bundles.get(identifier).cloned().ok_or(RegistryError {
            message: format!("not found: {}", identifier),
            kind: RegistryErrorKind::NotFound,
        })
    }

    fn url_for_id(&self, identifier: &str) -> Option<String> {
        Some(format!("https://my-registry/{}", identifier))
    }
}
```

The registry is just a lookup.

---

## Registering a Registry with the engine

The engine holds an **optional** registry. When you add Lemma files, it uses that registry to resolve `@...` references.

- **Default:** With the `registry` feature enabled, `Engine::new()` and `Engine::with_limits(...)` use **LemmaBase**, which fetches source from LemmaBase.com. With the feature disabled, no registry is set.
- **Custom registry:** Call `with_registry` when building the engine. The registry is passed as `Arc<dyn Registry>`.

```rust
let my_registry = Arc::new(my_registry_impl);
let mut engine = Engine::new().with_registry(my_registry);

let mut files = HashMap::new();
files.insert("app.lemma".into(), r#"
    doc app
    fact helper = doc @my/helper
    rule value = helper.x
"#.into());

engine.add_lemma_files(files).await?;
```

- **No registry:** To disable registry resolution (e.g. for a sandbox), construct an engine without a registry. The engine tests do this by building `Engine { ..., registry: None, ... }`. Any `@...` reference will then fail during `add_lemma_files` with a resolution error.

So: **creating** a registry means implementing the `Registry` trait; **registering** it means passing `Arc::new(your_impl)` to `Engine::with_registry(...)` (and using that engine when calling `add_lemma_files`).

---

## LemmaBase (default registry)

When the `registry` feature is enabled, the default registry is **LemmaBase**, which resolves identifiers by GET to `https://lemmabase.com/@{identifier}.lemma`. You can replace it with your own via `with_registry`. The LSP and other tools that need "open in browser" links use `url_for_id`; LemmaBase returns `https://lemmabase.com/@{identifier}` for that.

---

## Summary

| Goal | What to do |
|------|------------|
| Implement a Registry | Implement the `Registry` trait: `resolve_doc`, `resolve_type`, and optionally `url_for_id`. |
| Use your Registry | Build the engine with `Engine::new().with_registry(Arc::new(your_registry))`, then call `add_lemma_files` as usual. |
| Use no registry | Use an engine built with `registry: None` so that `@...` references are not resolved (they will fail). |
| Rely on default | Use `Engine::new()` with the `registry` feature enabled to use LemmaBase. |

The contract is the trait and the types (`RegistryBundle`, `RegistryError`, `RegistryErrorKind`). How you produce the bundle is up to you.
