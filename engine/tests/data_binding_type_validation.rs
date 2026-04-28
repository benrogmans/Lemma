use lemma::parsing::ast::DateTimeValue;
/// Comprehensive tests for data binding type validation
///
/// These tests ensure that the engine correctly validates that data bindings
/// match the expected types declared in the spec, preventing type confusion bugs.
use lemma::Engine;
use lemma::ErrorKind;
use std::collections::HashMap;

#[test]
fn test_number_type_validation_rejects_text() {
    let code = r#"
spec test
data age: number
rule doubled: age * 2
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("age".to_string(), "twenty".to_string());

    let now = DateTimeValue::now();
    let result = engine.run("test", Some(&now), data, false);

    assert!(result.is_err(), "Expected error but got: {:?}", result);
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Failed to parse data 'age'"),
        "Error was: {}",
        error
    );
}

#[test]
fn test_multiple_type_validations() {
    let code = r#"
spec test
data price: number
data quantity: number
data active: boolean
rule total: price * quantity
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("price".to_string(), "expensive".to_string());
    data.insert("quantity".to_string(), "5".to_string());
    data.insert("active".to_string(), "true".to_string());

    let now = DateTimeValue::now();
    let result = engine.run("test", Some(&now), data, false);
    assert!(result.is_err(), "Expected type mismatch error");
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Failed to parse data 'price'"));

    let mut data = HashMap::new();
    data.insert("price".to_string(), "100".to_string());
    data.insert("quantity".to_string(), "five".to_string());
    data.insert("active".to_string(), "true".to_string());

    let err = engine
        .run("test", Some(&now), data, false)
        .expect_err("quantity must reject non-number");
    assert!(err.to_string().contains("Failed to parse data 'quantity'"));

    let mut data = HashMap::new();
    data.insert("price".to_string(), "100".to_string());
    data.insert("quantity".to_string(), "5".to_string());
    data.insert("active".to_string(), "maybe".to_string());

    let err = engine
        .run("test", Some(&now), data, false)
        .expect_err("active must reject non-boolean");
    assert!(err.to_string().contains("Failed to parse data 'active'"));

    let mut data = HashMap::new();
    data.insert("price".to_string(), "100".to_string());
    data.insert("quantity".to_string(), "5".to_string());
    data.insert("active".to_string(), "true".to_string());

    let response = engine
        .run("test", Some(&now), data, false)
        .expect("valid data must evaluate");
    let total = response
        .results
        .get("total")
        .expect("total rule")
        .result
        .value()
        .expect("total value");
    assert_eq!(total.to_string(), "500");
}

