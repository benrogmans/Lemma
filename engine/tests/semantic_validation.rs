use lemma::Engine;

#[test]
fn test_rule_reference_without_question_mark_fails() {
    let mut engine = Engine::new();

    let lemma_code = r#"
doc test_validation

fact base = 100

rule calculated = base * 2

rule buggy_usage = calculated + 50
"#;

    let result = engine.add_lemma_code(lemma_code, "test.lemma");

    assert!(
        result.is_err(),
        "Should fail when referencing a rule without ?"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("calculated") && (err_msg.contains("rule") || err_msg.contains("?")),
        "Error should mention that 'calculated' is a rule and needs ?: {}",
        err_msg
    );
}

#[test]
fn test_fact_reference_with_question_mark_fails() {
    let mut engine = Engine::new();

    let lemma_code = r#"
doc test_validation

fact base = 100
fact multiplier = 2

rule buggy_usage = base? * multiplier?
"#;

    let result = engine.add_lemma_code(lemma_code, "test.lemma");

    assert!(
        result.is_err(),
        "Should fail when referencing a fact with ?"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("base") || err_msg.contains("multiplier"),
        "Error should mention the fact name: {}",
        err_msg
    );
}

#[test]
fn test_correct_rule_reference_with_question_mark_succeeds() {
    let mut engine = Engine::new();

    let lemma_code = r#"
doc test_validation

fact base = 100

rule calculated = base * 2

rule correct_usage = calculated? + 50
"#;

    let result = engine.add_lemma_code(lemma_code, "test.lemma");
    assert!(
        result.is_ok(),
        "Should succeed when using ? for rule reference: {:?}",
        result
    );
}

#[test]
fn test_correct_fact_reference_without_question_mark_succeeds() {
    let mut engine = Engine::new();

    let lemma_code = r#"
doc test_validation

fact base = 100
fact multiplier = 2

rule correct_usage = base * multiplier
"#;

    let result = engine.add_lemma_code(lemma_code, "test.lemma");
    assert!(
        result.is_ok(),
        "Should succeed when not using ? for fact reference: {:?}",
        result
    );
}

#[test]
fn test_rule_reference_in_unless_clause_without_question_mark_fails() {
    let mut engine = Engine::new();

    let lemma_code = r#"
doc test_validation

fact amount = 100

rule is_valid = amount > 50

rule discount = 0%
  unless is_valid then 10%
"#;

    let result = engine.add_lemma_code(lemma_code, "test.lemma");

    assert!(
        result.is_err(),
        "Should fail when referencing a rule without ? in unless condition"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("is_valid"),
        "Error should mention 'is_valid': {}",
        err_msg
    );
}

#[test]
fn test_document_field_rule_reference_without_question_mark_fails() {
    let mut engine = Engine::new();

    let lemma_code = r#"
doc base_doc
fact salary = 5000
rule annual = salary * 12

doc main_doc
fact employee = doc base_doc
rule buggy = employee.annual + 1000
"#;

    let result = engine.add_lemma_code(lemma_code, "test.lemma");

    assert!(
        result.is_err(),
        "Should fail when referencing document rule without ?"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("annual") || err_msg.contains("employee.annual"),
        "Error should mention the rule reference: {}",
        err_msg
    );
}

#[test]
fn test_document_field_fact_reference_with_question_mark_fails() {
    let mut engine = Engine::new();

    let lemma_code = r#"
doc base_doc
fact salary = 5000

doc main_doc
fact employee = doc base_doc
rule buggy = employee.salary? * 2
"#;

    let result = engine.add_lemma_code(lemma_code, "test.lemma");

    assert!(
        result.is_err(),
        "Should fail when referencing document fact with ?"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("salary") || err_msg.contains("employee.salary"),
        "Error should mention the fact reference: {}",
        err_msg
    );
}
