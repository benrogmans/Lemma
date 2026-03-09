# Lemma

[![CI](https://github.com/benrogmans/lemma/workflows/CI/badge.svg)](https://github.com/benrogmans/lemma/actions/workflows/quality.yml)
[![Crates.io](https://img.shields.io/crates/v/lemma-engine.svg)](https://crates.io/crates/lemma-engine)
[![Documentation](https://docs.rs/lemma-engine/badge.svg)](https://docs.rs/lemma-engine)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

> **A language that means business.**

Lemma is a declarative language for business rules. Specs flow like natural language and encode pricing rules, tax calculations, eligibility criteria, contracts, and policies. Business stakeholders can read and validate them, while software systems enforce and automate them.

```lemma
spec pricing

fact quantity: [number]
fact is_vip: false

rule discount: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip         then 25%

rule price: quantity * 20 - discount
```

The last matching `unless` wins — mirroring how business rules, legal documents, and SOPs are written: "In principle X applies, unless Y, unless Z..."

## Why Lemma?

Business rules traditionally live in natural language that humans can read but machines cannot execute, or in imperative code that machines execute but humans struggle to read. Lemma bridges this gap: one source of truth that is both human-readable and machine-executable.

### What about AI?

AI models approximate — they don't calculate. Great at language, not reliable for math or following protocols. For some things in life we need certainty, not probabilities.

**Lemma provides certainty.** Every answer is exact, delivered in microseconds, with verifiable reasoning. Use Lemma's MCP server to make your LLMs deterministic.

## Quick Start

### Installation

```bash
cargo install lemma-cli
```

### Your first spec

Create `shipping.lemma`:

```lemma
spec shipping

type money: scale
  -> unit eur 1.00
  -> unit usd 1.19
  -> decimals 2
  -> minimum 0

type weight: scale
  -> unit kilogram 1.0
  -> unit gram 0.001

fact is_express: true
fact package_weight: 2.5 kilogram

rule express_fee: 0 eur
  unless is_express then 4.99 eur

rule base_shipping: 5.99 eur
  unless package_weight > 1 kilogram  then  8.99 eur
  unless package_weight > 5 kilogram then 15.99 eur

rule total_cost: base_shipping + express_fee
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
Hash: b28318af
```

Override facts from the command line:

```bash
lemma run shipping is_express=false package_weight=6.0
```

## Key features

### Rich type system

Define custom types with units, constraints, and automatic conversion:

```lemma
type money: scale
  -> unit eur 1.00
  -> unit usd 1.19
  -> decimals 2
  -> minimum 0

type status: text
  -> option "active"
  -> option "inactive"

type discount: ratio
  -> minimum 0
  -> maximum 1
```

**Primitive types:** `boolean`, `number`, `scale` (with units), `text`, `date`, `time`, `duration`, `ratio`

### Spec composition

Reference facts and rules across specs:

```lemma
spec employee
fact years_service: 8

spec leave_policy
fact senior_threshold: 5
fact base_leave_days: 25
fact bonus_leave_days: 5

spec leave_entitlement
fact employee: spec employee
fact leave_policy: spec leave_policy

rule is_senior: employee.years_service >= leave_policy.senior_threshold
rule annual_leave_days: leave_policy.base_leave_days
  unless is_senior then leave_policy.base_leave_days + leave_policy.bonus_leave_days
```

### Temporal versioning

Multiple versions of a spec can coexist. The engine resolves the correct one based on a point in time:

```lemma
spec pricing
fact base_price: 20
fact quantity: [number]
rule total: base_price * quantity

spec pricing 2025-01-01
fact base_price: 25
fact quantity: [number]
rule total: base_price * quantity
```

```bash
lemma run pricing --effective 2024-06-01   # uses base_price: 20
lemma run pricing --effective 2025-06-01   # uses base_price: 25
```

### Veto

When type constraints are not enough, `veto` blocks a rule entirely:

```lemma
spec performance_review

fact start_date: [date]
fact review_date: [date]
fact performance_score: [number -> minimum 0 -> maximum 100]

rule bonus_percentage: 0%
  unless performance_score >= 70 then 5%
  unless performance_score >= 90 then 10%
  unless review_date < start_date then veto "Review date must be after start date"
```

A vetoed rule produces no result. See [veto semantics](documentation/veto_semantics.md).

### Registry dependencies

Reference shared specs from a registry with `@`:

```lemma
spec invoicing

type currency from @lemma/std/finance

fact subtotal: 250 eur
fact tax_rate: 21%

rule tax: subtotal * tax_rate
rule total: subtotal + tax
```

```bash
lemma get           # fetch all @... dependencies
lemma get -f        # force re-fetch if content changed
```

## CLI

```bash
lemma run pricing                         # evaluate all rules
lemma run pricing --rules=total,tax       # specific rules only
lemma run pricing quantity=10 is_vip=true # override facts
lemma run --interactive                   # interactive mode

lemma run pricing --effective 2025-01-01  # temporal query
lemma run spec~a1b2c3d4                   # pin to content hash (use lemma show for hash)

lemma run pricing -o json                 # JSON output
lemma run pricing -x                      # show reasoning

lemma show pricing                        # inspect spec structure (includes hash)
lemma list                                # list all specs
lemma format                               # format .lemma files
lemma get                                 # fetch registry dependencies
lemma info                                # show environment info
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

Evaluate specs in the browser or at the edge:

```bash
npm install @benrogmans/lemma-engine
```

See [WASM documentation](documentation/wasm.md).

### Docker

```bash
docker pull ghcr.io/benrogmans/lemma:latest

# Run a spec
docker run --rm -v "$(pwd):/specs" ghcr.io/benrogmans/lemma run shipping

# Deploy as HTTP API
docker run -d -p 8012:8012 -v "$(pwd):/specs" ghcr.io/benrogmans/lemma \
  server --host 0.0.0.0 --port 8012
```

Supports `linux/amd64` and `linux/arm64`.

## Documentation

- **[Language Guide](documentation/index.md)** -- specs, facts, rules, types
- **[Reference](documentation/reference.md)** -- operators, literals, syntax
- **[Veto Semantics](documentation/veto_semantics.md)** -- when rules produce no value
- **[Examples](documentation/examples/)** -- example `.lemma` files
- **[CLI Reference](documentation/CLI.md)** -- all commands and flags
- **[Registry](documentation/registry.md)** -- shared specs and `@` references

## Status

Lemma is in early development and **not yet recommended for production use**. Expect breaking changes and evolving semantics.

## Contributing

Contributions welcome! See [contributing](documentation/contributing.md) for setup and workflow.

## License

Apache 2.0 -- see LICENSE for details.

---

**[GitHub](https://github.com/benrogmans/lemma)** -- **[Issues](https://github.com/benrogmans/lemma/issues)** -- **[Documentation](documentation/index.md)** -- **[WASM](documentation/wasm.md)**
