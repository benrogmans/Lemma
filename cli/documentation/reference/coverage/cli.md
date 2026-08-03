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
| Lines | 1450 | 3451 | 42.02% |
| Functions | 143 | 333 | 42.94% |
| Regions | 2201 | 5221 | 42.16% |

## Test run

- Total: 138
- Passed: 138
- Skipped: 0
- Failed: 0

## Per-module coverage

Sorted by line coverage ascending (weakest first). Only files under `src/` for this crate are listed.

| Module | Line % | Function % | Region % | Lines covered/total |
|--------|-------:|-----------:|---------:|--------------------:|
| `server.rs` | 0.00 | 0.00 | 0.00 | 0/650 |
| `interactive.rs` | 1.08 | 3.57 | 0.59 | 7/651 |
| `main.rs` | 37.38 | 47.95 | 35.44 | 314/840 |
| `error_formatter.rs` | 61.76 | 100.00 | 62.26 | 42/68 |
| `formatter.rs` | 80.56 | 83.33 | 81.70 | 116/144 |
| `mcp/server.rs` | 87.22 | 73.91 | 82.85 | 819/939 |
| `data_json.rs` | 94.26 | 94.74 | 94.39 | 115/122 |
| `mcp/guide.rs` | 100.00 | 100.00 | 100.00 | 37/37 |

## Related docs

- [CLI integration test catalog](../../../cli/tests/README.md)
- [Engine test coverage](engine.md)
- [CLI benchmarks](../benchmarks/cli.md)
<!-- coverage-input-digest: 1ee823a78bf3ef48 -->
