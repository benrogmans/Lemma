# Changelog

Releases cover the Lemma engine, `lemma` CLI, OpenAPI crate, LSP, SDKs and VS Code extension. They all follow the same version everywhere.

The release version is `[workspace.package] version` in the root `Cargo.toml`. Git tags follow `cli-v{version}` (for example `cli-v0.8.5`).

Draft notes for the next version quickly: from the repo root run `cargo changelog` to print `git diff` / `git log` since the latest `cli-v*` tag (`xtask` `versions-diff`).

## [0.8.9] - 2026-03-30

### Changed

- Precompiled Hex NIFs: drop **`x86_64-unknown-linux-musl`** from the release **build-nif-binaries** matrix and **RustlerPrecompiled** `targets` (that triple cannot build this `cdylib`); Linux x86_64 uses **`x86_64-unknown-linux-gnu`** only.
- Hex **README**: precompiled Linux wording matches (gnu x86_64 + arm64).
- Workspace / crates / VS Code extension / lockfiles bumped to **0.8.9** (routine version alignment).
- Linux `lemma` CLI release assets are **musl static** only (`lemma-*-linux-musl.tar.gz`); **publish-docker** copies them into `FROM scratch`. GNU Linux CLI tarballs removed. Hex NIF prebuilds stay **linux-gnu** (`cdylib`).

## [0.8.8] - 2026-03-29

### Added

- Release workflow **build-nif-binaries** job: cross-build `lemma_hex` for macOS (arm64/x86_64), Linux (gnu arm64/x86_64, musl x86_64), Windows x86_64; package `.so`/`.dll` as versioned tarballs and upload to the `cli-v*` GitHub release.
- Hex package uses **rustler_precompiled**: consumers download matching NIFs from release assets; contributors can still compile from source with `LEMMA_BUILD_NIF=1` and Rust on `PATH`.
- **publish-hex** runs `mix rustler_precompiled.download Lemma.Native --all --print` (with `GITHUB_TOKEN`) so checksum files are generated before publish; job depends on **build-nif-binaries** completing.

### Changed

- Hex **mix.exs** OTP application `:lemma` → `:lemma_engine`; **package** `files` list includes checksum scripts and trimmed native sources for precompiled workflow.
- Hex **README**: documents precompiled targets and dev workflow (`LEMMA_BUILD_NIF=1` for `mix compile` / `mix precommit`).
- Workspace / crates / VS Code extension / lockfiles bumped to **0.8.8** (routine version alignment).

### Removed

- **engine/packages/hex/.mise.toml** (Erlang/Elixir pin no longer shipped in the package tree).

## [0.8.7] - 2026-03-28

### Added

- **SpecId** type (`name` + `plan_hash`) with `Display` impl (`name~hash`); replaces ad-hoc `Arc<ExecutionPlan>` set and `format!` string concatenation in fingerprints.
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
- Workspace and artifacts are bumped to **0.8.6** (root `Cargo.toml` / lockfile, `lemma-cli`, `lemma-engine`, `lemma-openapi`, `lsp`, VS Code `package.json` / lockfile, Hex `@version`).
- Root **README** rewrites the “Why Lemma?” and “What about AI?” sections: clearer story on rules vs systems, single source of truth, determinism and auditability, and how Lemma differs from approximate AI for compliance-style logic.

## [0.8.5] - 2025-03-27

### Added

- Cargo aliases `cargo bump`, `cargo verify`, and `cargo changelog` wired to xtask: centralized **versions-bump** (workspace semver + mirrored pins in CLI/OpenAPI/LSP manifests, Hex `mix.exs`, `engine/README.md`, VS Code `package.json`), **versions-verify**, and **versions-diff** (tag-to-tree or tag-range changelog helper).
- **versions-verify** step in the quality workflow lint job so CI matches local precommit.
- **xtask/README.md** and a maintainer **Release version** section in **documentation/contributing.md**; **README.md** documents running **versions-verify** in precommit and using bump/verify when changing the release.

### Changed

- Workspace release **0.8.5** across crates, **Cargo.lock**, exact path-dep pins, Hex `@version`, engine README quick-start line, and VS Code extension **version** (aligned with the workspace release; release workflow no longer rewrites extension version in a separate Node step).
- **cargo precommit** runs **versions-verify** before fmt, Clippy, nextest, and cargo-deny. Also triggers SDK precommits (npm precommit, mix precommit).
- Release workflow: Intel macOS build uses **macos-15-intel** instead of **macos-13**.
- Hex **mix.exs**: **ex_doc** added as a dev-only dependency; dependency ordering/lockfile updated.

### Removed

- Jekyll/GitHub Pages scaffolding: **documentation/Gemfile** and **documentation/_config.yml**.
