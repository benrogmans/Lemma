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

- Host: `Linux 7.0.0-28-generic x86_64`
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
| Lines | 2783 | 4915 | 56.62% |
| Functions | 238 | 447 | 53.24% |
| Regions | 4290 | 7451 | 57.58% |

## Test run

- Total: 185
- Passed: 185
- Skipped: 0
- Failed: 0

## Per-module coverage

Sorted by line coverage ascending (weakest first). Only files under `src/` for this crate are listed.

| Module | Line % | Function % | Region % | Lines covered/total |
|--------|-------:|-----------:|---------:|--------------------:|
| `server.rs` | 0.00 | 0.00 | 0.00 | 0/575 |
| `interactive.rs` | 1.08 | 3.57 | 0.59 | 7/651 |
| `main.rs` | 45.08 | 57.63 | 43.02 | 293/650 |
| `error_formatter.rs` | 61.76 | 100.00 | 62.26 | 42/68 |
| `workspace.rs` | 72.67 | 65.12 | 75.53 | 375/516 |
| `formatter.rs` | 80.56 | 83.33 | 81.70 | 116/144 |
| `mcp/server.rs` | 82.75 | 72.41 | 80.39 | 1410/1704 |
| `install.rs` | 86.36 | 83.78 | 83.91 | 380/440 |
| `data_json.rs` | 94.26 | 94.74 | 94.39 | 115/122 |
| `mcp/guide.rs` | 100.00 | 100.00 | 100.00 | 45/45 |

## Related docs

- [CLI integration test catalog](../../../cli/tests/README.md)
- [Engine test coverage](engine.md)
- [CLI benchmarks](../benchmarks/cli.md)
<!-- coverage-input-digest: 0aab0a7356599e28 -->
