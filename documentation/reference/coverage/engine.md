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

- Host: `Linux 6.17.0-35-generic x86_64`
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
| Lines | 33034 | 38753 | 85.24% |
| Functions | 2543 | 2911 | 87.36% |
| Regions | 48849 | 58132 | 84.03% |

## Test run

- Total: 2073
- Passed: 2073
- Skipped: 2
- Failed: 0

## Per-module coverage

Sorted by line coverage ascending (weakest first). Only files under `src/` for this crate are listed.

| Module | Line % | Function % | Region % | Lines covered/total |
|--------|-------:|-----------:|---------:|--------------------:|
| `computation/decimal_math.rs` | 48.15 | 100.00 | 40.18 | 26/54 |
| `evaluation/operations.rs` | 53.78 | 50.00 | 57.56 | 64/119 |
| `computation/comparison.rs` | 64.74 | 77.78 | 66.12 | 112/173 |
| `computation/arithmetic.rs` | 74.79 | 87.50 | 74.61 | 718/960 |
| `computation/measure_math.rs` | 76.92 | 100.00 | 83.70 | 50/65 |
| `computation/bigint/signed.rs` | 77.51 | 78.38 | 72.86 | 193/249 |
| `error.rs` | 79.25 | 76.47 | 73.58 | 424/535 |
| `planning/data_input.rs` | 79.34 | 81.25 | 85.67 | 192/242 |
| `computation/units.rs` | 80.00 | 100.00 | 78.95 | 172/215 |
| `literals.rs` | 80.59 | 73.64 | 77.99 | 652/809 |
| `evaluation/expression.rs` | 80.60 | 66.67 | 83.72 | 54/67 |
| `planning/semantics.rs` | 81.09 | 80.32 | 80.29 | 3363/4147 |
| `evaluation/response.rs` | 81.30 | 76.19 | 77.66 | 661/813 |
| `planning/graph.rs` | 81.39 | 86.97 | 82.12 | 7305/8975 |
| `computation/datetime.rs` | 84.32 | 78.65 | 83.01 | 941/1116 |
| `parsing/parser.rs` | 84.58 | 87.94 | 80.46 | 1788/2114 |
| `parsing/ast.rs` | 85.75 | 86.13 | 81.75 | 1029/1200 |
| `computation/range.rs` | 85.79 | 100.00 | 80.49 | 163/190 |
| `engine.rs` | 85.92 | 82.64 | 85.02 | 1233/1435 |
| `parsing/lexer.rs` | 86.02 | 95.71 | 81.58 | 640/744 |
| `planning/execution_plan.rs` | 86.81 | 87.02 | 83.75 | 2000/2304 |
| `spec_set_id.rs` | 87.72 | 100.00 | 95.52 | 50/57 |
| `evaluation/partial.rs` | 88.70 | 100.00 | 85.71 | 157/177 |
| `computation/rational.rs` | 89.02 | 96.77 | 81.11 | 454/510 |
| `computation/bigint/biguint.rs` | 89.26 | 93.18 | 87.50 | 432/484 |
| `planning/normalize.rs` | 89.55 | 94.30 | 88.41 | 2553/2851 |
| `evaluation/explanations.rs` | 90.94 | 95.29 | 90.62 | 1144/1258 |
| `planning/mod.rs` | 92.13 | 97.37 | 93.91 | 960/1042 |
| `parsing/mod.rs` | 92.85 | 98.86 | 90.43 | 1260/1357 |
| `computation/bigint/alloc.rs` | 92.86 | 91.67 | 88.41 | 39/42 |
| `evaluation/mod.rs` | 93.42 | 100.00 | 90.84 | 724/775 |
| `planning/transitive_normalization.rs` | 93.68 | 100.00 | 90.31 | 163/174 |
| `registry.rs` | 93.94 | 91.04 | 93.59 | 1023/1089 |
| `planning/spec_set.rs` | 94.22 | 93.48 | 95.25 | 310/329 |
| `planning/discovery.rs` | 94.72 | 94.64 | 95.48 | 951/1004 |
| `formatting/mod.rs` | 95.12 | 100.00 | 95.30 | 740/778 |
| `deps.rs` | 96.23 | 100.00 | 96.47 | 51/53 |
| `evaluation/branch_semantics.rs` | 97.22 | 100.00 | 84.17 | 105/108 |
| `parsing/source.rs` | 99.06 | 100.00 | 97.42 | 105/106 |
| `limits.rs` | 100.00 | 100.00 | 100.00 | 33/33 |

## Related docs

- [Engine integration test catalog](../../../engine/tests/README.md) — qualitative map of scenarios and subsystem overlap clusters
- [CLI test coverage](cli.md)
- [Engine benchmarks](../benchmarks/engine.md)
<!-- coverage-input-digest: 0993b197b553dc82 -->
