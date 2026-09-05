---
nav_title: Engine benchmarks
parent: Reference
nav_order: 60
---

# Engine evaluation benchmarks

Numbers are produced by `cargo benchmarks engine`. Hand-written Lemma specs and hand-written Python ports of the same business rules are measured on identical inline inputs.

## Methodology

- Hand-written Lemma specs vs hand-written Python ports of the same rules, on identical inline inputs.
- Cross-language latency compares per-request evaluation only: like comparing C execution to Python, not C compile time to Python runtime.
- Lemma: compile (`Engine::new()` + `load(in_memory_source)`, parse + plan) once before measurement; timed loop = inline input literals + `Engine::run` → terminal rule. Terminal rule is `total` (shipping, pricing) or `grand_total` (order_pipeline).
- Python: import module once before measurement; timed loop = inline input literals + `build_inputs(raw)` + `compute_terminal(inputs)`. `compute_terminal` evaluates only the terminal rule's dependency closure, matching Lemma's requested-rule walk.
- Literal constants are parsed once: Lemma at plan time, Python at import time (module-level `Fraction` constants). Rounding number types (`Decimal`, `float`) are out of scope; the comparison is exact arithmetic vs exact arithmetic.
- No disk I/O, no JSON input sidecars, no pre-built input maps outside the timed loop.
- Effective pinned to `2026-01-01T00:00:00Z` (no timezone) on the Lemma side; Python rules carry no temporal logic.
- Latency: Criterion (3s warmup, 30s measurement) for Lemma; 100 warmup + 10_000 measured `time.perf_counter_ns()` samples with `gc.disable()` bracketing for Python. Median and standard deviation reported.
- Numerical precision: a separate untimed pass compares all rule outputs. Lemma's `outputs` bench evaluates every local rule with explanations; Python's `compute(inputs)` returns a full `Outputs` dataclass. Both sides use exact rational arithmetic (`RationalInteger` / `fractions.Fraction`) and commit to decimal strings at the output boundary. The accuracy table compares both sides via `rust_decimal::Decimal` (28-digit precision).
- Memory: `stats_alloc` over 100 warmup + 1_000 measured eval-only `evaluate` calls per fixture (`cargo bench -p lemma-engine --bench memory`). Engine loaded once per fixture; each iteration wraps inline inputs + `Engine::run` in a fresh region.
- Snapshot: `cargo bench -p lemma-engine --bench snapshot` on the generated logistics ladder (1050 / 6300 / 18900 / 126000 rate cells; byte-identical to the Java `SpecGenerator.logistics` fixtures). One `load`, then 1 warmup + 10 measured `Engine::snapshot` and `Engine::from_snapshot` calls; medians reported. Heap columns are `stats_alloc` net bytes retained by the loaded and by the restored engine. The restored engine must evaluate `rate_shop.cheapest` to the same value as the loaded engine.

## Environment

- Host: `Linux 7.0.0-30-generic x86_64`
- Lemma git SHA: `b64088ee98bce6f58269a5165a3c4bf3f4d8416e`
- Python: `Python 3.12.3`
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

## Compile (Lemma, parse + plan)

One-time cost per spec load. Not included in the Python/Lemma latency ratio; amortized across requests in production.

| Spec | Median | Std dev |
|------|-------:|--------:|
| `bench_shipping` | 1.867 ms | 26.29 us |
| `bench_pricing` | 2.245 ms | 38.87 us |
| `bench_order_pipeline` | 2.773 ms | 135.23 us |

## Latency

| Spec | Terminal rule | Lemma median | Lemma std dev | Python median | Python iter | Python std dev | Python / Lemma |
|------|---------------|-------------:|--------------:|--------------:|------------:|---------------:|---------------:|
| `bench_shipping` | `total` | 5.52 us | 141 ns | 4.94 us | 10000 | 1.36 us | 0.8957 |
| `bench_pricing` | `total` | 13.81 us | 170 ns | 15.54 us | 10000 | 3.56 us | 1.125 |
| `bench_order_pipeline` | `grand_total` | 26.28 us | 1.36 us | 28.72 us | 10000 | 4.97 us | 1.093 |

## Explain latency (`evaluate_explain`)

Same fixtures and terminal rules as the latency table, with `explain: true`. Ratio is explain median divided by `evaluate` median on the same machine run.

| Spec | Terminal rule | `evaluate` median | `evaluate_explain` median | Explain / `evaluate` |
|------|---------------|------------------:|--------------------------:|---------------------:|
| `bench_shipping` | `total` | 5.52 us | 54.00 us | 9.790 |
| `bench_pricing` | `total` | 13.81 us | 199.72 us | 14.46 |
| `bench_order_pipeline` | `grand_total` | 26.28 us | 489.22 us | 18.62 |

## Memory (per `evaluate` call)

