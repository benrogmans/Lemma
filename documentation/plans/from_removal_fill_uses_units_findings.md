# Findings: removal of `from`, `fill` vs `data`, and `uses` unit visibility

> Internal engineering note (not user-facing language reference). For `fill` and `uses`, see [reference.md](../reference.md).

This document records what the **current code** does and does **not** guarantee, plus gaps and naming landmines. It is not a marketing summary.

---

## 2. Value copy and literals: `fill` vs `data`

**Intended split (as implemented)**

| Goal | Construct | AST |
|------|-----------|-----|
| Declare a slot (type, constraints, optional literal default) | `data …` | `DataValue::Definition` (never `Fill`) |
| Import a spec alias (`uses`) | `uses …` | `DataValue::Import` |
| Assign or define a slot (literal or copy; no `->` on fill) | `fill …` | `DataValue::Fill(FillRhs::Literal \| FillRhs::Reference { target })` |

**Hard rule:** `data` with **non-empty** LHS path segments is a **parse error**; the diagnostic tells the author to use `fill` (`engine/src/parsing/parser.rs`, `parse_data`). Parser test `parse_data_on_binding_path_is_rejected_with_fill_hint` locks the substring `fill` in the error text.

**Semantics:** Local `fill x: <literal>` and path `fill i.slot: <literal>` are the same product operation (slot assignment). Planning folds every `Fill` row through `data_bindings` into the same resolved slot representation as reference fills; `add_data` skips `Fill` rows so they never insert a second `DataPath` entry.

**What is no longer true:** `fill` never smuggles a literal through `Definition { value: Some }`. Tooling should classify `fill` from `matches!(value, DataValue::Fill(_))` without inspecting LHS segments for that distinction.

---

## 3. Quantity and ratio unit names visible in expressions (`uses` depth)

**Product rule**

A unit name may appear in expressions in spec **S** only if the unit is defined on a type declared **in S**, or on a type declared in a spec **T** where **S** has a **direct** `uses` edge to **T** (merged per `resolve_and_validate` in `engine/src/planning/graph.rs`). There is no transitive re-export of unit names across a chain of `uses` unless intermediate specs expose those units through their **own** resolved typedefs that the merge pass processes.

**Trait-duration quantity literals**

Unit words for time periods (`seconds`, `hours`, …) resolve like other quantity literals: the **quantity type** must declare `-> trait duration` (with canonical `second`) and the matching `unit` rows. They are indexed like other quantity units once the typedef is in scope (stdlib `repo lemma` / `uses lemma si` / `si.duration`).

**Implementation note**

`resolve_and_validate` builds `unit_index` from **S**’s resolved named types, then merges quantity and ratio **units** from each **direct** `uses` target’s registered `DataTypeDef` map (skipping qualified-parent typedef rows as drivers in that pass). `EvaluationContext` states that runtime unit visibility follows the plan’s `unit_index` (`engine/src/evaluation/mod.rs`, comment near the `unit_index` copy). This is intentional wiring, not a formal proof that no unit name can ever be reached through deeper qualified type chains inside a dependency.

---

## 4. Tests that anchor behavior (non-exhaustive)

| Area | Tests / location |
|------|------------------|
| `fill` → `DataValue::Fill` (reference and literal RHS) | `engine/src/parsing/mod.rs` (`parse_fill_with_dotted_rhs_is_fill_reference`, multi-segment RHS, trailing constraints, binding RHS) |
| Binding path + `data` diagnostic | `engine/src/parsing/mod.rs` (`parse_data_on_binding_path_is_rejected_with_fill_hint`) |
| Local fill literal + missing slot | `engine/tests/data_references.rs` (`local_fill_literal_assigns_into_declared_slot`, `local_fill_literal_without_declared_slot_fails_planning`) |
| Path fill literals and bindings | `engine/tests/data_nested_bindings_coverage.rs`, `nested_spec_references.rs`, `cross_spec_references.rs` |
| `uses` alone vs `uses` + qualified parent for **unit names in compounds** | `engine/tests/multidim_unit_system.rs`: **D5** vs **D6** |
| Same-named `length` across specs + imported velocity (**D7**) | `engine/tests/multidim_unit_system.rs` (`d7_cross_library_same_named_quantity_resolves_speed_literal`) |
| Temporal + qualified + `uses` | `engine/tests/type_import_temporal.rs`, `temporal_type_resolver_instant.rs`, `temporal_range_references.rs` |
| Stdlib duration literals after `uses lemma si` | `engine/tests/duration_trait_*.rs`, `calendar_duration_split.rs`, etc. |

---

## 5. Deliberate “major issue” checklist (plain language)

1. **`uses` “non-transitive”** names the planner merge hop, not a blanket guarantee about every way a unit symbol can appear through nested qualified typedefs inside a dependency.
2. **Stale onboarding** is reduced: `registry` trait docs no longer pair `uses` with the removed `from` keyword; `spec_set_id` avoids the word `from` for temporal sigils.

---

## 6. Suggested follow-ups (implementation optional)

- None tracked from this note; prior items (parser pin for `data` on binding path, `Fill` discriminant, D7 assertion, registry/spec_set_id wording, D6 rename) are done in tree.

Last updated from static code review (no `cargo nextest` run for this file).
