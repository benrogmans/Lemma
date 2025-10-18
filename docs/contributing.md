# Contributing to Lemma

## Setup

```bash
git clone https://github.com/benrogmans/lemma
cd lemma
cargo test --workspace
```

### Optional Development Tools

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
   cargo test --workspace
   cargo clippy --workspace -- -D warnings
   cargo fmt --all
   ```

## Pull Requests

**Automated checks that must pass:**
- Tests (stable + beta Rust)
- Clippy linting
- Formatting (rustfmt)
- Security audit (cargo-deny)
- Quick fuzz tests (30s)

**Quality analysis** (doesn't block merges):
- DeepSource: coverage + code quality + AI suggestions
- Property-based tests: 20 tests with 100 cases each

## Project Structure

- `cli/` - CLI application
- `lemma/` - Core parser and evaluator
- `lemma/fuzz/` - Fuzz testing targets
- `docs/examples/` - Example `.lemma` files

## Testing

### Unit and Integration Tests
```bash
cargo test --workspace
```

### Fuzz Testing
Requires nightly Rust. Uses cargo-fuzz to test parser robustness.

```bash
cd lemma/fuzz
cargo +nightly fuzz list                    # List available fuzz targets
cargo +nightly fuzz run fuzz_parser -- -max_total_time=60  # Run for 60 seconds
```

### WASM Build
```bash
wasm-pack build lemma --target web --out-dir target/wasm
wasm-pack build lemma --target nodejs --out-dir target/wasm-node
```

## Release (maintainers only)

To release:
1. Update version in `lemma/Cargo.toml` and/or `cli/Cargo.toml`
2. Open PR and merge to main
3. CI automatically detects version changes and publishes to crates.io

Releases are independent:
- `lemma` → tagged as `lemma-v0.2.1`
- `lemma` CLI → tagged as `v0.2.1` with GitHub release

## License

Apache 2.0

