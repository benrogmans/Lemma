use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use lemma::EvaluationRequest;
use lemma::ValueKind;
use lemma::VetoType;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

fn decimal_lit(d: &str) -> Decimal {
    Decimal::from_str(d).unwrap()
}

#[test]
fn quantity_comparison_converts_units_before_comparing() {
    // unit usd 0.84: 1 USD = 0.84 EUR (canonical).
    // 100 USD = 84 EUR in base, so 150 EUR > 100 USD triggers veto.
    let code = r#"
spec pricing
data money: quantity
    -> unit eur 1
    -> unit usd 0.84

data price: money

rule check: accept
    unless price > 100 usd then veto "This price is too high."
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::from([("price".to_string(), "150 eur".to_string())]),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "check")
        .unwrap();

    assert_eq!(
        rule_result.result,
        OperationResult::Veto(VetoType::UserDefined {
            message: Some("This price is too high.".to_string()),
        })
    );
}

#[test]
fn quantity_data_value_rejects_unknown_unit() {
    let code = r#"
spec pricing
data money: quantity
    -> unit eur 1
    -> unit usd 0.84

data price: money

rule check: accept
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let err = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::from([("price".to_string(), "100 btc".to_string())]),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("btc"), "actual error: {msg}");
}

#[test]
fn quantity_as_operator_converts_units() {
    // unit usd 0.84: 1 USD = 0.84 EUR (canonical). 100 usd as eur => 100 * 0.84 = 84.
    let code = r#"
spec pricing
data money: quantity
    -> unit eur 1
    -> unit usd 0.84

rule price_eur: 100 usd as eur
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "pricing",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "price_eur")
        .unwrap();

    let (value, lemma_type) = match &rule_result.result {
        OperationResult::Value(lit) => (&lit.value, &lit.lemma_type),
        other => panic!("Expected a Value result, got: {:?}", other),
    };

    assert!(
        lemma_type.is_quantity(),
        "Expected quantity type, got: {lemma_type:?}"
    );

    let (amount, unit) = match value {
        ValueKind::Quantity(amount, unit, _decomp) => (amount, unit),
        other => panic!("Expected a quantity value, got: {other:?}"),
    };

    assert_eq!(
        lemma::commit_rational_to_decimal(amount).unwrap(),
        Decimal::from(84)
    );
    assert_eq!(unit.as_str(), "eur");
}

#[test]
fn quantity_add_subtract_converts_units_when_same_family() {
    // Quantity add/subtract with different units (same quantity family) must convert, not Veto.
    // Regression: previously returned Veto "Cannot apply '-' to values with different units".
    let code = r#"
spec t
data money: quantity -> unit eur 1.00 -> unit usd 0.84
data gross: 7600 usd
data pension: 0 eur
rule taxable: gross - pension
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "t",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "taxable")
        .unwrap();

    match &rule_result.result {
        OperationResult::Value(lit) => {
            let (amount, unit) = match &lit.value {
                ValueKind::Quantity(a, u, _decomp) => (a, u),
                other => panic!("expected quantity, got {other:?}"),
            };
            assert_eq!(unit.as_str(), "usd", "result unit follows left operand");
            assert_eq!(
                lemma::commit_rational_to_decimal(amount).unwrap(),
                Decimal::from(7600),
                "7600 usd - 0 eur = 7600 usd"
            );
        }
        OperationResult::Veto(msg) => panic!("expected Value, got Veto: {msg:?}"),
    }
}

#[test]
fn quantity_as_operator_rejects_unknown_unit() {
    let code = r#"
spec pricing
data money: quantity
    -> unit eur 1
    -> unit usd 0.84

rule price_gbp: 100 eur as gbp
"#;

    let mut engine = Engine::new();
    let load_err = engine.load(code, lemma::SourceType::Volatile).unwrap_err();
    let msg = load_err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");

    assert!(msg.contains("Unknown unit 'gbp'"), "actual error: {msg}");
    assert!(msg.contains("Valid units:"), "actual error: {msg}");
}

