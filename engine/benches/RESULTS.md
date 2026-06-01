# Engine evaluation benchmarks

Numbers are produced by `cargo run -p xtask -- bench-report`. Lemma and the hand-written Python ports of the same business rules are measured on identical pinned inputs.

## Methodology

- Per-call boundary on both sides: JSON input bytes -> outputs in memory.
- Lemma per-call work: `serde_json::from_slice` of the inputs JSON into `HashMap<String, serde_json::Value>`, then `Engine::run_plan(plan, Some(&effective), data, false)`. `run_plan` clones the execution plan, applies declared defaults, converts data values to typed `LiteralValue`, evaluates, and constructs a `Response`.
- Python per-call work: `compute(build_inputs(json.loads(raw_bytes)))`. `build_inputs` converts the raw `dict[str, str]` to a typed `Inputs` dataclass (every `Decimal` constructed inside the call); `compute` returns a typed `Outputs` dataclass with one field per Lemma rule.
- Effective pinned to `2026-01-01T00:00:00Z` (no timezone) on the Lemma side; Python rules carry no temporal logic.
- Latency: Criterion (3s warmup, 5s measurement) for Lemma; 1_000 warmup + 100_000 measured `time.perf_counter_ns()` samples with `gc.disable()` bracketing for Python. Median and standard deviation reported.
- Numerical precision: Lemma's arithmetic uses `num_rational::BigRational` and `rust_decimal::Decimal` internally (see `engine/Cargo.toml`); intermediates stay exact until API output, where they serialize as decimal strings. Python uses `decimal.Decimal` at the default context (`prec=28`, `ROUND_HALF_EVEN`). The accuracy comparison uses `rust_decimal::Decimal` (28-digit precision) so the diff arithmetic matches Python's context.
- API note: `Engine::run_plan` accepts `HashMap<String, serde_json::Value>`, coupling the public surface to JSON as a wire format. The benchmark mirrors what callers actually pay; an API revision exposing a typed input map is out of scope here.

## Environment

- Host: `Linux 6.17.0-29-generic x86_64`
- Lemma git SHA: `e3b432c857e4f7300fc14d01e8d4f3e13652db3a`
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

| Spec | Rules | Lemma median | Lemma std dev | Python median | Python std dev | Python / Lemma |
|------|------:|-------------:|--------------:|--------------:|---------------:|---------------:|
| `bench_shipping` | 6 | 285.12 us | 12.76 us | 4.63 us | 2.47 us | 0.0162 |
| `bench_pricing` | 15 | 1.186 ms | 95.71 us | 9.46 us | 7.30 us | 0.00798 |
| `bench_order_pipeline` | 39 | 6.222 ms | 1.101 ms | 19.01 us | 4.97 us | 0.00306 |

## Numerical accuracy

60 rule outputs compared across the three fixtures; 0 deviations.

## Python implementation

Hand-written ports of the three Lemma specs live in [`python/business_rules/`](python/business_rules). Each module exports `Inputs`, `Outputs`, `build_inputs(raw)`, `compute(inputs)`. Standard library only (`decimal`, `dataclasses`, `json`, `time`, `gc`, `pathlib`, `statistics`). The Python benchmark harness is [`python/benchmark.py`](python/benchmark.py).

## Inputs

All fixtures share `effective = 2026-01-01T00:00:00Z` (no timezone). Data values are JSON strings; the benchmark parses them into the engine's `HashMap<String, serde_json::Value>` on every iteration.

### `bench_shipping`

Source: [`engine/benches/specs/shipping.lemma`](engine/benches/specs/shipping.lemma). Inputs: [`engine/benches/specs/shipping.inputs.json`](engine/benches/specs/shipping.inputs.json).

```json
{
  "weight": "3",
  "destination": "domestic",
  "is_member": "false"
}
```

### `bench_pricing`

Source: [`engine/benches/specs/pricing.lemma`](engine/benches/specs/pricing.lemma). Inputs: [`engine/benches/specs/pricing.inputs.json`](engine/benches/specs/pricing.inputs.json).

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

Source: [`engine/benches/specs/order_pipeline.lemma`](engine/benches/specs/order_pipeline.lemma). Inputs: [`engine/benches/specs/order_pipeline.inputs.json`](engine/benches/specs/order_pipeline.inputs.json).

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

