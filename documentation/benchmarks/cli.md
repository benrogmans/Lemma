# CLI benchmarks

Numbers are produced by `cargo benchmarks cli`. Measures the `lemma` binary and in-process engine wrappers used by the CLI.

## Methodology

### HTTP evaluate (`http_evaluate`)

- Spawns `lemma server --prefix documentation/examples` on `127.0.0.1:19877` once per Criterion group.
- Each iteration: blocking `reqwest` POST with `application/x-www-form-urlencoded` body (coffee order, library fees, Dutch net salary) or GET for schema-only retrieval.
- Examples loaded from [`../../documentation/examples/`](../../documentation/examples/).
- Latency: Criterion (3s warmup, 5s measurement). Median and standard deviation reported.

### Engine profile (`engine_profile`)

- In-process: loads all `.lemma` files from `documentation/examples` into one `Engine`.
- Fixture: Dutch net salary (`net_salary`) with `gross_salary=5000 eur`, `pay_period=month`, `income_source=employment`, `pension_contribution=150 eur`, `payroll_tax_credit=true`; effective is `DateTimeValue::now()` per iteration setup.
- Breakdown benches isolate evaluate, overlay resolve, plan clone, single-rule run, and JSON serialization paths.
- Latency: Criterion (3s warmup, 5s measurement). Median and standard deviation reported.

## Environment

- Host: `Linux 6.17.0-35-generic x86_64`
- Lemma git SHA: `4eba6cfdf5ffd07a75dedee2b239dd9ff2f4e50c`
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

## HTTP evaluate latency

| Case | Median | Std dev |
|------|-------:|--------:|
| POST `/coffee_order` | 300.68 us | 18.90 us |
| POST `/library_fees` | 172.58 us | 13.42 us |
| POST `/net_salary` | 1.675 ms | 58.02 us |
| GET `/net_salary` (schema only) | 320.81 us | 12.12 us |

## Engine profile latency (Dutch net salary)

| Case | Median | Std dev |
|------|-------:|--------:|
| Full `Engine::run` | 1.304 ms | 21.56 us |
| `DataOverlay::resolve` | 7.87 us | 249 ns |
| Single-rule evaluate (`periods_per_year`) | 28.99 us | 665 ns |
| Envelope JSON serialize | 16.96 us | 522 ns |
| Raw response JSON serialize | 14.75 us | 268 ns |