#[test]
fn named_quantity_type_comparison_with_unit_literal() {
    // Regression: planning rejected `package_weight > 1 kilogram` with
    // "Cannot compare different quantity types: quantity and weight" because it
    // used strict name equality instead of same_quantity_family.
    let code = r#"
spec shipping

data weight: quantity -> unit kilogram 1.0

data package_weight: 2.5 kilogram

rule base_shipping: 5.99
    unless package_weight > 1 kilogram then 8.99
    unless package_weight > 5 kilogram then 15.99
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "shipping",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "base_shipping")
        .unwrap();

    // package_weight = 2.5 kg, which is > 1 kg but not > 5 kg, so second unless wins: 8.99
    match &rule_result.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Number(d) => {
                assert_eq!(
                    lemma::commit_rational_to_decimal(d).unwrap(),
                    Decimal::new(899, 2)
                );
            }
            other => panic!("Expected Number value, got {:?}", other),
        },
        OperationResult::Veto(reason) => panic!("Expected value, got Veto({:?})", reason),
    }
}

#[test]
fn named_quantity_type_arithmetic_within_same_family() {
    // Regression: planning rejected arithmetic between values of the same
    // quantity family when their type names differed (e.g. "quantity" vs "weight").
    let code = r#"
spec shipping

data money: quantity -> unit USD 1.00

data base_fee: 5.99 USD
data surcharge: 2.00 USD

rule total: base_fee + surcharge
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "shipping",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();

    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .unwrap();

    match &rule_result.result {
        OperationResult::Value(v) => match &v.value {
            ValueKind::Quantity(d, _, _) => {
                assert_eq!(
                    lemma::commit_rational_to_decimal(d).unwrap(),
                    Decimal::new(799, 2)
                );
            }
            other => panic!("Expected Quantity value, got {:?}", other),
        },
        OperationResult::Veto(reason) => panic!("Expected value, got Veto({:?})", reason),
    }
}

// ═══════════════════════════════════════════════════════════════════
// Regression: convert_unit must use from_factor / to_factor.
// Quantity unit factor = canonical units per 1 unit (gram 0.001 => 1 g = 0.001 kg).
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_as_converts_kg_to_gram() {
    let code = r#"
spec physics
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001

rule result: 2 kilogram as gram
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "physics",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let rule = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .unwrap();

    let (amount, unit) = match &rule.result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Quantity(a, u, _decomp) => (*a, u.as_str()),
            other => panic!("expected Quantity, got {other:?}"),
        },
        other => panic!("expected Value, got {other:?}"),
    };

    assert_eq!(unit, "gram");
    assert_eq!(
        lemma::commit_rational_to_decimal(&amount).unwrap(),
        Decimal::from(2000),
        "2 kg = 2000 gram"
    );
}

#[test]
fn quantity_as_converts_gram_to_kg() {
    let code = r#"
spec physics
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001

rule result: 500 gram as kilogram
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "physics",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let rule = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .unwrap();

    let (amount, unit) = match &rule.result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Quantity(a, u, _decomp) => (*a, u.as_str()),
            other => panic!("expected Quantity, got {other:?}"),
        },
        other => panic!("expected Value, got {other:?}"),
    };

    assert_eq!(unit, "kilogram");
    assert_eq!(
        lemma::commit_rational_to_decimal(&amount).unwrap(),
        decimal_lit("0.5"),
        "500 gram = 0.5 kg"
    );
}

#[test]
fn quantity_as_converts_pound_to_gram() {
    let code = r#"
spec physics
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> unit pound 0.453592

rule result: 1 pound as gram
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "physics",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let rule = response
        .results
        .values()
        .find(|r| r.rule.name == "result")
        .unwrap();

    let (amount, unit) = match &rule.result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Quantity(a, u, _decomp) => (*a, u.as_str()),
            other => panic!("expected Quantity, got {other:?}"),
        },
        other => panic!("expected Value, got {other:?}"),
    };

    assert_eq!(unit, "gram");
    assert_eq!(
        lemma::commit_rational_to_decimal(&amount).unwrap(),
        decimal_lit("453.592"),
        "1 pound = 453.592 gram"
    );
}

