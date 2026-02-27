use lemma::{Engine, LiteralValue, Target};
mod common;
use common::add_lemma_code_blocking;
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn discount_multiple_paths_to_same_value() {
    // Use type annotations [text] to make facts free variables for inversion
    let code = r#"
        doc shop
        fact discount_code: [text]
        fact member_level: [text]

        rule discount: 0.20
          unless discount_code is "SAVE30" then 0.30
          unless member_level is "platinum" then 0.30
    "#;

    let mut engine = Engine::new();
    add_lemma_code_blocking(&mut engine, code, "test").unwrap();

    // Query: "What gives me 30% discount?"
    let solutions = engine
        .invert(
            "shop",
            "discount",
            Target::value(LiteralValue::number(
                Decimal::from_str_exact("0.30").unwrap(),
            )),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Should have solutions
    assert!(!solutions.is_empty(), "Expected at least one solution");

    // Should track discount_code and member_level in domains
    let all_keys: Vec<String> = solutions
        .domains
        .iter()
        .flat_map(|d| d.keys())
        .map(|k| k.to_string())
        .collect();

    assert!(
        all_keys.iter().any(|k| k.contains("discount_code")),
        "discount_code should be in domains: {:?}",
        all_keys
    );
    assert!(
        all_keys.iter().any(|k| k.contains("member_level")),
        "member_level should be in domains: {:?}",
        all_keys
    );
}
