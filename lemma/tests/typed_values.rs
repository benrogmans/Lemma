use lemma::*;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn test_percentage_arithmetic() {
    let code = r#"
doc pricing
fact discount = 25%
rule net_multiplier = 1 - discount
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("pricing", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .get("net_multiplier")
        .unwrap()
        .result
        .value()
        .unwrap();

    match result {
        LiteralValue::Number(n) => assert_eq!(n, &Decimal::from_str("0.75").unwrap()),
        _ => panic!("Expected Number, got {:?}", result),
    }
}

#[test]
fn test_mass_operations() {
    let code = r#"
doc shipping
fact weight = 10 kilograms
rule double_weight = weight * 2
rule is_heavy = weight > 5 kilograms
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();

    let response = engine.evaluate("shipping", vec![], HashMap::new()).unwrap();
    let result = response
        .results
        .get("double_weight")
        .unwrap()
        .result
        .value()
        .unwrap();

    match result {
        LiteralValue::Unit(NumericUnit::Mass(amount, unit)) => {
            assert_eq!(amount, &Decimal::from_str("20").unwrap());
            assert_eq!(*unit, MassUnit::Kilogram);
        }
        _ => panic!("Expected Mass, got {:?}", result),
    }

    let is_heavy = response.results.get("is_heavy").unwrap();
    assert_eq!(
        is_heavy.result,
        lemma::OperationResult::Value(lemma::LiteralValue::Boolean(lemma::BooleanValue::True))
    );
}
