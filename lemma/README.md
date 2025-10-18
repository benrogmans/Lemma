# Lemma Engine

**Logic for man and machine**

A declarative logic language engine for expressing rules, facts, and business logic in a way that is both human-readable and machine-executable.

## Features

- **Declarative syntax** - Express rules naturally
- **Type-safe units** - Built-in support for money, mass, length, time, etc.
- **Rule composition** - Reference other rules and documents
- **Conditional logic** - Unless clauses for business rules
- **Date arithmetic** - Native datetime operations
- **WebAssembly** - Run in the browser

## Quick Start

```rust
use lemma::{Engine, LemmaResult};

fn main() -> LemmaResult<()> {
    let mut engine = Engine::new();

    engine.add_lemma_code(r#"
        doc pricing
        fact base_price = 100 USD
        fact quantity = 5
        rule total = base_price * quantity
    "#, "pricing.lemma")?;

    let response = engine.evaluate("pricing", vec![])?;
    println!("{:#?}", response);

    Ok(())
}
```

## Documentation

- **API docs**: Run `cargo doc --open` or visit [docs.rs](https://docs.rs/lemma)
- **Language guide**: [Lemma language documentation](https://github.com/benrogmans/lemma/tree/main/docs)
- **Examples**: [Complete examples](https://github.com/benrogmans/lemma/tree/main/docs/examples)

## WebAssembly

Compile to WASM for use in browsers (from project root):

```bash
wasm-pack build lemma --target web --out-dir target/wasm
```

## License

Apache 2.0

