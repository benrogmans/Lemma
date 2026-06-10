# Plan: Explanations from Recorded Execution

Status: implemented — all phases executed and validated (workspace suite,
differential tests, manual `delivery.lemma` / `effective_distance` runs).
Implementation deviations from this plan, chosen during execution:

- Work landed in consolidated order (recording + tags → dual streams →
  explanation rewrite → rendering) rather than as the two shippable tracks;
  the optimized-stream-only intermediate (Phase 2's degraded conversions)
  was never shipped because the source stream landed in the same change.
- Phase 5's general `ExprId` provenance was narrowed to what the converged
  rendering actually consumes: conversion sites. `NormalForm::UnitConversion`
  carries an `Option<Source>` origin and the compiler emits `conversion_tags`
  (pc → source location); arm tags cover branch decisions; rule results and
  data leaves need no provenance. No other NF node needs an origin today.
- Named compound-unit literal expansion moved into constant interning
  (`CompileContext::constant_index`), applied identically in both streams;
  `fold_unit_literals` itself was left in the optimize pipeline (its
  remaining work is idempotent with the interning expansion).
- The `optimize` flag is realized structurally: `build_normalized_rule_instructions`
  always compiles both streams (`instructions` optimized, `source_instructions`
  with zero rewrite passes) rather than taking a boolean.

Scope: `engine/src/planning/normalize.rs`, `engine/src/planning/execution_plan.rs`,
`engine/src/evaluation/*`, `engine/src/engine.rs`, CLI/server/MCP formatting,
`documentation/`

## Problem

Explanations are currently produced by a **second interpreter** that re-evaluates
source expressions and must mirror the compiler's semantics by hand:

- `evaluation/expression.rs` contains a full postorder AST evaluator
  (`evaluate_postorder_expression` / `evaluate_single_value`, ~450 lines) used
  *only* by explanations, via `resolve_source_expression_values` (3 call sites in
  `evaluation/explanations.rs`).
- `winning_source_branch_and_causes` re-runs every unless condition and
  re-implements branch selection, with a comment admitting the hazard: *"Mirror
  the compiled piecewise exactly (`compile_piecewise_rule`)"*.
- `build_conversion_node` re-evaluates conversion operands and results.

If the compiled program and the shadow interpreter ever disagree — a semantics
change, a veto edge case, evaluation order — the explanation silently lies about
why a result was produced. It also doubles evaluation cost for explained runs.

A symptom that triggered this work (`delivery.lemma`, `distance = 50 kilometer`):

```
delivery_cost: 25 eur
└─ 0.5 eur_per_km * distance
   ├─ distance is 50 kilometer      ← unless-cause masquerading as an operand
   ├─ 0.5 eur_per_km                ← literal repeated from the expression line
   └─ distance: 50 kilometer        ← duplicate of the cause line
```

The condition actually evaluated (`distance < 5 mile`, false) appears nowhere.

### Why a naive execution trace is not enough

The VM does not execute source-shaped programs. `build_normalized_rule_instructions`
inlines rule references, converts to `NormalForm`, then runs a fixed-point
`simplify` loop (13 rewrite passes: associative flattening, identity elimination,
constant folding, De Morgan, short-circuit folds, canonical ordering, …). The
optimized instruction stream has **no runtime events** for many source-level
sub-expressions, so they cannot be recorded — they never happen.

### Supersedes prior design intent

`documentation/plans/planning_evaluation_contract_fixes.md` declared the two
paths intentional ("the VM is authoritative for outcomes; the explanation walker
narrates the full source reasoning"). This plan replaces that intent: source-level
*rendering* is preserved, but every runtime fact in an explanation must come from
an actual execution, never from a parallel evaluation.

## Architecture

Two orthogonal, independent switches:

1. **`optimize: bool`** — the `simplify` fixed-point in
   `build_normalized_rule_instructions` becomes optional. It is already a
   `NormalForm → NormalForm` transform (same types in and out), so skipping it is
   omitting a rewrite pass, not a second compiler. One lowering path, one
   instruction set, one VM.
2. **`record: bool`** — `execute_instructions` optionally retains what happened:
   the final register file, branch decisions, and which `Return` fired.

Explanations are the composition: **evaluate with recording on, and that run's
result is the authoritative result** returned in the response. Never run one
stream and explain from a separate run — result and explanation must come from
the same execution by construction.

The shadow interpreter is then deleted wholesale. The explanation builder becomes
a pure function of *(source structure × recording)*.

### Fidelity spectrum — the flags compose into independent deliverables

Recording does not care which instruction stream it observes; what varies is
how much of the recording maps back to source:

| Mode | Recordable facts | Explanation fidelity |
|---|---|---|
| `(optimize: true, record: false)` | — | none (today's fast path, unchanged) |
| `(optimize: true, record: true)` | rule results, data leaves, **branch decisions**; conversion operand/result values are physically recorded when the `UnitConversion` instruction executes, but unattributed to source nodes | full converged rendering; conversion steps complete where the operand value is attributable (leaf operands, folded-constant conversions), degraded otherwise |
| `(optimize: false, record: true)` | + every source sub-expression value, attributed | full, including conversion traces over compound operands |

Branch decisions survive optimization because `compile_piecewise_rule` runs
*after* all rewrite passes and emits `JumpIfFalse`/`Return` per arm; tagging
those instructions with arm indices needs no expression-level provenance.
Consequence for sequencing: **the shadow interpreter can be deleted and the
rendering redesign shipped against the optimized stream alone** (Phases 1–3
below), with conversions degrading gracefully (full steps when the operand is
a data leaf — the common case; no claimed operand value otherwise). The
`optimize: false` stream and fine-grained provenance are a purely additive
fidelity upgrade (Phases 4–5), not a prerequisite.

Precondition to verify with a pinning test: no rewrite pass eliminates or
reorders piecewise *arms* (conditions folding to literal `true`/`false` is
fine — the jump still executes; an arm being dropped would break arm-index
tagging). From the current `simplify` pass list, arms are rewritten internally
but the arm list is preserved.

Other free byproducts of the flags being independent:

- `(false, false)` — reference semantics for differential testing.

## Research findings (current state)

Compilation pipeline (`build_normalized_rule_instructions`, normalize.rs L96–143):

1. `unless_branches_to_piecewise` — branches → one `Piecewise` expression.
2. `substitute_completed_rule_paths_arc` / `substitute_rule_target_data_paths_arc`
   — **rule references are inlined** (the VM has no call instruction). The
   inlined `Arc<Expression>` is returned and kept in `completed_rules` for
   downstream inlining, but not stored on the plan.
3. `to_normal_form` — `Expression → NormalForm`, structure-preserving
   (binary ops stay binary; flattening is a separate pass).
4. Fixed-point loop over `normalize_once` = `normalize_children` + `simplify`
   (normalize.rs L1356–1375). **This loop is "normalization"** and is the part
   that becomes optional.
5. `compile_normal_form` → `Instructions` (register VM; loop-free forward jumps).

Facts the design relies on (verified):

- `compile_nf` handles **every** `NormalForm` variant directly (`Subtract`,
  `Divide`, `Not`, …), so an un-simplified NF tree compiles as-is.
- **Caveat — one expansion hides in the rewrite pipeline:** `fold_unit_literals`
  expands named compound-unit literals (e.g. `0.5 eur_per_km` → `eur/kilometer`
  signature) via `expand_named_quantity_literal`. The test
  `named_compound_unit_literal_must_expand_signature_in_normalized_instructions`
  pins this as required for correct arithmetic. This is lowering, not
  rewriting — Phase 1 relocates it into compilation so it applies in both
  modes (see Phase 1).
- Registers are allocated monotonically per rule and **never reused**
  (`CompileContext::allocate_register`). After `execute_instructions`, the
  register file already holds the value of every executed instruction; untaken
  branches are `None`. Recording is therefore nearly free: keep the register
  file instead of discarding it.
- `ExecutableRule` already retains source `branches` (condition/result
  `Expression`s) — the rendering structure needs nothing new.
- Plans are built eagerly at load time (`build_execution_plan`) and cached in
  `Engine::plan_sets` per (repository, spec, effective slice). They are also
  serializable (`ExecutionPlanSerialized`, `INSTRUCTIONS_VERSION`).
- `Evaluator::evaluate` already runs *all* local rules through the VM when
  `explain` is true, and `rule_results` holds every rule's authoritative result.
  Data values live in the overlay/`EvaluationContext`. So rule-reference and
  data-leaf values in explanations already need no recording — only branch
  decisions, condition outcomes, and intermediate values (conversion operands)
  are missing today, which is exactly what the shadow interpreter reconstructs.

## Design

Track A (Phases 1–3) ships the redesign against the **optimized** stream:
recording + arm tagging, explanation rewrite, rendering. Track B (Phases 4–5)
adds the `optimize: false` stream and fine-grained provenance as a fidelity
upgrade. Phase 6 (consumers/docs) applies to both.

### Phase 1 — Arm tagging + `record` flag in the VM

`engine/src/planning/normalize.rs` / `execution_plan.rs`:

- `compile_piecewise_rule` tags each arm's `JumpIfFalse` and each `Return`
  with its **arm index** (a small parallel table on `Instructions`, e.g.
  `arm_tags: Vec<(pc, ArmTag)>`; the `Instruction` enum stays untouched).
  This needs no expression-level provenance — the piecewise compiler knows
  the arm index at emit time, after all rewrite passes have run.
- Pinning test: rewrite passes preserve the piecewise arm list (count and
  order); conditions may fold to literals but arms are never dropped.
- Bump `INSTRUCTIONS_VERSION` / extend `ExecutionPlanSerialized`.

`engine/src/evaluation/mod.rs`:

- `execute_instructions(instructions, context, recording: Option<&mut RuleRecording>)`.
- `RuleRecording` (per rule, per run):
  - `registers: Vec<Option<OperationResult>>` — the register file, moved out
    instead of cleared (zero extra work during execution),
  - `branch_decisions: Vec<(pc, bool /* condition truth */)>` — appended at each
    `JumpIfFalse`,
  - `returned_pc: u32` — set when `Return` executes (identifies the winning arm
    via its arm tag).
- `Evaluator::evaluate(explain: true)` keeps a
  `HashMap<RulePath, RuleRecording>`. The recorded results **are** the response
  results (single-run authority).

### Phase 2 — Rewrite the explanation builder; delete the shadow interpreter

`engine/src/evaluation/explanations.rs`:

- `winning_source_branch_and_causes` reads the winning arm and each evaluated
  condition's outcome from the recording (arm tags + `branch_decisions` +
  `returned_pc`). The hand-mirrored evaluation-order logic and
  `branch_semantics::unless_condition_outcome` coupling disappear.
- Data leaves keep using `resolve_data_path_value` (a lookup, not an
  evaluation); rule references keep embedding from `built` + `rule_results`.
- `build_conversion_node`: conversion facts split three ways in this phase.
  *Structure and target* are always known (source expression). *Factor* needs
  only the operand's unit signature, not its magnitude
  (`quantity_unit_equivalence_step_text`). *Operand/result values* are
  rendered when attributable: data-leaf or literal operands (lookup — the
  common case), and constant-folded conversions (fully static). For compound
  runtime operands the values exist in the recorded register file but cannot
  yet be attributed to the source node; render the conversion without claimed
  values until Phase 5 makes attribution total. (Optional cheap improvement,
  only if it falls out naturally: a conversion id tag on `UnitConversion`
  instructions, best-effort through the rewrite pipeline — render values when
  the tag survives, degrade when not.)
- Delete `resolve_source_expression_values`, `evaluate_postorder_expression`,
  `evaluate_single_value`, `collect_postorder` and friends once explanations no
  longer call them (~500 lines). Check `partial.rs` is unaffected (it has its
  own conservative logic — it is).
- Sub-expressions the VM never executed render without a claimed value (the
  structure still shows; values show where they exist). This is a deliberate
  semantic improvement: the old full-narration behavior *invented* values for
  branches that were never taken.

### Phase 3 — Rendering redesign (the converged output format)

With facts now trustworthy, implement the agreed format:

- **Causes are true facts.** A falsified condition is flipped at cause-construction
  time (comparison operator complements: `<` → `>=`, `is` → `is not`, …;
  `not x` → `x`; fallback `{condition} is false` for non-trivially-flippable
  forms). Stored flipped in JSON too, so API consumers get the same fact.
- **Causes render at rule level**, siblings before the body — never nested under
  the body expression.
- **Cause children** are `ExplanationNode`s built from the condition's structure:
  data leaves and embedded rule references with their recorded values.
- **No literal repetition**: pure-literal operands are not emitted as children
  (the expression line already shows them). The `DataInput { data: "" }`
  placeholder hack for literals disappears.
- **Full trees, no dedup heuristics**: a rule reference embeds its full
  explanation wherever it appears. No occurrence tracking, no collapsing.
- The vetoing condition does not duplicate itself as both body and cause.
- Embedded rule nodes display `name: result` (requires carrying the result on
  `ExplanationNode::Rule`; additive JSON field).

Target output for the running example:

```
delivery_cost: 25 eur
├─ distance >= 5 mile
│  └─ distance: 50 kilometer
└─ 0.5 eur_per_km * distance
   └─ distance: 50 kilometer
```

### Phase 4 — `optimize` flag: all-or-nothing normalization (fidelity track)

Normalization is binary: with `optimize: true` the full `simplify` fixed-point
runs; with `optimize: false` **zero rewrite passes** run —
`to_normal_form` feeds `compile_normal_form` directly. There is no "required
passes" subset and no pass classification to maintain. The contract this
establishes: **the compiler must emit a correct program from any un-simplified
`NormalForm`**. If a rewrite ever turns out to be load-bearing for
correctness, that is a compiler/VM gap to fix (caught by the differential
suite), not a reason to reintroduce mandatory passes.

`engine/src/planning/normalize.rs`:

- **Relocate named compound-unit literal expansion out of the rewrite
  pipeline into compilation**: apply `expand_named_quantity_literal` when
  interning literal constants (`CompileContext::constant_index` / the
  `Leaf(Literal)` arm of `compile_nf`), identically in both modes. It is
  deterministic per literal given the unit index — lowering, not rewriting.
  `fold_unit_literals` keeps only its genuine optimization (folding
  number-to-number conversions) and runs only under `optimize`. The pinning
  test moves/extends to cover the unoptimized stream.
- `build_normalized_rule_instructions` gains the flag and returns both
  instruction streams (or is called twice).

`engine/src/planning/execution_plan.rs`:

- `ExecutableRule` gains `source_instructions: Instructions` — compiled with
  `optimize: false`, eagerly, at plan time. Eager keeps plans immutable and
  serializable; skipping the fixed-point loop makes the second compile cheap.
  Measure plan build time and memory on the documentation examples; if the
  overhead matters, gate building it behind an engine-level setting in a
  follow-up (do not complicate this phase).

**Differential test (the keystone):** for every spec in the engine test corpus
and `documentation/examples`, evaluate all rules with both instruction streams
across representative data inputs and assert identical `OperationResult`s
(values, vetoes, messages). This turns "normalization preserves semantics" from
an unstated assumption into a CI-enforced invariant. Any divergence found is a
pre-existing normalization bug and gets fixed (or explicitly documented) before
Phase 5 ships.

### Phase 5 — Provenance: instruction → source expression (fidelity track)

Maps recorded register values back to source nodes, restoring full conversion
traces (and enabling any future value-level rendering).

- Define `ExprId` (u32). Before compiling a rule, number the nodes of its
  **local source expressions** (each branch's condition and result, walked
  deterministically). Store the table on `ExecutableRule` or derive it on
  demand by the same deterministic walk (decide during implementation; derive
  is less state, store is less fragility — prefer store).
- `NormalForm` becomes origin-carrying: mechanical refactor to
  `struct NormalForm { kind: NormalFormKind, origin: Option<ExprId> }` (or an
  equivalent wrapper). `to_normal_form` threads the id from the `Expression` it
  consumes. Inlined subtrees (from rule substitution) carry the `ExprId` of the
  `RulePath` leaf they replaced at their root and `None` inside — explanations
  never descend into inlined copies; they embed the referenced rule's own
  explanation and its recorded result.
- Optimization passes may drop or merge origins freely — only the
  `optimize: false` stream is used for value-level provenance, so origins only
  have to survive `to_normal_form` and compilation.
- `CompileContext` emits a parallel `origins: Vec<Option<ExprId>>` (one per
  emitted instruction).
- `Evaluator::evaluate(explain: true)` switches to executing
  `source_instructions` (recorded run stays the single authority).
- Lookup helper: `ExprId → Option<&OperationResult>` via `origins` +
  `registers`. `None` means "not executed" (short-circuited / untaken arm) —
  a legitimate, honest answer. `build_conversion_node` upgrades from Phase 2's
  leaf-only values to recorded values for compound operands.

### Phase 6 — Consumers, docs, cleanup

- Update unit tests in `explanations.rs` (`unless_causes_*`, golden JSON
  `CALC_TOTAL_IS_RUSH_ONLY_GOLDEN_JSON`) and CLI integration tests
  (`cli/tests/integrations/run.rs`).
- JSON schema notes for HTTP/MCP consumers: `condition` becomes the true-form
  expression (semantics change), `children`/`result` fields are additive.
  `CHANGELOG.md` entry.
- Mark the superseded paragraph in
  `documentation/plans/planning_evaluation_contract_fixes.md` with a pointer to
  this plan.
- `coverage`/bench check: `cli/benches/engine_profile.rs` touches explanations.

## Validation

- Phase 1 pinning test: piecewise arms survive all rewrite passes (count and
  order).
- Phase 4 differential suite (optimized vs source instructions) across the
  full test corpus and examples — the load-bearing invariant for the fidelity
  track.
- Existing engine + CLI test suites per phase.
- Manual: `lemma run delivery -ix` on `playground/delivery.lemma` and the
  `effective_distance` round-trip scenario reproduce the target trees.

## Risks / open questions

- **Precision parity:** if the differential suite finds optimized/unoptimized
  result differences (intermediate rounding, canonical-unit precision), that is
  a pre-existing normalization soundness bug. It must be fixed or explicitly
  ruled in-scope/out-of-scope before Phase 5; switching the recorded run to the
  unoptimized stream means users would otherwise see different numbers with `-x`.
- **Plan size:** `source_instructions` + origins roughly double per-rule
  instruction storage. Expected to be small in absolute terms; measure in
  Phase 4 and only then consider gating.
- **`NormalForm` refactor breadth:** adding origins touches every pass in
  normalize.rs mechanically. Mitigated by doing it as a dedicated commit with
  no behavior change (origins unused until the Phase 5 lookup lands).
- **Inlined piecewise sub-values** (`compile_piecewise_value`): rule references
  with unless branches inlined into expressions record per-arm decisions inside
  the *dependent's* instructions. Explanations embed the referenced rule's own
  explanation instead, so these inner decisions are not consumed — verify no
  rendering path needs them.
