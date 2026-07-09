# xtask

Workspace automation from the repo root.

| Command | Alias | Purpose |
|---------|-------|---------|
| `cargo run -p xtask` | `cargo precommit` | `versions-verify`, mix precommit (hex), vscode npm precommit, `fmt --check`, clippy, nextest, npm WASM build+test, cargo-deny, `coverage all --check` |
| `cargo run -p xtask -- versions-verify` | `cargo verify` | Ensure release version matches everywhere (see below) |
| `cargo run -p xtask -- versions-bump <semver>` | `cargo bump <semver>` | Bump `[workspace.package] version` and all mirrored copies, then `cargo generate-lockfile`, `mix deps.get` (hex), `npm install --package-lock-only` (vscode) |
| `cargo run -p xtask -- versions-diff [semver]` | `cargo changelog [semver]` | `git fetch --tags`, then `git diff --stat`, `git log`, then `git diff`. **No arg:** latest release tag (`lemma-v*`, or legacy `cli-v*`) → **working tree** (includes uncommitted changes); log is `tag..HEAD`. **`versions-diff <semver>`:** previous tag → requested version's tag on history only. |
| `cargo benchmarks <engine\|cli\|all>` | `cargo run -p xtask -- benchmarks <suite>` | Run benchmark suites and write `documentation/reference/benchmarks/engine.md` and/or `documentation/reference/benchmarks/cli.md`. `engine`: Criterion evaluate/outputs + Python harness. `cli`: `http_evaluate` + `engine_profile`. Keep engine `FIXTURES` in `xtask/src/benchmarks/engine.rs` synced with `engine/benches/common/mod.rs`; CLI cases in `xtask/src/benchmarks/cli.rs` synced with `cli/benches/*.rs`. |
| `cargo coverage <engine\|cli\|all> [--check]` | `cargo run -p xtask -- coverage <suite> [--check]` | Without `--check`: run `cargo llvm-cov nextest` and write `documentation/reference/coverage/engine.md` and/or `documentation/reference/coverage/cli.md` (requires [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) and `cargo-nextest`). With `--check`: verify committed reports match source inputs (no llvm-cov). `cargo precommit` runs `coverage all --check`. |

Aliases are in [`.cargo/config.toml`](../.cargo/config.toml) (`-q` on bump/verify/changelog reduces Cargo noise).

**Precommit prerequisites:** [`cargo-nextest`](https://nexte.st/), [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny), Elixir/Mix, Node.js/npm, `wasm-pack` (CI uses 0.14.0: `cargo install wasm-pack --version 0.14.0 --locked`). [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) is only needed to regenerate coverage reports (`cargo coverage all`), not for precommit. Mix and VS Code npm precommits always run (not path-gated).

Release version must match in:

- `Cargo.toml` (`[workspace.package]`)
- Path dependency pins in `cli/`, `openapi/`, `engine/lsp/` `Cargo.toml` files (`lemma` / `lemma-openapi`, `=…` exact pins)
- `engine/packages/hex/mix.exs` (`@version`)
- `engine/README.md` (quick-start `lemma-engine = "…"`)
- `engine/lsp/editors/vscode/package.json` (`version`)

Single source of truth for those paths: [`src/versions.rs`](src/versions.rs) module `tracked`.
