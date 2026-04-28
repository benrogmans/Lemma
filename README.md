# Lemma

[![CI](https://github.com/lemma/lemma/workflows/CI/badge.svg)](https://github.com/lemma/lemma/actions/workflows/quality.yml)
[![Crates.io](https://img.shields.io/crates/v/lemma-engine.svg)](https://crates.io/crates/lemma-engine)
[![Documentation](https://docs.rs/lemma-engine/badge.svg)](https://docs.rs/lemma-engine)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

> **A language that means business.**

Lemma is a declarative language for business rules. It flows like natural language and encodes pricing rules, tax calculations, eligibility criteria, contracts, policies and law. Stakeholders can read them, systems can evaluate them.

Rules in Lemma are transparent, deterministic, logically consistent, temporally bound and explainable. So with the same data for the same spec and the same effective point in time, you will get the same result. Lemma can also tell you why you got that result. Audits and tracing become trivial, even as time passes and rules change.

```lemma
spec pricing 2026-01-01

data quantity : number
data is_vip   : false

rule discount:
  0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip then 25%

rule price:
  quantity * 20 - discount
```

The last matching `unless` wins, mirroring how business rules, legal documents, and SOPs are written: "In principle X applies, unless Y, unless Z..."


## Why Lemma?
Laws, policies, and business rules traditionally exist in natural language. While humans must understand these rules, we rely on systems to enforce them. Over time, organizations have built massive IT infrastructures to house these rules; however, as both the regulations and the systems evolve, they become harder to manage and the disconnect between them grows.

Lemma provides a single source of truth. Rules written in Lemma are human-readable, time-aware, and pure. Its logic engine guarantees deterministic and logically consistent outcomes through static analysis, eliminating runtime errors. Furthermore, Lemma provides unrivaled auditability by explaining exactly how rules were applied for every evaluation.

This allows you to implement policy changes rapidly without compromising compliance. Lemma requires no database and maintains no state; by design, it is secure, able to run within existing applications and yes, it is blazingly fast.

### Direction

Lemma aims to combine **deterministic evaluation**, **transparent explanations**, **temporal versioning** (rules that evolve on a timeline, separate from how you deploy code), **registry-style sharing** of specs, and **interop** (CLI, HTTP, WASM, MCP, and stable language bindings). Planned work includes **inversion** (constraint-style “what would satisfy this outcome?” queries), **tables** as a first-class data type for data-driven rules, and **performance** competitive with high performance programming languages.

### What about AI?

AI models operate on approximations. The complexity of their neural networks makes tracing decisions ("explaining") practically impossible. While they excel at natural language, they are ill-suited for mathematics, strict protocols, or compliance.

Lemma provides certainty and transparency. Every result is exact, verifiable, and delivered in microseconds. Lemma offers seamless interoperability, allowing you to ground your AI systems in deterministic logic.

## Quick Start

### Installation

```bash
cargo install lemma-cli
```

### Your first spec

Create `shipping.lemma`:

```lemma
spec shipping

data money: scale
  -> unit eur 1.00
  -> unit usd 1.19
  -> decimals 2
  -> minimum 0

data weight: scale
  -> unit kilogram 1
  -> unit gram 0.001

data is_express     : true
data package_weight : 2.5 kilogram

rule express_fee:
  0 eur
  unless is_express then 4.99 eur

rule base_shipping:
  5.99 eur
  unless package_weight > 1 kilogram then 8.99 eur
  unless package_weight > 5 kilogram then 15.99 eur

rule total_cost:
  base_shipping + express_fee
```

Run it:

```
$ lemma run shipping
┌───────────────┬───────────┐
│ base_shipping ┆ 8.99 eur  │
├───────────────┼───────────┤
│ express_fee   ┆ 4.99 eur  │
├───────────────┼───────────┤
│ total_cost    ┆ 13.98 eur │
└───────────────┴───────────┘
Hash: 1d2e8f6d
```

Override data from the command line:

```bash
lemma run shipping is_express=false package_weight=6.0
```

## Key features

### Rich type system

Define custom types with units, constraints, and automatic conversion:

```lemma
spec type_examples

data money: scale
  -> unit eur 1.00
  -> unit usd 1.19
  -> decimals 2
  -> minimum 0

data status: text
  -> option "active"
  -> option "inactive"

data discount: ratio
  -> minimum 0
  -> maximum 1
```

**Primitive types:** `boolean`, `number`, `scale` (with units), `text`, `date`, `time`, `duration`, `ratio`

### Spec composition

Reference data and rules across specs:

```lemma
spec employee

data years_service : 8


spec leave_policy

data senior_threshold : 5
data base_leave_days  : 25
data bonus_leave_days : 5


spec leave_entitlement

with employee

with leave_policy

rule is_senior:
  employee.years_service >= leave_policy.senior_threshold

rule annual_leave_days:
  leave_policy.base_leave_days
  unless is_senior
    then leave_policy.base_leave_days + leave_policy.bonus_leave_days
```

### Temporal versioning

Multiple versions of a spec can coexist. The engine resolves the correct one based on a point in time:

```lemma
spec pricing

data base_price : 20
data quantity   : number

rule total:
  base_price * quantity


spec pricing 2025-01-01

data base_price : 25
data quantity   : number

rule total:
  base_price * quantity
```

```bash
lemma run pricing --effective 2024-06-01   # uses base_price: 20
lemma run pricing --effective 2025-06-01   # uses base_price: 25
```

### Veto

When type constraints are not enough, `veto` blocks a rule entirely:

```lemma
spec performance_review

data start_date        : [date]
data review_date       : [date]
data performance_score : number -> minimum 0 -> maximum 100]

rule bonus_percentage:
  0%
  unless performance_score >= 70 then 5%
  unless performance_score >= 90 then 10%
  unless review_date < start_date
    then veto "Review date must be after start date"
```

A vetoed rule produces no result. See [veto semantics](documentation/veto_semantics.md).

### Registry dependencies

Reference shared specs from a registry with `@`:

```lemma
spec invoicing

data currency from @lemma/std/finance

data subtotal : 250 eur
data tax_rate : 21%

rule tax:
  subtotal * tax_rate

rule total:
  subtotal + tax
```

```bash
lemma get           # fetch all @... dependencies
lemma get -f        # force re-fetch if content changed
```

## CLI

```bash
lemma run pricing                         # evaluate all rules
lemma run pricing --rules=total,tax       # specific rules only
lemma run pricing quantity=10 is_vip=true # override data
lemma run --interactive                   # interactive mode

lemma run pricing --effective 2025-01-01  # temporal query
lemma run pricing -o json                 # JSON output
lemma run pricing -x                      # show reasoning

lemma schema pricing                      # spec schema
lemma list                                # list all specs
lemma format                               # format .lemma files
lemma get                                 # fetch registry dependencies
```

### HTTP Server

```bash
lemma server --dir ./policies

# Evaluate via query parameters
curl "http://localhost:8012/pricing?quantity=10&is_member=true"

# Evaluate via JSON body
curl -X POST http://localhost:8012/pricing \
  -H "Content-Type: application/json" \
  -d '{"quantity": 10, "is_member": true}'

# Evaluate specific rules
curl "http://localhost:8012/pricing/discount,total?quantity=10"
```

Routes: `GET /` (list specs), `GET /openapi.json`, `GET /docs` (interactive API docs), `GET /health`

Live-reload with `--watch`:

```bash
lemma server --dir ./policies --watch
```

### MCP Server

AI assistants interact with Lemma specs via the [Model Context Protocol](https://modelcontextprotocol.io):

```bash
lemma mcp             # read-only
lemma mcp --admin     # enable spec creation
```

### WebAssembly

```bash
npm install @lemmabase/lemma-engine
```

```javascript
import { Lemma } from '@lemmabase/lemma-engine';
const engine = await Lemma();
```

[documentation/wasm.md](documentation/wasm.md)

### Docker

```bash
docker pull ghcr.io/lemma/lemma:latest

# Run a spec
docker run --rm -v "$(pwd):/specs" ghcr.io/lemma/lemma run shipping

# Deploy as HTTP API
docker run -d -p 8012:8012 -v "$(pwd):/specs" ghcr.io/lemma/lemma \
  server --host 0.0.0.0 --port 8012
```

Supports `linux/amd64` and `linux/arm64`.

## Documentation

- **[Language Guide](documentation/index.md)** -- specs, data, rules, types
- **[Reference](documentation/reference.md)** -- operators, literals, syntax
- **[Veto Semantics](documentation/veto_semantics.md)** -- when rules produce no value
- **[Examples](documentation/examples/)** -- example `.lemma` files
- **[CLI Reference](documentation/CLI.md)** -- all commands and flags
- **[Registry](documentation/registry.md)** -- shared specs and `@` references

## Status

Lemma is in early development and **not yet recommended for production use**. Expect breaking changes and evolving semantics.

## Contributing

Contributions welcome! See [contributing](documentation/contributing.md) for setup and workflow.

From the repository root, run **`cargo precommit`** before opening a PR. It runs **`versions-verify`**, then `fmt --check`, Clippy, Nextest, and cargo-deny (install [`cargo-nextest`](https://nexte.st/) and [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) first, same as CI). When bumping the workspace release version, use **`cargo bump <version>`** and **`cargo verify`** so every mirrored copy stays aligned (see [`xtask/README.md`](xtask/README.md)).

## License

Apache 2.0 -- see LICENSE for details.

---

**[GitHub](https://github.com/lemma/lemma)** -- **[Issues](https://github.com/lemma/lemma/issues)** -- **[Documentation](documentation/index.md)** -- **[WASM](documentation/wasm.md)**
