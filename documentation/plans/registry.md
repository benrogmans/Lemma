# Registry

Lemma supports configuring a **Registry** in the engine to resolve **external references**.

Only two syntaxes are Registry-driven:

```lemma
type money from @lemma/std/finance
fact external_doc = doc @user/space/somedoc
```

Everything else (including `doc somedoc` without a leading `@`) resolves within the locally-loaded document set as it does today.

## Rules for `@...`

- **No complicated logic**: if the identifier starts with `@`, it is a Registry identifier.
- **Reserved words are allowed**: `@doc` is valid and `"doc"` is passed to the Registry as-is.
- **Allowed characters**: identifiers follow the same segment rules as doc identifiers in the parser grammar (see “Grammar changes” below).
- **Errors are fatal**: failing to resolve a Registry identifier is a hard error (no fallback).
- **No lock file**: Lemma does not have a lock file. Reproducibility depends on the Registry’s identifier/versioning guarantees.
- **Security/auth/caching are out of scope**: these are implementation details of a particular Registry.

## What the Registry returns (docs)

For `doc @...`, the Registry returns **Lemma source text** (a collection of raw Lemma docs).

Rationale: the current pipeline parses source into `Vec<LemmaDoc>`, then planning (`planning::plan`) builds an `ExecutionPlan` from a `main_doc` plus the full `all_docs` set. Returning an `ExecutionPlan` would bypass (and likely conflict with) cross-document planning/validation.

The Registry returns a **workspace bundle**: the set of documents needed to evaluate/plan the requested external document, including its dependencies. This may be represented as multiple `doc ...` blocks inside a single source string.

The engine will call `parse(...)` on the returned source and add the resulting documents to `all_docs` before planning.

## Registry rewriting contract (docs)

To keep Lemma’s core resolution logic simple and avoid name collisions, the Registry is responsible for rewriting external document names and references into their canonical, globally unique form.

Example (authored in a Registry workspace):

```lemma
doc example
fact e = doc somedoc
```

Example (returned to Lemma engine by the Registry):

```lemma
doc @user/workspace/example
fact e = doc @user/workspace/somedoc
```

Key points:

- The Registry rewrites **all** returned document declarations to their canonical `@user/workspace/...` doc names.
- The Registry rewrites **only** returned **local-style** `doc` references (no leading `@`) so they point to canonical `@user/workspace/...` doc names (i.e., no unresolved local `doc somedoc` links remain inside Registry-returned content).
- The Registry does **not** rewrite/normalize/validate already-Registry-qualified `doc @...` references. If the source already contains `doc @something`, it is left as-is.
- The same rule applies to **type imports** in returned content:
  - local-style `type X from somedoc` must be rewritten to `type X from @user/workspace/somedoc`
  - already-Registry-qualified `type X from @something` is left as-is
- Lemma treats the full `doc_name` string (including the leading `@` and any `/` segments) as the document key.

## Versioning

Versioning semantics are **Registry-defined**. Lemma does not parse or enforce a “version field”.

If a Registry supports versioning, it should encode it into the identifier string (examples are conventions, not requirements):

- `@user/workspace/somedoc/v1.2.3`
- `@user/workspace/somedoc-1.2.3`
- `@user/workspace/somedoc.1.2.3`

## URL mapping (for editor/LSP navigation)

The Registry should be able to map Registry identifiers (`@...`) to a human-facing URL.

Goal: in the editor, Ctrl/Cmd+clicking on an external reference like `doc @external/space/doc` (or a type import like `type money from @lemma/std/finance`) opens the URL defined by the configured Registry implementation.

Lemma core and the LSP must **not** hardcode URL formats.

## Grammar changes (required)

We need to support Registry-returned document declarations like:

```lemma
doc @user/workspace/somedoc
```

But we must **not** allow users to declare local docs with slashes:

```lemma
doc mydoc/v12   // NOT allowed (local docs)
```

### Proposed grammar shape

Introduce a single identifier segment rule (no `/`), then build local vs registry doc names on top:

- **`doc_ident`**: one segment (no `/`)
- **`local_doc_name`**: a single segment (user-authored doc names)
- **`registry_doc_name`**: `@` + slash-separated segments (Registry-returned doc names)
- **`doc_name`**: `registry_doc_name | local_doc_name`