#[test]
fn quantity_arithmetic_different_units_converts_correctly() {
    let code = r#"
spec physics
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001

data a: 1 kilogram
data b: 500 gram

rule total: a + b
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "physics",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let rule = response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .unwrap();

    let (amount, unit) = match &rule.result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Quantity(a, u, _decomp) => (*a, u.as_str()),
            other => panic!("expected Quantity, got {other:?}"),
        },
        other => panic!("expected Value, got {other:?}"),
    };

    assert_eq!(unit, "kilogram", "result unit follows left operand");
    assert_eq!(
        lemma::commit_rational_to_decimal(&amount).unwrap(),
        decimal_lit("1.5"),
        "1 kg + 500 g = 1.5 kg"
    );
}

#[test]
fn quantity_comparison_different_units_physical() {
    let code = r#"
spec physics
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001

data package: 1500 gram
rule heavy: package > 1 kilogram
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "physics",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap();
    let rule = response
        .results
        .values()
        .find(|r| r.rule.name == "heavy")
        .unwrap();

    match &rule.result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Boolean(b) => assert!(*b, "1500 gram > 1 kilogram should be true"),
            other => panic!("expected Boolean, got {other:?}"),
        },
        other => panic!("expected Value, got {other:?}"),
    };
}

// ═══════════════════════════════════════════════════════════════════
// Bug 2: validate_value_against_type compares raw quantity value
// against base-unit bounds without converting. A value like
// "1500 gram" with maximum "2 kilogram" (stored as 2.0 base)
// is wrongly rejected because 1500 > 2.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn quantity_validation_respects_unit_for_maximum() {
    let code = r#"
spec physics
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> maximum 2 kilogram

data weight: mass

rule result: weight
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    // 1500 gram = 1.5 kg, which is under the 2 kg maximum
    let response = engine.run(
        None,
        "physics",
        Some(&now),
        HashMap::from([("weight".to_string(), "1500 gram".to_string())]),
        false,
        lemma::EvaluationRequest::default(),
    );

    assert!(
        response.is_ok(),
        "1500 gram (= 1.5 kg) should pass maximum 2 kg, got: {:?}",
        response.unwrap_err()
    );
}

#[test]
fn quantity_validation_respects_unit_for_minimum() {
    let code = r#"
spec physics
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> minimum 1 kilogram

data weight: mass

rule result: weight
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    // 1500 gram = 1.5 kg, which is above the 1 kg minimum
    let response = engine.run(
        None,
        "physics",
        Some(&now),
        HashMap::from([("weight".to_string(), "1500 gram".to_string())]),
        false,
        lemma::EvaluationRequest::default(),
    );

    assert!(
        response.is_ok(),
        "1500 gram (= 1.5 kg) should pass minimum 1 kg, got: {:?}",
        response.unwrap_err()
    );
}

#[test]
fn quantity_validation_rejects_over_maximum_in_non_base_unit() {
    let code = r#"
spec physics
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> maximum 2 kilogram

data weight: mass

rule result: weight
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    // 3000 gram = 3 kg, which exceeds the 2 kg maximum
    let err = engine
        .run(
            None,
            "physics",
            Some(&now),
            HashMap::from([("weight".to_string(), "3000 gram".to_string())]),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("maximum") || msg.contains("above"),
        "3000 gram (= 3 kg) should fail maximum 2 kg, got: {msg}"
    );
    assert!(
        !msg.to_lowercase().contains("canonical"),
        "must not mention canonical units, got: {msg}"
    );
}

