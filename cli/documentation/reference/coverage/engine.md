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
| Lines | 40231 | 47266 | 85.12% |
| Functions | 3066 | 3506 | 87.45% |
| Regions | 58981 | 70218 | 84.00% |

## Test run

- Total: 2614
- Passed: 2614
- Skipped: 0
- Failed: 0

## Per-module coverage

Sorted by line coverage ascending (weakest first). Only files under `src/` for this crate are listed.

| Module | Line % | Function % | Region % | Lines covered/total |
|--------|-------:|-----------:|---------:|--------------------:|
| `mcp/catalog.rs` | 0.00 | 0.00 | 0.00 | 0/171 |
| `lib.rs` | 7.27 | 10.00 | 7.95 | 4/55 |
| `mcp/error.rs` | 40.91 | 40.00 | 47.22 | 9/22 |
| `computation/operation_result.rs` | 44.57 | 45.00 | 43.36 | 41/92 |
| `computation/decimal_math.rs` | 48.15 | 100.00 | 40.18 | 26/54 |
| `computation/bigint/signed.rs` | 60.82 | 57.14 | 57.08 | 177/291 |
| `computation/comparison.rs` | 68.78 | 85.71 | 66.77 | 130/189 |
| `mcp/tools.rs` | 70.95 | 60.00 | 68.59 | 232/327 |
| `api/value.rs` | 75.00 | 83.33 | 71.43 | 42/56 |
| `evaluation/explanations.rs` | 75.26 | 76.92 | 73.47 | 146/194 |
| `computation/measure_math.rs` | 75.36 | 100.00 | 83.18 | 52/69 |
| `computation/arithmetic.rs` | 76.78 | 76.92 | 74.76 | 787/1025 |
| `result_value.rs` | 76.99 | 64.52 | 72.35 | 271/352 |
| `documentation/mod.rs` | 77.22 | 76.92 | 78.63 | 61/79 |
| `error.rs` | 77.27 | 78.18 | 70.81 | 442/572 |
| `snapshot.rs` | 78.12 | 76.47 | 79.28 | 150/192 |
| `evaluation/run_data.rs` | 78.42 | 74.19 | 82.19 | 338/431 |
| `planning/explanation.rs` | 78.57 | 75.00 | 77.27 | 22/28 |
| `planning/execution_plan.rs` | 80.72 | 79.84 | 77.63 | 1968/2438 |
| `evaluation/expression.rs` | 81.54 | 100.00 | 86.02 | 53/65 |
| `computation/units.rs` | 81.56 | 89.47 | 79.34 | 199/244 |
| `planning/semantics.rs` | 81.61 | 83.58 | 82.33 | 3720/4558 |
| `computation/range.rs` | 81.93 | 93.75 | 80.35 | 195/238 |
| `planning/graph.rs` | 81.98 | 87.83 | 82.45 | 7835/9557 |
| `parsing/ast.rs` | 82.36 | 88.51 | 74.39 | 1097/1332 |
| `computation/bigint/biguint.rs` | 83.53 | 89.80 | 81.61 | 431/516 |
| `literals.rs` | 83.58 | 77.98 | 84.17 | 611/731 |
| `computation/datetime.rs` | 84.78 | 80.68 | 83.19 | 947/1117 |
| `parsing/parser.rs` | 85.02 | 88.82 | 81.06 | 1929/2269 |
| `computation/rational.rs` | 85.65 | 97.06 | 79.38 | 776/906 |
| `planning/ordered_dispatch.rs` | 86.23 | 90.62 | 85.71 | 313/363 |
| `spec_set_id.rs` | 87.72 | 100.00 | 95.52 | 50/57 |
| `api/types.rs` | 87.82 | 95.00 | 88.29 | 209/238 |
| `parsing/lexer.rs` | 88.35 | 100.00 | 84.97 | 637/721 |
| `planning/normalize.rs` | 88.63 | 88.75 | 87.19 | 3569/4027 |
| `evaluation/tree.rs` | 89.53 | 93.75 | 89.96 | 1180/1318 |
| `string_distance.rs` | 90.48 | 83.33 | 93.22 | 57/63 |
| `planning/unit_index.rs` | 91.26 | 96.15 | 92.27 | 501/549 |
| `evaluation/conversion_trace.rs` | 92.22 | 100.00 | 92.49 | 154/167 |
| `formatting/mod.rs` | 92.34 | 100.00 | 91.32 | 904/979 |
| `registry.rs` | 92.45 | 97.44 | 94.47 | 968/1047 |
| `evaluation/mod.rs` | 92.55 | 78.26 | 93.20 | 298/322 |
| `parsing/mod.rs` | 92.64 | 98.92 | 89.89 | 1321/1426 |
| `evaluation/branch_semantics.rs` | 92.86 | 100.00 | 81.82 | 39/42 |
| `quality.rs` | 93.12 | 96.77 | 89.24 | 907/974 |
| `limits.rs` | 93.27 | 100.00 | 85.40 | 97/104 |
| `planning/discovery.rs` | 93.34 | 97.06 | 94.16 | 1233/1321 |
| `engine.rs` | 93.43 | 91.89 | 94.17 | 2147/2298 |
| `planning/mod.rs` | 93.66 | 96.15 | 94.61 | 1358/1450 |
| `deps.rs` | 96.23 | 100.00 | 96.47 | 51/53 |
| `planning/spec_set.rs` | 96.50 | 95.24 | 98.23 | 138/143 |
| `planning/unit_family.rs` | 96.98 | 100.00 | 96.44 | 353/364 |
| `parsing/assignment_continuation_tests.rs` | 97.39 | 100.00 | 93.26 | 261/268 |
| `evaluation/response.rs` | 98.83 | 94.44 | 98.34 | 592/599 |
| `api/response.rs` | 100.00 | 100.00 | 100.00 | 22/22 |
| `api/show.rs` | 100.00 | 100.00 | 100.00 | 36/36 |
| `computation/bigint/alloc.rs` | 100.00 | 100.00 | 97.37 | 20/20 |
| `parsing/source.rs` | 100.00 | 100.00 | 99.46 | 125/125 |

## Related docs

- [Engine integration test catalog](../../../engine/tests/README.md) — qualitative map of scenarios and subsystem overlap clusters
- [CLI test coverage](cli.md)
- [Engine benchmarks](../benchmarks/engine.md)
<!-- coverage-input-digest: dcf6c87f9868fbfb -->
