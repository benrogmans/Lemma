use lemma::Engine;

#[test]
fn test_reference_not_found_fails() {
    let mut engine = Engine::new();

    let lemma_code = r#"
spec test_validation

data base: 100

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
fn test_ambiguous_data_and_rule_fails() {
    let mut engine = Engine::new();

    let lemma_code = r#"
spec test_validation

data ambiguous: 10
rule ambiguous: 20

rule usage: ambiguous + 1
"#;

    let result = engine.load(lemma_code, lemma::SourceType::Labeled("test.lemma"));
    assert!(
        result.is_err(),
        "Reference that is both data and rule should fail"
    );
    let errs = result.unwrap_err();
    let err_msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        err_msg.contains("'ambiguous' is both a data and a rule"),
        "Error should state the name is both a data and a rule: {}",
        err_msg
    );
}