#[test]
fn quantity_below_minimum_error_uses_per_unit_not_canonical() {
    let code = r#"
spec s
data money: quantity -> unit eur 1 -> unit usd 0.91
data mass: quantity -> unit kilogram 1
data cost_per_unit: quantity
  -> unit eur_per_kilo eur/kilogram
  -> minimum 1.20 eur_per_kilo
rule out: cost_per_unit
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let err = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::from([("cost_per_unit".to_string(), "1 eur_per_kilo".to_string())]),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("below minimum"),
        "expected below minimum, got: {message}"
    );
    assert!(
        message.contains("eur_per_kilo"),
        "expected user unit in message, got: {message}"
    );
    assert!(
        message.contains("1.2") || message.contains("1.20"),
        "expected per-unit minimum magnitude, got: {message}"
    );
    assert!(
        !message.to_lowercase().contains("canonical"),
        "must not mention canonical units, got: {message}"
    );
}

#[test]
fn quantity_below_minimum_error_respects_type_decimals() {
    let code = r#"
spec s
data money: quantity -> unit eur 1 -> unit usd 0.91
data mass: quantity -> unit kilogram 1 -> unit tonne 1000
data cost_per_unit: quantity
  -> unit eur_per_kilo eur/kilogram
  -> unit usd_per_tonne usd/tonne
  -> minimum 1.20 eur_per_kilo
  -> decimals 2
rule out: cost_per_unit
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let err = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::from([(
                "cost_per_unit".to_string(),
                "2.01 usd_per_tonne".to_string(),
            )]),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("below minimum"),
        "expected below minimum, got: {message}"
    );
    assert!(
        message.contains("usd_per_tonne"),
        "expected user unit in message, got: {message}"
    );
    assert!(
        message.contains("1318.68"),
        "expected minimum bound rounded to 2 decimals, got: {message}"
    );
    assert!(
        !message.contains("1318.681"),
        "must not show extra decimal precision beyond type decimals, got: {message}"
    );
}

const COMPOUND_COST_VALIDATION_SPEC: &str = r#"
spec s
data money: quantity -> unit eur 1 -> unit usd 0.91
data mass: quantity -> unit kilogram 1 -> unit tonne 1000
data cost_per_unit: quantity
  -> unit eur_per_kilo eur/kilogram
  -> unit usd_per_tonne usd/tonne
  -> maximum 2.00 eur_per_kilo
rule out: cost_per_unit
"#;

#[test]
fn compound_quantity_below_maximum_in_other_unit_passes() {
    let mut engine = Engine::new();
    engine
        .load(COMPOUND_COST_VALIDATION_SPEC, lemma::SourceType::Volatile)
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine.run(
        None,
        "s",
        Some(&now),
        HashMap::from([(
            "cost_per_unit".to_string(),
            "2.01 usd_per_tonne".to_string(),
        )]),
        false,
        lemma::EvaluationRequest::default(),
    );

    assert!(
        response.is_ok(),
        "2.01 usd_per_tonne is below converted maximum of 2.00 eur_per_kilo, got: {:?}",
        response.as_ref().err()
    );
}

#[test]
fn compound_quantity_above_maximum_shows_converted_bound_in_user_unit() {
    let mut engine = Engine::new();
    engine
        .load(COMPOUND_COST_VALIDATION_SPEC, lemma::SourceType::Volatile)
        .unwrap();

    let now = DateTimeValue::now();
    let err = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::from([(
                "cost_per_unit".to_string(),
                "5000 usd_per_tonne".to_string(),
            )]),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("above maximum"),
        "expected above maximum, got: {message}"
    );
    assert!(
        message.contains("usd_per_tonne"),
        "expected user unit in message, got: {message}"
    );
    assert!(
        !message.contains("2 usd_per_tonne"),
        "must not compare against unconverted maximum 2 usd_per_tonne, got: {message}"
    );
}

const TRI_COMPOUND_COST_VALIDATION_SPEC: &str = r#"
spec s
uses lemma si

data money: quantity
  -> unit eur 1
  -> unit usd 0.91

data mass: quantity
  -> unit kilogram 1
  -> unit tonne 1000

data storage_cost: quantity
  -> unit eur_per_kilo_hour eur/kilogram/hour
  -> unit usd_per_ton_hour usd/tonne/hour
  -> maximum 2.00 eur_per_kilo_hour

