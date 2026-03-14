# Lemma Engine

> **A language that means business.**

Lemma Engine is the Rust crate behind the Lemma language. It lets you parse, validate, and evaluate Lemma docs from your own applications while keeping the same natural, auditable semantics that the CLI exposes.

## Status

Lemma is still early-stage and **not yet recommended for production use**. Expect breaking changes, evolving semantics, and incomplete tooling while the project matures.

## Why Lemma?

- **Readable by business stakeholders** – rules look like the policies people already write
- **Deterministic and auditable** – every evaluation returns a full trace explaining the result
- **Type-aware** – dates, percentages, units, and automatic conversions are first-class
- **Composable** – specs extend and reference each other without boilerplate
- **Multi-platform** – use the engine from Rust, power the CLI/HTTP server, or ship via WebAssembly

## Quick start

Add the crate:

```toml
[dependencies]
lemma-engine = "0.8.3"
```

### Minimal example

```rust
use lemma::Engine;
use std::collections::HashMap;

let mut engine = Engine::new();

engine.add_lemma_files(HashMap::from([("example.lemma".into(), r#"
    spec compensation
    fact base_salary: 60000
    fact bonus_rate: 10%
    rule bonus: base_salary * bonus_rate
    rule total: base_salary + bonus
"#.into())]))?;

let response = engine.evaluate("compensation", vec![], HashMap::new())?;

for result in response.results {
    if let Some(value) = result.result {
        println!("{}: {}", result.rule_name, value);
    }
}
```

### Providing values at runtime

```rust
use lemma::Engine;
use std::collections::HashMap;

let mut engine = Engine::new();

engine.add_lemma_files(HashMap::from([("example.lemma".into(), r#"
    spec shipping

    fact weight: 5 kilogram
    fact destination: "domestic"

    rule rate: 10
      unless weight > 10 kilogram           then 15
      unless destination is "international" then 25

    rule valid: weight <= 30 kilogram
      unless veto "Package too heavy for shipping"

"#.into())]))?;

let mut values = HashMap::new();
values.insert("weight".to_string(), "12 kilogram".to_string());
values.insert("destination".to_string(), "international".to_string());

let response = engine.evaluate("shipping", vec![], values)?;
```

### Inverse reasoning

Inversion allows you to find what input values produce a desired output. This is useful for questions like "What quantity gives me a 30% discount?" or "What salary produces a total compensation of €100,000?"

#### Basic example

```rust
use lemma::{Engine, Target, LiteralValue};
use std::collections::HashMap;
use rust_decimal::Decimal;

let mut engine = Engine::new();

engine.add_lemma_files(HashMap::from([("example.lemma".into(), r#"
    spec pricing
    fact quantity: [number]
    fact is_vip: false

    rule discount: 0%
      unless quantity >= 10 then 10%
      unless quantity >= 50 then 20%
      unless is_vip then 25%
"#.into())]))?;

// Find what quantity gives a 30% discount
use rust_decimal::Decimal;
let response = engine.invert(
    "pricing",
    "discount",
    Target::value(LiteralValue::Percentage(Decimal::from(30))),
    HashMap::new()
)?;

// Response contains solutions showing: is_vip must be true
```

#### API variants

**1. `invert()` - String-based values (user-friendly)**

Accepts string values that are automatically parsed based on spec types:

```rust
let mut values = HashMap::new();
values.insert("is_vip".to_string(), "true".to_string());

let response = engine.invert(
    "pricing",
    "discount",
    Target::value(LiteralValue::Percentage(Decimal::from(25))),
    values
)?;
```

**2. `invert_json()` - JSON input (convenience)**

Accepts JSON bytes directly:

```rust
let json = br#"{"is_vip": true}"#;

let response = engine.invert_json(
    "pricing",
    "discount",
    Target::value(LiteralValue::Percentage(Decimal::from(25))),
    json
)?;
```

#### Target specification

Use `Target` to specify the desired outcome:

```rust
use lemma::{Target, TargetOp, OperationResult};

// Exact value (equality)
Target::value(LiteralValue::Percentage(Decimal::from(30)))

// Comparison operators
Target::with_op(
    TargetOp::Gt,
    OperationResult::Value(LiteralValue::number(100))
)  // > 100

Target::with_op(
    TargetOp::Lte,
    OperationResult::Value(LiteralValue::number(50))
)  // <= 50

// Find any veto
Target::any_veto()

// Find specific veto message
Target::veto(Some("Invalid input".to_string()))
```

#### Response structure

`InversionResponse` contains:

- **`solutions`**: Concrete domain constraints for each free variable
- **`shape`**: Symbolic representation of the solution space (piecewise function)
- **`free_variables`**: Facts that are not fully determined
- **`is_fully_constrained`**: Whether all facts have concrete values

```rust
let response = engine.invert(...)?;

if response.is_fully_constrained {
    println!("All variables are determined");
} else {
    println!("Free variables: {:?}", response.free_variables);
}

for (var, domain) in &response.solutions {
    println!("{}: {:?}", var, domain);
}
```

## Features

- **Rich type system** – percentages, mass, length, duration, temperature, pressure, power, energy, frequency, and data sizes
- **Automatic unit conversions** – convert between units inside expressions without extra code
- **Spec composition** – extend specs, bind facts, and reuse rules across modules
- **Audit trail** – every evaluation returns the operations that led to each result
- **Inverse reasoning** – find what inputs produce desired outputs
- **WebAssembly build** – `npm install @benrogmans/lemma-engine` to run Lemma in browsers and at the edge

## Installation options

### As a library

```bash
cargo add lemma-engine
```

### CLI tool

```bash
cargo install lemma-cli
lemma run pricing quantity=10
```

### HTTP server

```bash
cargo install lemma-cli
lemma server --port 8080
```

### WebAssembly

```bash
npm install @benrogmans/lemma-engine
```

## Documentation

- Language guide: <https://benrogmans.github.io/lemma/>
- API documentation: <https://docs.rs/lemma-engine>
- Examples: <https://github.com/benrogmans/lemma/tree/main/documentation/examples>
- CLI usage: <https://github.com/benrogmans/lemma/blob/main/documentation/CLI.md>
- Roadmap: <https://github.com/benrogmans/lemma/blob/main/documentation/roadmap.md>

## Use cases

- Compensation plans and employment contracts
- Pricing, shipping, and discount policies
- Tax and finance calculations
- Insurance eligibility and premium rules
- Compliance and validation logic
- SLA and service-level calculations

## Contributing

Contributions are very welcome! See [documentation/contributing.md](https://github.com/benrogmans/lemma/blob/main/documentation/contributing.md) and the [project roadmap](https://github.com/benrogmans/lemma/blob/main/documentation/roadmap.md) for ideas.

## License

Apache 2.0
