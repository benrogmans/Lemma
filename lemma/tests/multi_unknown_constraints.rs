#![cfg(feature = "inversion")]

use lemma::{Engine, LengthUnit, LiteralValue, Target, TargetOp, VolumeUnit};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn multi_unknown_implicit_relationship() {
    let code = r#"
        doc pricing
        fact price = [number]
        fact quantity = [number]
        rule total = price * quantity
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // No given facts - both price and quantity are unknown
    let solutions = engine
        .invert_strict(
            "pricing",
            "total",
            Target::value(LiteralValue::number(100)),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Both price and quantity should be free variables (can't solve uniquely)
    assert_eq!(solutions.iter().flat_map(|r| r.keys()).count(), 2);
    let price_ref = lemma::FactPath::new(vec![], "price".to_string());
    let quantity_ref = lemma::FactPath::new(vec![], "quantity".to_string());
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v == &price_ref));
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v == &quantity_ref));
}

#[test]
fn multi_unknown_inequality_should_be_implicit() {
    let code = r#"
        doc pricing
        fact price = [number]
        fact quantity = [number]
        rule total = price * quantity
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: total > 50 with no given facts
    let solutions = engine
        .invert_strict(
            "pricing",
            "total",
            Target::with_op(
                TargetOp::Gt,
                lemma::OperationResult::Value(LiteralValue::number(50)),
            ),
            HashMap::new(),
        )
        .expect("invert should succeed");

    // Should return relationship showing constraint
    // Both variables should be free
    assert_eq!(solutions.iter().flat_map(|r| r.keys()).count(), 2);
}

#[test]
fn multi_unknown_with_partial_constraint() {
    let code = r#"
        doc geometry
        fact length = [length]
        fact width = [length]
        fact height = [length]
        rule volume = length * width * height
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Give one fact, query with two unknowns remaining
    let mut given = HashMap::new();
    given.insert(
        "length".to_string(),
        LiteralValue::Unit(lemma::NumericUnit::Length(
            Decimal::from(5),
            LengthUnit::Meter,
        )),
    );

    let target_volume = LiteralValue::Unit(lemma::NumericUnit::Volume(
        Decimal::from(100),
        VolumeUnit::CubicMeter,
    ));

    let solutions = engine
        .invert_strict("geometry", "volume", Target::value(target_volume), given)
        .expect("invert should succeed");

    // width * height = 100/5 still has two unknowns
    // width and height should both be free
    assert_eq!(solutions.iter().flat_map(|r| r.keys()).count(), 2);
    let width_ref = lemma::FactPath::new(vec![], "width".to_string());
    let height_ref = lemma::FactPath::new(vec![], "height".to_string());
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v == &width_ref));
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v == &height_ref));
}
