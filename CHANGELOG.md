# Changelog

Releases cover the Lemma engine, `lemma` CLI, OpenAPI crate, LSP, SDKs and VS Code extension. They all follow the same version everywhere. The release version is `[workspace.package] version` in the root `Cargo.toml`. Git tags follow `lemma-v{version}` (for example `lemma-v0.8.20`); releases before the rename used `cli-v{version}`. Draft notes for the next version quickly by running `cargo changelog` to print `git diff` / `git log` since the latest release tag (`xtask` `versions-diff`). Tip: feed that into an LLM to create a summary for this changelog.

## [0.8.20] - 2026-06-19

0.8.20 CI-gates every ```lemma fence in the repo and adds offline registry fixtures for tests.

### Added

- **`documentation_fences` integration test**: parses, loads, and runs every ```lemma fence in repo `*.md` / `*.txt` files.
- **`LemmaBase::test()` / `with_fixture_dir`**: offline registry backed by bundled fixtures in `engine/tests/registry_fixtures/` (no network).
- **`lemma lsp` in CLI docs**: documents the language server command.

### Changed

- **Documentation overhaul**: registry examples and test fixtures migrated from `@lemma/std` to `@iso/countries`; embedded stdlib remains `uses lemma units`.
- **README direction**: inversion listed as planned work rather than already exposed.

### Removed

- **Internal plan documents** under `documentation/plans/` (5 files).

## [0.8.19] - 2026-06-11

0.8.19 fixes registry resolution and quantity planning/materialization bugs; removes a redundant SDK method.

### Added

- **Quantity ceil/floor/round/abs**: preserve operand unit.

### Fixed

- **Registry resolve skips non-`@` repository qualifiers**: workspace-local repository references no longer trigger registry fetches.
- **Decomposition promotion**: binding aliases no longer collide across quantity families.
- **Materialization**: converted quantities honor type `decimals`; decimal overflow vetoes the rule.
- **Inherited units**: conflicting inherited unit definitions rejected at planning.
- **Unit-index validation**: structural plan checks run at planning and deserialize, not first `run`.

### Removed

- **`Engine.repositories()` / `Lemma.repositories/1`**: returned only `{ name, dependency }` per loaded repository — the same fields already on `list()[].repository`. Use `list()` for loaded-repo discovery.

## [0.8.18] - 2026-06-10

0.8.18 completes the recorded-execution explanation architecture: explanations now read all runtime facts (register values, branch decisions, winning arm) from a recorded execution of the rule's source-shaped instruction stream — they never re-evaluate expressions. The language server is unified into the `lemma` CLI binary, eliminating the standalone `lsp` crate.

### Added

- **Explanations from recorded execution**: each rule now carries a second instruction stream (`source_instructions`) compiled from the unoptimized source expression graph. When explanations are requested the VM executes this stream, records a `RuleRecording` (register values, `BranchDecision` per `JumpIfFalse`, winning `Return` pc), and the explanation builder reads that recording — it never calls back into the evaluator. This makes it structurally impossible for an explanation to disagree with its result.
- **Arm and conversion tags on instructions**: `Instructions` now carries `arm_tags` (mapping each `JumpIfFalse`/`Return` to a source branch index and `ArmRole`) and `conversion_tags` (mapping `UnitConversion` instructions to their source context). These let the explanation builder correlate recorded execution with source structure without re-parsing.
- **`UnitEquivalence` explanation node**: implicit unit conversions inside arithmetic now emit an equivalence fact (`1 mile is 1.60934 kilometer`) as a child node, so cross-unit math is auditable without external lookup tables.
- **`result` field on `Rule` explanation node**: every rule explanation now includes the computed result as a formatted string alongside the body and causes.
- **Cause `children`**: `Cause` nodes now carry child `ExplanationNode`s showing the data values and embedded rule explanations that drove the condition.
- **Negated-condition causes as true facts**: a failed comparison is flipped to its complement (`distance < 5 mile` that failed → `distance >= 5 mile`) so the explanation states what held rather than what was tested.
- **Differential optimization test suite** (`engine/tests/differential_optimize.rs`): pins the optimized and source instruction streams to identical results across the test corpus, catching optimizer divergence automatically.
- **LSP built into the CLI**: the language server now compiles directly into the `lemma` binary (`cli/src/lsp/`) using `tower-lsp` instead of depending on the separate `lsp` crate. `lemma lsp` works as before — editors need no configuration changes.

### Changed

- **Explanation builder reads recordings, not the evaluator**: `winning_source_branch_and_causes` and the body walker receive an `ExplainCtx` containing the immutable `EvaluationContext` and `RuleRecording`, removing the `&mut` evaluator dependency that allowed re-evaluation divergence.
- **`branch_semantics` functions `and_conjunct_outcome` / `or_disjunct_outcome` are now `#[cfg(test)]`**: the explanation walker no longer calls them at runtime — they remain as executable specifications verified by unit tests.
- **Instruction stream version bumped to 2**: `INSTRUCTIONS_VERSION` incremented for the new `arm_tags`, `conversion_tags`, and `source_instructions` fields; stale serialized plans are rejected at load.
- **Identity conversions omitted from explanations**: when an operand is already in the target unit, the redundant source step is suppressed.
- **Conversion multipliers prefer decimal display**: unit factors that round-trip exactly through decimal render as `1.60934` rather than a rational fraction.
- **Release workflow**: increased crates.io index propagation wait (30 s → 60 s) to reduce transient publish failures in CI.

