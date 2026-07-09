---
nav_title: Engine benchmarks
parent: Reference
nav_order: 60
---

# Engine evaluation benchmarks

Numbers are produced by `cargo benchmarks engine`. Lemma and the hand-written Python ports of the same business rules are measured on identical pinned inputs.

## Methodology

- Per-call boundary on both sides: pre-built typed inputs -> terminal rule value in memory. Fixture JSON is loaded once per spec before warmup.
- Lemma per-call work: clone a pre-built `HashMap<String, DataValueInput>`, then `Engine::run_plan(plan, Some(&effective), data, explain: false, Some(&[terminal_rule]))` where `terminal_rule` is `total` (shipping, pricing) or `grand_total` (order_pipeline). One VM pass per call. `run_plan` applies declared defaults via `DataOverlay::resolve`, converts data values to typed `LiteralValue`, evaluates the requested rule(s), and constructs a `Response` (explanation trees omitted).
- Python per-call work: `compute_terminal(inputs)` where `inputs` is a pre-built `Inputs` dataclass lifted from the fixture JSON before warmup. Shipping and pricing evaluate through `total`; order_pipeline evaluates only through `grand_total` (same terminal rule as Lemma).
- Effective pinned to `2026-01-01T00:00:00Z` (no timezone) on the Lemma side; Python rules carry no temporal logic.
- Latency: Criterion (3s warmup, 5s measurement) for Lemma; 1_000 warmup + 100_000 measured `time.perf_counter_ns()` samples with `gc.disable()` bracketing for Python. Median and standard deviation reported.
- Numerical precision: a separate pass compares all rule outputs. Lemma's `outputs` bench evaluates every local rule with explanations; Python's `compute(inputs)` returns a full `Outputs` dataclass. Both sides use exact rational arithmetic internally and commit to decimal strings at the output boundary. The accuracy table compares both sides via `rust_decimal::Decimal` (28-digit precision).
- API note: `Engine::run_plan` accepts `HashMap<String, DataValueInput>`. The benchmark mirrors what native callers pay after constructing typed inputs; JSON parsing at API boundaries (CLI, WASM) is out of scope.
- Memory: `stats_alloc` over 1_000 warmup + 10_000 measured `run_plan` calls per fixture (`cargo bench -p lemma-engine --bench memory`). Allocations and bytes are totals divided by iteration count.
- Profiling: install [`cargo-flamegraph`](https://github.com/flamegraph-rs/flamegraph) and run `cargo flamegraph -p lemma-engine --bench evaluate -- --bench bench_order_pipeline/run_plan` to attribute CPU time inside a single fixture.
- Micro-benchmarks: `cargo bench -p lemma-engine --bench internal_micro` isolates `DataOverlay::resolve` and full `run_plan` on `bench_order_pipeline`.

## Environment

- Host: `Linux 6.17.0-35-generic x86_64`
- Lemma git SHA: `acb98535040ad9312dd945f482fa035c81244e58`
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

## Latency

| Spec | Terminal rule | Lemma median | Lemma std dev | Python median | Python iter | Python std dev | Python / Lemma |
|------|---------------|-------------:|--------------:|--------------:|------------:|---------------:|---------------:|
| `bench_shipping` | `total` | 15.20 us | 477 ns | 4.56 us | 100000 | 1.64 us | 0.3002 |
| `bench_pricing` | `total` | 51.37 us | 662 ns | 19.89 us | 100000 | 4.18 us | 0.3872 |
| `bench_order_pipeline` | `grand_total` | 123.17 us | 1.01 us | 34.82 us | 100000 | 8.27 us | 0.2827 |

## Explain latency (`run_plan_explain`)

Same fixtures and terminal rules as the latency table, with `explain: true` (source-shaped bytecode, full rule VM, explanation recording). Ratio is explain median divided by `run_plan` median on the same machine run.

| Spec | Terminal rule | `run_plan` median | `run_plan_explain` median | Explain / `run_plan` |
|------|---------------|------------------:|--------------------------:|---------------------:|
| `bench_shipping` | `total` | 15.20 us | 59.12 us | 3.889 |
| `bench_pricing` | `total` | 51.37 us | 251.25 us | 4.891 |
| `bench_order_pipeline` | `grand_total` | 123.17 us | 875.78 us | 7.110 |

## Memory (per `run_plan` call)

| Spec | Iterations | Allocations/eval | Bytes allocated/eval | Reallocations/eval | Net bytes retained/eval |
|------|-----------:|-----------------:|---------------------:|-------------------:|------------------------:|
| `bench_shipping` | 10000 | 270.00 | 17335 | 1.00 | 0.00 |
| `bench_pricing` | 10000 | 863.30 | 39609 | 3.15 | 0.00 |
| `bench_order_pipeline` | 10000 | 2109.81 | 80925 | 4.77 | 0.00 |

## Numerical accuracy

60 rule outputs compared across the three fixtures; 0 deviations.

## Python implementation

Hand-written ports of the three Lemma specs live in [`../../engine/benches/python/business_rules/`](../../engine/benches/python/business_rules). Each module exports `Inputs`, `Outputs`, `TERMINAL_RULE`, `build_inputs(raw)`, `compute_terminal(inputs)`, and `compute(inputs)`. Standard library only (`fractions`, `dataclasses`, `json`, `time`, `gc`, `pathlib`, `statistics`). The Python benchmark harness is [`../../engine/benches/python/benchmark.py`](../../engine/benches/python/benchmark.py).

## Inputs

All fixtures share `effective = 2026-01-01T00:00:00Z` (no timezone). Fixture JSON is parsed once per spec at setup into typed inputs on both sides.

### `bench_shipping`

Source: [`engine/benches/specs/shipping.lemma`](../../engine/benches/specs/shipping.lemma). Inputs: [`engine/benches/specs/shipping.inputs.json`](../../engine/benches/specs/shipping.inputs.json).

```json
{
  "weight": "3",
  "destination": "domestic",
  "is_member": "false"
}
```

### `bench_pricing`

Source: [`engine/benches/specs/pricing.lemma`](../../engine/benches/specs/pricing.lemma). Inputs: [`engine/benches/specs/pricing.inputs.json`](../../engine/benches/specs/pricing.inputs.json).

```json
{
  "product_type": "premium",
  "quantity": "25",
  "unit_price": "100",
  "coupon_percent": "5",
  "loyalty_years": "2",
  "is_member": "true",
  "is_loyalty": "true",
  "is_tax_exempt": "false"
}
```

### `bench_order_pipeline`

Source: [`engine/benches/specs/order_pipeline.lemma`](../../engine/benches/specs/order_pipeline.lemma). Inputs: [`engine/benches/specs/order_pipeline.inputs.json`](../../engine/benches/specs/order_pipeline.inputs.json).

```json
{
  "customer_tier": "gold",
  "payment_method": "credit",
  "shipping_zone": "national",
  "quantity": "12",
  "unit_price": "85",
  "package_weight": "3.5",
  "delivery_distance": "180",
  "loyalty_points": "6500",
  "coupon_percent": "10",
  "is_fragile": "true",
  "is_express": "true",
  "is_hazardous": "false",
  "is_gift": "false",
  "is_first_time": "false"
}
```