This `doc_name` is then used in:

- `doc_declaration` (`doc ...`)
- `doc_reference` (`doc ...` as a fact value)
- `type_import_def` (`type ... from ...`)

## Parser changes

Parsing should remain “pass-through”:

- A `doc_name` token (local or `@...`) is stored as a plain `String`.
- No additional rewriting occurs inside Lemma.

Concretely, ensure:

- `parsing/mod.rs`: `doc` declarations can parse `doc @...` and set `LemmaDoc.name` to the full `@...` string.
- `parsing/facts.rs`: `doc_reference` can parse `doc @...` and store it as `FactValue::DocumentReference("@...".to_string())`.

## Planner changes

The planner already resolves document traversal by looking up referenced docs by name in the `all_docs` map.

Required behavior:

- Treat `@user/workspace/...` as a normal document name key.
- Keep behavior strict: if a referenced document name is not present, return a fatal “Document not found” error (no fallback).

Recommended hardening:

- Detect duplicate document names up-front and error (to avoid `HashMap` overwrite behavior). This should remain fatal.

## Tests (no Registry implementation needed)

Add tests to cover the new syntax and restrictions:

- **Parse success**: `doc @user/workspace/somedoc` is accepted as a document name.
- **Parse success**: `fact e = doc @user/workspace/somedoc` is accepted as a doc reference.
- **Parse failure**: `doc mydoc/v12` is rejected (local doc names cannot contain `/`).
- **Plan success**: a source string containing:
  - `doc @user/workspace/example` referencing `doc @user/workspace/somedoc`
  - and the referenced `doc @user/workspace/somedoc`
  should plan successfully.

## Non-goals

- Registry auth/security configuration
- Caching strategies, retries, offline behavior
- Defining a universal version syntax (Registry-defined)

## Implementation (start designing the API)

### Core principle

Lemma only needs two Registry capabilities:

- **Resolve** an external identifier to Lemma source text (for evaluation/planning)
- **Map** an external identifier to a URL (for editor navigation)

Everything else (auth, caching, retries, etc.) is Registry-implementation specific.

### Proposed trait (docs + types)

The engine/LSP treat the identifier after `@` as an opaque string, and pass it to the Registry as-is.

```rust
pub struct RegistryBundle {
    /// Lemma source containing one or more `doc ...` blocks.
    /// Registry-returned doc declarations must use canonical `doc @user/workspace/...` names.
    pub lemma_source: String,

    /// Optional source identifier used for diagnostics/proofs (e.g. "@user/workspace/somedoc").
    pub attribute: String,
}

pub struct RegistryError {
    pub message: String,
}

pub trait Registry: Send + Sync {
    /// Resolve a `doc @...` reference.
    ///
    /// Input is the identifier *without* the leading `@` (e.g. "user/workspace/somedoc").
    /// The Registry returns a workspace bundle with rewritten canonical doc names/references.
    fn resolve_doc(&self, id: &str) -> Result<RegistryBundle, RegistryError>;

    /// Map a Registry identifier to a human-facing URL for navigation.
    ///
    /// Input is the identifier *without* the leading `@`.
    /// Returning `None` means: no URL available for this id.
    fn url_for_id(&self, id: &str) -> Option<String>;

    /// Resolve a `type ... from @...` reference.
    ///
    /// Input is the identifier *without* the leading `@` (e.g. "lemma/std/finance").
    ///
    /// The Registry returns a workspace bundle containing the document(s) needed to resolve
    /// the imported types. Returned content must follow the same rewriting rules as `resolve_doc`:
    /// canonical `doc @user/workspace/...` names, and local-style references/imports rewritten.
    fn resolve_type(&self, id: &str) -> Result<RegistryBundle, RegistryError>;
}
```

Notes:

- “Errors are fatal” means: `resolve_doc` failures are propagated as hard `LemmaError`s (no fallback).
- “Errors are fatal” means: `resolve_type` failures are propagated as hard `LemmaError`s (no fallback).
- `url_for_id` is intentionally best-effort (used only for navigation).