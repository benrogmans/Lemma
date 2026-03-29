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
lemma-engine = "0.8.9"
```

### Minimal example

```rust
use lemma::Engine;
use lemma::parsing::ast::DateTimeValue;
use std::collections::HashMap;

let mut engine = Engine::new();

engine.load(r#"
    spec compensation
    fact base_salary: 60000
    fact bonus_rate: 10%
    rule bonus: base_salary * bonus_rate
    rule total: base_salary + bonus
"#, Some("example.lemma"))?;

let now = DateTimeValue::now();
let response = engine.run("compensation", Some(&now), HashMap::new())?;

for result in response.results {
    if let Some(value) = result.result {
        println!("{}: {}", result.rule_name, value);
    }
}
```

### Providing values at runtime

```rust
use lemma::Engine;
use lemma::parsing::ast::DateTimeValue;
use std::collections::HashMap;

let mut engine = Engine::new();

engine.load(r#"
    spec shipping

    fact weight: 5 kilogram
    fact destination: "domestic"

    rule rate: 10
      unless weight > 10 kilogram           then 15
      unless destination is "international" then 25

    rule valid: weight <= 30 kilogram
      unless veto "Package too heavy for shipping"

"#, Some("example.lemma"))?;

let mut values = HashMap::new();
values.insert("weight".to_string(), "12 kilogram".to_string());
values.insert("destination".to_string(), "international".to_string());

let now = DateTimeValue::now();
let response = engine.run("shipping", Some(&now), values)?;
```

## Features

- **Rich type system** – percentages, mass, length, duration, temperature, pressure, power, energy, frequency, and data sizes
- **Automatic unit conversions** – convert between units inside expressions without extra code
- **Spec composition** – extend specs, bind facts, and reuse rules across modules
- **Audit trail** – every evaluation returns the operations that led to each result
- **WebAssembly build** – `npm install @benrogmans/lemma-engine` to run Lemma in browsers and at the edge

Constraint-style **inversion** (what inputs would yield a given outcome?) is planned; it is not documented as a supported API yet.

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
lemma server --port 8012
```

### WebAssembly

```bash
npm install @benrogmans/lemma-engine
```

```javascript
import { Lemma } from '@benrogmans/lemma-engine';
const engine = await Lemma();
```

Build: `node build.js` (from `engine/packages/npm/`). See [packages/npm/README.md](packages/npm/README.md).

## Documentation

- Language guide: <https://benrogmans.github.io/lemma/>
- API documentation: <https://docs.rs/lemma-engine>
- Examples: <https://github.com/benrogmans/lemma/tree/main/documentation/examples>
- CLI usage: <https://github.com/benrogmans/lemma/blob/main/documentation/CLI.md>

## Use cases

- Compensation plans and employment contracts
- Pricing, shipping, and discount policies
- Tax and finance calculations
- Insurance eligibility and premium rules
- Compliance and validation logic
- SLA and service-level calculations

## Contributing

Contributions are very welcome!

## License

Apache 2.0
