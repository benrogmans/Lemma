# Changelog

Releases cover the Lemma engine, `lemma` CLI, OpenAPI crate, LSP, SDKs and VS Code extension. They all follow the same version everywhere. The release version is `[workspace.package] version` in the root `Cargo.toml`. Git tags follow `cli-v{version}` (for example `cli-v0.8.5`). Draft notes for the next version quickly by running `cargo changelog` to print `git diff` / `git log` since the latest `cli-v*` tag (`xtask` `versions-diff`). Tip: feed that into an LLM to create a summary for this changelog.

## [0.8.11] - 2026-04-28

### Added

**Data references (value-copy)**
- New `DataValue::Reference` AST variant: `data license2: l.other` or `data i.slot: src` copies the value of another data or rule result into the declared name. Dotted RHS paths always produce a reference; a non-dotted RHS in a binding LHS (e.g. `data i.slot: src`) also produces a reference. `data x: someident` without a dotted path or binding LHS remains a type annotation.
- Reference targets may be data paths or rule results. Rule-target references are resolved lazily in topological order at evaluation time.
- Local `-> ...` constraints on a reference (e.g. `data clamped: l.price -> maximum 1000 eur`) are merged with the LHS-declared type and validated against the copied value at runtime — a violation produces a Veto, not a planning error.
- `-> default N` on a reference supplies a fallback when the target has no value (missing input or rule veto). The default is also surfaced in the spec schema (`SpecSchema.data[].default`).
- Planning rejects a reference whose LHS-declared scale family differs from the target's family (e.g. `eur` vs `celsius`) — same `scale` discriminant is no longer sufficient.
- Runtime `LiteralValue` stored under a reference path carries the reference's `resolved_type` (LHS-merged), not the target's looser type.
- `engine/tests/data_references.rs` covers the full reference surface: value copy, chain resolution, user-value override, cycle detection, type mismatch, rule-target lazy resolution, scale-family mismatch, local default in schema, runtime type invariant.

**Temporal ranges**
- `Engine::get_spec_set`, `LemmaSpecSet::iter_with_ranges`, `Context::iter_with_ranges`, `Engine::list_specs_with_ranges`: catalog queries returning half-open `[effective_from, effective_to)` ranges per temporal version.
- HTTP schema JSON `versions[]`: `effective_to` alongside `effective_from`. OpenAPI: `x-effective-from` / `x-effective-to` on spec path items; `versions` schema documents both bounds; legacy `/schema/*` routes omitted from generated OpenAPI.
- Hex `Lemma.list/1`: `:effective_to` per entry. WASM `WasmEngine::list`: compact `{name, effective_from, effective_to}`.
- `engine/tests/temporal_range_references.rs`: blueprint §2.1 test suite — qualified ref transitive subtree resolution, qualified-only edges do not split consumer slices, qualified ref skips coverage requirement, unqualified still requires full-range coverage, mixed qualified/unqualified slice counts, qualified type-import instant isolation.

**Literal layer**
- `ScaleUnits` / `RatioUnits` structs replacing unstructured vecs; `ScaleUnit` / `RatioUnit` carry name + factor.
- Stricter `NumberWithUnit` and `RatioLiteral` parsing: unit must be present for scale and ratio literals.

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
- New `SpecSetId` module for parsing and identifying spec-set identifiers.
- New `discovery` module: `resolve_spec_ref`, `dependency_edges`, `validate_dependency_interfaces`, `build_dag_for_spec` for topological sort and cycle detection.
- `LemmaSpecSet`: `effective_range`, `temporal_boundaries`, `effective_dates`, `coverage_gaps` for temporal slice computation.
- `SpecSchema.data[].default` now uses `DataDefinition::schema_default()`, which surfaces `-> default N` from both `TypeDeclaration` and `Reference` entries. Previously references silently dropped their declared default.
- `CommandArg` enum collapsed to `Literal(Value)` — command arguments are directly typed literals rather than raw strings.

**Types**
- `TypeSpecification::Text` drops `minimum` / `maximum` length-range constraints; only `length` (exact match) remains. Specs using `text -> minimum N` or `text -> maximum N` are rejected at planning.
- `TypeSpecification::Duration` gains `minimum` / `maximum`.
- Reference kind compatibility check replaced discriminant-only comparison with `has_same_base_type` + `same_scale_family` — scale types in different families are now correctly rejected.

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
- `TypeSpecification::Text` `minimum` / `maximum` length commands (breaking change; use `length` for exact length).

## [0.8.10] - 2026-03-31

### Added

- Nix flake dev shell (Rust from `rust-toolchain.toml`, cargo-nextest, cargo-deny, wasm-pack, Node 24, Elixir, nixpkgs-fmt formatter) plus `flake.lock`.
- `rust-toolchain.toml`: `wasm32-unknown-unknown` target.
- Test cases for temporal type imports.
- `ExecutionPlan.sources`: keyed `SpecSources` map (`IndexMap<(name, effective_from), source>`) with AST-reconstructed canonical source for every spec in the plan. Custom serde serializes as `[{name, effective_from, source}]` for downstream consumers.

### Changed

- CLI: workspace or `.lemma` file is a positional argument (`run`/`schema`/`get [source] [spec]…`, `list`/`server`/`mcp [source]`); `-d`/`--dir` removed. Spec auto-selected when the source defines exactly one spec; multiple specs without a name yield an error listing names (or use `-i`). Lemma source from filesystem only; positional `-` is rejected (not a valid path).
- Planning: `DataDefinition::SpecRef.resolved_plan_hash` is a required `String`; fingerprints always build `SpecId` from it (no optional fallback to bare spec name).
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

- `SpecId` type (`name` + `plan_hash`) with `Display` impl (`name~hash`); replaces ad-hoc `Arc<ExecutionPlan>` set and `format!` string concatenation in fingerprints.
- Execution plans now carry `dependencies: IndexSet<SpecId>` populated from dependency rules in topological order.
- Six dependency-tracking unit tests: basic cross-spec, standalone, multiple deps, hash correctness, unused spec ref, and implicit dep via rules.

### Changed

- Cross-spec interface validation improvements and stricter test assertions.
- Fingerprint `spec_id` fields use `SpecId::to_string()` instead of raw `format!("{}~{}", ...)`.

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
