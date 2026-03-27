# AGENTS.md — Guide for AI assistants working on Lemma

This file states **implementation quality standards** and **non-negotiable guarantees** for the engine, CLI, and tooling. It does **not** describe Lemma’s language design, syntax, or pipeline—read **`documentation/`**, **`README.md`**, and the **codebase** for that.

---

## Core guarantees (do not weaken these)

### Validation before execution

- **Planning** (semantic + graph + execution plan) fully validates the spec. Only if planning succeeds does the engine run evaluation.
- Invalid Lemma must be rejected with **Error** (Parse, Semantic, Engine, CircularDependency, etc.). Never “try to run and see.”

### Post-validation: evaluation is guaranteed to complete

- **After** a spec has been successfully planned, evaluation is **guaranteed** to run to completion for the requested rules (modulo resource limits). The execution plan is self-contained and fully resolved.
- If something **impossible** happens during evaluation (e.g. missing explanation node, wrong variant in a match that was supposed to be exhaustive), the program must **fail fast**: use `unreachable!()` or `panic!()`. Do **not** return a fallback value or “best guess.”

### No silent defaults or heuristics

- **Silent defaults and heuristics are forbidden.** If the semantics don’t define a value, the code must either:
  - return **Error** (during parse/planning), or
  - return **Veto** (during evaluation, for domain-level “no value” or impossible computations based on facts like "division by zero"), or
  - **panic / unreachable** when an invariant is violated.
- Never infer or guess to “keep going”; that undermines certainty.

### Determinism

- **Execution plans** are deterministic: same spec + same fact inputs ⇒ same evaluation order and same results (or same Veto).
- Evaluation must not depend on undefined order (e.g. iteration over unordered collections in a way that affects results). Rules are evaluated in **topological order** by dependency.

### Veto is a result, not an error

- Rules that **cannot produce a value** (e.g. division by zero, missing fact, user-defined `veto "reason"`, date overflow) yield **Veto**, not Error.
- **OperationResult** is either `Value(LiteralValue)` or `Veto(Option<String>)`.
- Veto propagates to dependent rules only when the dependent rule **needs** the vetoed value; if an `unless` branch provides a value without evaluating the vetoed rule, that branch can still succeed. See `documentation/veto_semantics.md`.
- **Planning returns Error** for invalid specs (type mismatches, unsupported operations). **Runtime panics** for bugs that fell through validation (use `unreachable!()`). **Veto** is only for domain-level “no value”.

### No placeholders in Lemma code

- **Placeholders in `.lemma` sources, docs, tests, or examples are unacceptable** (dummy values, “TODO” literals, fake numbers, placeholder text). They make specs look valid while hiding missing or wrong logic.
- Use **real, intended** values that match the domain, or omit and fail validation—do **not** fill gaps with placeholders.

---

## Research the codebase before adding behavior

**Do not add new functionality until existing implementations are thoroughly explored.** Lemma already encodes many invariants, helpers, and code paths; parallel or duplicate logic (second parsers, overlapping validators, ad hoc conversions, copy-pasted checks) drifts from guarantees, hides bugs, and makes review harder.

- **Search and read:** Use ripgrep, navigation, and semantic search across `engine/`, `cli/`, `openapi/`, and related crates. Find call sites, tests, and modules that already address the same concern (planning, evaluation, computation, serialization, registry, WASM, LSP, etc.).
- **Prefer extension:** Reuse or extend existing types and functions; wire into the established pipeline instead of introducing a parallel one.
- **When in doubt:** Follow tests and production callers to see how the engine is supposed to behave; align with that rather than inventing a second path.

Skipping this step is how duplicate implementations appear—treat discovery as mandatory, not optional.

---

## Error handling rules for implementers

- **Parse/planning:** Invalid input ⇒ return **Error** with clear, localized message (include source location where possible). Do not continue with a “best effort” plan.
- **Evaluation:**
  - Domain failures (e.g. division by zero, missing fact, user veto, date overflow) ⇒ **OperationResult::Veto(...)**.
  - Type/operator mismatches that planning should have rejected ⇒ **unreachable!()** (planning bug).
  - Bug or invariant violation (e.g. missing node, wrong enum variant) ⇒ **panic!()** or **unreachable!()** with a message that includes context (e.g. “BUG: …”).
- **No silent fallbacks:** Do not use default values or heuristics to avoid failing; that violates Lemma’s guarantee of certainty.

---

## Testing and development

- **Use TDD.** Failing tests define missing or broken behaviour; do not hide or remove them to make the suite pass.
- **Unit tests** live in the same module (to allow testing private functions); **integration tests** in `engine/tests/`.
- Run tests with **cargo nextest**, not `cargo test`.
- From repo root, **cargo precommit** runs **`versions-verify`**, then `fmt --check`, clippy, nextest, and cargo-deny (needs `cargo-nextest` and `cargo-deny` on `PATH`; CI runs the same checks across lint / test / security jobs).
- **The release version** is `[workspace.package] version` in the root `Cargo.toml`. Use **`cargo bump <version>`** to bump the version everywhere, then commit. **`cargo verify`** verifies that all version values are aligned.
- When adding features, add tests that lock in the intended behaviour (including Veto propagation and error cases).

---

## Where to read more

| Resource | Purpose |
|----------|--------|
| **README.md** | Project overview, quick start |
| **xtask/README.md** | precommit, `cargo bump` / `cargo verify`, tracked release paths |
| **documentation/index.md** | Language concepts |
| **documentation/reference.md** | Operators, types, literals, syntax |
| **documentation/veto_semantics.md** | When Veto applies and propagates |
| **documentation/examples/** | Example `.lemma` files |
| **.cursor/rules/** | Project rules (e.g. tests, TDD) |

---

## Summary

**Research the codebase before adding behavior** (first section). Preserve **Error** vs **Veto** vs **panic/unreachable** as above. **Execution plans** stay **deterministic**. **Never** use placeholders in Lemma-facing content. Prefer **cargo nextest** and TDD. For CLI/API surface details (`lemma schema`, plan hash pins, etc.), see **README.md** and **OpenAPI** / CLI sources.
