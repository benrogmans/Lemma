---
nav_title: Engine test coverage
parent: Reference
nav_order: 55
---

# Engine test coverage

Numbers are produced by `cargo coverage engine`.

## Methodology

- Tool: [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) driving [`cargo-nextest`](https://nexte.st/) on native (non-wasm) targets.
- Scope: `lemma-engine` library unit tests (`engine/src/**`) plus integration tests (`engine/tests/**`) via `cargo llvm-cov nextest -p lemma-engine --lib --tests`.
- Line, function, and region percentages come from LLVM source-based coverage.
- Each run starts with `cargo llvm-cov clean` on the target crate so repeated measurements stay deterministic.
- Tests run single-threaded (`NEXTEST_TEST_THREADS=1`) so coverage counters stay stable across runs.

### Out of scope

- `engine/src/wasm.rs` (built for `wasm32-unknown-unknown` only)
- Fuzz targets under `engine/fuzz/`
- Hex NIF (`lemma_hex`), LSP, OpenAPI, and CLI crates

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
| Lines | 32464 | 38272 | 84.82% |
| Functions | 2458 | 2820 | 87.16% |
| Regions | 48245 | 57699 | 83.61% |

## Test run

- Total: 2298
- Passed: 2298
- Skipped: 2
- Failed: 0

## Per-module coverage

Sorted by line coverage ascending (weakest first). Only files under `src/` for this crate are listed.

| Module | Line % | Function % | Region % | Lines covered/total |
|--------|-------:|-----------:|---------:|--------------------:|
| `computation/operation_result.rs` | 42.27 | 42.86 | 40.94 | 41/97 |
| `computation/decimal_math.rs` | 48.15 | 100.00 | 40.18 | 26/54 |
| `computation/comparison.rs` | 64.53 | 77.78 | 66.01 | 111/172 |
| `result_value.rs` | 67.44 | 56.00 | 62.32 | 203/301 |
| `computation/measure_math.rs` | 74.24 | 100.00 | 81.25 | 49/66 |
| `computation/arithmetic.rs` | 74.37 | 87.50 | 74.25 | 708/952 |
| `evaluation/explanations.rs` | 74.74 | 76.92 | 73.01 | 142/190 |
| `computation/bigint/signed.rs` | 77.51 | 78.38 | 72.36 | 193/249 |
| `planning/explanation.rs` | 78.57 | 75.00 | 77.27 | 22/28 |
| `error.rs` | 79.41 | 78.85 | 73.36 | 428/539 |
| `parsing/ast.rs` | 79.68 | 85.71 | 71.42 | 984/1235 |
| `planning/execution_plan.rs` | 80.58 | 82.41 | 76.65 | 1847/2292 |
| `computation/units.rs` | 80.82 | 100.00 | 79.36 | 177/219 |
| `planning/graph.rs` | 81.64 | 85.77 | 82.61 | 7252/8883 |
| `planning/semantics.rs` | 81.72 | 83.53 | 81.41 | 3513/4299 |
| `literals.rs` | 81.77 | 75.63 | 80.67 | 704/861 |
| `evaluation/expression.rs` | 83.33 | 100.00 | 87.37 | 55/66 |
| `computation/datetime.rs` | 84.32 | 78.65 | 83.01 | 941/1116 |
| `parsing/parser.rs` | 84.77 | 87.94 | 80.68 | 1792/2114 |
| `computation/range.rs` | 85.25 | 100.00 | 80.19 | 156/183 |
| `evaluation/run_data.rs` | 86.99 | 87.50 | 92.26 | 254/292 |
| `spec_set_id.rs` | 87.72 | 100.00 | 95.52 | 50/57 |
| `parsing/lexer.rs` | 88.07 | 100.00 | 83.09 | 635/721 |
| `planning/normalize.rs` | 88.16 | 89.67 | 87.36 | 2740/3108 |
| `computation/bigint/biguint.rs` | 89.19 | 93.18 | 87.32 | 429/481 |
| `computation/rational.rs` | 89.21 | 96.72 | 82.04 | 488/547 |
| `planning/spec_set.rs` | 90.20 | 90.91 | 90.26 | 138/153 |
| `engine.rs` | 90.58 | 90.09 | 91.31 | 1375/1518 |
| `formatting/mod.rs` | 90.82 | 98.53 | 90.53 | 722/795 |
| `evaluation/conversion_trace.rs` | 91.25 | 100.00 | 91.75 | 146/160 |
| `parsing/mod.rs` | 92.84 | 98.86 | 90.43 | 1257/1354 |
| `evaluation/branch_semantics.rs` | 92.86 | 100.00 | 81.82 | 39/42 |
| `planning/discovery.rs` | 93.13 | 97.06 | 93.96 | 1220/1310 |
| `evaluation/tree.rs` | 93.69 | 94.00 | 94.12 | 1039/1109 |
| `planning/mod.rs` | 93.94 | 97.56 | 95.02 | 496/528 |
| `registry.rs` | 94.04 | 90.44 | 93.44 | 1073/1141 |
| `evaluation/mod.rs` | 94.76 | 95.45 | 95.83 | 271/286 |
| `deps.rs` | 96.23 | 100.00 | 96.47 | 51/53 |
| `evaluation/response.rs` | 99.19 | 91.67 | 98.62 | 492/496 |
| `computation/bigint/alloc.rs` | 100.00 | 100.00 | 97.37 | 20/20 |
| `lib.rs` | 100.00 | 100.00 | 100.00 | 26/26 |
| `limits.rs` | 100.00 | 100.00 | 100.00 | 34/34 |
| `parsing/source.rs` | 100.00 | 100.00 | 99.46 | 125/125 |

## Related docs

- [Engine integration test catalog](../../../engine/tests/README.md) — qualitative map of scenarios and subsystem overlap clusters
- [CLI test coverage](cli.md)
- [Engine benchmarks](../benchmarks/engine.md)
<!-- coverage-input-digest: 47a9aaf8d5df5444 -->
