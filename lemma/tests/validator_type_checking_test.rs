use lemma::Engine;

#[test]
fn test_logical_and_requires_boolean_operands() {
    let code = r#"
doc test
rule result = 5 and true
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_err(), "Should reject non-boolean in 'and'");
    assert!(result.unwrap_err().to_string().contains("boolean"));
}

#[test]
fn test_logical_or_requires_boolean_operands() {
    let code = r#"
doc test
rule result = "hello" or false
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_err(), "Should reject non-boolean in 'or'");
    assert!(result.unwrap_err().to_string().contains("boolean"));
}

#[test]
fn test_unless_condition_must_be_boolean() {
    let code = r#"
doc test
rule result = 10
  unless 5 then 20
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_err(), "Unless condition must be boolean");
}

#[test]
fn test_conversion_to_valid_unit() {
    let code = r#"
doc test
fact distance = 1000
rule km = distance in kilometers
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Valid unit conversion should pass: {:?}",
        result
    );
}

#[test]
fn test_percentage_literal_type() {
    let code = r#"
doc test
fact rate = 15%
rule doubled = rate
  unless rate > 10% then 20%
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Percentage types should be consistent: {:?}",
        result
    );
}

#[test]
fn test_money_requires_same_currency_in_comparison() {
    let code = r#"
doc test
fact price_usd = 100 USD
fact price_eur = 80 EUR
rule comparison = price_usd > price_eur
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject different currency comparison"
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("currenc"));
}

#[test]
fn test_money_requires_same_currency_in_arithmetic() {
    let code = r#"
doc test
fact price_usd = 100 USD
fact price_gbp = 80 GBP
rule total = price_usd + price_gbp
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject different currency arithmetic"
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .to_lowercase()
        .contains("currenc"));
}

#[test]
fn test_text_number_comparison_allowed() {
    let code = r#"
doc test
fact name = "Alice"
fact age = 30
rule check = name == "Bob" and age > 25
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Text and number comparisons should be allowed separately: {:?}",
        result
    );
}

#[test]
fn test_date_comparison() {
    let code = r#"
doc test
fact start = 2024-01-01
fact end = 2024-12-31
rule is_valid_range = end > start
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Date comparison should be allowed: {:?}",
        result
    );
}

#[test]
fn test_all_unit_types_in_conversions() {
    let test_cases = vec![
        ("(value * 100) in kilograms", "Mass"),
        ("(value * 50) in meters", "Length"),
        ("(value * 200) in liters", "Volume"),
        ("(value * 60) in seconds", "Duration"),
        ("(value * 25) in celsius", "Temperature"),
        ("(value * 1000) in watts", "Power"),
        ("(value * 100) in newtons", "Force"),
        ("(value * 101325) in pascals", "Pressure"),
        ("(value * 1000) in joules", "Energy"),
        ("(value * 60) in hertz", "Frequency"),
        ("(value * 1024) in bytes", "DataSize"),
    ];

    for (conversion, unit_name) in test_cases {
        let code = format!(
            r#"
doc test
fact value = 1
rule converted = {}
"#,
            conversion
        );

        let mut engine = Engine::new();
        let result = engine.add_lemma_code(&code, "test.lemma");
        assert!(
            result.is_ok(),
            "{} conversion should work: {:?}",
            unit_name,
            result
        );
    }
}

#[test]
fn test_percentage_conversion_from_number() {
    let code = r#"
doc test
fact ratio = 0.25
rule as_percentage = ratio in percentage
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Number to percentage conversion should work: {:?}",
        result
    );
}

#[test]
fn test_veto_type_is_never() {
    let code = r#"
doc test
fact age = 15
rule result = 100
  unless age < 18 then veto "Too young"
  unless age > 65 then 50
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Veto should not conflict with other return types: {:?}",
        result
    );
}

#[test]
fn test_mixed_number_and_money_not_allowed() {
    let code = r#"
doc test
fact base = 100
rule amount = base
  unless base > 50 then 200 USD
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing number and money types"
    );
    assert!(result.unwrap_err().to_string().contains("incompatible"));
}

#[test]
fn test_mixed_text_and_number_not_allowed() {
    let code = r#"
doc test
fact flag = true
rule value = "default"
  unless flag then 42
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing text and number types"
    );
    assert!(result.unwrap_err().to_string().contains("incompatible"));
}

#[test]
fn test_mixed_date_and_number_not_allowed() {
    let code = r#"
doc test
fact use_date = true
rule value = 2024-01-01
  unless use_date then 100
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing date and number types"
    );
    assert!(result.unwrap_err().to_string().contains("incompatible"));
}

#[test]
fn test_same_category_units_allowed_in_rule() {
    let code = r#"
doc test
fact weight = 1000 grams
rule adjusted = weight
  unless weight > 500 grams then 2 kilograms
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Same category units should be allowed: {:?}",
        result
    );
}

#[test]
fn test_boolean_consistency() {
    let code = r#"
doc test
fact x = 5
fact y = 10
rule check = x < y
  unless x == 0 then y > 0
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Boolean results should be consistent: {:?}",
        result
    );
}

#[test]
fn test_arithmetic_result_type_inference() {
    let code = r#"
doc test
fact a = 10
fact b = 20
rule sum = a + b
  unless a == 0 then 0
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Arithmetic should infer number type: {:?}",
        result
    );
}

#[test]
fn test_multiple_unless_clauses_type_consistency() {
    let code = r#"
doc test
fact x = 5
rule value = 10
  unless x < 0 then 0
  unless x > 100 then 100
  unless x == 5 then 5
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "All number branches should be consistent: {:?}",
        result
    );
}

#[test]
fn test_multiple_unless_clauses_type_inconsistency() {
    let code = r#"
doc test
fact x = 5
rule value = 10
  unless x < 0 then 0
  unless x > 100 then "overflow"
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_err(), "Mixed number/text should be rejected");
    assert!(result.unwrap_err().to_string().contains("incompatible"));
}

#[test]
fn test_conversion_changes_type() {
    let code = r#"
doc test
fact meters = 100
rule as_km = meters in kilometers
rule back_to_number = as_km
  unless as_km > 0 kilometers then 0
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    // as_km is Length type, so mixing with number should fail
    assert!(result.is_err(), "Conversion should create distinct type");
}

#[test]
fn test_rule_reference_type_propagation() {
    let code = r#"
doc test
fact base = 100
rule derived = base * 2
rule another = derived?
  unless derived? > 150 then 0
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(
        result.is_ok(),
        "Rule reference types should propagate: {:?}",
        result
    );
}
