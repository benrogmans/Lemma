# Lemma

[![CI](https://github.com/benrogmans/lemma/workflows/CI/badge.svg)](https://github.com/benrogmans/lemma/actions/workflows/ci.yml)
[![DeepSource](https://app.deepsource.com/gh/benrogmans/lemma.svg/?label=active+issues&show_trend=true)](https://app.deepsource.com/gh/benrogmans/lemma/)
[![Crates.io](https://img.shields.io/crates/v/lemma-engine.svg)](https://crates.io/crates/lemma-engine)
[![Documentation](https://docs.rs/lemma-engine/badge.svg)](https://docs.rs/lemma-engine)
[![License](https://img.shields.io/crates/l/lemma.svg)](LICENSE)

> **Rules for man and machine**

Lemma is a declarative programming language for business logic. It reads like English but evaluates like code. Write business rules, contracts, and policies that both humans and computers can understand.

```lemma
doc pricing

fact quantity   = [number]
fact is_vip     = false

rule discount = 0%
  unless quantity >= 10 then 10%
  unless quantity >= 50 then 20%
  unless is_vip         then 25%

rule price = 200 eur - discount?
```

The `200 eur - discount?` expression automatically applies percentage semantics, resulting in `180 eur` when discount is `10%`.

## Why Lemma?

- **Natural syntax** - Reads like plain English, no cryptic symbols
- **Type-safe** - Built-in support for money, dates, durations, units with automatic conversions
- **Declarative** - Describe what, not how
- **Composable** - Documents reference and extend each other
- **Auditable** - Every decision has a clear audit trail
- **Pure Rust** - Fast, deterministic execution without external dependencies

## Quick Start

### Installation

```bash
cargo install lemma-cli
```

### Your First Rule

Create `hello.lemma`:

```lemma
doc hello

fact name = "World"
fact age = 25

rule greeting = "Hello, " + name + "!"

rule can_vote = false
  unless age >= 18 then true
```

Query it:

```bash
lemma run hello -d .
# Output: Shows all rules in the hello document with operation records
```

### Real-World Example

```lemma
doc tax_policy

fact income = 75000 USD
fact filing_status = "single"

rule standard_deduction = 13850 USD
  unless filing_status == "married" then 27700 USD

rule taxable_income = income - standard_deduction?

rule tax_owed = 0 USD
  unless taxable_income? > 11000 USD then (taxable_income? - 11000 USD) * 10%
  unless taxable_income? > 44725 USD then 3372.50 USD + (taxable_income? - 44725 USD) * 12%
  unless taxable_income? > 95375 USD then 9875 USD + (taxable_income? - 95375 USD) * 22%
```

## Key Features

### Unless Clauses

Rules start with a default value, then conditions override:

```lemma
rule discount = 0%
  unless quantity > 10 then 10%
  unless quantity > 50 then 20%
  unless is_premium_member then 25%

rule price = base_price * (1 - discount)
```

**The last matching condition wins** - just like natural language!

### Rich Type System

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

### Rule References

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

### Document Composition

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

### Veto for Hard Constraints

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

**Veto blocks the rule entirely** (no valid result), while `reject` just returns `false`.

## Documentation

- **[Language Guide](docs/index.md)** - Complete language reference
- **[Reference](docs/reference.md)** - All operators, units, and types
- **[Examples](docs/examples/)** - 10 comprehensive examples

[📚 View Full Documentation](docs/)

## CLI Usage

```bash
# Run a document (evaluates all rules)
lemma run document_name -d .

# Override facts
lemma run document_name -d . age=25 income="50000 USD"

# Show document structure
lemma show document_name -d .

# List all documents in workspace
lemma list ./policies/

# Start HTTP server
lemma serve -d ./policies --port 3000
```

### HTTP Server

Start a server with your workspace pre-loaded:

```bash
lemma serve -d ./policies

# Evaluate with inline code
curl -X POST http://localhost:3000/evaluate \
  -H "Content-Type: application/json" \
  -d '{
    "code": "doc calc\nfact x = 10\nrule double = x * 2",
    "facts": {"x": 25}
  }'
```

## Project Structure

```
lemma/
├── cli/                # CLI application (includes HTTP server)
│   └── src/
│       ├── main.rs     # CLI commands
│       ├── server.rs   # HTTP server module
│       └── formatter.rs
├── lemma/              # Core library
└── docs/               # Documentation & examples
    ├── examples/       # Example .lemma files
    └── *.md            # Detailed guides
```

## Use Cases

- **Business Rules** - Pricing, eligibility, validation logic
- **Contracts** - Legal terms and conditions
- **Policies** - HR, compliance, governance rules
- **Configuration** - Complex conditional settings
- **Tax & Finance** - Progressive calculations, brackets
- **Logistics** - Shipping rules, routing decisions

## Implementation

Lemma is implemented in pure Rust, providing:

- **Pure Rust evaluator** - Fast, deterministic execution with no external dependencies
- **Type-aware operations** - Automatic type checking and semantic conversions
- **Composability** - Rules reference other rules with automatic dependency resolution
- **Rich type system** - Built-in units with automatic conversions
- **Operation tracking** - Complete audit trail of every evaluation step
- **HTTP Server** - REST API with workspace pre-loading
- **WebAssembly support** - Run in browsers and edge environments

## Contributing

Contributions welcome! See [docs/contributing.md](docs/contributing.md) for setup and workflow.

## WebAssembly

Use in browsers via NPM:

```bash
npm install @benrogmans/lemma-engine
```

See [WASM documentation](docs/wasm.md) for usage examples.

## License

Apache 2.0 - see LICENSE file for details.

---

**[View on GitHub](https://github.com/benrogmans/lemma)** • **[Report Issue](https://github.com/benrogmans/lemma/issues)** • **[Documentation](docs/index.md)** • **[Contributing](docs/contributing.md)**
