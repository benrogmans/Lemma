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

## Environment

- Host: `Linux 6.17.0-35-generic x86_64`
- Lemma git SHA: `259f0735a3186e33725b7076b534b830119b0443`
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
| `bench_shipping` | `total` | 17.26 us | 173 ns | 4.75 us | 100000 | 2.24 us | 0.2750 |
| `bench_pricing` | `total` | 64.54 us | 664 ns | 20.82 us | 100000 | 3.50 us | 0.3227 |
| `bench_order_pipeline` | `grand_total` | 162.70 us | 4.52 us | 36.31 us | 100000 | 5.36 us | 0.2232 |

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

