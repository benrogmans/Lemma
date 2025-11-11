use lemma::{Engine, LiteralValue, MoneyUnit, NumericUnit, Target, TargetOp};
use rust_decimal::Decimal;
use std::collections::HashMap;

fn eur(amount: i64) -> LiteralValue {
    LiteralValue::Unit(NumericUnit::Money(Decimal::from(amount), MoneyUnit::Eur))
}

#[test]
fn multi_unknown_implicit_relationship() {
    let code = r#"
        doc pricing
        fact price = [money]
        fact quantity = [number]
        rule total = price * quantity
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // No given facts - both price and quantity are unknown
    let solutions = engine
        .invert("pricing", "total", Target::value(eur(100)), HashMap::new())
        .expect("invert should succeed");

    // Both price and quantity should be free variables (can't solve uniquely)
    assert_eq!(solutions.iter().flat_map(|r| r.keys()).count(), 2);
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "price"));
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "quantity"));
}

#[test]
fn multi_unknown_inequality_should_be_implicit() {
    let code = r#"
        doc pricing
        fact price = [money]
        fact quantity = [number]
        rule total = price * quantity
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Query: total > 50 EUR with no given facts
    let solutions = engine
        .invert(
            "pricing",
            "total",
            Target::with_op(TargetOp::Gt, lemma::OperationResult::Value(eur(50))),
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
        "geometry.length".to_string(),
        LiteralValue::Unit(NumericUnit::Length(
            Decimal::from(5),
            lemma::LengthUnit::Meter,
        )),
    );

    let target_volume = LiteralValue::Unit(NumericUnit::Volume(
        Decimal::from(100),
        lemma::VolumeUnit::CubicMeter,
    ));

    let solutions = engine
        .invert("geometry", "volume", Target::value(target_volume), given)
        .expect("invert should succeed");

    // width * height = 100/5 still has two unknowns
    // width and height should both be free
    assert_eq!(solutions.iter().flat_map(|r| r.keys()).count(), 2);
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "width"));
    assert!(solutions
        .iter()
        .flat_map(|r| r.keys())
        .any(|v| v.reference.join(".") == "height"));
}
