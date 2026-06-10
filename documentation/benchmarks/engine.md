# Engine evaluation benchmarks

Numbers are produced by `cargo benchmarks engine`. Lemma and the hand-written Python ports of the same business rules are measured on identical pinned inputs.

## Methodology

- Per-call boundary on both sides: typed inputs -> outputs in memory. Fixture JSON is loaded once per spec before warmup.
- Lemma per-call work: clone a pre-built `HashMap<String, DataValueInput>`, then `Engine::run_plan(plan, Some(&effective), data, explain: false, Some(&[terminal_rule]))` where `terminal_rule` is `total` (shipping, pricing) or `grand_total` (order_pipeline). One VM pass per call. `run_plan` applies declared defaults via `DataOverlay::resolve`, converts data values to typed `LiteralValue`, evaluates the requested rule(s), and constructs a `Response` (explanation trees omitted).
- Python per-call work: `compute(build_inputs(raw_dict))` where `raw_dict` is the pre-parsed `dict[str, str]` from the fixture JSON. `build_inputs` lifts string values to exact `fractions.Fraction`; `compute` returns a typed `Outputs` dataclass with one field per Lemma rule.
- Effective pinned to `2026-01-01T00:00:00Z` (no timezone) on the Lemma side; Python rules carry no temporal logic.
- Latency: Criterion (3s warmup, 5s measurement) for Lemma; 1_000 warmup + 100_000 measured `time.perf_counter_ns()` samples with `gc.disable()` bracketing for Python. Median and standard deviation reported.
- Numerical precision: Lemma evaluates with exact arbitrary-precision rationals internally; API output commits rationals to decimal strings. Python ports use exact `fractions.Fraction` arithmetic and commit to decimal strings at the output boundary. The accuracy table compares both sides via `rust_decimal::Decimal` (28-digit precision).
- API note: `Engine::run_plan` accepts `HashMap<String, DataValueInput>`. The benchmark mirrors what native callers pay after constructing typed inputs; JSON parsing at API boundaries (CLI, WASM) is out of scope.

## Environment

- Host: `Linux 6.17.0-35-generic x86_64`
- Lemma git SHA: `4eba6cfdf5ffd07a75dedee2b239dd9ff2f4e50c`
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
| `bench_shipping` | `total` | 19.81 us | 409 ns | 7.03 us | 100000 | 1.85 us | 0.3548 |
| `bench_pricing` | `total` | 68.61 us | 3.01 us | 28.33 us | 100000 | 4.78 us | 0.4130 |
| `bench_order_pipeline` | `grand_total` | 168.75 us | 10.67 us | 60.26 us | 100000 | 7.40 us | 0.3571 |

## Numerical accuracy

60 rule outputs compared across the three fixtures; 0 deviations.

## Python implementation

Hand-written ports of the three Lemma specs live in [`../../engine/benches/python/business_rules/`](../../engine/benches/python/business_rules). Each module exports `Inputs`, `Outputs`, `build_inputs(raw)`, `compute(inputs)`. Standard library only (`decimal`, `dataclasses`, `json`, `time`, `gc`, `pathlib`, `statistics`). The Python benchmark harness is [`../../engine/benches/python/benchmark.py`](../../engine/benches/python/benchmark.py).

## Inputs

All fixtures share `effective = 2026-01-01T00:00:00Z` (no timezone). Data values are JSON strings; the benchmark parses them into the engine's `HashMap<String, serde_json::Value>` on every iteration.

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