rule out: storage_cost
"#;

#[test]
fn tri_compound_quantity_below_maximum_in_other_unit_passes() {
    let mut engine = Engine::new();
    engine
        .load(
            TRI_COMPOUND_COST_VALIDATION_SPEC,
            lemma::SourceType::Volatile,
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine.run(
        None,
        "s",
        Some(&now),
        HashMap::from([(
            "storage_cost".to_string(),
            "2.01 usd_per_ton_hour".to_string(),
        )]),
        false,
        lemma::EvaluationRequest::default(),
    );

    assert!(
        response.is_ok(),
        "2.01 usd_per_ton_hour is below converted maximum of 2.00 eur_per_kilo_hour, got: {:?}",
        response.as_ref().err()
    );
}

#[test]
fn tri_compound_quantity_above_maximum_shows_converted_bound_in_user_unit() {
    let mut engine = Engine::new();
    engine
        .load(
            TRI_COMPOUND_COST_VALIDATION_SPEC,
            lemma::SourceType::Volatile,
        )
        .unwrap();

    let now = DateTimeValue::now();
    let err = engine
        .run(
            None,
            "s",
            Some(&now),
            HashMap::from([(
                "storage_cost".to_string(),
                "5000 usd_per_ton_hour".to_string(),
            )]),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("above maximum"),
        "expected above maximum, got: {message}"
    );
    assert!(
        message.contains("usd_per_ton_hour"),
        "expected user unit in message, got: {message}"
    );
    assert!(
        !message.contains("2 usd_per_ton_hour"),
        "must not compare against unconverted maximum 2 usd_per_ton_hour, got: {message}"
    );
}

#[test]
fn quantity_below_minimum_cross_unit_error_message() {
    let code = r#"
spec physics
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> minimum 1 kilogram

data weight: mass

rule result: weight
"#;

    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let err = engine
        .run(
            None,
            "physics",
            Some(&now),
            HashMap::from([("weight".to_string(), "500 gram".to_string())]),
            false,
            lemma::EvaluationRequest::default(),
        )
        .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("500 gram is below minimum 1000 gram"),
        "expected cross-unit bound in user's unit, got: {message}"
    );
    assert!(
        !message.to_lowercase().contains("canonical"),
        "must not mention canonical units, got: {message}"
    );
}

// --- precision stress: prime unit chains from seed 37 ---

fn reference_named_unit_conversion(
    stored: Decimal,
    from_factor: Decimal,
    to_factor: Decimal,
) -> Decimal {
    stored
        .checked_mul(from_factor)
        .expect("reference conversion multiply failed")
        .checked_div(to_factor)
        .expect("reference conversion divide failed")
}

fn reference_named_unit_conversion_rational(
    stored: lemma::RationalInteger,
    from_factor: lemma::RationalInteger,
    to_factor: lemma::RationalInteger,
) -> lemma::RationalInteger {
    let scaled = lemma::checked_mul(&stored, &from_factor).expect("reference conversion multiply");
    let inverse_to = lemma::RationalInteger::new(*to_factor.denom(), *to_factor.numer());
    lemma::checked_mul(&scaled, &inverse_to).expect("reference conversion divide")
}

fn rational_prime_unit(prime: u32) -> lemma::RationalInteger {
    lemma::RationalInteger::new(i128::from(prime), 1)
}

fn eval_rule_quantity_magnitude(
    engine: &Engine,
    spec_name: &str,
    rule_name: &str,
) -> (Decimal, String) {
    eval_rule_quantity_magnitude_with_request(
        engine,
        spec_name,
        rule_name,
        EvaluationRequest::default(),
    )
}

