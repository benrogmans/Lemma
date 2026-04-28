---
layout: default
title: Contributing
---

# Contributing to Lemma

## Setup

```bash
git clone https://github.com/lemma/lemma
cd lemma
cargo nextest run --workspace
```

### Optional tools

For WASM development:
```bash
cargo install wasm-pack
```

For fuzzing (requires nightly Rust):
```bash
rustup install nightly
cargo install cargo-fuzz
```

For security audits:
```bash
cargo install cargo-deny
cargo deny check --config .cargo/deny.toml
```

## Making Changes

1. Write a test first
2. Make your changes
3. Run before submitting:
   ```bash
   cargo nextest run --workspace
   cargo clippy --all-targets --all-features -- -D warnings
   cargo fmt --all
   ```

### Release version (maintainers)

The workspace release is `[workspace.package] version` in the root `Cargo.toml`. The same number must appear in path-dep pins, Hex `mix.exs`, `engine/README.md`, and the VS Code extension `package.json` (see `xtask/src/versions.rs` module `tracked`).

- **`cargo bump <semver>`** — update all locations, then refresh `Cargo.lock` (`cargo generate-lockfile`), Hex `mix.lock` (`mix deps.get`), and VS Code `package-lock.json` (`npm install --package-lock-only`).
- **`cargo verify`** — confirm everything matches; CI runs this in the lint job.

Do not hand-edit those copies unless you keep them in sync.

## Pull Requests

Automated checks that must pass:
- Tests (stable + beta Rust)
- Clippy linting
- Formatting (rustfmt)
- Security audit (cargo-deny)
- Quick fuzz tests (30s)
- Coverage threshold (50%+)

## Project Structure

- `cli/` -- CLI application (HTTP server, MCP server, interactive mode, formatter)
- `engine/` -- core parser, planner, and evaluator
- `engine/fuzz/` -- fuzz testing targets
- `openapi/` -- Lemma-to-OpenAPI generation
- `documentation/examples/` -- example `.lemma` files

## Testing

### Unit and integration tests
```bash
cargo nextest run --workspace
```

### Fuzz testing
Requires nightly Rust:

```bash
cd engine/fuzz
cargo +nightly fuzz list
cargo +nightly fuzz run fuzz_parser -- -max_total_time=60
```

### WASM build and test (from `engine/packages/npm`)
```bash
node build.js   # wasm-pack → lemma.bindings.js; copies entrypoints and lsp-client
node test.js
```

## Release (maintainers only)

1. Update version in `engine/Cargo.toml` and/or `cli/Cargo.toml`
2. Open PR and merge to main
3. CI automatically detects version changes and publishes to crates.io

Releases are independent:
- `lemma-engine` tagged as `lemma-v{version}`
- `lemma-cli` tagged as `v{version}` with GitHub release

## License

Apache 2.0
