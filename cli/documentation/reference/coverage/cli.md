---
nav_title: CLI test coverage
parent: Reference
nav_order: 56
---

# CLI test coverage

Numbers are produced by `cargo coverage cli`.

## Methodology

- Tool: [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) driving [`cargo-nextest`](https://nexte.st/) on native targets.
- Scope: `lemma` CLI crate library unit tests, integration tests (`cli/tests/**`), and the `lemma` binary entrypoint via `cargo llvm-cov nextest -p lemma --lib --tests --bin lemma`.
- Line, function, and region percentages come from LLVM source-based coverage.
- Each run starts with `cargo llvm-cov clean` on the target crate so repeated measurements stay deterministic.
- Tests run single-threaded (`NEXTEST_TEST_THREADS=1`) so coverage counters stay stable across runs.

### Out of scope

- `lemma-engine` source (see [Engine test coverage](engine.md); CLI integration tests exercise engine code but engine line coverage is authoritative there)
- WASM npm package, Hex NIF, LSP, and OpenAPI crates

## Environment

- Host: `Linux 7.0.0-30-generic x86_64`
- Rustc:

```
rustc 1.92.0 (ded5c06cf 2025-12-08)
binary: rustc
commit-hash: ded5c06cf21d2b93bffd5d884aa6e96934ee4234
commit-date: 2025-12-08
host: x86_64-unknown-linux-gnu
release: 1.92.0
LLVM version: 21.1.3
```

## Summary

| Metric | Covered | Total | Percent |
|--------|--------:|------:|--------:|
| Lines | 2737 | 4801 | 57.01% |
| Functions | 234 | 426 | 54.93% |
| Regions | 4223 | 7246 | 58.28% |

## Test run

- Total: 211
- Passed: 211
- Skipped: 0
- Failed: 0

## Per-module coverage

Sorted by line coverage ascending (weakest first). Only files under `src/` for this crate are listed.

| Module | Line % | Function % | Region % | Lines covered/total |
|--------|-------:|-----------:|---------:|--------------------:|
| `server.rs` | 0.00 | 0.00 | 0.00 | 0/571 |
| `interactive.rs` | 25.32 | 16.13 | 23.99 | 218/861 |
| `main.rs` | 44.39 | 56.67 | 42.65 | 293/660 |
| `error_formatter.rs` | 61.76 | 100.00 | 62.26 | 42/68 |
| `workspace.rs` | 72.29 | 65.12 | 75.18 | 373/516 |
| `mcp/server.rs` | 84.05 | 80.49 | 82.27 | 1191/1417 |
| `formatter.rs` | 85.62 | 84.62 | 87.87 | 125/146 |
| `install.rs` | 86.36 | 83.78 | 83.91 | 380/440 |
| `data_json.rs` | 94.26 | 94.74 | 94.39 | 115/122 |

## Related docs

- [CLI integration test catalog](../../../cli/tests/README.md)
- [Engine test coverage](engine.md)
- [CLI benchmarks](../benchmarks/cli.md)
<!-- coverage-input-digest: bac2c50909e72a2b -->