fn eval_rule_quantity_magnitude_with_request(
    engine: &Engine,
    spec_name: &str,
    rule_name: &str,
    request: EvaluationRequest,
) -> (Decimal, String) {
    let now = DateTimeValue::now();
    let response = engine
        .run(None, spec_name, Some(&now), HashMap::new(), false, request)
        .expect("precision stress evaluation must complete");
    let rule = response
        .results
        .values()
        .find(|r| r.rule.name == rule_name)
        .unwrap_or_else(|| panic!("rule '{rule_name}' missing in spec '{spec_name}'"));
    match &rule.result {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Quantity(amount, unit, _) => (
                lemma::commit_rational_to_decimal(amount).unwrap(),
                unit.clone(),
            ),
            other => panic!("expected Quantity from '{rule_name}', got {other:?}"),
        },
        OperationResult::Veto(reason) => {
            panic!("rule '{rule_name}' must not Veto, got {reason:?}");
        }
    }
}

fn prime_precision_spec_header(primes: &[u32]) -> String {
    let mut spec = String::from(
        "spec prime_precision\n\
         data seed: quantity\n    -> unit base 1\n",
    );
    for prime in primes {
        spec.push_str(&format!("    -> unit p{prime} {prime}\n"));
    }
    spec.push_str("data anchor: 37 base\nrule step0: anchor\n");
    spec
}

#[test]
fn precision_baseline_roundtrip_base_p2_base() {
    let code = r#"
spec prime_precision
data seed: quantity
    -> unit base 1
    -> unit p2 2
data anchor: 37 base
rule step0: anchor
rule step1: step0 as p2
rule step2: step1 as base
"#;
    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let (amount, unit) = eval_rule_quantity_magnitude(&engine, "prime_precision", "step2");
    assert_eq!(unit, "base");
    assert_eq!(amount, Decimal::from(37));
}

#[test]
fn precision_short_prime_chain_matches_stepwise_reference() {
    let primes: &[u32] = &[2, 3, 5];
    let mut code = prime_precision_spec_header(primes);
    let mut last = String::from("step0");
    let mut step_index = 0;
    for prime in primes {
        step_index += 1;
        let rule = format!("step{step_index}");
        code.push_str(&format!("rule {rule}: {last} as p{prime}\n"));
        last = rule;
    }
    let units_reverse: Vec<u32> = primes.iter().rev().copied().collect();
    for index in 0..units_reverse.len() - 1 {
        step_index += 1;
        let rule = format!("step{step_index}");
        code.push_str(&format!(
            "rule {rule}: {last} as p{}\n",
            units_reverse[index + 1]
        ));
        last = rule;
    }
    step_index += 1;
    let final_rule = format!("step{step_index}");
    code.push_str(&format!("rule {final_rule}: {last} as base\n"));

    let mut engine = Engine::new();
    engine.load(&code, lemma::SourceType::Volatile).unwrap();

    let mut reference = forward_prime_chain_reference(lemma::RationalInteger::new(37, 1), primes);
    reference = reverse_prime_chain_reference(reference, primes);

    let (lemma_amount, unit) =
        eval_rule_quantity_magnitude(&engine, "prime_precision", &final_rule);
    assert_eq!(unit, "base");
    assert_eq!(
        lemma_amount,
        lemma::commit_rational_to_decimal(&reference).unwrap()
    );
    assert_eq!(lemma_amount, Decimal::from(37));
}

fn forward_prime_chain_reference(
    mut reference: lemma::RationalInteger,
    primes: &[u32],
) -> lemma::RationalInteger {
    let mut from_factor = lemma::RationalInteger::new(1, 1);
    for prime in primes {
        let to_factor = rational_prime_unit(*prime);
        reference = reference_named_unit_conversion_rational(reference, from_factor, to_factor);
        from_factor = to_factor;
    }
    reference
}

fn reverse_prime_chain_reference(
    mut reference: lemma::RationalInteger,
    primes: &[u32],
) -> lemma::RationalInteger {
    let units_reverse: Vec<u32> = primes.iter().rev().copied().collect();
    for index in 0..units_reverse.len() - 1 {
        reference = reference_named_unit_conversion_rational(
            reference,
            rational_prime_unit(units_reverse[index]),
            rational_prime_unit(units_reverse[index + 1]),
        );
    }
    reference_named_unit_conversion_rational(
        reference,
        rational_prime_unit(*units_reverse.last().expect("prime list non-empty")),
        lemma::RationalInteger::new(1, 1),
    )
}

