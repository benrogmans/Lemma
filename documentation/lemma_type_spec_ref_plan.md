# Resolved spec identity on `LemmaType` (`Arc<LemmaSpec>`)

## Goal

Custom resolved types should record **which spec defines the imported parent type**, using the same idea as spec-reference **facts**: [`FactData::SpecRef { spec: Arc<LemmaSpec>, … }`](engine/src/planning/semantics.rs)—**resolved** dependency, not a second copy of parse-time references.

Facts and rules already carry `LemmaType`; provenance belongs **on the type**, not in parallel maps or duplicate name strings.

## Prerequisites

- ~~**Remove semantic hashing from inversion** ([`engine/src/inversion/derived.rs`](engine/src/inversion/derived.rs): `semantic_hash`, `Hash`/`Eq` on `DerivedExpression`, and literal hashing via `LiteralValue::hash`).~~ **Done.** `DerivedExpression` no longer derives `Hash`. `LiteralValue`, `FactData`, `LemmaType`, `TypeExtends`, `TypeDefiningSpec` all have `Hash` removed from their derives.

## Current state (updated after provenance cleanup)

- **`TypeDefiningSpec`** enum exists in [`semantics.rs`](engine/src/planning/semantics.rs): `Local` and `Import { spec: Arc<LemmaSpec> }`.
- **`TypeExtends::Custom`** carries `defining_spec: TypeDefiningSpec` — set during per-slice type resolution in [`PerSliceTypeResolver`](engine/src/planning/types.rs) and in [`GraphBuilder::resolve_type_declaration`](engine/src/planning/graph.rs).
- **`is_same_spec`** exists in [`semantics.rs`](engine/src/planning/semantics.rs); currently delegates to `LemmaSpec`'s derived `PartialEq` (compares all fields — semantic, not pointer equality).
- **`ExecutionPlan`** carries `named_types: BTreeMap<String, LemmaType>` only. Imported-type provenance is carried by `LemmaType.extends` via `TypeDefiningSpec::Import { spec: Arc<LemmaSpec> }`.
- **`TypeResolver` is deleted.** All type resolution is per-slice via `PerSliceTypeResolver` inside `Graph::build`, using `Context.get_spec(name, resolve_at)` for cross-spec imports.
- **`Hash` removed** from `LemmaType`, `TypeExtends`, `TypeDefiningSpec`, `LiteralValue`, `FactData`. `TypeSpecification` and lower-level types still derive `Hash` (no reason to remove — they don't contain `Arc<LemmaSpec>`).
- **Equality tests exist** for local-vs-import inequality and same-resolved-spec equality (`test_lemma_type_inequality_local_vs_import_same_shape`, `test_lemma_type_equality_import_same_resolved_spec_semantics`).

## Design

### 1. Extend `TypeExtends::Custom` — DONE

`TypeExtends::Custom { parent, family, defining_spec: TypeDefiningSpec }` is implemented. `TypeDefiningSpec::Local` and `TypeDefiningSpec::Import { spec: Arc<LemmaSpec> }` exist.

### 2. Construction sites — DONE

`defining_spec` is set everywhere a `LemmaType` with `Custom` is built:

- [`PerSliceTypeResolver`](engine/src/planning/types.rs): `resolve_type_internal`, `resolve_parent`, `resolve_inline_type_definition`.
- [`GraphBuilder`](engine/src/planning/graph.rs): `resolve_type_declaration`.

Rules (all enforced):
- Cross-spec import: `Import { spec: Arc<…> }` with the same arc planning uses for that dependency.
- Same-spec extension: `Local`.
- Unclassifiable cases: planning error.

### 3. Equality, hashing, comparison — DONE

- **`Hash` removed** from `LemmaType`, `TypeExtends`, `TypeDefiningSpec`, `LiteralValue`, `FactData`.
- **`PartialEq`/`Eq` removed** from `TypeDefiningSpec` (no derive, no manual impl). `TypeExtends` has a **manual `PartialEq`** that routes `defining_spec` comparison through `is_same_spec`. `LemmaType` and everything above it keeps derived `PartialEq` — no cascade.
- **`is_same_spec(a, b)`** in [`semantics.rs`](engine/src/planning/semantics.rs) delegates to `LemmaSpec`'s derived `PartialEq`.
- **Compatibility helpers** (`same_scale_family`, graph inference, validation) remain separate from `==`.
- **Call site audit** (completed): `LemmaType` equality used in inversion (`constraint.rs` match guards, `domain.rs` lit_cmp) and `SliceInterface` slice comparison. `LiteralValue` equality used in inversion domain algebra (`.contains()`, `.dedup()`). All consistent with strict identity.

### 4. Serialization — DONE

- `ExecutionPlan` serializes `LemmaType` (including `TypeDefiningSpec`) inside `named_types`. `Arc<LemmaSpec>` in `TypeDefiningSpec::Import` round-trips via serde.
- `plan_hash()` includes `named_types`.
- Deterministic JSON confirmed (all maps are `BTreeMap`).

### 5. Cleanup — DONE

Cleanup completed:
- [x] Removed duplicated provenance from `ExecutionPlan` (`type_imports` / `TypeImport` removed).
- [x] Removed remaining planning-surface references to the old global `TypeResolver`.

## Tests — PARTIALLY DONE

**Done:**
- [x] `test_lemma_type_inequality_local_vs_import_same_shape` — local vs import with same shape are not equal.
- [x] `test_lemma_type_equality_import_same_resolved_spec_semantics` — two imports with `is_same_spec`-equal specs are equal.

**Remaining tests:**
- [x] Imported vs local types with the **same local name**: `defining_spec` differs as intended.
- [x] Fact `[money from examples]`: `LemmaType` carries `Import { spec }` whose arc matches the resolved `examples` spec.
- [x] `serde_json` round-trip of `ExecutionPlan` with cross-spec types (verify `TypeDefiningSpec::Import` survives serialization).
- [ ] Full stabilization pass (`cargo nextest run --nff --all`, `cargo clippy --all-targets --all-features -- -D warnings`) is clean after cleanup.

## Files

| File | Role | Status |
|------|------|--------|
| `engine/src/inversion/derived.rs` | Remove `semantic_hash` / `Hash` on `DerivedExpression` | Done |
| `engine/src/planning/semantics.rs` | `TypeDefiningSpec`, `TypeExtends::Custom`, `is_same_spec`, manual `PartialEq` on `TypeExtends` | Done |
| `engine/src/planning/types.rs` | `PerSliceTypeResolver` attaches `Arc<LemmaSpec>` | Done |
| `engine/src/planning/graph.rs` | Fact type declarations attach `Arc<LemmaSpec>` | Done |
| `engine/src/planning/execution_plan.rs` | `named_types`, `plan_hash()` | Done |
| `engine/src/planning/slice_interface.rs` / `validation.rs` | Comparisons use per-slice resolved types | Done (audit equality semantics) |

## Verification

`cargo nextest run -p lemma-engine`