| Spec | Iterations | Allocations/eval | Bytes allocated/eval | Reallocations/eval | Net bytes retained/eval |
|------|-----------:|-----------------:|---------------------:|-------------------:|------------------------:|
| `bench_shipping` | 1000 | 57.00 | 7278 | 1.00 | 0.00 |
| `bench_pricing` | 1000 | 110.00 | 16118 | 2.00 | 0.00 |
| `bench_order_pipeline` | 1000 | 221.00 | 27439 | 3.00 | 0.00 |

## Snapshot (logistics ladder)

Multi-spec rating workspace: `rates_*` cards with 1050 `unless` arms each, `zones_*` with 900 arms per service, one quote pipeline per carrier and service, and a `rate_shop` that `uses` every quote. Restore is the number to watch; snapshot bytes and restored heap are dominated by the per-cell type payload the plan store ships today.

| Profile | Rate cells | Source | Load | Loaded heap | Snapshot | Encode median | Restore median | Allocations/restore | Restored heap | Restored / loaded heap |
|---------|-----------:|-------:|-----:|------------:|---------:|--------------:|---------------:|--------------------:|--------------:|-----------------------:|
| `logistics_ground` | 1050 | 0.1 MiB | 147.336 ms | 7.5 MiB | 0.9 MiB | 1.826 ms | 5.749 ms | 89800 | 14.1 MiB | 1.879 |
| `logistics_carrier` | 6300 | 0.5 MiB | 1209.356 ms | 42.2 MiB | 5.0 MiB | 11.342 ms | 28.942 ms | 473104 | 69.7 MiB | 1.653 |
| `logistics_d2c` | 18900 | 1.5 MiB | 3672.743 ms | 121.5 MiB | 14.9 MiB | 32.764 ms | 82.771 ms | 1365914 | 204.7 MiB | 1.684 |
| `logistics_enterprise` | 126000 | 10.1 MiB | 32270.465 ms | 796.5 MiB | 101.4 MiB | 279.801 ms | 537.316 ms | 8954799 | 1298.2 MiB | 1.630 |

## Numerical accuracy

60 rule outputs compared across the three fixtures; 0 deviations.

## Python implementation

Hand-written ports of the three Lemma specs live in [`engine/benches/python/business_rules`](https://github.com/lemma/lemma/tree/b64088ee98bce6f58269a5165a3c4bf3f4d8416e/engine/benches/python/business_rules). Each module exports `Inputs`, `Outputs`, `TERMINAL_RULE`, `build_inputs(raw)`, `compute_terminal(inputs)`, and `compute(inputs)`. Standard library only (`fractions`, `dataclasses`, `importlib`, `time`, `gc`, `pathlib`, `statistics`). The Python benchmark harness is [`engine/benches/python/benchmark.py`](https://github.com/lemma/lemma/blob/b64088ee98bce6f58269a5165a3c4bf3f4d8416e/engine/benches/python/benchmark.py).

## Inputs

All fixtures share `effective = 2026-01-01T00:00:00Z` (no timezone). Input values are inline string literals built inside every timed iteration on both sides.

### `bench_shipping`

Lemma source: [`engine/benches/specs/shipping.lemma`](https://github.com/lemma/lemma/blob/b64088ee98bce6f58269a5165a3c4bf3f4d8416e/engine/benches/specs/shipping.lemma). Python module: `business_rules.shipping`.

| Field | Value |
|-------|-------|
| `weight` | `3` |
| `destination` | `domestic` |
| `is_member` | `false` |

### `bench_pricing`

Lemma source: [`engine/benches/specs/pricing.lemma`](https://github.com/lemma/lemma/blob/b64088ee98bce6f58269a5165a3c4bf3f4d8416e/engine/benches/specs/pricing.lemma). Python module: `business_rules.pricing`.

| Field | Value |
|-------|-------|
| `product_type` | `premium` |
| `quantity` | `25` |
| `unit_price` | `100` |
| `coupon_percent` | `5` |
| `loyalty_years` | `2` |
| `is_member` | `true` |
| `is_loyalty` | `true` |
| `is_tax_exempt` | `false` |

### `bench_order_pipeline`

Lemma source: [`engine/benches/specs/order_pipeline.lemma`](https://github.com/lemma/lemma/blob/b64088ee98bce6f58269a5165a3c4bf3f4d8416e/engine/benches/specs/order_pipeline.lemma). Python module: `business_rules.order_pipeline`.

| Field | Value |
|-------|-------|
| `customer_tier` | `gold` |
| `payment_method` | `credit` |
| `shipping_zone` | `national` |
| `quantity` | `12` |
| `unit_price` | `85` |
| `package_weight` | `3.5` |
| `delivery_distance` | `180` |
| `loyalty_points` | `6500` |
| `coupon_percent` | `10` |
| `is_fragile` | `true` |
| `is_express` | `true` |
| `is_hazardous` | `false` |
| `is_gift` | `false` |
| `is_first_time` | `false` |