#[test]
fn precision_prime_ladder_forward_reverse_matches_reference() {
    let primes: &[u32] = &[2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 41, 43, 47];
    let mut code = prime_precision_spec_header(primes);
    let mut last = String::from("step0");
    let mut step_index = 0;
    for prime in primes {
        step_index += 1;
        let rule = format!("step{step_index}");
        code.push_str(&format!("rule {rule}: {last} as p{prime}\n"));
        last = rule;
    }
    let units_reverse: Vec<u32> = primes.iter().rev().copied().collect();
    for index in 0..units_reverse.len() - 1 {
        step_index += 1;
        let rule = format!("step{step_index}");
        code.push_str(&format!(
            "rule {rule}: {last} as p{}\n",
            units_reverse[index + 1]
        ));
        last = rule;
    }
    step_index += 1;
    let final_rule = format!("step{step_index}");
    code.push_str(&format!("rule {final_rule}: {last} as base\n"));

    let mut engine = Engine::new();
    engine.load(&code, lemma::SourceType::Volatile).unwrap();

    let reference = reverse_prime_chain_reference(
        forward_prime_chain_reference(lemma::RationalInteger::new(37, 1), primes),
        primes,
    );

    let (lemma_amount, unit) =
        eval_rule_quantity_magnitude(&engine, "prime_precision", &final_rule);
    assert_eq!(unit, "base");
    assert_eq!(
        lemma_amount,
        lemma::commit_rational_to_decimal(&reference).unwrap(),
        "Lemma must match stepwise reference"
    );
    assert_eq!(
        lemma_amount,
        Decimal::from(37),
        "37 base through fourteen prime units forward and back must stay exactly 37"
    );
}

#[test]
fn precision_many_prime_cycles_deterministic() {
    let cycle: &[u32] = &[2, 3, 5, 7, 11];
    let mut code = String::from(
        "spec prime_cycles\n\
         data seed: quantity\n    -> unit base 1\n",
    );
    for prime in cycle {
        code.push_str(&format!("    -> unit p{prime} {prime}\n"));
    }
    code.push_str("data anchor: 37 base\nrule step0: anchor\n");
    let mut previous = String::from("step0");
    let mut step_number = 0;
    for _cycle_index in 0..20 {
        for prime in cycle {
            step_number += 1;
            let rule = format!("step{step_number}");
            code.push_str(&format!("rule {rule}: {previous} as p{prime}\n"));
            previous = rule;
        }
    }
    code.push_str(&format!("rule final_step: {previous} as base\n"));

    let mut engine = Engine::new();
    engine.load(&code, lemma::SourceType::Volatile).unwrap();

    let (first, _) = eval_rule_quantity_magnitude(&engine, "prime_cycles", "final_step");
    let (second, _) = eval_rule_quantity_magnitude(&engine, "prime_cycles", "final_step");
    assert_eq!(first, second, "repeated evaluation must be deterministic");
    assert!(first > Decimal::ZERO);
}

#[test]
fn precision_ping_pong_p7_p11_forty_hops() {
    let mut code = String::from(
        "spec ping_pong\n\
         data seed: quantity\n    -> unit base 1\n    -> unit p7 7\n    -> unit p11 11\n\
         data anchor: 37 base\nrule step0: anchor\n",
    );
    let mut previous = String::from("step0");
    for index in 0..40 {
        let unit = if index % 2 == 0 { "p7" } else { "p11" };
        let rule = format!("step{}", index + 1);
        code.push_str(&format!("rule {rule}: {previous} as {unit}\n"));
        previous = rule;
    }
    code.push_str(&format!("rule final_step: {previous} as base\n"));

    let mut engine = Engine::new();
    engine.load(&code, lemma::SourceType::Volatile).unwrap();
    let (amount, unit) = eval_rule_quantity_magnitude(&engine, "ping_pong", "final_step");
    assert_eq!(unit, "base");
    assert!(amount > Decimal::ZERO);
}

