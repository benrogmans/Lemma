---
nav_title: LemmaBase
nav_order: 30
---

# LemmaBase Registry

A registry is where Lemma looks up the specs you import from outside your own workspace. When a spec refers to something with an @ prefix, like @iso/countries, that reference has to be turned into actual Lemma source before the engine can use it. Resolving those references is the registry's job. By default Lemma uses LemmaBase.com, but you can run entirely without a registry if you want complete isolation, or you can plug in your own private one. There is no authentication or authorization in the Registry API yet.

## The engine never fetches

The engine itself never touches the network. It does not know about any registry, and it never downloads anything. Every external reference has to be resolved into source text first, and only then is that source handed to the engine.

How you resolve references depends on how you run Lemma. From the command line you run lemma fetch, which follows the @ references and saves the downloaded specs into a lemma_deps folder inside your workspace. Every other command, such as run, server, show, list, mcp, and format, then reads your local .lemma files together with whatever is cached in lemma_deps. There is no lock file, so you should commit lemma_deps to version control.

If you are calling Lemma as a Rust crate, you resolve references yourself by calling resolve_registry_references, and then you load the resulting source into the engine. The example further down shows exactly how. If you are using the JavaScript package from npm (`@lemmabase/lemma-engine`), call `fetch` to download the source and then `load` (using the returned id as the source label) before loading your own specs.

Whatever the path, the rule is the same: resolve first, load second. If you load a spec while some of its @ references are still unresolved, planning will simply report those references as missing specs.

## The Registry trait

If you want to supply specs from your own source instead of LemmaBase, you implement the Registry trait. Every method receives the repository name exactly as it appears in the source, including the `@` prefix, so a reference to `@iso/countries` arrives as the string `"@iso/countries"`. The trait is asynchronous and must be safe to share across threads (Send + Sync), except on WebAssembly where that requirement is relaxed.

There are two methods. The main one, get, takes a repository name and downloads all of its temporal versions, returning either the source bundle or an error. The optional one, url_for_id, returns a URL that editors can use to navigate to a spec.

A successful download comes back as a bundle holding two things: the raw Lemma source, which is one or more top-level spec declarations, and an attribute string used to label the source in diagnostics. The bundle requirements below explain what that source has to look like.

A failure comes back as an error carrying a human-readable message and a kind that says what went wrong: the spec was not found, access was denied, the network failed, the server failed, or something else.

## Resolving dependencies

To resolve references from Rust, call resolve_registry_references with a context, a map of your source files, and your registry implementation:

```rust
use lemma::{resolve_registry_references, Context, Engine, ResourceLimits, SourceType};
use std::collections::HashMap;
use std::sync::Arc;

let mut context = Context::new(); // includes embedded stdlib (`repo lemma`, `spec units`); import with `uses lemma units`
// List: context.repositories / Engine::list() always includes `lemma`. Source: engine.source(Some("lemma"), None, None)?.
let mut sources = HashMap::new();
// ... insert local workspace specs into `context`, mirror their text in `sources` ...

let registry = my_registry_impl;
resolve_registry_references(&mut context, &mut sources, &registry, &ResourceLimits::default())
    .await?;

// Typical pattern: rebuild or extend an `Engine` from the merged `sources` map.
let mut engine = Engine::new();
let batch: Vec<(SourceType, String)> = sources
    .into_iter()
    .map(|(path, code)| (SourceType::Path(Arc::new(path)), code))
    .collect();
engine.load(batch)?;
```

## Bundle requirements

A bundle is just ordinary Lemma source. A published registry bundle opens with a `repo` line whose name matches the registry id, for example `repo @iso/countries` for the `@iso/countries` dependency—that `@` name is assigned when the bundle is published, not chosen freely in a local workspace. Once everything is loaded, the engine keeps dependencies isolated from each other: a repository loaded as a dependency will never merge with your workspace or with another dependency, and every spec in a repository must come from the same place.

Within a bundle, spec names are just normal identifiers. You write `spec billing` or `spec rates` exactly as you would in a local file. When one bundle needs something from another registry repository, you qualify the import with `@`, as in `uses rates: @acme/finance rates` or `uses iso: @iso/countries alpha2`. An unqualified import like `uses x: rates` only looks inside the same repository as the spec doing the importing. In your own workspace files, declare local repos without `@` (`repo finance`).

You do not have to bundle a whole dependency tree into one response. The resolver keeps fetching unresolved references until everything is satisfied, so a single .lemma response per identifier is enough. A registry is free to serve friendlier or edited forms on its own side, as long as what the engine finally parses follows these rules.

## LemmaBase (default registry)

When the registry feature is enabled, LemmaBase is the registry Lemma uses out of the box. It looks up an identifier by fetching it directly from lemmabase.com, so @iso/countries becomes a request for the corresponding .lemma file. Editors use the navigation URL to jump from a repository reference to its page on LemmaBase, while the individual spec name links to the copy fetched into your local lemma_deps folder.

For tests there is an offline mode. LemmaBase::test() serves fixtures bundled under engine/tests/registry_fixtures/ instead of hitting the network, which is how the documentation examples that use @ references are checked.
