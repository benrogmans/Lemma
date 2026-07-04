---
nav_title: Rust
nav_order: 10
---

# Rust

`lemma-engine` is the engine itself. Crate name `lemma-engine`, imported as `lemma`.

## Install

```bash
cargo add lemma-engine --rename lemma
```

## Usage

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

## Providing values at runtime

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

`Engine::new()` loads `repo lemma` / `spec units` at compile time (import with `uses lemma units`). It always appears in `Engine::list`. Inspect formatted source with `engine.format_repository("lemma")`.

## API docs

Full Rust API documentation is published on [docs.rs](https://docs.rs/lemma-engine).
