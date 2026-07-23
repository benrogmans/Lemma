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
| Lines | 32480 | 38371 | 84.65% |
| Functions | 2467 | 2845 | 86.71% |
| Regions | 48491 | 58125 | 83.43% |

## Test run

- Total: 2195
- Passed: 2195
- Skipped: 2
- Failed: 0

## Per-module coverage

Sorted by line coverage ascending (weakest first). Only files under `src/` for this crate are listed.

| Module | Line % | Function % | Region % | Lines covered/total |
|--------|-------:|-----------:|---------:|--------------------:|
| `evaluation/operations.rs` | 42.27 | 42.86 | 40.94 | 41/97 |
| `computation/decimal_math.rs` | 48.15 | 100.00 | 40.18 | 26/54 |
| `computation/comparison.rs` | 64.53 | 77.78 | 66.01 | 111/172 |
| `error.rs` | 70.04 | 65.96 | 65.03 | 360/514 |
| `evaluation/explanations.rs` | 73.98 | 76.92 | 72.20 | 145/196 |
| `computation/measure_math.rs` | 74.24 | 100.00 | 81.25 | 49/66 |
| `computation/arithmetic.rs` | 74.58 | 87.50 | 74.54 | 710/952 |
| `computation/bigint/signed.rs` | 77.51 | 78.38 | 72.61 | 193/249 |
| `planning/explanation.rs` | 78.57 | 75.00 | 77.27 | 22/28 |
| `parsing/ast.rs` | 79.43 | 85.71 | 71.27 | 981/1235 |
| `computation/units.rs` | 80.82 | 100.00 | 79.36 | 177/219 |
| `literals.rs` | 81.51 | 74.77 | 80.25 | 670/822 |
| `planning/graph.rs` | 81.61 | 86.11 | 82.57 | 7274/8913 |
| `planning/semantics.rs` | 82.03 | 82.29 | 81.42 | 3648/4447 |
| `evaluation/response.rs` | 82.71 | 73.17 | 77.44 | 646/781 |
| `planning/execution_plan.rs` | 82.77 | 86.96 | 79.32 | 2238/2704 |
| `evaluation/expression.rs` | 82.81 | 100.00 | 86.52 | 53/64 |
| `evaluation/data_input.rs` | 83.44 | 82.35 | 88.54 | 257/308 |
| `computation/datetime.rs` | 84.32 | 78.65 | 83.01 | 941/1116 |
| `parsing/parser.rs` | 84.77 | 87.94 | 80.65 | 1792/2114 |
| `computation/range.rs` | 85.25 | 100.00 | 80.19 | 156/183 |
| `spec_set_id.rs` | 87.72 | 100.00 | 95.52 | 50/57 |
| `parsing/lexer.rs` | 88.07 | 100.00 | 83.09 | 635/721 |
| `planning/normalize.rs` | 88.25 | 89.67 | 87.36 | 2734/3098 |
| `computation/bigint/biguint.rs` | 89.05 | 93.18 | 87.39 | 431/484 |
| `computation/rational.rs` | 90.25 | 98.36 | 82.99 | 500/554 |
| `engine.rs` | 90.73 | 89.38 | 91.67 | 1410/1554 |
| `formatting/mod.rs` | 90.82 | 98.53 | 90.53 | 722/795 |
| `planning/spec_set.rs` | 90.96 | 91.67 | 91.10 | 151/166 |
| `evaluation/conversion_trace.rs` | 91.25 | 100.00 | 91.75 | 146/160 |
| `evaluation/mod.rs` | 92.74 | 84.00 | 94.42 | 294/317 |
| `parsing/mod.rs` | 92.84 | 98.86 | 90.43 | 1257/1354 |
| `computation/bigint/alloc.rs` | 92.86 | 91.67 | 88.41 | 39/42 |
| `evaluation/branch_semantics.rs` | 92.86 | 100.00 | 81.82 | 39/42 |
| `evaluation/tree.rs` | 93.00 | 91.07 | 93.36 | 1049/1128 |
| `registry.rs` | 94.04 | 90.44 | 93.44 | 1073/1141 |
| `planning/discovery.rs` | 94.86 | 96.72 | 95.73 | 1052/1109 |
| `deps.rs` | 96.23 | 100.00 | 96.47 | 51/53 |
| `planning/mod.rs` | 97.54 | 94.44 | 96.95 | 198/203 |
| `limits.rs` | 100.00 | 100.00 | 100.00 | 34/34 |
| `parsing/source.rs` | 100.00 | 100.00 | 99.46 | 125/125 |

## Related docs

- [Engine integration test catalog](../../../engine/tests/README.md) — qualitative map of scenarios and subsystem overlap clusters
- [CLI test coverage](cli.md)
- [Engine benchmarks](../benchmarks/engine.md)
<!-- coverage-input-digest: 6e8c91bae5d36997 -->
