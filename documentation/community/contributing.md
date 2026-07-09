---
nav_title: Contributing
parent: Community
nav_order: 10
---

# Contributing

Open issues, propose language changes, improve docs, or build examples around real rules. Pull requests are welcome.

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

## Making changes

1. Write a test first
2. Make your changes
3. Run before submitting (from repo root):

   ```bash
   cargo precommit
   ```

   Same pipeline as CI: versions-verify, fmt, clippy, nextest (including ignored benches), WASM npm build+test, cargo-deny, `cargo coverage all --check`. Requires `cargo-nextest`, `cargo-deny`, Node.js, and `wasm-pack`. Regenerate coverage with `cargo coverage all` when engine/cli sources change (`cargo-llvm-cov` required).

### Release version (maintainers)

The workspace release is `[workspace.package] version` in the root `Cargo.toml`. The same number must appear in path-dep pins, Hex `mix.exs`, `engine/README.md`, and the VS Code extension `package.json` (see `xtask/src/versions.rs` module `tracked`).

- **`cargo bump <semver>`**: update all locations, then refresh `Cargo.lock` (`cargo generate-lockfile`), Hex `mix.lock` (`mix deps.get`), and VS Code `package-lock.json` (`npm install --package-lock-only`).
- **`cargo verify`**: confirm everything matches; CI runs this in the lint job.

Do not hand-edit those copies unless you keep them in sync.

## Pull requests

Automated checks that must pass:

- Tests (stable + beta Rust), including WASM npm `build.js` + `test.js`
- Clippy linting
- Formatting (rustfmt)
- Security audit (cargo-deny)
- Quick fuzz tests (30s)
- Coverage reports up to date (`cargo coverage all --check`)

## Project structure

- `cli/`: CLI application (HTTP server, MCP server, interactive mode, formatter)
- `engine/`: core parser, planner, and evaluator
- `engine/fuzz/`: fuzz testing targets
- `openapi/`: Lemma-to-OpenAPI generation

## Testing

### Unit and integration tests

```bash
cargo nextest run --workspace
```

Hex NIF (ExUnit):

```bash
cd engine/packages/hex && mix test
```

See [engine/tests/README.md](https://github.com/lemma/lemma/blob/main/engine/tests/README.md) (catalog + semantics audit) and [cli/tests/README.md](https://github.com/lemma/lemma/blob/main/cli/tests/README.md).

### Fuzz testing

Requires nightly Rust:

```bash
cd engine/fuzz
cargo +nightly fuzz list
cargo +nightly fuzz run fuzz_parser -- -max_total_time=60
```

### WASM build and test

`cargo precommit` runs these from `engine/packages/npm` automatically. To run manually:

```bash
cd engine/packages/npm
node build.js   # wasm-pack → lemma.bindings.js; copies entrypoints and lsp-client
node test.js
```
