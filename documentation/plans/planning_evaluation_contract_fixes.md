# Plan: Planning & Evaluation Contract Fixes

Status: implemented — all phases executed and validated (see the regression
tests referenced per finding)
Scope: `engine/src/planning/*`, `engine/src/evaluation/*`, `engine/src/engine.rs`

## Background

A review of the planning and evaluation phases found six groups of issues that
violate the engine's two core contracts:

- **Planning** must catch all user errors and never crash.
- **Evaluation** must never return an `Error`; runtime conditions are vetoes,
  and violations of planning's guarantees must crash (panic) rather than
  silently produce wrong results.

Both the VM path and the explanation path consume the artifacts planning
produces (transitively normalized rules compiled to instructions), so
normalization soundness and path parity are part of the same contract.

**Design intent (explanations):** explanations exist to show users which
rules were followed *in source terms*, even when normalized evaluation
logically skipped parts (short-circuits, folds). The two paths are therefore
intentionally different in nature: the VM is authoritative for outcomes; the
explanation walker narrates the full source reasoning. "Parity" in this plan
means **agreement on outcomes** (results, winning branches, veto reasons) —
never reducing the explanation to a replay of the VM's execution trace.
Finding 1 is the keystone of this intent: source-level explanations are only
truthful if normalization preserves observable semantics.

## Prerequisite: settle veto semantics for absorbing folds

The engine currently mixes two semantics: the VM propagates vetoes strictly
through every arithmetic/comparison operand, while the normalizer applies
absorbing algebraic folds (`x * 0 → 0`, etc.) at plan time. **Decision:
adopt strict semantics everywhere.** Rationale:

- The runtime (`Instruction::Arithmetic` et al.) already propagates vetoes;
  strictness is the implemented behavior for every value not known at plan
  time.
- In a business rule language, a veto is a decision, not an unknown number.
  `veto "credit check failed" * 0` must not silently become `0`.
