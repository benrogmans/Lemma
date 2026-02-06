# Registry

Lemma supports configuring a **Registry** in the engine to resolve **external references**.

---

## Status (implementation readiness)

| Area | Done | Remaining |
|------|------|-----------|
| **Grammar** | `doc_name` allows optional `@` and `/` segments (`lemma.pest`). Parser stores full string. The `@` prefix is the sole indicator of a Registry reference. | None. Grammar is already correct. |
| **Parser** | `doc` declarations and `doc_reference` / `type_import_def` already parse `doc_name` as a string (including `@...`). | Ensure `FactValue::DocumentReference` and type-import `from` store the full `@...` string. Add parse tests for `doc @...`. |
| **Language Server Protocol / address mapping** | The Language Server has a `Registry` trait with `url_for_id`; `StubRegistry`; document links for `doc @...` and `type ... from @...`. | None for address mapping. |
| **Engine / resolution** | Planner resolves Lemma docs by name in `all_docs`; "Document not found" for missing references. | Engine has **no** Registry and no resolution step. After parsing local files, collect `@...` references from the parsed docs, call the Registry, parse the returned source, and resolve further `@...` references recursively. Then plan with the complete document set. Harden: detect duplicate document names before planning. |
| **Registry trait (engine)** | — | Extend (or share) trait with `resolve_doc` and `resolve_type` returning `RegistryBundle`; keep `url_for_id` for the Language Server. Engine uses resolve; Language Server uses url_for_id. |
| **Tests** | — | Parse success and plan success tests as described in the "Tests" section. |

**Summary:** Parsing and Language Server navigation for `@...` are in place. Remaining work: a resolution step in the engine (after parsing local files, recursively resolve `@...` references via Registry before planning), a duplicate-document check before planning, and tests.

---

Only two syntaxes are Registry-driven:

```lemma
type money from @lemma/std/finance
fact external_doc = doc @user/space/somedoc
```

Everything else (including `doc somedoc` without a leading `@`) resolves within the locally-loaded document set as it does today.

## Rules for `@...`

- **No complicated logic**: if the identifier starts with `@`, it is a Registry identifier.
- **Reserved words are allowed**: `@doc` is valid and `"doc"` is passed to the Registry as-is.
- **Allowed characters**: identifiers follow the same segment rules as doc identifiers in the parser grammar (see "Grammar changes" below).
- **Errors are fatal**: failing to resolve a Registry identifier is a hard error (no fallback).
- **No lock file**: Lemma does not have a lock file. Reproducibility depends on the Registry's identifier and versioning guarantees.
- **Security, authentication, and caching are out of scope**: these are implementation details of a particular Registry.

## What the Registry returns

For `doc @...` (and `type ... from @...`), the Registry returns **Lemma source text** (a collection of raw Lemma docs).

Rationale: the current pipeline parses source into `Vec<LemmaDoc>`, then planning (`planning::plan`) builds an `ExecutionPlan` from a `main_doc` plus the full `all_docs` set. Returning an `ExecutionPlan` would bypass (and likely conflict with) cross-document planning and validation.

The Registry returns a **workspace bundle**: the source text needed to evaluate and plan the requested external document, including its dependencies. This may be represented as multiple `doc ...` blocks inside a single source string. The engine parses the returned source text and adds the resulting Lemma docs to the document set.

## Engine pipeline with Registry

1. **Parse** the local source files into Lemma docs.
2. **Resolve** — collect all `@...` references from the parsed docs. For each, call the Registry (which returns source text), parse that source text, and check the resulting docs for further `@...` references. Resolve those recursively until no unresolved `@...` references remain. A set of already-requested identifiers guards against circular resolution. If a requested identifier cannot be resolved, that is a fatal error.
3. **Plan** with the complete document set, exactly as today.

## Registry rewriting contract

To keep Lemma's core resolution logic simple and avoid name collisions, the Registry is responsible for rewriting external document names and references into their canonical, globally unique form.

Example (authored in a Registry workspace):

```lemma
doc example
fact e = doc somedoc
```

Example (returned to the Lemma engine by the Registry):

```lemma
doc @user/workspace/example
fact e = doc @user/workspace/somedoc
```

Key points:

- The Registry rewrites **all** returned document declarations to their canonical `@user/workspace/...` doc names.
- The Registry rewrites **only** returned **local-style** `doc` references (no leading `@`) so they point to canonical `@user/workspace/...` doc names (that is, no unresolved local `doc somedoc` links remain inside Registry-returned content).
- The Registry does **not** rewrite, normalize, or validate already-Registry-qualified `doc @...` references. If the source already contains `doc @something`, it is left as-is.
- The same rule applies to **type imports** in returned content:
  - local-style `type X from somedoc` must be rewritten to `type X from @user/workspace/somedoc`
  - already-Registry-qualified `type X from @something` is left as-is