#[test]
fn test_literal_data_type_validation() {
    let code = r#"
spec test
data base_price: 50
rule total: base_price * 1.2
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("base_price".to_string(), "sixty".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("test", Some(&now), data, false)
        .expect_err("base_price must reject non-number");
    assert!(err
        .to_string()
        .contains("Failed to parse data 'base_price'"));

    let mut data = HashMap::new();
    data.insert("base_price".to_string(), "60".to_string());

    let response = engine
        .run("test", Some(&now), data, false)
        .expect("valid base_price must evaluate");
    let total = response
        .results
        .get("total")
        .expect("total rule")
        .result
        .value()
        .expect("total value");
    assert!(
        total.to_string().starts_with("72"),
        "60 * 1.2 = 72, got {}",
        total
    );
}

#[test]
fn test_unknown_data_binding_rejected() {
    let code = r#"
spec test
data price: number
rule total: price * 1.1
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("test.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("price".to_string(), "100".to_string());
    data.insert("unknown_data".to_string(), "42".to_string());

    let now = DateTimeValue::now();
    let result = engine.run("test", Some(&now), data, false);
    assert!(result.is_err(), "Expected error for unknown data binding");
    assert!(result.unwrap_err().to_string().contains("unknown_data"));
}

/// Matrix: primitive × applicable constraint × violating user value.
/// Each row asserts the (load accepted, run rejected with constraint name)
/// behavior for a valid constraint-primitive pairing, and the (load rejected)
/// behavior for an incompatible pairing.
///
/// Tests that encode intended behavior stay red when the planner silently
/// accepts an invalid combination — that's the deliverable.

#[test]
fn percent_minimum_violation_on_override() {
    let code = r#"
spec s
data p: percent -> minimum 10%
rule r: p
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("m.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("p".to_string(), "5%".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("5% < 10%");
    let s = err.to_string();
    assert!(
        s.contains("minimum") || s.contains("at least"),
        "expected minimum violation, got: {s}"
    );
}

/// Pin that runtime override `"5%"` parses as `Ratio(0.05, "percent")` exactly.
/// Without this, a 100x regression (storing `Ratio(5, "percent")`) would silently
/// SATISFY a `minimum 10%` constraint (5 > 0.10), making the constraint-violation
/// tests above pass via the wrong path.
#[test]
fn percent_override_value_is_pinned() {
    use lemma::evaluation::OperationResult;
    use lemma::ValueKind;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    let code = r#"
spec s
data p: percent
rule r: p
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("m.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("p".to_string(), "5%".to_string());

    let now = DateTimeValue::now();
    let resp = engine
        .run("s", Some(&now), data, false)
        .expect("'5%' must parse on a percent type without constraints");
    let rr = resp.results.get("r").expect("rule 'r' not found");
    let lit = match &rr.result {
        OperationResult::Value(v) => v.as_ref(),
        OperationResult::Veto(v) => panic!("unexpected veto: {v}"),
    };
    match &lit.value {
        ValueKind::Ratio(n, u) => {
            assert_eq!(*n, Decimal::from_str("0.05").unwrap());
            assert_eq!(u.as_deref(), Some("percent"));
        }
        other => panic!("expected Ratio, got: {:?}", other),
    }
}

#[test]
fn percent_maximum_violation_on_override() {
    let code = r#"
spec s
data p: percent -> maximum 50%
rule r: p
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("m.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("p".to_string(), "90%".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("90% > 50%");
    let s = err.to_string();
    assert!(
        s.contains("maximum") || s.contains("at most") || s.contains("exceeds"),
        "expected maximum violation, got: {s}"
    );
}

#[test]
fn duration_minimum_violation_on_override() {
    let code = r#"
spec s
data d: duration -> minimum 1 day
rule r: d
"#;
    let mut engine = Engine::new();
    let load_result = engine.load(code, lemma::SourceType::Labeled("m.lemma"));
    if let Err(errors) = &load_result {
        // If `minimum` with duration literal RHS is not supported, that
        // itself is a landmine — durations can definitely have minimums.
        panic!(
            "duration minimum must be supported; load failed with: {}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    let mut data = HashMap::new();
    data.insert("d".to_string(), "12 hours".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("12 hours < 1 day");
    let s = err.to_string();
    assert!(
        s.contains("minimum") || s.contains("at least"),
        "expected minimum violation, got: {s}"
    );
}

#[test]
fn date_minimum_violation_on_override() {
    let code = r#"
spec s
data when: date -> minimum 2024-01-01
rule r: when
"#;
    let mut engine = Engine::new();
    let load_result = engine.load(code, lemma::SourceType::Labeled("m.lemma"));
    if let Err(errors) = &load_result {
        panic!(
            "date minimum must be supported; load failed with: {}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    let mut data = HashMap::new();
    data.insert("when".to_string(), "2023-06-15".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("date before minimum");
    let s = err.to_string();
    assert!(
        s.contains("minimum") || s.contains("at least") || s.contains("2024"),
        "expected minimum-date violation, got: {s}"
    );
}

#[test]
fn number_decimals_constraint_truncation_or_rejection() {
    // `decimals 2` on a number: pin behavior. Either the value is stored as
    // at most 2 decimals (rounded/truncated) or the override is rejected.
    // Silent precision gain (keeping 3.14159) is a bug.
    let code = r#"
spec s
data n: number -> decimals 2
rule r: n
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("m.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("n".to_string(), "3.14159".to_string());

    let now = DateTimeValue::now();
    match engine.run("s", Some(&now), data, false) {
        Ok(resp) => {
            let rr = resp.results.get("r").expect("rule 'r'");
            match &rr.result {
                lemma::OperationResult::Value(v) => {
                    let s = v.to_string();
                    assert!(
                        !s.contains("3.14159"),
                        "decimals 2 must not preserve 5 decimals; got: {s}"
                    );
                }
                other => panic!("expected value, got: {:?}", other),
            }
        }
        Err(e) => {
            let s = e.to_string();
            assert!(
                s.contains("decimals") || s.contains("precision"),
                "rejection must reference the decimals constraint, got: {s}"
            );
        }
    }
}

#[test]
fn text_length_exactly_at_boundary_accepted() {
    let code = r#"
spec s
data msg: text -> length 5
rule r: msg
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("m.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("msg".to_string(), "exact".to_string());

    let now = DateTimeValue::now();
    let resp = engine
        .run("s", Some(&now), data, false)
        .expect("5-char string must be accepted");
    let rr = resp.results.get("r").expect("rule 'r'");
    match &rr.result {
        lemma::OperationResult::Value(v) => assert_eq!(v.to_string(), "exact"),
        other => panic!("expected value, got: {:?}", other),
    }
}

#[test]
fn scale_override_with_wrong_unit_rejected() {
    let code = r#"
spec s
data money: scale -> unit eur 1 -> unit usd 1.19
data price: money
rule r: price
"#;
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("m.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    // `meter` is not a unit of `money`.
    data.insert("price".to_string(), "100 meter".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("s", Some(&now), data, false)
        .expect_err("wrong scale unit must fail");
    let s = err.to_string();
    assert!(
        s.contains("unit") || s.contains("meter"),
        "expected unit-mismatch error, got: {s}"
    );
}

#[test]
fn test_structured_error_related_data_attribution() {
    let code = r#"
spec bridge
data bridge_height: scale -> unit meter 1.0
rule span: bridge_height
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("workspace.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("bridge_height".to_string(), "4 mete".to_string());

    let now = DateTimeValue::now();
    let err = engine
        .run("bridge", Some(&now), data, false)
        .expect_err("bad scale unit must error");

    assert_eq!(err.related_data(), Some("bridge_height"));
    assert_eq!(err.kind(), ErrorKind::Validation);
    assert!(
        err.message().starts_with("Unknown unit"),
        "message must be the inner reason only (no 'Failed to parse data' wrapper), got: {}",
        err.message()
    );

    let display = err.to_string();
    assert_eq!(
        display.matches(" at ").count(),
        1,
        "Display must include the source location exactly once, got: {display}"
    );
    assert!(
        display.contains("Failed to parse data 'bridge_height':"),
        "Display must carry the data-binding prefix, got: {display}"
    );
}
