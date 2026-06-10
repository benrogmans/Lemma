# Lemma Engine

> **A language that means business.**

Lemma Engine is the Rust crate behind the Lemma language. It lets you parse, validate, and evaluate Lemma docs from your own applications while keeping the same natural, auditable semantics that the CLI exposes.

## Status

Lemma is pre-1.0. The language and APIs are stable for most use cases, but breaking changes may occur between minor versions. Pin your dependency version and review the [changelog](https://github.com/lemma/lemma/blob/main/CHANGELOG.md) before upgrading.

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
lemma-engine = "0.8.18"
```

### Minimal example

```rust
use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, SourceType};
use std::collections::HashMap;

let mut engine = Engine::new();

engine.load(
    r#"
    spec compensation
    data base_salary: 60000
    data bonus_rate: 10%
    rule bonus: base_salary * bonus_rate
    rule total: base_salary + bonus
"#,
    SourceType::Labeled("example.lemma"),
)?;

let now = DateTimeValue::now();
let response = engine.run(
    None,
    "compensation",
    Some(&now),
    HashMap::new(),
    false,
)?;

for (rule_name, rule_result) in &response.results {
    if !rule_result.vetoed {
        println!("{rule_name}: {}", rule_result.display.as_deref().unwrap_or(""));
    }
}
```

### Providing values at runtime

```rust
use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, SourceType};
use std::collections::HashMap;

let mut engine = Engine::new();

engine.load(
    r#"
    spec shipping

    data weight: 5 kilogram
    data destination: "domestic"

    rule rate: 10
      unless weight > 10 kilogram           then 15
      unless destination is "international" then 25

    rule valid: weight <= 30 kilogram
      unless weight > 30 kilogram then veto "Package too heavy for shipping"

"#,
    SourceType::Labeled("example.lemma"),
)?;

let mut values = HashMap::new();
values.insert("weight".to_string(), "12 kilogram".to_string());
values.insert("destination".to_string(), "international".to_string());

let now = DateTimeValue::now();
let response = engine.run(
    None,
    "shipping",
    Some(&now),
    values,
    false,
)?;
```

## Embedded units stdlib

`Engine::new()` loads `repo lemma` / `spec units` from [`src/lemma/units.lemma.std`](src/lemma/units.lemma.std) at compile time (import with `uses lemma units`). The `.lemma.std` suffix keeps workspace discovery from loading it as a user spec. It always appears in [`Engine::list`](engine/src/engine.rs). Inspect formatted source with `engine.format_repository("lemma")`.

## Features

- **Rich type system** – percentages, mass, length, duration, temperature, pressure, power, energy, frequency, and data sizes
- **Automatic unit conversions** – convert between units inside expressions without extra code
- **Page composition** – extend specs, bind data, and reuse rules across modules
- **Audit trail** – every evaluation returns the operations that led to each result
- **WebAssembly build** – `npm install @lemmabase/lemma-engine` to run Lemma in browsers and at the edge

Constraint-style **inversion** (what inputs would yield a given outcome?) is planned; it is not documented as a supported API yet.

## Installation options

### As a library

```bash
cargo add lemma-engine
```

### CLI tool

```bash
cargo install lemma
lemma run pricing quantity=10
```

### HTTP server

```bash
cargo install lemma
lemma server --port 8012
```

### WebAssembly

```bash
npm install @lemmabase/lemma-engine
```

```javascript
import { Lemma } from '@lemmabase/lemma-engine';
const engine = await Lemma();
```

Build: `node build.js` (from `engine/packages/npm/`). See [packages/npm/README.md](packages/npm/README.md).

## Documentation

- Language guide: <https://github.com/lemma/lemma/blob/main/documentation/index.md>
- API documentation: <https://docs.rs/lemma-engine>
- Examples: <https://github.com/lemma/lemma/tree/main/documentation/examples>
- CLI usage: <https://github.com/lemma/lemma/blob/main/documentation/CLI.md>

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
