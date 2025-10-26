# Lemma

[![CI](https://github.com/benrogmans/lemma/workflows/CI/badge.svg)](https://github.com/benrogmans/lemma/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/lemma-engine.svg)](https://crates.io/crates/lemma-engine)
[![Documentation](https://docs.rs/lemma-engine/badge.svg)](https://docs.rs/lemma-engine)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

> **A language that means business.**

Lemma is a declarative language designed specifically for expressing business logic. Lemma docs flow like natural language and encode pricing rules, tax calculations, eligibility criteria, contracts, and policies. Business stakeholders can read and validate them, while software systems can enforce and automate them.

```lemma
doc pricing

fact quantity = [number]
fact is_vip   = false

rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip         then 25%

rule price = quantity * 20 eur - discount?
```

Note how Lemma automatically deducts the discount percentage in the expression `quantity * 20 eur - discount?`.

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

fact is_express = true
fact package_weight = 2.5 kilograms

rule express_fee = 0 USD
  unless is_express then 4.99 USD

rule base_shipping = 5.99 USD
  unless package_weight > 1 kilogram  then  8.99 USD
  unless package_weight > 5 kilograms then 15.99 USD

rule total_cost = base_shipping + express_fee
```

Use spaces and tabs in `unless` expressions to align it like a table, making scanning the rule at a glance really easy.

**What this calculates:**
- Express fee: $0.00 USD, unless `is_express` is true, then $4.99
- Base shipping: $5.99, but for packages that weigh 1-5kg it is $8.99, and for all packages >5kg it is $15.99
- Total cost: Base shipping plus express fee

As obvious as it looks, that is how Lemma encodes it.

Query it:

```bash
lemma run shipping
# Output:
# ┌───────────────┬──────────────────────────────────────────────────────┐
# │ Rule          ┆ Evaluation                                           │
# ╞═══════════════╪══════════════════════════════════════════════════════╡
# │ express_fee   ┆ 4.99 USD                                             │
# │               ┆                                                      │
# │               ┆    0. fact is_express = true                         │
# │               ┆    1. unless clause 0 matched → 4.99 USD             │
# │               ┆    2. result = 4.99 USD                              │
# ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
# │ base_shipping ┆ 8.99 USD                                             │
# │               ┆                                                      │
# │               ┆    0. fact package_weight = 2.5 kilogram             │
# │               ┆    1. greater_than(2.5 kilogram, 5 kilogram) → false │
# │               ┆    2. unless clause 1 skipped                        │
# │               ┆    3. fact package_weight = 2.5 kilogram             │
# │               ┆    4. greater_than(2.5 kilogram, 1 kilogram) → true  │
# │               ┆    5. unless clause 0 matched → 8.99 USD             │
# │               ┆    6. result = 8.99 USD                              │
# ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
# │ total_cost    ┆ 13.98 USD                                            │
# │               ┆                                                      │
# │               ┆    0. rule base_shipping = 8.99 USD                  │
# │               ┆    1. rule express_fee = 4.99 USD                    │
# │               ┆    2. add(8.99 USD, 4.99 USD) → 13.98 USD            │
# │               ┆    3. result = 13.98 USD                             │
# └───────────────┴──────────────────────────────────────────────────────┘
```


## Key Features

### Rules with unless clauses

Rules start with a default value, then conditions override:

```lemma
rule discount = 0%
  unless quantity > 10 then 10%
  unless quantity > 50 then 20%
  unless is_premium_member then 25%

rule price = base_price * (1 - discount)
```

**The last matching condition wins** - mirroring how business rules, legal documents, and standard operating procedures are written: "In principle X applies, unless [more specific condition] Y, unless [even more specific] Z..."

### Rich type system

```lemma
fact salary = 50000 USD
fact workweek = 40 hours
fact vacation = 3 weeks
fact weight = 75 kilograms
fact tax_rate = 22%
fact deadline = 2024-12-31
fact pattern = /[A-Z]{3}-\d{4}/
```

**Supported Types:**
- **Basic**: `text`, `number`, `boolean`, `date`, `percentage`, `regex`
- **Units**: `mass` (kilograms, grams, pounds, ounces), `length` (meters, kilometers, feet, inches, miles), `volume` (liters, gallons, cubic meters), `duration` (seconds, minutes, hours, days, weeks, months, years), `temperature` (celsius, fahrenheit, kelvin)
- **Advanced**: `power` (watts, kilowatts, megawatts, horsepower), `energy` (joules, kilojoules, kilowatt-hours, calories), `force` (newtons, kilonewtons, pound-force), `pressure` (pascals, kilopascals, atmospheres, bars, psi), `frequency` (hertz, kilohertz, megahertz, gigahertz), `data_size` (bytes, kilobytes, megabytes, gigabytes, terabytes)
- **Money**: `USD`, `EUR`, `GBP`, `JPY`, `CNY`, `CHF`, `CAD`, `AUD`, `INR`

Automatic unit conversions:

```lemma
doc conversions

fact weight = 75 kilograms
fact distance = 10 kilometers
fact temperature = 25 celsius

rule weight_in_pounds = weight in pounds
rule distance_in_miles = distance in miles
rule temperature_f = temperature in fahrenheit
```

### Rule references

Compose complex logic from simple rules:

```lemma
doc driving_eligibility

fact age = 25
fact license_status = "valid"
fact license_suspended = false

rule is_adult = age >= 18

rule has_license = license_status == "valid"

rule can_drive = is_adult? and has_license?
  unless license_suspended? then veto "License suspended"
```

### Document composition

```lemma
doc employee
fact base_salary = 60000 USD
fact years_service = 5

doc manager
fact base_salary = 80000 USD

doc bonus_policy
fact bonus_rate = 10%

doc calculations
rule employee_bonus = employee.base_salary * bonus_policy.bonus_rate
rule manager_bonus = manager.base_salary * bonus_policy.bonus_rate
```

### Veto for hard constraints

```lemma
doc loan_approval

fact credit_score = 650
fact age = 25
fact bankruptcy_flag = false

rule loan_approval = reject
  unless credit_score >= 600 then accept
  unless age < 18 then veto "Must be 18 or older"
  unless bankruptcy_flag then veto "Cannot approve due to bankruptcy"
```

**Veto blocks the rule entirely**; there will not be any result.

## Documentation

- **[Language Guide](documentation/index.md)** - Complete language reference
- **[Reference](documentation/reference.md)** - All operators, units, and types
- **[Examples](documentation/examples/)** - 10 comprehensive examples

[📚 View Full Documentation](documentation/)

## CLI Usage

```bash
# Run a document (evaluates all rules)
lemma run examples/simple_facts

# Run specific rules only
lemma run examples/tax_calculation:tax_owed

# Override facts at runtime
lemma run examples/tax_calculation income=75000 filing_status="married"

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
lemma server --port 3000

# Start server with specific workspace
lemma server --dir ./policies --port 3000

# Start MCP server for AI assistant integration
lemma mcp
```

### HTTP Server

Start a server with your workspace pre-loaded:

```bash
lemma server --dir ./policies

# Evaluate with inline code
curl -X POST http://localhost:3000/evaluate \
  -H "Content-Type: application/json" \
  -d '{
    "code": "doc calc\nfact x = 10\nrule double = x * 2",
    "facts": {"x": 25}
  }'
```

The server provides endpoints for doc evaluation, fact inspection, and rule validation.

### MCP Server

The MCP (Model Context Protocol) server enables AI assistants to interact with Lemma docs programmatically, providing tools for doc creation, evaluation, and inspection.

### WebAssembly

Lemma also ships as a WebAssembly module (WASM), letting you evaluate rules directly in the browser or at the edge. This keeps latency low and data local. Install Lemma from NPM:

```bash
npm install @benrogmans/lemma-engine
```

See [WASM documentation](documentation/wasm.md) for usage examples.


## Status

Lemma is still in an early stage of development and is **not yet recommended for production use**. Expect breaking changes, incomplete features, and evolving semantics while the project matures.

## Project structure overview

```
lemma/
├── cli/                    # CLI application (includes HTTP, MCP, interactive modes)
│   ├── src/
│   │   ├── main.rs         # CLI commands
│   │   ├── server.rs       # HTTP server module
│   │   ├── mcp.rs          # MCP (Model Context Protocol) server
│   │   ├── interactive.rs  # Interactive command helpers
│   │   └── formatter.rs
│   └── tests/
│       └── cli_integration_test.rs
├── lemma/                  # Core engine library
│   ├── src/
│   │   ├── parser/         # Grammar and parsing logic
│   │   ├── evaluator/      # Evaluation pipeline
│   │   ├── serializers/    # Output serializers (JSON, etc.)
│   │   └── ...             # Engine modules (analysis, validator, wasm, tests)
│   └── tests/              # Engine integration tests
├── documentation/                   # Documentation & examples
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