#[test]
fn precision_mixed_arithmetic_conversion_completes() {
    let code = r#"
spec mixed_arith
data seed: quantity
    -> unit base 1
    -> unit p3 3
    -> unit p7 7
data anchor: 37 base
data five: 5
data two: 2
rule step0: anchor
rule step1: step0 as p3
rule step2: step1 * five
rule step3: step2 as p7
rule step4: step3 / two
rule step5: step4 as base
"#;
    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();

    let mut reference =
        reference_named_unit_conversion(Decimal::from(37), Decimal::ONE, Decimal::from(3));
    reference = reference
        .checked_mul(Decimal::from(5))
        .expect("reference multiply by five");
    reference = reference_named_unit_conversion(reference, Decimal::from(3), Decimal::from(7));
    reference = reference
        .checked_div(Decimal::from(2))
        .expect("reference divide by two");
    reference = reference_named_unit_conversion(reference, Decimal::from(7), Decimal::ONE);

    let (lemma_amount, unit) = eval_rule_quantity_magnitude(&engine, "mixed_arith", "step5");
    assert_eq!(unit, "base");
    assert_eq!(lemma_amount, reference);
}

#[test]
fn precision_api_display_ping_pong_twenty_unit_toggles() {
    let mut code = String::from(
        "spec api_ping_pong\n\
         data seed: quantity\n    -> unit base 1\n    -> unit p7 7\n    -> unit p11 11\n\
         data anchor: 37 base\nrule step0: anchor\n",
    );
    let mut previous = String::from("step0");
    for index in 0..20 {
        let unit = if index % 2 == 0 { "p7" } else { "p11" };
        let rule = format!("step{}", index + 1);
        code.push_str(&format!("rule {rule}: {previous} as {unit}\n"));
        previous = rule;
    }

    let mut engine = Engine::new();
    engine.load(&code, lemma::SourceType::Volatile).unwrap();

    let (in_spec_amount, in_spec_unit) =
        eval_rule_quantity_magnitude(&engine, "api_ping_pong", &previous);
    let in_spec_factor = match in_spec_unit.as_str() {
        "p7" => Decimal::from(7),
        "p11" => Decimal::from(11),
        other => panic!("unexpected in-spec unit {other}"),
    };

    let now = DateTimeValue::now();
    let plan = engine
        .get_plan(None, "api_ping_pong", Some(&now))
        .expect("plan");
    for index in 0..20 {
        let target_unit = if index % 2 == 0 { "p7" } else { "p11" };
        let target_factor = if index % 2 == 0 {
            Decimal::from(7)
        } else {
            Decimal::from(11)
        };
        let request = EvaluationRequest::from_rule_conversion_strings(
            HashMap::from([(previous.clone(), target_unit.to_string())]),
            plan,
        )
        .expect("valid API unit toggle");
        let (api_amount, api_unit) =
            eval_rule_quantity_magnitude_with_request(&engine, "api_ping_pong", &previous, request);
        assert_eq!(api_unit, target_unit);
        let expected =
            reference_named_unit_conversion(in_spec_amount, in_spec_factor, target_factor);
        assert_eq!(
            api_amount, expected,
            "API display at hop {index} must match in-spec conversion"
        );
    }
}

#[test]
fn precision_division_by_zero_control_vetoes() {
    let code = r#"
spec div0_control
data ten: 10
data zero: 0
rule quotient: ten / zero
"#;
    let mut engine = Engine::new();
    engine.load(code, lemma::SourceType::Volatile).unwrap();
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "div0_control",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("evaluation must complete");
    let rule = response
        .results
        .values()
        .find(|r| r.rule.name == "quotient")
        .expect("quotient missing");
    assert!(
        matches!(
            rule.result,
            OperationResult::Veto(lemma::VetoType::Computation { .. })
        ),
        "division by zero path must Veto, not panic"
    );
}
