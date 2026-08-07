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
| Lines | 34767 | 40795 | 85.22% |
| Functions | 2643 | 3001 | 88.07% |
| Regions | 51272 | 61107 | 83.91% |

## Test run

- Total: 2375
- Passed: 2375
- Skipped: 2
- Failed: 0

## Per-module coverage

Sorted by line coverage ascending (weakest first). Only files under `src/` for this crate are listed.

| Module | Line % | Function % | Region % | Lines covered/total |
|--------|-------:|-----------:|---------:|--------------------:|
| `computation/operation_result.rs` | 42.27 | 42.86 | 40.94 | 41/97 |
| `computation/decimal_math.rs` | 48.15 | 100.00 | 40.18 | 26/54 |
| `computation/comparison.rs` | 63.95 | 77.78 | 65.69 | 110/172 |
| `result_value.rs` | 67.44 | 56.00 | 62.32 | 203/301 |
| `computation/measure_math.rs` | 74.24 | 100.00 | 81.25 | 49/66 |
| `evaluation/explanations.rs` | 74.74 | 76.92 | 73.01 | 142/190 |
| `computation/arithmetic.rs` | 74.95 | 87.50 | 74.95 | 718/958 |
| `planning/explanation.rs` | 78.57 | 75.00 | 77.27 | 22/28 |
| `computation/bigint/signed.rs` | 79.12 | 81.08 | 73.87 | 197/249 |
| `parsing/ast.rs` | 79.51 | 85.71 | 71.01 | 982/1235 |
| `error.rs` | 80.32 | 79.63 | 74.93 | 453/564 |
| `computation/units.rs` | 80.82 | 100.00 | 79.36 | 177/219 |
| `planning/execution_plan.rs` | 80.96 | 82.41 | 76.91 | 1862/2300 |
| `planning/graph.rs` | 81.57 | 86.40 | 82.53 | 7258/8898 |
| `planning/semantics.rs` | 81.71 | 83.76 | 81.39 | 3516/4303 |
| `literals.rs` | 82.11 | 76.47 | 81.08 | 707/861 |
| `evaluation/expression.rs` | 83.33 | 100.00 | 87.37 | 55/66 |
| `computation/datetime.rs` | 84.41 | 79.78 | 83.19 | 942/1116 |
| `parsing/parser.rs` | 84.45 | 88.11 | 80.60 | 1830/2167 |
| `computation/range.rs` | 85.25 | 100.00 | 80.12 | 156/183 |
| `planning/ordered_dispatch.rs` | 85.38 | 96.30 | 84.52 | 292/342 |
| `evaluation/run_data.rs` | 86.99 | 87.50 | 92.26 | 254/292 |
| `spec_set_id.rs` | 87.72 | 100.00 | 95.52 | 50/57 |
| `parsing/lexer.rs` | 88.07 | 100.00 | 83.09 | 635/721 |
| `planning/normalize.rs` | 88.80 | 90.79 | 87.52 | 3417/3848 |
| `computation/bigint/biguint.rs` | 89.19 | 93.18 | 87.32 | 429/481 |
| `computation/rational.rs` | 89.21 | 96.72 | 82.04 | 488/547 |
| `planning/spec_set.rs` | 90.20 | 90.91 | 90.26 | 138/153 |
| `formatting/mod.rs` | 90.36 | 98.51 | 89.22 | 731/809 |
| `evaluation/conversion_trace.rs` | 91.25 | 100.00 | 91.75 | 146/160 |
| `evaluation/tree.rs` | 91.32 | 92.86 | 92.19 | 1126/1233 |
| `engine.rs` | 91.72 | 91.45 | 92.44 | 1429/1558 |
| `parsing/mod.rs` | 92.84 | 98.86 | 90.43 | 1257/1354 |
| `evaluation/branch_semantics.rs` | 92.86 | 100.00 | 81.82 | 39/42 |
| `planning/discovery.rs` | 93.13 | 97.06 | 93.96 | 1220/1310 |
| `planning/unit_index.rs` | 93.19 | 97.78 | 94.39 | 424/455 |
| `limits.rs` | 93.27 | 100.00 | 85.40 | 97/104 |
| `planning/mod.rs` | 93.94 | 97.56 | 95.02 | 496/528 |
| `registry.rs` | 94.08 | 90.71 | 93.50 | 1128/1199 |
| `evaluation/mod.rs` | 94.76 | 95.45 | 95.83 | 271/286 |
| `quality.rs` | 95.09 | 96.61 | 92.15 | 562/591 |
| `deps.rs` | 96.23 | 100.00 | 96.47 | 51/53 |
| `evaluation/response.rs` | 99.19 | 91.67 | 98.62 | 492/496 |
| `computation/bigint/alloc.rs` | 100.00 | 100.00 | 97.37 | 20/20 |
| `lib.rs` | 100.00 | 100.00 | 100.00 | 4/4 |
| `parsing/source.rs` | 100.00 | 100.00 | 99.46 | 125/125 |

## Related docs

- [Engine integration test catalog](../../../engine/tests/README.md) — qualitative map of scenarios and subsystem overlap clusters
- [CLI test coverage](cli.md)
- [Engine benchmarks](../benchmarks/engine.md)
<!-- coverage-input-digest: 9e365107c0d7cea7 -->
