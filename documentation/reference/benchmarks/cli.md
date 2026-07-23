---
nav_title: CLI benchmarks
parent: Reference
nav_order: 50
---

# CLI benchmarks

Numbers are produced by `cargo benchmarks cli`. Measures the `lemma` binary and in-process engine wrappers used by the CLI.

## Methodology

### HTTP evaluate (`http_evaluate`)

- Spawns `lemma server --prefix documentation/examples` on `127.0.0.1:19877` once per Criterion group.
- Each iteration: blocking `reqwest` POST with `application/x-www-form-urlencoded` body (coffee order, library fees, Dutch net salary) or GET for show-only retrieval.
- Examples loaded from [`../../documentation/examples/`](../../documentation/examples/).
- Latency: Criterion (3s warmup, 10s measurement for evaluate group, 5s for show). Median and standard deviation reported.

### Engine profile (`engine_profile`)

- In-process: loads all `.lemma` files from `documentation/examples` into one `Engine`.
- Fixture: Dutch net salary (`net_salary`) with `gross_salary=5000 eur`, `pay_period=month`, `income_source=employment`, `pension_contribution=150 eur`, `payroll_tax_credit=true`; effective is `DateTimeValue::now()` per iteration setup.
- Breakdown benches isolate evaluate, overlay resolve, single-rule run, and JSON serialization paths.
- Latency: Criterion (3s warmup, 5s measurement). Median and standard deviation reported.

## Environment

- Host: `Linux 7.0.0-28-generic x86_64`
- Lemma git SHA: `7a90395b195a7267e268ca23092dbb603745def5`
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
| POST `/coffee_order` | 286.80 us | 29.46 us |
| POST `/library_fees` | 146.17 us | 16.14 us |
| POST `/net_salary` | 640.33 us | 31.33 us |
| GET `/net_salary` (show only) | 185.93 us | 11.70 us |

## Engine profile latency (Dutch net salary)

| Case | Median | Std dev |
|------|-------:|--------:|
| Full `Engine::run` | 381.46 us | 6.34 us |
| Single-rule evaluate (`periods_per_year`) | 23.81 us | 500 ns |
| Envelope JSON serialize | 20.37 us | 542 ns |
| Raw response JSON serialize | 2.33 us | 114 ns |

