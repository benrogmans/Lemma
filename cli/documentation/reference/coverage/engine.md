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
| Lines | 36621 | 43108 | 84.95% |
| Functions | 2799 | 3189 | 87.77% |
| Regions | 53521 | 63927 | 83.72% |

## Test run

- Total: 2494
- Passed: 2494
- Skipped: 0
- Failed: 0

## Per-module coverage

Sorted by line coverage ascending (weakest first). Only files under `src/` for this crate are listed.

| Module | Line % | Function % | Region % | Lines covered/total |
|--------|-------:|-----------:|---------:|--------------------:|
| `mcp/catalog.rs` | 0.00 | 0.00 | 0.00 | 0/171 |
| `mcp/error.rs` | 40.91 | 40.00 | 47.22 | 9/22 |
| `computation/operation_result.rs` | 44.00 | 45.45 | 42.48 | 44/100 |
| `computation/decimal_math.rs` | 48.15 | 100.00 | 40.18 | 26/54 |
| `computation/comparison.rs` | 63.95 | 77.78 | 65.69 | 110/172 |
| `result_value.rs` | 67.44 | 56.00 | 62.32 | 203/301 |
| `mcp/tools.rs` | 71.91 | 57.58 | 71.12 | 215/299 |
| `computation/measure_math.rs` | 74.24 | 100.00 | 81.25 | 49/66 |
| `computation/arithmetic.rs` | 74.53 | 87.50 | 74.52 | 714/958 |
| `evaluation/explanations.rs` | 74.74 | 76.92 | 73.01 | 142/190 |
| `documentation/mod.rs` | 77.22 | 76.92 | 78.63 | 61/79 |
| `evaluation/run_data.rs` | 78.54 | 78.57 | 82.67 | 322/410 |
| `planning/explanation.rs` | 78.57 | 75.00 | 77.27 | 22/28 |
| `computation/bigint/signed.rs` | 79.12 | 81.08 | 73.87 | 197/249 |
| `error.rs` | 80.32 | 79.63 | 74.93 | 453/564 |
| `computation/units.rs` | 80.82 | 100.00 | 79.36 | 177/219 |
| `planning/execution_plan.rs` | 80.96 | 82.41 | 76.92 | 1862/2300 |
| `planning/graph.rs` | 81.61 | 86.36 | 82.55 | 7440/9117 |
| `planning/semantics.rs` | 81.75 | 83.87 | 81.39 | 3534/4323 |
| `parsing/ast.rs` | 81.76 | 86.75 | 73.40 | 1098/1343 |
| `literals.rs` | 82.02 | 76.67 | 81.32 | 716/873 |
| `evaluation/expression.rs` | 83.33 | 100.00 | 87.37 | 55/66 |
| `parsing/parser.rs` | 84.49 | 88.74 | 80.63 | 1929/2283 |
| `computation/datetime.rs` | 84.59 | 79.78 | 83.30 | 944/1116 |
| `computation/range.rs` | 85.25 | 100.00 | 80.12 | 156/183 |
| `planning/ordered_dispatch.rs` | 85.38 | 96.30 | 84.52 | 292/342 |
| `spec_set_id.rs` | 87.72 | 100.00 | 95.52 | 50/57 |
| `parsing/lexer.rs` | 88.23 | 100.00 | 83.20 | 637/722 |
| `planning/normalize.rs` | 88.88 | 90.79 | 87.69 | 3420/3848 |
| `computation/bigint/biguint.rs` | 89.19 | 93.18 | 87.32 | 429/481 |
| `computation/rational.rs` | 89.21 | 96.72 | 82.04 | 488/547 |
| `planning/spec_set.rs` | 90.20 | 90.91 | 90.26 | 138/153 |
| `string_distance.rs` | 90.48 | 83.33 | 93.22 | 57/63 |
| `planning/unit_index.rs` | 90.89 | 96.15 | 92.02 | 499/549 |
| `evaluation/conversion_trace.rs` | 91.25 | 100.00 | 91.75 | 146/160 |
| `engine.rs` | 91.76 | 91.87 | 92.51 | 1469/1601 |
| `evaluation/tree.rs` | 91.87 | 93.44 | 92.41 | 1175/1279 |
| `formatting/mod.rs` | 92.34 | 100.00 | 91.32 | 904/979 |
| `parsing/mod.rs` | 92.64 | 98.92 | 89.89 | 1321/1426 |
| `evaluation/branch_semantics.rs` | 92.86 | 100.00 | 81.82 | 39/42 |
| `planning/discovery.rs` | 93.06 | 97.06 | 93.93 | 1220/1311 |
| `quality.rs` | 93.12 | 96.77 | 89.24 | 907/974 |
| `limits.rs` | 93.27 | 100.00 | 85.40 | 97/104 |
| `planning/mod.rs` | 93.94 | 97.56 | 95.02 | 496/528 |
| `registry.rs` | 94.08 | 90.71 | 93.50 | 1128/1199 |
| `evaluation/mod.rs` | 95.90 | 100.00 | 96.80 | 234/244 |
| `deps.rs` | 96.23 | 100.00 | 96.47 | 51/53 |
| `parsing/assignment_continuation_tests.rs` | 97.39 | 100.00 | 93.26 | 261/268 |
| `evaluation/response.rs` | 98.71 | 93.33 | 98.13 | 536/543 |
| `computation/bigint/alloc.rs` | 100.00 | 100.00 | 97.37 | 20/20 |
| `lib.rs` | 100.00 | 100.00 | 100.00 | 4/4 |
| `parsing/source.rs` | 100.00 | 100.00 | 99.46 | 125/125 |

## Related docs

- [Engine integration test catalog](../../../engine/tests/README.md) — qualitative map of scenarios and subsystem overlap clusters
- [CLI test coverage](cli.md)
- [Engine benchmarks](../benchmarks/engine.md)
<!-- coverage-input-digest: b7afec8dec6764b5 -->
