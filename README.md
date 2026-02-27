# Lemma

[![CI](https://github.com/benrogmans/lemma/workflows/CI/badge.svg)](https://github.com/benrogmans/lemma/actions/workflows/quality.yml)
[![Crates.io](https://img.shields.io/crates/v/lemma-engine.svg)](https://crates.io/crates/lemma-engine)
[![Documentation](https://docs.rs/lemma-engine/badge.svg)](https://docs.rs/lemma-engine)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

> **A language that means business.**

Lemma is a declarative language designed specifically for expressing business logic. Lemma docs flow like natural language and encode pricing rules, tax calculations, eligibility criteria, contracts, and policies. Business stakeholders can read and validate them, while software systems can enforce and automate them.

```lemma
doc pricing

fact quantity: [number]
fact is_vip  : false

rule discount: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip         then 25%

rule price: quantity * 20 - discount?
```

Note how Lemma automatically deducts the discount percentage in the expression `quantity * 20 - discount?`.

## Why Lemma?

Business rules are traditionally encoded in either natural language documents that humans can read but machines cannot execute, or in imperative code that machines can execute but humans struggle to read. This creates a fundamental disconnect: legal contracts, compliance policies, and business rules live in one world, while their software implementations live in another. Changes to policies require translation by developers, introducing delay, cost, and the risk of misinterpretation.

Lemma bridges this gap—eliminating the translation layer and unifying business logic.

### What about AI?
AI models operate on probability. By design, they approximate—they don't calculate. This makes them great at language, but not reliable for math or following protocol.

**Lemma provides certainty**. Every answer is exact, delivered in microseconds, and the reasoning is verifiable.

Pro tip: use Lemma's MCP server to make your LLMs deterministic. Use LLMs as a friendly interface for your Lemma docs.

## Quick Start

### Installation

```bash
cargo install lemma-cli
```

### Your first Lemma doc

Create `shipping.lemma`:

```lemma
doc shipping

type weight: scale
  -> unit kilogram 1.0
  -> unit gram 0.001

fact is_express: true
fact package_weight: 2.5 kilograms

rule express_fee: 0
  unless is_express then 4.99

rule base_shipping: 5.99
  unless package_weight > 1 kilogram  then  8.99
  unless package_weight > 5 kilograms then 15.99

rule total_cost: base_shipping? + express_fee?
```

Use spaces and tabs in `unless` expressions to align it like a table, making scanning the rule at a glance really easy.

**What this calculates:**
- Express fee: €0.00, unless `is_express` is true, then €4.99
- Base shipping: €5.99, but for packages that weigh 1-5kg it is €8.99, and for all packages >5kg it is €15.99
- Total cost: Base shipping plus express fee

As obvious as it looks, that is how Lemma encodes it.

Query it:

```bash
lemma run shipping
# Output:
# ┌───────────────┬──────────────────────────────────────────────────────┐
# │ Rule          ┆ Evaluation                                           │
# ╞═══════════════╪══════════════════════════════════════════════════════╡
# │ express_fee   ┆ 4.99                                             │
# │               ┆                                                      │
# │               ┆    0. fact is_express: true                         │
# │               ┆    1. unless clause 0 matched → 4.99             │
# │               ┆    2. result = 4.99                              │
# ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
# │ base_shipping ┆ 8.99                                             │
# │               ┆                                                      │
# │               ┆    0. fact package_weight: 2.5 kilograms             │
# │               ┆    1. greater_than(2.5 kilograms, 5 kilograms) → false │
# │               ┆    2. unless clause 1 skipped                        │
# │               ┆    3. fact package_weight: 2.5 kilograms             │
# │               ┆    4. greater_than(2.5 kilograms, 1 kilogram) → true  │
# │               ┆    5. unless clause 0 matched → 8.99             │
# │               ┆    6. result = 8.99                              │
# ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
# │ total_cost    ┆ 13.98                                            │
# │               ┆                                                      │
# │               ┆    0. rule base_shipping: 8.99                  │
# │               ┆    1. rule express_fee: 4.99                    │
# │               ┆    2. add(8.99, 4.99) → 13.98            │
# │               ┆    3. result = 13.98                             │
# └───────────────┴──────────────────────────────────────────────────────┘
```


## Key Features

### Rules with unless clauses

Rules start with a default value, then conditions override:

```lemma
doc pricing

fact quantity: [number]
fact is_vip  : false

rule discount: 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip         then 25%

rule price: 20 * quantity - discount?
```

**The last matching condition wins** - mirroring how business rules, legal documents, and standard operating procedures are written: "In principle X applies, unless [more specific condition] Y, unless [even more specific] Z..."

### Rich type system

Define custom types with units and constraints:

```lemma
doc type_examples

type money: scale
  -> unit eur 1.00
  -> unit usd 1.18
  -> decimals 2
  -> minimum 0

type mass: scale
  -> unit gram 1
  -> unit kilogram 1000
  -> unit pound 453.592

fact salary: 50_000 eur
fact workweek: 40 hours
fact vacation: 3 weeks
fact weight: 75 kilogram
fact tax_rate: 22%
fact deadline: 2024-12-31
```

**Primitive types:**
- `boolean` - true/false values
- `number` - dimensionless numeric values
- `scale` - numeric values that can have units
- `text` - string values
- `date` - ISO 8601 dates
- `time` - time values
- `duration` - time periods (hours, days, weeks, etc.)
- `ratio` - proportional values (percent, permille)

**User-Defined Types:**
Define custom types with units, constraints, and validation:

```lemma
doc unit_conversions

type money: scale
  -> unit eur 1.00
  -> unit usd 1.18

type length: scale
  -> unit meter 1.0
  -> unit kilometer 1000.0
  -> unit centimeter 0.01

type discount: ratio
  -> minimum 0
  -> maximum 1
```

Unit conversions work within the same type:

```lemma
doc unit_conversions

type money: scale
  -> unit eur 1.00
  -> unit usd 1.18

fact price: 100 eur

rule price_usd: price in usd
```

### Rule references

Compose complex logic from simple rules:

```lemma
doc driving_eligibility

type license_status: text
  -> option "valid"
  -> option "suspended"
  -> option "expired"

fact age              : [number]
fact license_status   : [license_status]
fact license_suspended: [boolean]

rule is_adult: age >= 18

rule has_license: license_status is "valid"

rule can_drive: is_adult? and has_license?
  unless license_suspended then veto "License suspended"
```

### Document composition


```lemma
doc employee
fact years_service: 8

doc leave_policy
fact senior_threshold: 5
fact base_leave_days : 25
fact bonus_leave_days: 5

doc leave_entitlement
fact employee    : doc employee
fact leave_policy: doc leave_policy

rule is_senior        : employee.years_service >= leave_policy.senior_threshold
rule annual_leave_days: leave_policy.base_leave_days
  unless is_senior? then leave_policy.base_leave_days + leave_policy.bonus_leave_days
```

### Veto for hard constraints

You should use types to constrain facts whenever possible. Sometimes though, you might need to consider multiple data points to validate a rule. This is where `veto` comes in. In the example below, we want to ensure that the review date is after the start date.

```lemma
doc performance_review

fact start_date       : [date]
fact review_date      : [date]
fact performance_score: [number -> minimum 0 -> maximum 100]

rule bonus_percentage: 0%
  unless performance_score >= 70    then 5%
  unless performance_score >= 90    then 10%
  unless review_date < start_date then veto "Review date must be after start date"
```

**Veto blocks the rule entirely**; there will not be any result.

## Documentation

- **[Language Guide](documentation/index.md)** - Complete language reference
- **[Reference](documentation/reference.md)** - All operators and types
- **[Examples](documentation/examples/)** - Example Lemma documents

[📚 View Full Documentation](documentation/)

## CLI Usage

```bash
# Run a document (evaluates all rules)
lemma run simple_facts

# Run specific rules only
lemma run tax_calculation:tax_owed

# Provide fact values
lemma run tax_calculation income=75000 filing_status="married"

# Interactive mode for exploring documents and facts
lemma run --interactive

# Machine-readable output (for scripts and tools)
lemma run pricing --raw

# Show document structure
lemma show pricing

# List all documents in workspace
lemma list

# List documents in specific directory
lemma list ./policies/

# Start HTTP server (workspace auto-detected)
lemma server --port 8012

# Start server with specific workspace
lemma server --dir ./policies --port 8012

# Start MCP server for AI assistant integration
lemma mcp
```

### HTTP Server

Start a server with your workspace pre-loaded:

```bash
lemma server --dir ./policies

# Evaluate a document (all rules) via query parameters
curl "http://localhost:8012/@pricing?quantity=10&is_member=true"

# Evaluate specific rules only
curl "http://localhost:8012/@pricing/discount,total?quantity=10"

# Evaluate via JSON body
curl -X POST http://localhost:8012/@pricing \
  -H "Content-Type: application/json" \
  -d '{"quantity": 10, "is_member": true}'
```

The server auto-generates typed REST endpoints for each loaded document. Meta routes:
- `GET /` — list all documents with their schemas
- `GET /openapi.json` — OpenAPI 3.1 specification
- `GET /docs` — interactive API documentation (Scalar)
- `GET /health` — health check

Use `--watch` to live-reload when `.lemma` files change:

```bash
lemma server --dir ./policies --watch
```

### MCP Server

The MCP (Model Context Protocol) server enables AI assistants to interact with Lemma docs programmatically, providing tools for doc creation, evaluation, and inspection.

### WebAssembly

Lemma also ships as a WebAssembly module (WASM), letting you evaluate rules directly in the browser or at the edge. This keeps latency low and data local. Install Lemma from NPM:

```bash
npm install @benrogmans/lemma-engine
```

See [WASM documentation](documentation/wasm.md) for usage examples.

### Docker

A minimal multi-architecture container image is published to the GitHub Container Registry on each release.

```bash
docker pull ghcr.io/benrogmans/lemma:latest
```

The image supports `linux/amd64` and `linux/arm64`. Docker automatically pulls the correct architecture.

**Run the CLI:**

```bash
docker run --rm ghcr.io/benrogmans/lemma --help
```

**Evaluate a document:**

Mount your workspace into the container's `/docs` directory:

```bash
docker run --rm -v "$(pwd):/docs" ghcr.io/benrogmans/lemma run shipping
```

**Deploy as an HTTP API:**

```bash
docker run -d -p 8012:8012 -v "$(pwd):/docs" ghcr.io/benrogmans/lemma \
  server --host 0.0.0.0 --port 8012
```

Then visit `http://localhost:8012/docs` for interactive API documentation.

**Docker Compose example:**

```yaml
services:
  lemma:
    image: ghcr.io/benrogmans/lemma:latest
    ports:
      - "8012:8012"
    volumes:
      - ./policies:/docs:ro
    command: ["server", "--host", "0.0.0.0", "--port", "8012", "--watch"]
```

## Status

Lemma is still in an early stage of development and is **not yet recommended for production use**. Expect breaking changes, incomplete features, and evolving semantics while the project matures.

## Project structure overview

```
├── cli/                    # CLI application (includes HTTP, MCP, interactive modes)
│   ├── src/
│   │   ├── main.rs         # CLI commands
│   │   ├── server.rs       # HTTP server (auto-generated REST API + OpenAPI)
│   │   ├── mcp.rs          # MCP (Model Context Protocol) server
│   │   ├── interactive.rs  # Interactive command helpers
│   │   └── formatter.rs    # Output formatting
│   └── tests/              # CLI integration tests
├── engine/                 # Core engine library
│   ├── src/
│   │   ├── parsing/        # Grammar (Pest) and AST
│   │   ├── planning/       # Validation, type resolution, execution plans
│   │   ├── evaluation/     # Expression evaluation pipeline
│   │   ├── computation/    # Arithmetic, comparison, datetime, units
│   │   ├── inversion/      # Inverse reasoning (find inputs for desired outputs)
│   │   ├── serialization/  # Output serializers (JSON, etc.)
│   │   └── ...             # Engine, error, registry, wasm modules
│   └── tests/              # Engine integration tests
├── openapi/                # Shared crate for Lemma-to-OpenAPI generation
├── documentation/          # Documentation & examples
│   ├── examples/           # Example .lemma files
│   └── *.md                # Guides, reference, roadmap, etc.
└── README.md               # This file
```


## Contributing

Contributions are very welcome! See [documentation/contributing.md](documentation/contributing.md) for setup and workflow, and check the [project roadmap](documentation/roadmap.md) for exciting features you can help shape.

## License

Apache 2.0 - see LICENSE file for details.

---

**[View on GitHub](https://github.com/benrogmans/lemma)** • **[Report Issue](https://github.com/benrogmans/lemma/issues)** • **[Documentation](documentation/index.md)** • **[Contributing](documentation/contributing.md)** • **[Roadmap](documentation/roadmap.md)** • **[WASM](documentation/wasm.md)**