### Removed

- **`lsp` crate dependency from CLI**: `cli/Cargo.toml` no longer depends on the workspace `lsp` crate; `tower-lsp` is used directly.
- **`unique_data_value_by_name`**: the fallback data-path lookup used by the old re-evaluating explanation walker is removed.
- **Source expression re-evaluation in explanations**: `resolve_source_expression_values` is no longer called by the explanation builder (the function remains for other internal uses).

## [0.8.17] - 2026-06-10

0.8.17 replaces tree-walking evaluation with a compiled virtual machine, makes exact math hold at any magnitude and gives every result a machine-readable explanation. Planning now compiles each rule into a validated instruction stream that a register-based VM executes, so evaluation costs only what the requested rules cost: the engine skips unrequested rules and builds explanations only on demand. Execution plans are no longer cloned per request. Larger calculations whose intermediate values exceed machine-integer range stay exact instead of switching to approximation. Current measured performance is published in [`documentation/benchmarks/`](documentation/benchmarks/).

### Added

- **Explanations fit for audit trails**: every rule result carries a flat explanation object holding the rule's body, its operand values, the branch that applied, and the condition that vetoed. The format is specified in a JSON schema ([`documentation/schemas/explanation.v1.json`](documentation/schemas/explanation.v1.json)); the previous trace format was undocumented and is replaced.
- **Explanations read recorded execution, never re-evaluate**: when explanations are requested, the engine executes a source-shaped instruction stream (compiled from the same inlined rule equation with the optimizer's rewrite passes skipped) and records what happened — branch decisions, the winning arm, register values. The explanation is rendered purely from source structure plus that recording, and the recorded run's result is the response result, so an explanation can never disagree with the answer it explains. The previous implementation re-evaluated source expressions in a parallel interpreter, which could silently diverge from the VM. A differential test suite pins both instruction streams to identical results across the test corpus and documentation examples.
- **Explanations state causes as facts**: evaluated unless conditions appear as true statements — a failed `distance < 5 mile` is stated as `distance >= 5 mile` — with the data values that drove them as children. Causes render at the rule level (they explain branch selection, not the body computation), literal operands are no longer repeated below expressions that already display them, embedded rule references show `name: result` and carry their full explanation tree wherever they appear. Implicit unit reconciliation inside arithmetic and comparisons is stated as an equivalence fact (`1 mile is 1.60934 kilometer`, decimal when exact) so cross-unit math is followable without external lookup tables; identity conversions and steps that would restate an already-visible value are omitted. JSON consumers: `causes[].condition` now holds the true-form condition expression instead of a datum name, `causes[].children`, rule-node `result`, and the `unit_equivalence` node are new, and the wrapping `compose` node duplicating the rule body is gone (operands are direct children).
- **One-binary editor setup**: installing the `lemma` CLI is now the only requirement for editor support — the new `lemma lsp` subcommand starts the language server over stdio. This removes the separate language-server binary and the version skew it allowed.
- **A shared server survives bad specs**: a service evaluating specs it did not author can no longer be hung or crashed by them. Self-doubling rule chains are rejected at planning with a resource-limit error (`ResourceLimits::max_normalized_expression_nodes`, default 30,000) instead of exhausting memory; tampered or stale serialized execution plans are rejected at load by full instruction validation instead of crashing the virtual machine; a step budget halts instruction streams that loop.
- **Reproducible performance reports**: `cargo benchmarks <engine|cli|all>` regenerates the engine and CLI benchmark reports in [`documentation/benchmarks/`](documentation/benchmarks/), so the published numbers can be independently re-measured from the repository.

### Changed

- **Compiled virtual machine**: rules are compiled at planning into register-based instruction streams that the engine executes directly, replacing per-request tree-walking of the expression graph. Compilation happens once per plan; evaluation then dispatches flat instructions over a register file. Run output is unchanged.
- **Greater precision for math with large numbers**: financial and scientific calculations whose intermediate values grow very large now stay exact end to end. Previously, magnitudes were bounded by `i128` (~1.7×10³⁸) and arithmetic beyond that bound fell back to decimal approximation; that fallback is gone. A calculation that genuinely exhausts memory vetoes the affected rule with `out of memory` rather than taking the process down. Transcendental functions (`sqrt`, `sin`, `log`, …) compute in decimal as before; see [`documentation/numeric_precision.md`](documentation/numeric_precision.md).
- **Improved performance**: callers that need one answer no longer pay for the whole spec. Evaluation computes only the requested rules (`rules: Option<&[String]>` on `Engine::run` / `Engine::run_plan`), explanations are built only when `explain` is set, and immutable plans (`DataOverlay`) remove the per-request plan clone; the VM (above) removes per-request expression-tree walking. On the benchmark specs, a single-rule evaluation measures 20–169 µs where 0.8.16 measured 285 µs–6.2 ms evaluating every rule with per-call JSON parsing — methodology and numbers in [`documentation/benchmarks/engine.md`](documentation/benchmarks/engine.md). API: `None` means all local rules, and `lemma::plan(context)` is now `lemma::plan(context, &ResourceLimits)`.
- **Plans serve concurrent requests**: an `ExecutionPlan` is immutable — data values ride alongside in a `DataOverlay` instead of mutating the plan — so one compiled plan can be shared across requests and memory allocation is reduced. Run output unchanged.
- **Decisions always show what they depended on**: the optimizer can no longer change which inputs a result requires — algebraic folds (`x * 0`, `false and …`, …) apply only to literal operands, so `rule r: x * 0` still requires `x` and vetoes when it is missing. `response.data` again lists the effective values of the data behind the requested rules (it had regressed to always empty). Together these guarantee an auditor sees the true inputs of every decision.
- **Consistent explanations**: a result exceeding the decimal output limit now vetoes identically in downstream references, `is veto` checks, explanations, and the response — previously these could disagree. Explanations of vetoed unless conditions now name the vetoing condition and carry its veto instead of describing a branch that never ran. Callers and auditors can no longer receive contradictory accounts of the same evaluation.
- **Range error messages**: mixed-type range literals (`data x: 1 ... yes`), text range literals, type references into a spec that failed its own type resolution, and temporal slices that change a type mid-history now fail planning with a descriptive error where they previously crashed the engine.
- **LSP integration**: extensions call `lemma lsp`; requires a globally installed `lemma` CLI. Release the CLI before publishing the extension update. `cargo lsp` (`xtask`) release-builds `lemma` accordingly.
- **Honest cross-language benchmarks**: the Lemma-vs-Python comparison now measures equivalent work — typed inputs on both sides, JSON parsed once before the timed loop, one terminal rule per fixture, and Python ports using exact `fractions.Fraction` arithmetic matching Lemma's rational model.

### Removed

- **Dependencies**: `num-rational`, `num-integer`, `postcard`, `sha2`, and `boolean_expression` dropped from the engine; `proptest` and `insta` dropped from dev-dependencies. Fewer third-party crates to audit and update.
- **Legacy trace API**: `EvaluationTrace` / `TraceNode`, `format_provenance_explanation`, and `Response::filter_rules` are replaced by the explanation object, `format_explanation`, and the `rules` evaluation parameter.
- **Inversion module**: the experimental inversion API was unfinished and has been removed from this release. `Engine::invert`, Elixir `Lemma.invert`, and the `lemma_invert` NIF are no longer available. Inversion will return in a future release.

## [0.8.16] - 2026-06-03

0.8.16 makes unit math smarter and the API simpler. Quantity arithmetic now flows across types — `rule wage: rate * hours` resolves to a money amount on its own — and every quantity or ratio result reports all of its declared units, so callers read the unit they want instead of passing display-conversion flags. Calendar periods (years, months) are now ordinary quantity units from the standard library, and spec authors set values on imported specs with the clearer `with` keyword.

```lemma
spec employment_contract

data salary: quantity 
  -> unit eur 1

rule net: salary * 1.3


spec employment

uses contract: employment_contract
with contract.salary: 5000 eur

rule net_salary: contract.net
```

### Added

- **Cross-type quantity arithmetic**: multiplying or dividing quantities of different types now produces the right unit automatically and promotes the result to a matching named type when one exists in scope (e.g. `rate * hours` → money). Ambiguous results are rejected at planning rather than guessed.
- **Cross-type quantity comparison**: dimensionally equal quantities (e.g. a per-hour rate vs a per-minute rate) compare correctly in rule conditions and inversion.
- **Named type ranges**: declare a range over any rangeable named type, e.g. `data estimate: money range`. Unsupported bases (`text range`, …) are rejected at planning.
- **`time range`**: half-open time-of-day intervals such as `09:00...17:00`, with `in` containment and span in duration units. Endpoints must share a timezone; reversed literals do not wrap past midnight.
- **Quantity-range span**: any specialized `quantity range` (mass, money, duration, …) projects its width with `(lo...hi) as <unit> as number` when the unit is in the same family; cross-family span is rejected.
- **Structured data input**: JSON unit maps (`{"eur": "84"}`) are accepted at the CLI, HTTP, and WASM boundaries.

### Changed

- **Binding keyword `fill` → `with`**: set values on an imported spec with `with alias.field: …`. Local `with name: …` is rejected — use `data` for slots in the current spec.
- **In-spec unit conversion only**: display-time conversion flags (`lemma run --as`, HTTP `as_units`, WASM `rule_result_units`) are removed. Convert with `as <unit>` in the spec; quantity and ratio rule results now return every declared unit as a map.
- **Calendar periods are units**: years and months are quantity units in the standard library via `uses lemma units` (`units.calendar`). The standalone `calendar` and `calendar range` types are removed; a calendar range comes from `units.calendar -> default 18 year...67 year` or inline literals like `18 year...67 year`. The names `month`, `year`, `week`, and `day` are reserved for calendar/duration units.
- **No canonical unit required**: a `quantity` type no longer needs a factor-1 unit; magnitudes stay anchored to the units you declare.
- **Compound unit display**: results whose unit is a combination render in operator style (e.g. `26.66… eur·hour/minute`); single-unit values stay `<magnitude> <unit>`.

### Fixed

- Unit-conversion explanations no longer drop a step when both the source and target units are explicitly declared.
- Comparing dimensionally compatible quantities of different types during inversion no longer crashes.

### Breaking

- **`fill` → `with`**: update binding rows and tooling; the serde `DataValue` tag is now `"with"`. A bare `with name:` / `fill name:` (no import alias) is a parse error.
- **Display-conversion API removed**: drop `--as`, `as_units`, `rule_result_units`, and `EvaluationRequest`; read the unit you need from each rule result's unit map (`results.<rule>.quantity`, etc.). Evaluate/load no longer accept legacy `{value, unit}` payloads — use unit maps.
- **Calendar types removed**: replace `data band: calendar range` with `uses lemma units` and `data band: units.calendar -> default 18 year...67 year`. The API `kind` tags `calendar` and `calendar_range` are gone.

## [0.8.15] - 2026-05-25

### Added

- **Cross-type result unit derivation via symbolic unit signatures**: arithmetic between named quantity types now derives a result unit from the user-chosen operand units. `batch_size_ce / packaging_speed` (with `packaging_speed` declared as `ce/minute`) produces `<n> minute` directly, with no `as <unit>` cast required. Combined signatures that resolve unambiguously to a single named unit in scope auto-promote the anonymous intermediate to that named type; ambiguous signatures (the same composite signature matching units in two distinct types) are now a planning error that asks the spec to rename one of the conflicting units or differentiate the factor.
- **Unified ratio units across types**: same unit name (e.g. `percent`, `permille`, `basis_points`) may be reused across distinct `ratio` typedefs in the same spec as long as the conversion factors match. Mismatched factors still error at planning. Built-in `percent` / `permille` collisions across multiple `data: ratio` fields are now valid; cross-type ratio rule-result conversion (`lemma run --as rule:unit`) works across the unified unit space.
- **Ratio range defaults**: ratio ranges may declare a default literal range, e.g. `data band: ratio range -> default 10%...50%`. The default participates in schema (`SpecSchema.data[].default`) the same way scalar ratio defaults do.
- **LSP navigation for `uses` references**: a `uses @org/repo spec` line becomes a single clickable link that jumps to the resolved dependency file in `lemma_deps/` at the spec's starting line; hover shows the LemmaBase URL. `uses lemma units` opens an on-demand snapshot at `lemma_deps/lemma.std`.
- **`documentation/llms.txt`**: authoring guide for LLMs translating natural-language policy into Lemma specs. Linked from `documentation/index.md` and the root README.
- **`lemma` CLI on npm**: install without Rust via `npm install -g lemma` or run ad-hoc with `npx lemma`. The umbrella `lemma` package resolves a per-platform binary from `@lemmabase/cli-{linux,darwin,win32}-{x64,arm64}` optional dependencies; no postinstall scripts, works offline once installed.

### Changed

- **Per-quantity-type unit normalisation removed**: the engine no longer rescales a quantity's natural-factor units to a per-type canonical at planning. Stored magnitudes follow the unit declarations as written; cross-type arithmetic combines natural factors directly, so `1 ce_per_minute * 1 minute` now lands on `1 ce` rather than going through an opaque per-type scale. Specs that relied on hidden rescaling for derived types lacking a factor-1 unit must add one (e.g. declare the canonical base unit explicitly) so that result magnitudes remain anchored to a known unit. No user-visible value change for specs whose canonical unit was already factor 1.
- **Case-insensitive logical identifiers**: spec, data, rule, unit, and repo names are canonicalised to lowercase at parse. `repo` blocks that differ only by case are merged. API surfaces (spec lookup, data override keys, `rule_result_units` keys) lowercase inputs at the boundary; internal `eq_ignore_ascii_case` lookups are replaced with exact match on canonical names. The formatter emits identifiers in lowercase.
- Test registry references and the `12_registry_references` integration example modernised to `uses lemma units` plus reformatted `uses @iso/countries alpha2` blocks.
- Quality CI workflow declares an explicit `contents: read` permission.

### Fixed

- Local test runs no longer load the embedded stdlib twice (deduplicated stdlib include when validating workspaces that already reference `lemma units`).
- Ratio typedef `default` now requires a unit (matching `minimum` / `maximum` and the 0.8.14 contract): `-> default 0.015` is rejected; use `-> default 1.5%` or `-> default 500 basis_points`.

### Breaking

- **Workspace dependency directory renamed from `.deps/` to `lemma_deps/`**. Move any existing `.deps/` content into `lemma_deps/`; old `.deps/` directories are no longer recognised. The public `LEMMA_DEPS_DIR_NAME` constant in `lemma::deps` is the single source of truth. Embedded stdlib snapshot path moved from `engine/src/lemma/si.lemma` to `engine/src/lemma/units.lemma.std` (and surfaces in workspaces as `lemma_deps/lemma.std`).
- **Identifier canonicalisation is lossy**: any integrator that round-tripped mixed-case identifiers through the engine (API calls, override maps, rule-result unit maps) now receives lowercase. Callers comparing identifiers case-sensitively to engine output must lowercase their side too.

## [0.8.14] - 2026-05-21

- Branch on failed rules: `is veto` / `is not veto` (e.g. `unless price is veto then fallback`).
- Return a rule’s result in another unit without changing the spec: CLI `lemma run --as rule:unit`, HTTP `?as_units=rule:unit,...`, MCP/WASM `rule_result_units` (quantity conversion or ratio relabel).
- Time: elapsed intervals via `uses lemma units` and types like `units.duration` (no built-in `duration` type); calendar periods (`year`, `month`, `week`) on **calendar** and **date** types — not mixed with elapsed durations.
- **Calendar** type and **calendar range** for calendar-aware periods; **date**, **number**, **quantity**, and **ratio** ranges with half-open `lo...hi`; width via `(lo...hi) as <unit>` for number, duration quantity, and ratio ranges.
- Compound quantity units (e.g. rates built from SI units in `uses lemma units`).
- Arithmetic stays exact until API output; JSON magnitudes are decimal strings (see `documentation/numeric_precision.md`). Division by zero in rule bodies is rejected at planning time.

### Breaking (0.8.13 → 0.8.14)

- `scale` → `quantity` (and `scale range` → `quantity range`) in specs and API `kind` tags.
- `uses lemma` → `uses lemma units`; stdlib is embedded `repo lemma` / `spec units` (`units.duration`, `units.length`, …).
- `Engine::run`, `run_plan`, `run_plan_without_defaults`, and `evaluate_plan` take `EvaluationRequest` after `record_operations` — pass `EvaluationRequest::default()` when you do not need display conversion.
- Value-copy rows use `fill` (removed `from` keyword); integrators: `rule_result_quantity_units` → `rule_result_units`.
- Ratio typedef `minimum` / `maximum` / `default` must include units (`10%`, not bare `10`).

## [0.8.13] - 2026-05-06

### Added

- Public `lemma::deps`: `lemma_deps_dir`, `relative_dependency_cache_path`, `dependency_identifier_from_dependency_path`, `dependency_cache_file` — `.deps/` layout shared by CLI fetch, workspace load, and LSP.
- `cargo lsp` (`xtask`): release-build `lemma`, then `npm ci` + `npm run compile` in `engine/lsp/editors/vscode`; `cargo lsp vsix` runs `npm run package` and prints the newest `.vsix` path.
- `@lemmabase/lemma-engine` npm bundle: `LspClient.didClose` sends `textDocument/didClose`.

### Changed

**Engine public API**
- Removed `Engine::list_specs()`. Use `Engine::get_workspace()` or `Engine::get_repository(qualifier)` instead — both return `ResolvedRepository { repository: Arc<LemmaRepository>, specs: Vec<LemmaSpecSet> }`.
- `Engine::get_workspace()` returns `ResolvedRepository` (was `Arc<LemmaRepository>`).
- `Engine::get_repository(qualifier)` returns `Result<ResolvedRepository, Error>` (was `Result<Arc<LemmaRepository>, Error>`).
- `Engine::list()` returns `Vec<ResolvedRepository>` (was `Vec<Arc<LemmaRepository>>`).
- `Engine::load(code, source_type)` replaces the old `load(HashMap<SourceType, String>)`. Single source, single call.
- `Engine::load_batch(sources, dependency)` replaces `load_files` / `load_dependency`. Accepts `HashMap<SourceType, String>`.
- `collect_lemma_sources` replaces `collect_lemma_files` (filesystem path expansion helper).
- `ResourceLimits`: `max_sources` and `max_source_size_bytes` replace `max_files` and `max_file_size_bytes`; matching resource-limit error ids updated.
- `SourceType::Path` replaces `SourceType::File` (Serde variant `"path"` instead of `"file"`).
- `Engine::sources()` removed — error formatting uses `Error`'s `Display` impl directly.
- `repo: None` in `get_plan` / `get_spec` / `remove` means workspace (not global search).
- `ExecutionPlan::with_defaults` simplified; call order in `run_plan` is now `with_defaults()` then `set_data_values()`.
- `ResolvedRepository` / `Engine::list()` documented: `LemmaRepository` and each `LemmaSpec` carry `start_line` and `source_type` from parse/load.

**WASM / NPM package**
- WASM `Engine.list()` matches `Engine::list()` JSON (`ResolvedRepository[]`: each `specs` is `LemmaSpecSet[]`; each set’s `specs` is `LemmaSpec[]` with full AST nodes, not flat catalog rows). `Engine.schema` / `Engine.run` take optional `repository` first (workspace when `null`/empty), matching the Rust engine.
- Various improvements to the LSP and syntax highlighting (removed redundant TM)

**Hex**
- `Lemma.list/1` returns specs grouped by repository via `Engine::list()`; each `repository` and each spec row includes `start_line` and `attribute` (load source label).
- Moved `temporal_api_sources` and `generate_openapi` to `Lemma.OpenAPI` module.
- Engine limits map keys: `max_sources` and `max_source_size_bytes` replace `max_files` and `max_file_size_bytes`.

**Parsing / AST**

- Removed the unused `TokenKind::DurationKw` surface and `PrimitiveKind::Duration` / `ConversionTarget::Duration` AST variants: the word `duration` is a normal identifier (e.g. a typedef may be named `duration`). The old built-in duration value/type shapes are gone; time periods are **quantity** values whose type declares `-> trait duration` (canonical **second**), carried as `Value::Quantity` / `ValueKind::Quantity` only.
- `parse_value_from_string` has no separate duration primitive; duration-shaped values are quantity literals resolved against in-scope trait-duration quantities.
- On quantity types that declare `-> trait duration`, `minimum`, `maximum`, and `default` constraints accept legacy duration-shaped literals and normalize them through the quantity unit table (or canonical base when the unit name is spelled differently).
- Removed the `-> precision` type constraint command on `quantity` and `number` types (use `-> decimals` for decimal-place limits).
- Schema `units[]` on `quantity` and `ratio` types include per-unit `minimum`, `maximum`, and `default` magnitudes (type-level bounds stay canonical).
- `DataValue::Fill` with `FillRhs` (`Literal` | `Reference`): every `fill` row uses this variant so `fill` is never encoded as `Definition`. Literal and reference right-hand sides for `fill` share no AST shape with `data …: <literal>`.
- `SpecRef` records optional `repository_span` and `target_span` (serde omits when absent) for tooling; parser fills spans on registry qualifiers and spec-reference targets.

**Formatter**

- Spec bodies indent `meta`, sorted `data`, and each rule line consistently; `data` definitions with `->` constraints wrap constraints onto indented continuation lines under the head.

**LSP**

- `documentLink` uses parsed `SpecRef` spans: registry URL on the qualifier span when available; on native, resolved target opens the dependency file (`SourceType::Path` when known, else `.deps` cache path) with `#L<start_line>`.
- Workspace tracks a host root for those paths; file discovery includes `.deps/**/*.lemma` (other dot-directories still skipped). Debounced validation loads fetched bundles under `.deps/` like the CLI when present.
- Removed regex-based `spec_links` module in favor of AST-driven links.

**CLI**

- Fetch and workspace loading call `lemma::deps::*` instead of duplicate helpers.

**VS Code extension**

- Ships `LICENSE` alongside the extension manifest.

## [0.8.12] - 2026-04-28

### Fixed

- **Hex publish**: the standalone `Cargo.toml` rewrite for the published Hex tarball now also rewrites `lemma-openapi` from a workspace path dep to the matching registry version, mirroring the existing `lemma-engine` rewrite. Without this, end users compiling from the Hex tarball (or `mix hex.publish`'s own verification compile) saw two distinct `lemma-engine` instances — one pulled from crates.io via `lemma_hex`, one from the local path via `lemma-openapi` — producing type mismatches on shared types like `Engine` and `DateTimeValue`.

### Changed

- **Schema: bound value vs default suggestion**: Stored execution plans keep `-> default ...` on `TypeDeclaration` / reference `local_default` instead of folding them into `Value` during planning. `SpecSchema` `DataEntry` now has `bound_value` (explicit spec literal or caller override) and `default` (suggestion only). `ExecutionPlan::with_defaults` materializes suggestions before evaluation; `Engine::run` and `Engine::run_plan` invoke it after `ExecutionPlan::set_data_values`. `Engine::run_plan_without_defaults` skips materialization (CLI interactive trial runs, inversion).
- **Evaluator**: Reference resolution copies only from the target path's binding; it does not read `local_default` (defaults are plan-prep only).
- **npm release workflow**: `publish-npm` now uses npm Trusted Publishing (OIDC) via `npm/publish@v1.0.1` with `id-token: write`, eliminating the long-lived `NPM_TOKEN` secret and the `EOTP` 2FA failure mode for automation.
- **npm package metadata**: `engine/packages/npm/build.js` emits `repository.url` as `git+https://github.com/lemma/lemma.git`, silencing npm's autocorrect warning on publish.

## [0.8.11] - 2026-04-28

### Added

**Data references (value-copy)**
- New `DataValue::Reference` AST variant: `data license2: l.other` or `data i.slot: src` copies the value of another data or rule result into the declared name. Dotted RHS paths always produce a reference; a non-dotted RHS in a binding LHS (e.g. `data i.slot: src`) also produces a reference. `data x: someident` without a dotted path or binding LHS remains a type annotation.
- Reference targets may be data paths or rule results. Rule-target references are resolved lazily in topological order at evaluation time.
- Local `-> ...` constraints on a reference (e.g. `data clamped: l.price -> maximum 1000 eur`) are merged with the LHS-declared type and validated against the copied value at runtime — a violation produces a Veto, not a planning error.
- `-> default N` on a reference supplies a fallback when the target has no value (missing input or rule veto). The default is also surfaced in the spec schema (`SpecSchema.data[].default`).
- Planning rejects a reference whose LHS-declared quantity family differs from the target's family (e.g. `eur` vs `celsius`) — same `quantity` discriminant is no longer sufficient.
- Runtime `LiteralValue` stored under a reference path carries the reference's `resolved_type` (LHS-merged), not the target's looser type.
- `engine/tests/data_references.rs` covers the full reference surface: value copy, chain resolution, user-value override, cycle detection, type mismatch, rule-target lazy resolution, quantity-family mismatch, local default in schema, runtime type invariant.

**Temporal ranges**
- `Engine::get_spec_set`, `LemmaSpecSet::iter_with_ranges`, `Context::iter_with_ranges`, `Engine::list_specs_with_ranges`: catalog queries returning half-open `[effective_from, effective_to)` ranges per temporal version.
- HTTP schema JSON `versions[]`: `effective_to` alongside `effective_from`. OpenAPI: `x-effective-from` / `x-effective-to` on spec path items; `versions` schema documents both bounds; legacy `/schema/*` routes omitted from generated OpenAPI.
- Hex `Lemma.list/1`: `:effective_to` per entry. WASM `WasmEngine::list`: compact `{name, effective_from, effective_to}`.
- `engine/tests/temporal_range_references.rs`: blueprint §2.1 test suite — qualified ref transitive subtree resolution, qualified-only edges do not split consumer slices, qualified ref skips coverage requirement, unqualified still requires full-range coverage, mixed qualified/unqualified slice counts, qualified type-import instant isolation.

**Literal layer**
- `QuantityUnits` / `RatioUnits` structs replacing unstructured vecs; `QuantityUnit` / `RatioUnit` carry name + factor.
- Stricter `NumberWithUnit` and `RatioLiteral` parsing: unit must be present for quantity and ratio literals.

**CLI and tooling**
- Interactive mode improvements.
- Veto type enum for classification in responses.

**Documentation**
- `documentation/blueprint.md`: normative semantics document covering goals, temporal composition, planning architecture, feature catalog.
- `documentation/reference.md`: new "Data References" section; corrected text / duration type command tables; duration gains `minimum` / `maximum`.

### Changed

**Terminology**
- `fact` / `type` keywords unified into `data` everywhere: integration examples (`01_simple_data.lemma`), engine tests (`data_bindings`), fuzz targets (`fuzz_data_bindings`), all docs and examples.

**Planning subsystem**
- Major refactor: `graph.rs`, `execution_plan.rs`, `semantics.rs` — consolidated from standalone `fingerprint`, `temporal`, `types`, `validation`, `slice_interface` modules into core planning files.
- New `PageSetId` module for parsing and identifying spec-set identifiers.
- New `discovery` module: `resolve_spec_ref`, `dependency_edges`, `validate_dependency_interfaces`, `build_dag_for_spec` for topological sort and cycle detection.
- `LemmaSpecSet`: `effective_range`, `temporal_boundaries`, `effective_dates`, `coverage_gaps` for temporal slice computation.
- `SpecSchema.data[].default` now uses `DataDefinition::schema_default()`, which surfaces `-> default N` from both `TypeDeclaration` and `Reference` entries. Previously references silently dropped their declared default.
- `CommandArg` enum collapsed to `Literal(Value)` — command arguments are directly typed literals rather than raw strings.

**Types**
- `TypePageification::Text` drops `minimum` / `maximum` length-range constraints; only `length` (exact match) remains. Specs using `text -> minimum N` or `text -> maximum N` are rejected at planning.
- `TypePageification::Duration` gains `minimum` / `maximum`.
- Reference kind compatibility check replaced discriminant-only comparison with `has_same_base_type` + `same_quantity_family` — quantity types in different families are now correctly rejected.

**Inversion subsystem**
- Refactored into separate modules: constraints, domain, solve, world, target.

**Other**
- Parser, lexer, AST, evaluation, formatting improvements.
- LSP: workspace, spec links, server improvements.
- OpenAPI crate rewrite.
- Hex NIF native API and tests.
- npm package renamed `@lemmabase/lemma-engine`; repository moved to `github.com/lemma/lemma`.

### Removed

- `engine/tests/wasm_build.rs`.
- Tracked scratch files `plan.txt`, `deleted_tests.txt`.
- Superseded engine integration tests: `bdd`, cross-spec interface contract, end-to-end, older inversion suites, `type_propagation`, `missing_fact_propagation` (replaced by focused missing-data tests).
- `cli/tests/integrations/interactive.rs` (superseded by interactive mode tests).
- `documentation/plans/temporal_ranges_blueprint_alignment.md` and `temporal_ranges_tests.md` (implementation complete; absorbed into `blueprint.md §2.1` and `engine/tests/temporal_range_references.rs`).
- `documentation/plans/tables.md` (obsolete syntax; tables not yet implemented; direction noted in `blueprint.md §3.14`).
- `TypePageification::Text` `minimum` / `maximum` length commands (breaking change; use `length` for exact length).

## [0.8.10] - 2026-03-31

### Added

- Nix flake dev shell (Rust from `rust-toolchain.toml`, cargo-nextest, cargo-deny, wasm-pack, Node 24, Elixir, nixpkgs-fmt formatter) plus `flake.lock`.
- `rust-toolchain.toml`: `wasm32-unknown-unknown` target.
- Test cases for temporal type imports.
- `ExecutionPlan.sources`: keyed `PageSources` map (`IndexMap<(name, effective_from), source>`) with AST-reconstructed canonical source for every spec in the plan. Custom serde serializes as `[{name, effective_from, source}]` for downstream consumers.

### Changed

- CLI: workspace or `.lemma` file is a positional argument (`run`/`schema`/`fetch [source] [spec]…`, `list`/`server`/`mcp [source]`); `-d`/`--dir` removed. Page auto-selected when the source defines exactly one spec; multiple specs without a name yield an error listing names (or use `-i`). Lemma source from filesystem only; positional `-` is rejected (not a valid path).
- Planning: `DataDefinition::SpecRef.resolved_plan_hash` is a required `String`; fingerprints always build `PageId` from it (no optional fallback to bare spec name).
- Graph / types: missing plan hash on type-import or spec-reference binding yields validation errors instead of `unreachable!` when a dependency spec failed validation or is absent from the hash registry.
- `build_graph` test helper pre-plans dependency specs so `PlanHashRegistry` matches topological `plan()` behavior.
- `.gitignore`: `result` / `result-*` (Nix build outputs).
- Fixes for temporal type imports, to properly pin and resolve them.
- Fix for docker image building in CI.
- Formatter cleanup: deterministic output improvements.
- Deterministic fingerprinting on semantics.
- Type resolver: rename contributing-spec registration to `register_dependency_specs` to clarify scope.

### Removed

- `==` / `!=` syntax (use `=` and `!=` was already removed).
- Raw source text from operation records and expression evaluation (replaced by plan-level `sources`).

## [0.8.9] - 2026-03-30

### Changed

- Precompiled Hex NIFs: drop `x86_64-unknown-linux-musl` from the release build-nif-binaries matrix and RustlerPrecompiled `targets` (that triple cannot build this `cdylib`); Linux x86_64 uses `x86_64-unknown-linux-gnu` only.
- Hex README: precompiled Linux wording matches (gnu x86_64 + arm64).
- Workspace / crates / VS Code extension / lockfiles bumped to 0.8.9 (routine version alignment).
- Linux `lemma` CLI release assets are musl static only (`lemma-*-linux-musl.tar.gz`); publish-docker copies them into `FROM scratch`. GNU Linux CLI tarballs removed. Hex NIF prebuilds stay linux-gnu (`cdylib`).

## [0.8.8] - 2026-03-29

### Added

- Release workflow build-nif-binaries job: cross-build `lemma_hex` for macOS (arm64/x86_64), Linux (gnu arm64/x86_64, musl x86_64), Windows x86_64; package `.so`/`.dll` as versioned tarballs and upload to the `cli-v*` GitHub release.
- Hex package uses rustler_precompiled: consumers download matching NIFs from release assets; contributors can still compile from source with `LEMMA_BUILD_NIF=1` and Rust on `PATH`.
- publish-hex runs `mix rustler_precompiled.download Lemma.Native --all --print` (with `GITHUB_TOKEN`) so checksum files are generated before publish; job depends on build-nif-binaries completing.

### Changed

- Hex `mix.exs` OTP application `:lemma` → `:lemma_engine`; package `files` list includes checksum scripts and trimmed native sources for precompiled workflow.
- Hex README: documents precompiled targets and dev workflow (`LEMMA_BUILD_NIF=1` for `mix compile` / `mix precommit`).
- Workspace / crates / VS Code extension / lockfiles bumped to 0.8.8 (routine version alignment).

### Removed

- `engine/packages/hex/.mise.toml` (Erlang/Elixir pin no longer shipped in the package tree).

## [0.8.7] - 2026-03-28

### Added

- `PageId` type (`name` + `plan_hash`) with `Display` impl (`name~hash`); replaces ad-hoc `Arc<ExecutionPlan>` set and `format!` string concatenation in fingerprints.
- Execution plans now carry `dependencies: IndexSet<PageId>` populated from dependency rules in topological order.
- Six dependency-tracking unit tests: basic cross-spec, standalone, multiple deps, hash correctness, unused spec ref, and implicit dep via rules.

### Changed

- Cross-spec interface validation improvements and stricter test assertions.
- Fingerprint `spec_id` fields use `PageId::to_string()` instead of raw `format!("{}~{}", ...)`.

### Removed

- `serde(alias = "expected_hash_pin")` backwards-compat shim and its test.

## [0.8.6] - 2025-03-27

### Changed

- Hex publishes the Elixir package as `lemma_engine` instead of `lemma`. Replace `{:lemma, ...}` with `{:lemma_engine, ...}` in `mix.exs`, README, and the GitHub release workflow Elixir snippet; `mix.exs` sets `package` `name: "lemma_engine"`.
- Workspace and artifacts are bumped to 0.8.6 (root `Cargo.toml` / lockfile, `lemma-cli`, `lemma-engine`, `lemma-openapi`, `lsp`, VS Code `package.json` / lockfile, Hex `@version`).
- Root README rewrites the “Why Lemma?” and “What about AI?” sections: clearer story on rules vs systems, single source of truth, determinism and auditability, and how Lemma differs from approximate AI for compliance-style logic.

## [0.8.5] - 2025-03-27

### Added

- Cargo aliases `cargo bump`, `cargo verify`, and `cargo changelog` wired to xtask: centralized versions-bump (workspace semver + mirrored pins in CLI/OpenAPI/LSP manifests, Hex `mix.exs`, `engine/README.md`, VS Code `package.json`), versions-verify, and versions-diff (tag-to-tree or tag-range changelog helper).
- versions-verify step in the quality workflow lint job so CI matches local precommit.
- `xtask/README.md` and a maintainer Release version section in `documentation/contributing.md`; `README.md` documents running versions-verify in precommit and using bump/verify when changing the release.

### Changed

- Workspace release 0.8.5 across crates, `Cargo.lock`, exact path-dep pins, Hex `@version`, engine README quick-start line, and VS Code extension version (aligned with the workspace release; release workflow no longer rewrites extension version in a separate Node step).
- `cargo precommit` runs versions-verify before fmt, Clippy, nextest, and cargo-deny. Also triggers SDK precommits (npm precommit, mix precommit).
- Release workflow: Intel macOS build uses macos-15-intel instead of macos-13.
- Hex `mix.exs`: ex_doc added as a dev-only dependency; dependency ordering/lockfile updated.

### Removed

- Jekyll/GitHub Pages scaffolding: `documentation/Gemfile` and `documentation/_config.yml`.