- The data manifest (the spec's published interface) must not change based on
  whether the optimizer fired.

Consequence: a fold may only delete a subexpression if that subexpression is
**provably total** — no data paths, no rule paths, no `veto`, no operations
that can fail at runtime (division, `log`, unit conversion, …). In practice:
literals only.

---

## Finding 1 — Veto/unit-unsound normalization rewrites

`engine/src/planning/normalize.rs`

### 1.1 Add a totality analysis on `NormalForm`

Add `fn is_total(nf: &NormalForm) -> bool`: true only for literal leaves and
compositions of total operations over total children (constant arithmetic that
`constant_fold` would resolve exactly, boolean literals, …). Conservative by
construction: anything containing `LeafKind::DataPath`, `LeafKind::RulePath`,
`NormalForm::Veto`, `Divide`/`Reciprocal`/`Modulo`/`Power` with non-literal
operands, `MathOp`, `UnitConversion`, `Now`, or date/range ops is non-total.

### 1.2 Gate the deleting folds in `eliminate_identities` (L1823–1972)

| Fold | New rule |
|---|---|
| `Product` with a zero child → `0` | Only when **all other children** are total. Otherwise keep the product. |
| `x ^ 0 → 1` | Only when the base is total. |
| `And` containing literal `false` → `false` | Only when all other children are total. |
| `Or` containing literal `true` → `true` | Same gate. |
| Dropping `true` conjuncts / `false` disjuncts | Keep as is: only a literal is deleted, which can never veto. |
| `x + 0 → x`, `x * 1 → x`, `x ^ 1 → x`, `x - 0 → x`, `x / 1 → x` | Keep: these never delete a non-literal operand. But see 1.3 for the unit-mismatch caveat on `x + 0`. |

Apply the same gate to the duplicated logic in `logical_short_circuit`
(L2215–2247).

Dry-run note (pass ordering): `simplify` runs `eliminate_identities` **before**
`constant_fold`, and `constant_fold`/`as_rational_literal` match only plain
`ValueKind::Number` — quantity literals never constant-fold. So even after
gating, an all-literal unit-bearing product (`5 eur * 0`) still reaches the
Product-zero arm. **1.3 is therefore mandatory, not optional.**

### 1.3 Preserve units in folded literals

- The original idea ("take the type from `CompileContext.rule_type`") is
  wrong: no type context reaches the normalization passes (`normalize_once`
  receives only `unit_ctx` and `source`), and `rule_type` is the rule's
  *root* result type, not the type of a nested subexpression. Instead:
  **derive the folded literal's unit signature locally from the sibling
  literal children** — every `LiteralValue` carries `lemma_type`, and
  `ValueKind::Quantity` carries a canonical signature; the product's zero is
  the pointwise exponent-sum of the children's signatures. This is always
  computable in the gated all-total case (all children are literals).
- The unit-erasure bug lives in the inline
  `LiteralValue::number(rational_zero()/rational_one())` constructions in
  `eliminate_identities` (L1837, 1846, 1861, 1876, 1885) and
  `collect_like_base_powers` (L2041–2043). `literal_from_folded_rational`
  (L27–35) can never receive unit-bearing input today (all callers gated by
  `as_rational_literal`) — leave it, but add a debug assertion.
- `x + 0` where `x` is a quantity/date and `0` is unitless: a DataPath
  operand's type is unknowable inside the passes without plumbing the `data`
  map in. Decision: **keep the `x + 0 → x` fold for unknown-typed operands**
  (it deletes only a literal; the runtime unit-mismatch it erases is a
  planning-detectable type error anyway — track separately if planning does
  not already reject `quantity + unitless 0`). This keeps `identity_add_zero`
  (L2630) green. Restrict only when the operand is a *literal* of
  non-matching type.

### 1.4 Restrict veto-asymmetric boolean rewrites

- **De Morgan** (`demorgan`, L2185–2209): AND propagates all vetoes; OR
  swallows non-`MissingData` vetoes in non-last disjuncts. Rewriting between
  them changes veto observability. Gate: apply De Morgan only when every
  child is total. (Alternative — making AND/OR veto semantics symmetric — is
  a language change; out of scope here.)
- **OR idempotency dedup** (`logical_idempotency`, L2249–2271): deduplication
  may change which disjunct is last (the position whose veto propagates).
  Gate: for `Or`, never remove the last child, and only remove a duplicate if
  the duplicate is total. AND dedup stays as is (vetoes propagate from every
  conjunct position).
- **`exp (log x) → x`** (`math_identities`, L2296–2302): drops the domain
  veto for `x ≤ 0`. Gate on total `x` (where it constant-folds anyway) or
  remove the rewrite.
- **Power-law exponent merging** (`power_laws`, L2001–2019,
  `collect_like_base_powers` L2021–2048): a sign-change criterion is
  insufficient — fractional-exponent merges also change the domain
  (`sqrt(x) * sqrt(x) → x^1 → x` drops the `x < 0` domain veto with no sign
  change). Gate: merge only when the base is total, **or** all participating
  exponents and the merged exponent are integers of the same sign
  (domain-preserving).
- **`negated_comparisons`** (L2273–2283) stays ungated — verified safe:
  `Instruction::Comparison` propagates operand vetoes symmetrically, so
  `not (a < b) → a >= b` preserves veto behavior. `demorgan`'s duplicated
  `Not(Comparison)` arm is covered by this pass, so gating `demorgan`
  wholesale loses nothing.

### 1.5 Regression tests

For each gated fold, add a pair of tests in `normalize.rs`'s test module plus
end-to-end evaluation tests:

- total operands → fold still happens (assert instruction count / constant
  result);
- data-path / vetoing operand → fold does not happen, and evaluation of the
  compiled plan returns the veto (`MissingData` for absent data, user veto for
  inlined `veto` rules).

Key cases: `x * 0` (x missing → veto; x present → `0` with x's unit),
`v * 0` with `v: veto "blocked"` → veto, `flag and (1 > 2)` with `flag`
missing → missing-data veto, `x or true` likewise, `price * 0 → 0 eur`,
`exp (log offset)` with `offset = -5` → computation veto.

Existing tests that flip under the gating and must be updated to assert the
new (non-folded) shape: `identity_mul_zero` (L2644),
`identity_pow_one_and_zero` (the `^0` half, L2655),
`logical_short_circuit_and_false` (L2827), `de_morgan_not_and` (L2763),
`exp_log_identity` (L3057); `power_laws_sqrt_squared` (L3019) flips under the
domain-preserving criterion (fractional exponents). Unaffected:
`identity_add_zero` (kept per 1.3), `identity_mul_one`,
`logical_idempotency_and_duplicate_paths` (AND dedup kept),
`negated_comparison_not_less_than`, `power_law_nested_power` /
`power_law_like_base_product` (integer same-sign exponents). E2e suite is
clean: `edge_zero_multiplication` (tests/realworld_edge_cases.rs:228)
supplies its data, so `0 * price` still evaluates to `0`.

---

## Finding 2 — Explanation path diverges from the VM

`engine/src/evaluation/explanations.rs`, `expression.rs`, `mod.rs`

### 2.1 Single source of truth for branch/veto semantics

Today unless/AND/OR veto semantics live in three places: the compiler
(`normalize.rs:617–721`, `compile_piecewise_rule`), the VM jump handler
(`mod.rs:545–575`, `JumpVetoSemantics`), and the explanation tree-walker
(`expression.rs:255–302`, `explanations.rs:362–464`). Extract the decision
logic into shared functions in one module (suggested:
`evaluation/branch_semantics.rs`):

```rust
enum BranchOutcome { Taken, NotTaken, Propagate(OperationResult) }
fn unless_condition_outcome(cond: &OperationResult, sem: JumpVetoSemantics) -> BranchOutcome;
fn or_disjunct_outcome(value: &OperationResult, is_last: bool) -> BranchOutcome;
fn and_conjunct_outcome(value: &OperationResult) -> BranchOutcome;
```

Rejected alternative — making explanations replay recorded VM outcomes —
would contradict the design intent above: a trace-replay explainer can only
narrate what the VM executed, losing the skipped source parts users need to
see. The walker keeps visiting the full source tree and delegates only the
*decisions* (branch selection, veto meaning) to the shared functions.

The VM consults these in `JumpIfFalse`; the explanation walker consults the
same functions. Mirror-divergence becomes structurally impossible.

Dry-run notes:

- Variant selection is **structural** at HEAD, not "rule-reference vs inline
  expression": `compile_piecewise_rule`/`compile_piecewise_value` hardcode
  `UnlessRuleReference`; `UnlessExpression` appears only inside And/Or
  compilation. The walker therefore knows the variant from context:
  top-level branch conditions → `UnlessRuleReference`, And/Or operands →
  `UnlessExpression`. (The enum names are misleading; consider renaming.)
- Binary vs n-ary: the source AST is binary `LogicalOr(left, right)` while
  the VM compiles a flattened n-ary Or; `canonical_order` never reorders
  And/Or children, so source order matches compiled order. The walker must
  treat **every right child as `is_last = true`** — binary composition then
  reproduces flat n-ary semantics exactly.

### 2.2 Fix the two known divergences

- **OR / missing data** (`expression.rs:286–302`): the tree-walker treats any
  left veto as "fall through"; the VM returns `MissingData` for the whole rule
  from a non-last disjunct. After 2.1 both use `or_disjunct_outcome`.
- **Vetoed unless conditions** (`explanations.rs:378–394`): the explanation
  marks a vetoed condition as `wins = true` and renders the cause as `"true"`.
  After 2.1, a `Propagate` outcome must produce an explanation that says the
  condition vetoed (new cause rendering: the veto message / missing data
  path), with no branch body selected.

### 2.3 Decimal-limit veto must reach `rule_results`

`Evaluator::evaluate` (`mod.rs:979–988`) stores the raw result in
`context.rule_results`, then clamps only the response copy via
`ensure_rule_result_within_decimal_limit`. Apply the clamp **before** storing
in `rule_results` so downstream consumers and explanations all see the same
vetoed value the response reports.

Dry-run notes on scope (verified):

- Rule references in *expressions* are fully inlined, but **data references
  with `ReferenceTarget::Rule` read `rule_results` at runtime** (`LoadData`
  → `resolve_data_path_value` → `rule_results.get`), so this change is not
  explanation-only: downstream rules consuming an over-limit result through a
  reference now see the veto (and `ResultIsVeto` over it flips to `true`).
  That is the intended strict behavior — changelog entry required.
- The clamp currently applies only to response-scope rules (it sits after
  the response filter); moving it before the insert applies it to **all**
  evaluated rules, including nested `uses` rules. Also intended; say so in
  the changelog.
- Interior explanation nodes still recompute source expressions with exact
  rationals; 2.3 makes `explanation.result` consistent — full interior
  parity is covered by the 2.4 tests.

### 2.4 Parity tests

Extend `engine/tests/explanation_e2e.rs` with non-happy paths, asserting that
`explanation.result`, branch selection, and causes agree with the response:

- unless condition `a or b` with `a` missing;
- unless condition that vetoes (`UnlessRuleReference` early return);
- rule whose result exceeds the decimal limit;
- vetoing rule referenced inside another rule's expression.

Vision-pinning test (must be added alongside): a rule where the VM
short-circuits past an operand (e.g. `flag or expensive_check` with
`flag = true`) — assert the explanation **still narrates the skipped source
part** (the `expensive_check` operand appears in the tree) while the
headline result matches the VM. This guards the source-walking architecture
against future drift toward VM-trace replay.

### 2.5 Explanation completeness: stop discarding operand nodes

`build_expression_children` (`explanations.rs:529–559` and parallel arms)
builds operand nodes for comparisons/boolean operators and then discards
them unless a rule path or messaged veto is present — `rule active: a >= b`
yields an explanation with no operand values. This is the inverse of the
design intent: users should see the values that drove the comparison. Fix:
keep the operand nodes (data inputs with their values) as children. Update
`explanation_format.rs` expectations accordingly; check the rendered output
and `documentation/schemas/explanation.v1.json` for compatibility (additive
children should be schema-compatible, but verify).

---

## Finding 3 — `response.data` is always empty

`engine/src/evaluation/mod.rs`, `expression.rs`,
`engine/src/planning/execution_plan.rs`

Premise (verified): the only `record_data_use` call site is dead code, so
`used_data_paths` is never populated and `response.data` is always `[]`.

**Decision: do not resurrect the runtime tracking — delete it and populate
`response.data` statically from the plan.** Evidence for this direction:
the field only ships in explain mode (the CLI strips `"data"` from JSON
otherwise, `cli/src/formatter.rs:86–88`; OpenAPI documents it as
explain-mode only, `openapi/src/lib.rs:481–486`), and the explanation tree
already embeds every displayed data value. Runtime consumption tracking
would record *optimizer behavior* (what the VM happened to read), which
contradicts the design intent of explaining source-level reasoning, and
reintroduces the displayed-vs-listed asymmetry. Source-level static demand
is the right semantics, and the machinery already exists.

Mechanism:

- `ExecutionPlan::schema_for_rules` (execution_plan.rs:969+) already
  performs exactly the needed walk: a worklist over the requested rules'
  live branches collecting `needed_data: HashSet<DataPath>`, following
  rule-target references transitively, overlay-aware. Extract that walk
  into a reusable helper (e.g.
  `collect_needed_data_paths(&self, rule_names, overlay) -> Result<HashSet<DataPath>, Error>`)
  used by both `schema_for_rules` and the response builder.
- In `Evaluator::evaluate` (mod.rs:1002–1024), replace the
  `used_data_values()` source with: needed paths for the requested
  `response_rules` ∩ `context.data_values` (the effective values — overlay
  plus spec literals — already built in `EvaluationContext::new`). Keep the
  existing iteration over `plan.data.keys()` for stable ordering. Rule
  names are pre-validated by `validated_response_rule_names` in
  `Engine::run_plan`, so the helper's `Err` is an invariant there
  (`expect("BUG: …")`).
- Paths with no effective value (missing data) are simply not listed —
  they surface through missing-data vetoes as today.
- Delete the dead machinery: `used_data_paths` field, `record_data_use`,
  `used_data_values` (mod.rs:67–77, 162–175), and the dead
  `source_context == false` branch plus the now-unused `source_context`
  parameter in `evaluate_single_value` / `evaluate_postorder_expression`
  (`expression.rs:152–195, 594–599`).
- OpenAPI wording tweak (openapi/src/lib.rs:481–486): "Data entries in
  effect for the evaluated rules when explanations are enabled" (was "used
  during evaluation").

Properties of the new semantics: deterministic, optimizer-independent (a
short-circuited-past operand is still listed — it is referenced by the
source), and intended to be a superset of every value the explanation
walker displays. The displayed-vs-listed asymmetry from the earlier draft
disappears by construction; the parity suite should assert the superset
property rather than assume it (note: `schema_for_rules` prunes statically
dead branches given overlay-known values — verify the explanation walker
never displays a value from a pruned branch, or include pruned-branch data
too).

Tests:

- engine test: `response.data` lists exactly the effective values of data
  statically referenced by the requested rules — including a data path the
  VM short-circuits past, and excluding data only referenced by
  *unrequested* rules;
- engine test: data referenced through a rule-target reference chain is
  included (the worklist follows it);
- CLI-level check that the explain-mode "Data" section renders.

---

## Finding 4 — User-reachable panics in planning

### 4.1 Mixed-type range literals (`graph.rs` ~L7491, ~L7547)

`data x: 1 ... yes` panics in `inferred_parent_type_from_literal`
(graph.rs:7491, empirically confirmed: the CLI panics on this exact source)
because `TypeResolver::register_all` is the first step of `Graph::build`
(graph.rs:1156–1158), before the graceful range validation in
`semantics.rs:737–742`. Fix: make `inferred_parent_type_from_literal` return
`Result` (single call site, graph.rs:7547) — `register_all` already returns
`Vec<Error>` and already pushes errors in its `(None, None)` arm, so the
plumbing exists. Also cover text…text ranges, which have no match arm.

Dedup caveat (verified): `dedup_errors` keys on exact
`(kind, message, location)`. For the later duplicate from
`insert_literal_data` to collapse, the new `register_all` error must
reproduce the semantics message verbatim, including type display names
("got number and boolean") — `register_all` works on `ast::Value`, so it
needs a small value→display-name mapping matching `lemma_type.name()`. If the
later path instead emits a different message (e.g. "Type 'x' is not
defined"), two errors survive — acceptable, but add a test asserting the
exact error count.

### 4.2 Qualified type lookup `.expect` (`graph.rs` ~L7963, ~L2281–2310)

When an imported spec fails its own type validation, the consumer's
`lookup_parent_type` panics ("qualified import target must be in
local_types…") — empirically confirmed with the repro below
(graph.rs:7963–7969). Fix:

- `lookup_parent_type` already returns
  `Result<_, Vec<Error>>` and the caller propagates with `?` into
  `self.errors` — so converting the `.expect` into an `Err` mirroring the
  adjacent "Type '{name}' is not defined" branch (graph.rs:7973–7990) is
  sufficient on its own.
- Additionally thread the discarded recursive `bool` out of
  `ensure_spec_types_resolved` (both call sites, graph.rs:2281–2288 and
  2302–2309) to skip the redundant doomed `resolve_and_validate` —
  complementary hardening, not required for correctness.
- Test: spec `b` with `minimum 10 -> maximum 5` (invalid per
  `validate_type_specifications`, graph.rs:9485–9493), spec `a` with
  `uses b` + `data x: b.money` → planning returns errors for both, no panic.

### 4.3 Unbounded transitive inlining blowup (`normalize.rs:89–115, 142`)

A short chain of self-doubling rules (`rule rN: rN-1 + rN-1`) explodes when
`to_normal_form` materializes the `Arc`-shared substitution into an owned
tree → OOM or `assert!(id < u16::MAX)` register panic. Fix:

- Add `max_normalized_expression_nodes` to `ResourceLimits`
  (`engine/src/limits.rs`), with a default well below the register ceiling
  (e.g. 50_000 nodes).
- Count nodes during `to_normal_form` (or in the `normalize_once` fixpoint
  loop) and return a planning `Error` ("rule expands beyond the expression
  size limit after inlining; restructure the rule or reduce repeated
  references") when exceeded. `build_normalized_rule_instructions` already
  returns `Result<_, Error>` and the call site
  (execution_plan.rs:415–430) already maps it into `Vec<Error>` — the error
  return path exists.
- **Plumbing gap (dry-run finding):** `ResourceLimits` does not reach
  planning today. `Engine` holds limits and calls `plan(&Context)`, but
  neither `Context` nor `plan` → `plan_spec` → `Graph::build` →
  `build_execution_plan` carries them. Required: add `&ResourceLimits` to
  that chain (or store limits on `Context`). Note `plan` is publicly
  re-exported (`lib.rs:103`) — signature change is a public API change, and
  ~20 internal test call sites need updating.
- The `u16::MAX` asserts on registers / constants / data / veto tables
  (`normalize.rs:140–185`) sit in an infallible call family
  (`compile_nf` returns bare `u16`). Rather than threading `Result` through
  ~6 compile functions, rely on the pre-compile node-count limit to bound
  table sizes, making the asserts genuine unreachable backstops. Choose the
  default limit so that node count provably bounds register/constant/data
  counts below `u16::MAX`.
- Test: a doubling chain that exceeds the limit → planning error, no panic,
  reasonable runtime.

---

## Finding 5 — Evaluation invariant violations go silent

`engine/src/evaluation/mod.rs`, `engine/src/planning/execution_plan.rs`

### 5.1 Fuel counter in the production VM

`execute_instructions` (mod.rs:273–585) has no step budget; the test
`unpatched_jump_to_zero_hits_step_budget` exercises a private
reimplementation (`run_insn`) instead. Fix:

- Add a step counter to the real loop; on exhaustion,
  `panic!("BUG: instruction step budget exceeded …")` (invariant violation —
  planning guarantees termination). Budget derived from
  `instructions.code.len()` times a generous factor, or a plan-level
  constant.
- Rewrite the VM tests to call `execute_instructions` directly and delete
  `run_insn` (five tests: the four short-circuit tests plus
  `unpatched_jump_to_zero_hits_step_budget`). Ordering constraint: the fuel
  counter must land in the same change as the test rewrite —
  `unpatched_jump_to_zero` has no `Return` and would hang today's
  `execute_instructions`.

### 5.2 Validate jumps at the deserialization trust boundary

`TryFrom<ExecutionPlanSerialized>` (`execution_plan.rs:1570–1599`) must
validate every rule's instructions at load time (same posture as the
existing `validate_unit_index_references` defense in `Engine::run_plan`). A
tampered/stale serialized plan must yield an `Error` at load time, not a
hang. Dry-run findings:

- `validate_instruction_jumps` is `assert!`-based — calling it as-is inside
  `TryFrom` would *panic* on a tampered plan. Add a `Result`-returning
  variant for the trust boundary; keep the panicking form in
  `CompileContext::finish` for the compiler invariant.
- The `JumpIfFalse <= code_len` allowance is dead: audited every emission/
  patch site (`compile_piecewise_rule/_value`, `compile_short_circuit_and/or`)
  — every patched target is followed by at least one further emission plus
  the trailing `Return`, so jump-to-end never occurs in real output, and the
  VM would panic at `pc == len` anyway. Tighten to `< code_len` and delete
  the stale doc comment (execution_plan.rs:137–139).
- Extend load-time validation to the other operand pools (register indices
  vs `register_count`, `constant_index`, `data_index`, `message_index`,
  `INSTRUCTIONS_VERSION`) so the VM's `expect`s remain invariant-only.

### 5.3 Unset registers must panic, not veto

Registers are pre-filled with
`OperationResult::Veto(VetoType::computation("BUG: unset register"))`
(mod.rs:263–267), which makes read-before-write a silent domain veto —
`ResultIsVeto` would even convert it to `true`. Fix:

- Use `Option<OperationResult>` (or an explicit `Unset` variant) for the
  register file; `read_register` panics with "BUG: read of unset register
  r{n}" when encountered. Mechanical change: field decl (mod.rs:77),
  `read_register`/`write_register` (L208–230), the `clear+resize` filler in
  `execute_instructions` — no other users (grep-verified).
- Fix the register-count check (mod.rs:260–262): compare `register_count`
  against `context.plan.max_register_count` (accessible in-module), not
  `Vec::capacity()` — `with_capacity` only guarantees *at least* the
  requested capacity (allocator slack), so the current check is weaker than
  its panic message claims.

### 5.4 `UserVeto` message index

`UserVeto` silently drops the message on an out-of-range `message_index`
(mod.rs:532–545). Use
`.expect("BUG: invalid message_index")` for consistency with `LoadConstant`
/ `LoadData`. (Deserialized-plan tampering is covered by extending the 5.2
load-time validation to table indices, which also lets `LoadConstant`'s
existing `expect`s remain invariant-only.)

---

## Finding 6 — Planning stays silent where it must report

### 6.1 Non-transitive schema compatibility (`planning/mod.rs:88–93`)

`schema_over` checks only adjacent pairs (`windows(2)`), but
`SpecSchema::is_type_compatible` compares only names present in both schemas
and is not transitive (S1 `{x: number}` ↔ S2 `{y}` ↔ S3 `{x: text}` all
"compatible"). Fix: fold the in-range slices into a unified surface map
(`name → type`), erroring (returning `None`) on the first conflict — O(n·m)
instead of all-pairs. `SpecSchema` has `data: IndexMap<String, DataEntry>` +
`rules: IndexMap<String, LemmaType>`, so the fold is direct. Verified single
caller each: `schema_over` ← `validate_dependency_interfaces`
(discovery.rs:478, only checks `is_none()`); `is_type_compatible` ←
`schema_over`. Update `schema_over`'s doc comment (it promises "one of the
in-range slices' full-surface schemas"), and add a three-slice test where a
name skips the middle slice and changes type. Existing temporal-slicing
tests assert rejection of adjacent incompatibilities — a unified fold
strictly widens detection, so they stay green.

### 6.2 Residual `Undetermined` types in successful plans (`graph.rs:386–518`)

`resolve_data_reference_types` (graph.rs:387–518) iterates `self.data` in
insertion order **and batches its updates** (applied after the loop), giving
the pass snapshot semantics — reference→reference chains read the target's
*pre-pass* `Undetermined` type regardless of order, so reordering alone is
not a fix. Fix:

- Iterate in `compute_reference_evaluation_order`'s order (it needs only
  `self.data`, available at this point, and already topo-sorts and reports
  cycles) **and apply each update incrementally** before processing
  dependents — or keep batched updates inside a fixpoint loop. Either works;
  incremental + topo order is the cheaper choice since the order is free.
- Add a final planning gate in `build_execution_plan`
  (execution_plan.rs:356, after `graph.build_data`): any `Undetermined`
  resolved type remaining in the manifest is a planning `Error` — **except
  `Reference { target: Rule(_) }` entries**, which deliberately ship
  `Undetermined` so runtime veto propagation surfaces the rule's veto reason
  (graph.rs:563–570). Scope the gate to data-target references and plain
  data.

### 6.3 Silent incomplete plans (`graph.rs:2268–2270, 2367–2374`)

`ensure_spec_types_resolved` returns `false` without pushing an error when a
nested spec is not registered, and `build_spec` then returns `Ok(())` —
potentially an incomplete plan with no diagnostic. Fix: push an internal
planning `Error` ("spec '{name}' was not registered during discovery") in
the `!is_registered` branch. Related hardening: key `dfs_discover`'s visited
set on `(spec ptr, effective instant)` (`discovery.rs:585–587`) so slices
reached at different instants are both discovered.

---

## Execution order

| Phase | Items | Rationale |
|---|---|---|
| 1 | 4.1, 4.2, 5.3, 5.4 | Small, isolated, immediate contract violations (panics / silent sentinels). |
| 2 | 3 (`response.data` via static schema walk), 5.1, 5.2 | Self-contained evaluation fixes with clear tests. |
| 3 | 1 (normalization gating + units) | Largest semantic change; needs the totality analysis and the test matrix. |
| 4 | 2 (shared branch semantics, decimal clamp, 2.5 operand nodes) | Builds on 1's settled semantics; refactor touches three modules. |
| 5 | 4.3, 6.1, 6.2, 6.3 | Resource limits and silent-gap reporting. |

Each phase lands with its regression tests; phases 3–4 additionally re-run
the full `engine/tests` suite plus the explanation e2e/format suites, since
they intentionally change observable results (folded plans, explanation
bodies, decimal-limit propagation). Changelog entries required for: strict
fold gating (plans may demand data they previously optimized away), typed
zero folding, decimal-limit veto visibility to downstream rules.

## Acceptance criteria

- No `panic!` / `unreachable!` / `expect` / `assert!` in planning reachable
  from spec source text or programmatic AST input (findings 4.x); fuzz-style
  test over malformed range literals and broken imports passes.
- Evaluation: no code path converts an invariant violation into a veto or a
  default; the VM terminates (fuel) and panics with a `BUG:` message on any
  planning-guarantee violation; deserialized plans are fully validated at
  load.
- For every gated fold: folded and unfolded forms of the same expression
  produce identical evaluation results for all of {value present, value
  missing, operand vetoes}.
- VM result, `response.data` (effective values of statically referenced
  data), and explanation (result, winning branch, causes) agree on every
  test in the parity suite; every value the explanation displays is listed
  in `response.data`.
- Explanations continue to narrate source parts the VM logically skipped
  (vision-pinning test in 2.4 stays green), and comparison/boolean
  explanation nodes include their operand values (2.5).
