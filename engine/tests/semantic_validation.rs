use lemma::Engine;

#[test]
fn test_reference_to_rule_succeeds() {
    let mut engine = Engine::new();

    let lemma_code = r#"
spec test_validation

fact base: 100

rule calculated: base * 2

rule correct_usage: calculated + 50
"#;

    let result = engine.load(lemma_code, lemma::SourceType::Labeled("test.lemma"));
    assert!(
        result.is_ok(),
        "Reference to rule should succeed: {:?}",
        result
    );
}

#[test]
fn test_reference_to_fact_succeeds() {
    let mut engine = Engine::new();

    let lemma_code = r#"
spec test_validation

fact base: 100
fact multiplier: 2

rule correct_usage: base * multiplier
"#;

    let result = engine.load(lemma_code, lemma::SourceType::Labeled("test.lemma"));
    assert!(
        result.is_ok(),
        "Reference to facts should succeed: {:?}",
        result
    );
}

#[test]
fn test_reference_in_unless_to_rule_succeeds() {
    let mut engine = Engine::new();

    let lemma_code = r#"
spec test_validation

fact amount: 100

rule is_valid: amount > 50

rule discount: 0%
  unless is_valid then 10%
"#;

    let result = engine.load(lemma_code, lemma::SourceType::Labeled("test.lemma"));
    assert!(
        result.is_ok(),
        "Reference to rule in unless condition should succeed: {:?}",
        result
    );
}

#[test]
fn test_cross_spec_reference_to_rule_succeeds() {
    let mut engine = Engine::new();

    let lemma_code = r#"
spec base_spec
fact salary: 5000
rule annual: salary * 12

spec main_spec
fact employee: spec base_spec
rule total: employee.annual + 1000
"#;

    let result = engine.load(lemma_code, lemma::SourceType::Labeled("test.lemma"));
    assert!(
        result.is_ok(),
        "Cross-spec reference to rule should succeed: {:?}",
        result
    );
}

#[test]
fn test_reference_not_found_fails() {
    let mut engine = Engine::new();

    let lemma_code = r#"
spec test_validation

fact base: 100

rule usage: nonexistent + 1
"#;

    let result = engine.load(lemma_code, lemma::SourceType::Labeled("test.lemma"));
    assert!(
        result.is_err(),
        "Reference to non-existent name should fail"
    );
    let errs = result.unwrap_err();
    let err_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        err_msg.contains("nonexistent"),
        "Error should mention the reference name: {}",
        err_msg
    );
}

#[test]
fn test_ambiguous_fact_and_rule_fails() {
    let mut engine = Engine::new();

    let lemma_code = r#"
spec test_validation

fact ambiguous: 10
rule ambiguous: 20

rule usage: ambiguous + 1
"#;

    let result = engine.load(lemma_code, lemma::SourceType::Labeled("test.lemma"));
    assert!(
        result.is_err(),
        "Reference that is both fact and rule should fail"
    );
    let errs = result.unwrap_err();
    let err_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        err_msg.contains("'ambiguous' is both a fact and a rule"),
        "Error should state the name is both a fact and a rule: {}",
        err_msg
    );
}
