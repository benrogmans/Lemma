# Lemma Engine

> **A language that means business.**

Lemma Engine is the Rust crate behind the Lemma language. It lets you parse, validate, and evaluate Lemma docs from your own applications while keeping the same natural, auditable semantics that the CLI exposes.

## Status

Lemma is still early-stage and **not yet recommended for production use**. Expect breaking changes, evolving semantics, and incomplete tooling while the project matures.

## Why Lemma?

- **Readable by business stakeholders** – rules look like the policies people already write
- **Deterministic and auditable** – every evaluation returns a full trace explaining the result
- **Type-aware** – money, dates, percentages, units, and automatic conversions are first-class
- **Composable** – documents extend and reference each other without boilerplate
- **Multi-platform** – use the engine from Rust, power the CLI/HTTP server, or ship via WebAssembly

## Quick start

Add the crate:

```toml
[dependencies]
lemma-engine = "0.6"
```

### Minimal example

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
    if let Some(value) = result.result {
        println!("{}: {}", result.rule_name, value);
    }
}
```

### Overriding facts at runtime

```rust
use lemma::{Engine, parse_facts};

let mut engine = Engine::new();

engine.add_lemma_code(r#"
    doc shipping

    fact weight = 5 kilogram
    fact destination = "domestic"

    rule rate = 10 USD
      unless weight > 10 kilogram           then 15 USD
      unless destination is "international" then 25 USD

    rule valid = weight <= 30 kilogram
      unless veto "Package too heavy for shipping"

"#, "shipping.lemma")?;

let overrides = parse_facts(&["weight=12 kilogram"])?;
let response = engine.evaluate("shipping", None, Some(overrides))?;
```

### Working with JSON

```rust
use lemma::{Engine, serializers};

let mut engine = Engine::new();
engine.add_lemma_code(r#"
    doc pricing

    fact base_price = 100 USD
    fact discount = 15%

    rule final_price = base_price - discount

"#, "pricing.lemma")?;

let json = br#"{"base_price": "150 USD", "discount": 0.2}"#;
let doc = engine.get_document("pricing").unwrap();
let facts_raw = serializers::from_json(json, doc, engine.get_all_documents())?;
let overrides = lemma::parse_facts(&facts_raw.iter().map(|s| s.as_str()).collect::<Vec<_>>())?;

let response = engine.evaluate("pricing", None, Some(overrides))?;
```

## Features

- **Rich type system** – money, percentages, mass, length, duration, temperature, pressure, power, energy, frequency, and data sizes
- **Automatic unit conversions** – convert between units inside expressions without extra code
- **Document composition** – extend documents, override facts, and reuse rules across modules
- **Audit trail** – every evaluation returns the operations that led to each result
- **WebAssembly build** – `npm install @benrogmans/lemma-engine` to run Lemma in browsers and at the edge

## Installation options

### As a library

```bash
cargo add lemma-engine
```

### CLI tool

```bash
cargo install lemma-cli
lemma run examples/pricing quantity=10
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
