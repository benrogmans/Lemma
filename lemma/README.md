# Lemma Engine

**A programming language that means business**

Lemma is a declarative logic language for expressing business rules, contracts, and policies in a way that's both human-readable and machine-executable. Write rules once, audit them easily, and execute them reliably.

## Why Lemma?

- **Human-readable** - Business logic that looks like the contracts and policies you already write
- **Type-safe** - Built-in support for money (usd, eur), mass (kilograms, pounds), length (meters, feet), time (hours, days), percentages, and dates
- **Auditable** - Every evaluation provides a complete operation trail showing how decisions were made
- **Composable** - Reference other documents and rules to build complex logic from simple pieces
- **Multi-platform** - Rust library, CLI tool, HTTP server, WebAssembly for browsers, and Model Context Protocol support

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
lemma-engine = "0.6"
```

### Simple Example

```rust
use lemma::Engine;

let mut engine = Engine::new();

engine.add_lemma_code(r#"
    doc compensation
    fact base_salary = 60000 USD
    fact bonus_rate = 10%
    rule bonus = base_salary * bonus_rate
    rule total = base_salary + bonus?
"#, "compensation.lemma")?;

let response = engine.evaluate("compensation", None, None)?;

for result in response.results {
    println!("{}: {}", result.rule_name, result.result.unwrap());
}
// Output:
// bonus: 6000 USD
// total: 66000 USD
```

### Business Rules with Conditionals

```rust
use lemma::{Engine, parse_facts};

let mut engine = Engine::new();

engine.add_lemma_code(r#"
    doc shipping
    fact weight = 5 kilogram
    fact destination = "domestic"
    
    rule rate = 10 USD
        unless weight > 10 kilogram then 15 USD
        unless destination = "international" then 25 USD
    
    rule valid = weight <= 30 kilogram
        unless veto "Package too heavy for shipping"
"#, "shipping.lemma")?;

// Override facts at runtime
let facts = parse_facts(&["weight=12 kilogram"])?;
let response = engine.evaluate("shipping", None, Some(facts))?;
```

### JSON Integration

```rust
use lemma::{Engine, serializers};

let mut engine = Engine::new();
engine.add_lemma_code(r#"
    doc pricing
    fact base_price = 100 USD
    fact discount = 15%
    rule final_price = base_price * (1 - discount)
"#, "pricing.lemma")?;

// Load facts from JSON
let json = r#"{"base_price": "150 USD", "discount": 0.2}"#;
let doc = engine.get_document("pricing").unwrap();
let facts_str = serializers::from_json(json.as_bytes(), doc, engine.get_all_documents())?;
let facts = lemma::parse_facts(&facts_str.iter().map(|s| s.as_str()).collect::<Vec<_>>())?;

let response = engine.evaluate("pricing", None, Some(facts))?;
```

## Features

### Type System

Built-in support for business-critical types:
- **Money**: `100 USD`, `50 EUR`, automatic currency validation
- **Percentages**: `15%`, `0.5%`, works with unit arithmetic
- **Mass**: `10 kilogram`, `5 pound`, automatic conversions
- **Length**: `100 meter`, `5 feet`, `10 mile`
- **Time**: `2 hour`, `30 minute`, `1 day`
- **Dates**: `2024-12-25`, `2024-12-25T14:30:00Z`, date arithmetic
- **And more**: Volume, temperature, pressure, power, energy, frequency, data sizes

### Document Composition

```rust
engine.add_lemma_code(r#"
    doc employee
    fact name = "Alice"
    fact salary = 70000 USD
    
    doc benefits extends employee
    fact health_coverage = 5000 USD
    rule total_compensation = salary + health_coverage
"#, "benefits.lemma")?;
```

### Audit Trail

Every evaluation returns complete operation records showing how each result was computed:

```rust
let response = engine.evaluate("pricing", None, None)?;
for result in &response.results {
    for operation in &result.operations {
        println!("{}", operation);  // Full trace of computation
    }
}
```

## Installation Options

### As a Library

```bash
cargo add lemma-engine
```

### CLI Tool

```bash
cargo install lemma-cli
```

Then run:

```bash
lemma run examples/pricing quantity=10
lemma inspect examples/pricing
```

### WebAssembly

```bash
npm install @benrogmans/lemma-engine
```

### HTTP Server

```bash
cargo install lemma-cli
lemma server --port 8080
```

## Documentation

- **Language Guide**: [https://benrogmans.github.io/lemma/](https://benrogmans.github.io/lemma/)
- **API Documentation**: [https://docs.rs/lemma-engine](https://docs.rs/lemma-engine)
- **Examples**: [GitHub examples directory](https://github.com/benrogmans/lemma/tree/main/docs/examples)
- **CLI Guide**: [CLI.md](https://github.com/benrogmans/lemma/blob/main/docs/CLI.md)

## Use Cases

- Employment contracts and compensation calculations
- Shipping and pricing policies
- Tax calculations
- Insurance premium calculations
- Discount and promotional rules
- Compliance and validation rules
- SLA and service level calculations

## License

Apache 2.0

## Contributing

Contributions welcome! See [contributing.md](https://github.com/benrogmans/lemma/blob/main/docs/contributing.md)
