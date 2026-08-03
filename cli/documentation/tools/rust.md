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
use lemma::{DateTimeValue, Engine, SourceType};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

let mut engine = Engine::new();

engine.load([(
    SourceType::Path(Arc::new(PathBuf::from("example.lemma"))),
    r#"
    spec compensation
    data base_salary: 60000
    data bonus_rate: 10%
    rule bonus: base_salary * bonus_rate
    rule total: base_salary + bonus
"#
    .to_string(),
)])?;

let now = DateTimeValue::now();
let response = engine.run(
    None,
    "compensation",
    Some(&now),
    HashMap::new(),
    None,
    false,
)?;

for (rule_name, rule_result) in &response.results {
    if !rule_result.vetoed {
        println!("{rule_name}: {}", rule_result.display().unwrap_or(""));
    }
}
```

## Providing values at runtime

```rust
use lemma::{DateTimeValue, Engine, SourceType};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

let mut engine = Engine::new();

engine.load([(
    SourceType::Path(Arc::new(PathBuf::from("example.lemma"))),
    r#"
    spec shipping

    data weight: 5 kilogram
    data destination: "domestic"

    rule rate: 10
      unless weight > 10 kilogram           then 15
      unless destination is "international" then 25

    rule valid: weight <= 30 kilogram
      unless weight > 30 kilogram then veto "Package too heavy for shipping"
"#
    .to_string(),
)])?;

let mut values = HashMap::new();
values.insert("weight".to_string(), "12 kilogram".to_string());
values.insert("destination".to_string(), "international".to_string());

let now = DateTimeValue::now();
let response = engine.run(
    None,
    "shipping",
    Some(&now),
    values,
    None,
    false,
)?;
```

## Show vs run discovery

`Engine::show` returns the static planning interface: data used by the spec's rules, plus local rule result types.

For requirements on a partial run, call `run` and inspect each rule's `missing_data` (`string[]` input keys). Types, prefilled literals, and `-> suggest` hints are on `Engine::show` (`Show.data` values are `ShowData`) only. Bound inputs (caller run bindings or spec prefilled) are omitted from `missing_data`; suggestions do not bind until supplied in `run`'s data. Non-veto rule results flatten `RuleResultValue` onto each result (`display()` / typed fields). Pass `explain: true` as the last `run` argument to attach per-rule explanation trees ([api.v1.json](../schemas/api.v1.json)).

```rust
let response = engine.run(
    None,
    "shipping",
    Some(&now),
    HashMap::new(),
    Some(&["rate".to_string()]),
    false,
)?;

for key in &response.results["rate"].missing_data {
    println!("need: {key}");
}
```

## Embedded units stdlib

`Engine::new()` loads `repo lemma` / `spec units` at compile time (import with `uses lemma units`). It always appears in `Engine::list`. Formatted source: `engine.source(Some("lemma"), None, None)?`.

## API docs

Full Rust API documentation is published on [docs.rs](https://docs.rs/lemma-engine).
