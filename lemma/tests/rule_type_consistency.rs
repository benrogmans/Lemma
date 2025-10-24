use lemma::*;

#[test]
fn test_consistent_number_types() {
    let code = r#"
doc test
fact x = 10
fact condition = true

rule result = 5
    unless condition then 10
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}

#[test]
fn test_consistent_text_types() {
    let code = r#"
doc test
fact condition = true

rule status = "pending"
    unless condition then "approved"
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}

#[test]
fn test_consistent_boolean_types() {
    let code = r#"
doc test
fact x = 10
fact y = 20

rule check = x > 5
    unless y > 15 then y < 25
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}

#[test]
fn test_mixed_number_and_text_rejected() {
    let code = r#"
doc test
fact condition = true

rule result = 100
    unless condition then "text"
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("type") || err.to_string().contains("incompatible"));
}

#[test]
fn test_mixed_text_and_boolean_rejected() {
    let code = r#"
doc test
fact condition = true

rule result = "text"
    unless condition then true
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("type") || err.to_string().contains("incompatible"));
}

#[test]
fn test_mixed_number_and_boolean_rejected() {
    let code = r#"
doc test
fact condition = true

rule result = 42
    unless condition then false
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("type") || err.to_string().contains("incompatible"));
}

#[test]
fn test_multiple_unless_clauses_consistent() {
    let code = r#"
doc test
fact a = true
fact b = false

rule result = 1
    unless a then 2
    unless b then 3
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}

#[test]
fn test_multiple_unless_clauses_inconsistent() {
    let code = r#"
doc test
fact a = true
fact b = false

rule result = 1
    unless a then 2
    unless b then "three"
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("type") || err.to_string().contains("incompatible"));
}

#[test]
fn test_veto_with_consistent_types() {
    let code = r#"
doc test
fact blocked = true
fact condition = false

rule result = 10
    unless blocked then veto "blocked"
    unless condition then 20
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}

#[test]
fn test_veto_with_mixed_types() {
    let code = r#"
doc test
fact blocked = true
fact condition = false

rule result = 10
    unless blocked then veto "blocked"
    unless condition then "text"
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("type") || err.to_string().contains("incompatible"));
}

#[test]
fn test_all_veto_clauses_allowed() {
    let code = r#"
doc test
fact a = true
fact b = false

rule result = 10
    unless a then veto "a"
    unless b then veto "b"
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}

#[test]
fn test_consistent_money_types() {
    let code = r#"
doc test
fact condition = true

rule price = 100 USD
    unless condition then 200 USD
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}

#[test]
fn test_mixed_money_and_number_rejected() {
    let code = r#"
doc test
fact condition = true

rule price = 100 USD
    unless condition then 200
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("type") || err.to_string().contains("incompatible"));
}

#[test]
fn test_consistent_mass_types() {
    let code = r#"
doc test
fact heavy = true

rule weight = 10 kilograms
    unless heavy then 20 kilograms
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}

#[test]
fn test_mixed_mass_and_number_rejected() {
    let code = r#"
doc test
fact heavy = true

rule weight = 10 kilograms
    unless heavy then 20
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("type") || err.to_string().contains("incompatible"));
}

#[test]
fn test_complex_expression_consistent_types() {
    let code = r#"
doc test
fact x = 10
fact y = 20
fact condition = true

rule result = x + y
    unless condition then x * 2
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}

#[test]
fn test_comparison_expression_consistent_types() {
    let code = r#"
doc test
fact x = 10
fact condition = true

rule check = x > 5
    unless condition then x < 20
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");
    assert!(result.is_ok());
}