- Lemma treats the full `doc_name` string (including the leading `@` and any `/` segments) as the document key.

## Versioning

Versioning semantics are **Registry-defined**. Lemma does not parse or enforce a "version field".

If a Registry supports versioning, it should encode it into the identifier string (examples are conventions, not requirements):

- `@user/workspace/somedoc/v1.2.3`
- `@user/workspace/somedoc-1.2.3`
- `@user/workspace/somedoc.1.2.3`

## Address mapping (for editor and Language Server navigation)

The Registry should be able to map Registry identifiers (`@...`) to a human-facing address (for example a uniform resource locator).

Goal: in the editor, Ctrl/Cmd+clicking on an external reference like `doc @external/space/doc` (or a type import like `type money from @lemma/std/finance`) opens the address defined by the configured Registry implementation.

Lemma core and the Language Server must **not** hardcode address formats.

## Grammar changes (required)

We need to support Registry-returned document declarations like:

```lemma
doc @user/workspace/somedoc
```

The `@` prefix is the sole indicator that a reference is Registry-driven. No other restrictions are needed.

### Required grammar shape

The grammar already has `doc_name` with an optional `@` prefix. That is all we need: if a `doc_name` starts with `@`, it is a Registry reference; otherwise it is local.

Current grammar (already correct):

```pest
doc_name       = { "@"? ~ doc_identifier ~ ("/" ~ doc_identifier)* }
doc_identifier = { ASCII_ALPHA ~ (ASCII_ALPHANUMERIC | "_" | "-" | "/" | "." )* }
```

`doc_name` is used in:

- `doc_declaration` (`doc ...`)
- `doc_reference` (`doc ...` as a fact value)
- `type_import_def` (`type ... from ...`)

No grammar changes are needed.

## Parser changes

Parsing should remain "pass-through":

- A `doc_name` token (local or `@...`) is stored as a plain `String`.
- No additional rewriting occurs inside Lemma.

Concretely, ensure:

- `parsing/mod.rs`: `doc` declarations can parse `doc @...` and set `LemmaDoc.name` to the full `@...` string.
- `parsing/facts.rs`: `doc_reference` can parse `doc @...` and store it as `FactValue::DocumentReference("@...".to_string())`.

## Planner changes

The planner already resolves document traversal by looking up referenced Lemma docs by name in the `all_docs` map. Because resolution (described in "Engine pipeline with Registry" above) recursively resolves all `@...` references before planning begins, the document set is complete. The planner does not need to know about the Registry at all.

Required behavior:

- Treat `@user/workspace/...` as a normal document name key (no special handling needed).
- Keep behavior strict: if a referenced document name is not present after resolution, return a fatal "Document not found" error (no fallback).

Recommended hardening:

- Detect duplicate document names before building the graph and return a fatal error. This prevents silent `HashMap` overwrite behavior.

## Tests (no Registry implementation needed)

Add tests to cover the expected behavior:

- **Parse success**: `doc @user/workspace/somedoc` is accepted as a document name.
- **Parse success**: `fact e = doc @user/workspace/somedoc` is accepted as a doc reference (fact value).
- **Plan success**: a source string containing:
  - `doc @user/workspace/example` referencing `doc @user/workspace/somedoc`
  - and the referenced `doc @user/workspace/somedoc`
  should plan successfully.

## Non-goals

- Registry authentication and security configuration
- Caching strategies, retries, offline behavior
- Defining a universal version syntax (Registry-defined)

## Implementation (designing the application programming interface)

### Core principle

Lemma only needs two Registry capabilities:

- **Resolve** an external identifier to Lemma source text (for evaluation and planning)
- **Map** an external identifier to an address (for editor navigation)

Everything else (authentication, caching, retries, and so on) is Registry-implementation specific.

### Proposed trait

The engine and Language Server treat the identifier after `@` as an opaque string, and pass it to the Registry as-is.

