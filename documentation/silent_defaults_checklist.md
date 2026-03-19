# Checklist: silent defaults, shortcuts, and enforcement

Track remediation or explicit **won’t fix** decisions. Scope: engine, CLI, OpenAPI, LSP, lemma_hex, tests (called out where patterns differ).

---

## A. Intentional panic / not production-ready (must resolve or keep deliberate)

- [x] **A1** — plan hash wired into `run_command`, `get_single_spec`, `get_all_workspace_deps` via `get_plan_hash()` / `plan_hash()`
- [x] **A2** — plan hash wired into `spec_post_evaluate` and `VersionEntry.hash` via `get_plan().plan_hash()`
- [x] **A3** — plan hash wired into MCP evaluate via `get_plan().plan_hash()`
- [x] **A4** — confirmed: no empty-string or placeholder hashes remain in `cli/`

---

## C. OpenAPI / HTTP / WASM surfaces

- [ ] **C3** — [`openapi/src/lib.rs`](openapi/src/lib.rs) (and related): audit other `unwrap_or(false)` / `.or_else` on schema generation paths (~598, ~756) for silent fallbacks

---

## F. Planning graph / facts / types

- [ ] **F2** — [`engine/src/planning/graph.rs`](engine/src/planning/graph.rs): `original_schema_type.unwrap_or(inferred_type)` for literals — document invariant or tighten when schema required
- [ ] **F3** — [`engine/src/planning/graph.rs`](engine/src/planning/graph.rs): `resolve_path_segments` / `effective_spec_refs.get(segment).or_else(|| resolve_spec_ref)` — `index == 0` branch vs deeper segments; confirm no wrong “first match” semantics
- [ ] **F5** — [`engine/src/planning/graph.rs`](engine/src/planning/graph.rs): `from.as_ref().map(|r| r.name.as_str()).unwrap_or("")` — empty string parent name path; validate callers

---

## G. Type resolver (`types.rs`)

- [ ] **G2** — [`engine/src/planning/types.rs`](engine/src/planning/types.rs): `scale_family_name().map(String::from).unwrap_or_else(|| parent.clone())` — confirm no silent wrong family
- [ ] **G3** — [`engine/src/planning/types.rs`](engine/src/planning/types.rs): `contains_key(parent).unwrap_or(false)`-style chains (~554, ~747) — audit for Option layering

---

## H. Semantics / type specifications (`semantics.rs`)

- [ ] **H2** — [`engine/src/planning/semantics.rs`](engine/src/planning/semantics.rs): multiple `.unwrap_or_default()` on constraint/command parsing (non-`help`) — batch review for “empty means OK”

---

## I. Slice interface / temporal validation

- [ ] **I1** — [`engine/src/planning/slice_interface.rs`](engine/src/planning/slice_interface.rs): `ref_spec_in_slice.as_ref().unwrap_or(ref_spec_arc)` — missing ref in slice falls back to first slice’s spec; prove or replace with error

---

## O. lemma_hex (NIF)

- [ ] **O1** — [`engine/packages/hex/native/lemma_hex/src/lib.rs`](engine/packages/hex/native/lemma_hex/src/lib.rs): `.unwrap_or_default()` and similar — audit for Elixir boundary silent defaults (~106, ~311 if still present)

---

## R. Tests, fuzz, integration (pattern debt — optional cleanup)

- [ ] **R1** — `engine/tests/**`, `cli/tests/**`: pervasive `.unwrap()` / `.expect()` — consider `expect()` messages everywhere for faster failures
- [ ] **R3** — **Ignored tests** (4): [`engine/tests/wasm_build.rs`](engine/tests/wasm_build.rs), [`engine/tests/resource_limits_test.rs`](engine/tests/resource_limits_test.rs), [`engine/tests/run_fuzz_targets.rs`](engine/tests/run_fuzz_targets.rs) — run in CI matrix or document why ignored

---

## S. Meta / process

- [ ] **S1** — Add CI step: fail on new `TODO:`/`FIXME:` in production crates (allowlist test data if needed)
- [ ] **S2** — Link each closed item to PR or “WON’T FIX: reason” in this file or ADR

---

### How to use

1. Work top-down by risk (A/C/F for user-visible certainty).
2. Check the box when fixed **or** when explicitly accepted with a one-line note and date.
3. Keep **S2** so the checklist does not rot.
