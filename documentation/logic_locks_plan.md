# Logic locks: implementation complete

Parent plan hash commits to exact dependency logic by slice, for both pinned and unpinned references.

## Completed

- Type provenance on `LemmaType` via `TypeDefiningSpec`.
- Spec dependency ordering: Kahn topo sort with hard cycle error; typed `SpecName` keys.
- Plan hash: deterministic 8-char hex SHA-256 of serialized plan (minus sources/meta).
- `PlanHashRegistry`: slice-keyed `BTreeMap<(String, Option<DateTimeValue>), String>` + pin-keyed `BTreeMap<(String, String), Arc<LemmaSpec>>`. Replaces old `Vec<(Arc<LemmaSpec>, String)>`.
- `Graph::build` and `GraphBuilder` take `&PlanHashRegistry`.
- Resolve-then-verify for fact spec refs: always resolve spec first, then lookup dependency hash from registry; if AST has `hash_pin`, compare with looked-up hash (mismatch -> error). Unpinned refs now store `resolved_plan_hash` on `FactData::SpecRef`.
- Renamed `expected_hash_pin` -> `resolved_plan_hash` on `FactData::SpecRef` with `#[serde(alias = "expected_hash_pin")]` for backward compat.
- `PerSliceTypeResolver` takes `&PlanHashRegistry`; `resolve_spec_for_import` verifies `hash_pin` on type imports (returns `Result<Arc<LemmaSpec>, Error>` for pin mismatch).
- All tests pass (1130 engine tests).

## Test coverage

- `hash_pinned_ref_resolves_correct_version`
- `hash_pinned_ref_wrong_hash_fails_planning`
- `hash_pinned_type_import_resolves`
- `test_spec_dependency_cycle_returns_global_error_and_aborts`
- `test_spec_order_includes_fact_type_declaration_from_edges`
- `unpinned_ref_stores_resolved_plan_hash` (new)
- `parent_hash_changes_when_dependency_changes` (new)
- `type_import_pin_mismatch_fails_planning` (new)
- `type_import_pin_match_succeeds_with_correct_hash` (new)
- `missing_dependency_hash_when_dep_fails_planning` (new)
- `serde_round_trip_resolved_plan_hash` (new)

## Out of scope

- Hash format changes (length/derivation).
- `Engine::get_spec` using parsed hash for API-level resolution.
- LSP/CLI/HTTP/MCP UX for hash display/input.