```rust
pub struct RegistryBundle {
    /// Lemma source containing one or more `doc ...` blocks.
    /// Registry-returned doc declarations must use canonical `doc @user/workspace/...` names.
    pub lemma_source: String,

    /// Optional source identifier used for diagnostics and proofs
    /// (for example "@user/workspace/somedoc").
    pub attribute: String,
}

pub struct RegistryError {
    pub message: String,
}

pub trait Registry: Send + Sync {
    /// Resolve a `doc @...` reference.
    ///
    /// Input is the identifier *without* the leading `@`
    /// (for example "user/workspace/somedoc").
    /// The Registry returns a workspace bundle with rewritten canonical doc names and references.
    fn resolve_doc(&self, identifier: &str) -> Result<RegistryBundle, RegistryError>;

    /// Map a Registry identifier to a human-facing address for navigation.
    ///
    /// Input is the identifier *without* the leading `@`.
    /// Returning `None` means: no address available for this identifier.
    fn url_for_id(&self, identifier: &str) -> Option<String>;

    /// Resolve a `type ... from @...` reference.
    ///
    /// Input is the identifier *without* the leading `@`
    /// (for example "lemma/std/finance").
    ///
    /// The Registry returns a workspace bundle containing the Lemma doc(s) needed to resolve
    /// the imported types. Returned content must follow the same rewriting rules as `resolve_doc`:
    /// canonical `doc @user/workspace/...` names, and local-style references and imports rewritten.
    fn resolve_type(&self, identifier: &str) -> Result<RegistryBundle, RegistryError>;
}
```

Notes:

- "Errors are fatal" means: `resolve_doc` failures are propagated as hard `LemmaError` values (no fallback).
- "Errors are fatal" means: `resolve_type` failures are propagated as hard `LemmaError` values (no fallback).
- `url_for_id` is intentionally best-effort (used only for navigation in the editor).

---

## Implementation phases

Implement in this order so each step is testable.

### Phase 1: Parse tests

The grammar is already correct. This phase adds tests to lock in the expected behavior.

1. **Tests** (in parsing or planning tests):
   - Parse success: `doc @user/workspace/somedoc` produces a `LemmaDoc.name` of `"@user/workspace/somedoc"`.
   - Parse success: `fact e = doc @user/workspace/somedoc` produces a `FactValue::DocumentReference("@user/workspace/somedoc")`.
   - Parse success: `type money from @lemma/std/finance` stores the full `@lemma/std/finance` string.

**Done when:** All parsing tests pass.

### Phase 2: Duplicate document names and plan test

1. **Planning** (`engine/src/planning/mod.rs` or `validation.rs`):
   - Before building the graph, detect duplicate document names in `all_docs` (same `doc.name`). Return a fatal `LemmaError` listing the duplicate.
2. **Test**:
   - Plan success: source containing `doc @user/workspace/example` and `doc @user/workspace/somedoc`, with a reference from the first to the second, and both docs present in `all_docs`, plans successfully.

**Done when:** Duplicate-document check exists and the cross-document plan test passes (with both docs supplied in `all_docs`; no Registry yet).

### Phase 3: Registry trait and resolution step

1. **Registry trait** (in the engine crate):
   - Define `RegistryBundle`, `RegistryError`, and a trait with `resolve_doc(identifier)`, `resolve_type(identifier)`, and `url_for_id(identifier)`. The Language Server can depend on this trait or keep its own address-only version.
2. **Resolution step** (new module, for example `engine/src/resolution.rs`):
   - After parsing local files, collect all `@...` references from the parsed docs: `FactValue::DocumentReference` values that start with `@`, and `type ... from` imports that start with `@`.
   - For each `@...` reference not already in the document set, call `Registry::resolve_doc` (or `Registry::resolve_type`).
   - Parse the returned `lemma_source` into Lemma docs and add them to the document set.
   - Check the newly added docs for further `@...` references and resolve those recursively.
   - Keep a set of already-requested identifiers to guard against circular resolution.
   - Map `RegistryError` to a fatal `LemmaError`.
3. **Engine integration**:
   - Add an optional Registry to the engine (for example `Option<Arc<dyn Registry>>`).
   - In `add_lemma_code`, after parsing local files and before planning, run the resolution step if a Registry is configured.
   - If no Registry is configured and a `@...` reference exists, planning will naturally fail with "Document not found" (no special handling needed).
4. **Stub and test Registry**: Implement the trait with stub behavior (for example return a fixed bundle or a specific error) so tests and the command line interface can run without a real Registry.

**Done when:** Engine can resolve `doc @...` and `type ... from @...` via Registry when configured; unresolved `@...` references and Registry errors produce fatal errors.

### Phase 4: Language Server Registry alignment (if needed)

- If the engine's Registry trait lives in the engine crate, the Language Server can depend on it and implement the same trait for `url_for_id` (and provide stub implementations for `resolve_doc` and `resolve_type` since the Language Server does not need to resolve documents). Alternatively, keep the Language Server's trait for addresses only and have the engine define a separate resolve-only interface; both are valid as long as address mapping is not hardcoded.

**Done when:** Language Server and engine share (or align on) the Registry contract; document links keep working.
