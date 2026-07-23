---
nav_title: Engine benchmarks
parent: Reference
nav_order: 60
---

# Engine evaluation benchmarks

Numbers are produced by `cargo benchmarks engine`. Hand-written Lemma specs and hand-written Python ports of the same business rules are measured on identical inline inputs.

## Methodology

- Hand-written Lemma specs vs hand-written Python ports of the same rules, on identical inline inputs.
- Cross-language latency compares per-request evaluation only — like comparing optimized C execution to Python, not C compile time to Python runtime.
- Lemma: compile (`Engine::new()` + `load(in_memory_source)`, parse + plan) once before measurement; timed loop = inline input literals + `run_plan` → terminal rule. Terminal rule is `total` (shipping, pricing) or `grand_total` (order_pipeline).
- Python: import module once before measurement; timed loop = inline input literals + `build_inputs(raw)` + `compute_terminal(inputs)`.
- No disk I/O, no JSON input sidecars, no pre-built input maps outside the timed loop.
- Effective pinned to `2026-01-01T00:00:00Z` (no timezone) on the Lemma side; Python rules carry no temporal logic.
- Latency: Criterion (3s warmup, 30s measurement) for Lemma; 100 warmup + 10_000 measured `time.perf_counter_ns()` samples with `gc.disable()` bracketing for Python. Median and standard deviation reported.
- Numerical precision: a separate untimed pass compares all rule outputs. Lemma's `outputs` bench evaluates every local rule with explanations; Python's `compute(inputs)` returns a full `Outputs` dataclass. Both sides use exact rational arithmetic internally and commit to decimal strings at the output boundary. The accuracy table compares both sides via `rust_decimal::Decimal` (28-digit precision).
- Memory: `stats_alloc` over 100 warmup + 1_000 measured eval-only `evaluate` calls per fixture (`cargo bench -p lemma-engine --bench memory`). Engine loaded once per fixture; each iteration wraps inline inputs + `run_plan` in a fresh region.

## Environment

- Host: `Linux 7.0.0-28-generic x86_64`
- Lemma git SHA: `7a90395b195a7267e268ca23092dbb603745def5`
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
| `bench_shipping` | 2.827 ms | 30.03 us |
| `bench_pricing` | 3.562 ms | 47.10 us |
| `bench_order_pipeline` | 5.153 ms | 138.25 us |

## Latency

| Spec | Terminal rule | Lemma median | Lemma std dev | Python median | Python iter | Python std dev | Python / Lemma |
|------|---------------|-------------:|--------------:|--------------:|------------:|---------------:|---------------:|
| `bench_shipping` | `total` | 15.94 us | 1.14 us | 7.14 us | 10000 | 720 ns | 0.4483 |
| `bench_pricing` | `total` | 39.71 us | 507 ns | 28.16 us | 10000 | 4.75 us | 0.7091 |
| `bench_order_pipeline` | `grand_total` | 72.14 us | 1.61 us | 48.40 us | 10000 | 6.76 us | 0.6709 |

## Explain latency (`evaluate_explain`)

Same fixtures and terminal rules as the latency table, with `explain: true`. Ratio is explain median divided by `evaluate` median on the same machine run.

| Spec | Terminal rule | `evaluate` median | `evaluate_explain` median | Explain / `evaluate` |
|------|---------------|------------------:|--------------------------:|---------------------:|
| `bench_shipping` | `total` | 15.94 us | 97.03 us | 6.089 |
| `bench_pricing` | `total` | 39.71 us | 340.47 us | 8.573 |
| `bench_order_pipeline` | `grand_total` | 72.14 us | 701.63 us | 9.726 |

## Memory (per `evaluate` call)

| Spec | Iterations | Allocations/eval | Bytes allocated/eval | Reallocations/eval | Net bytes retained/eval |
|------|-----------:|-----------------:|---------------------:|-------------------:|------------------------:|
| `bench_shipping` | 1000 | 303.00 | 17637 | 1.00 | 0.00 |
| `bench_pricing` | 1000 | 763.00 | 39289 | 1.00 | 0.00 |
| `bench_order_pipeline` | 1000 | 1331.00 | 61264 | 1.00 | 0.00 |

## Numerical accuracy

60 rule outputs compared across the three fixtures; 0 deviations.

## Python implementation

Hand-written ports of the three Lemma specs live in [`../../engine/benches/python/business_rules/`](../../engine/benches/python/business_rules). Each module exports `Inputs`, `Outputs`, `TERMINAL_RULE`, `build_inputs(raw)`, `compute_terminal(inputs)`, and `compute(inputs)`. Standard library only (`fractions`, `dataclasses`, `importlib`, `time`, `gc`, `pathlib`, `statistics`). The Python benchmark harness is [`../../engine/benches/python/benchmark.py`](../../engine/benches/python/benchmark.py).

## Inputs

All fixtures share `effective = 2026-01-01T00:00:00Z` (no timezone). Input values are inline string literals built inside every timed iteration on both sides.

### `bench_shipping`

Lemma source: [`engine/benches/specs/shipping.lemma`](../../engine/benches/specs/shipping.lemma). Python module: `business_rules.shipping`.

| Field | Value |
|-------|-------|
| `weight` | `3` |
| `destination` | `domestic` |
| `is_member` | `false` |

### `bench_pricing`

Lemma source: [`engine/benches/specs/pricing.lemma`](../../engine/benches/specs/pricing.lemma). Python module: `business_rules.pricing`.

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

Lemma source: [`engine/benches/specs/order_pipeline.lemma`](../../engine/benches/specs/order_pipeline.lemma). Python module: `business_rules.order_pipeline`.

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

