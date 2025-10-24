use lemma::{parse, Validator};

#[test]
fn test_semantic_validator_basic_validation() {
    let input = r#"doc person
fact name = "John"
fact age = 25
rule is_adult = age >= 18"#;

    let docs = parse(input, Some("test.lemma".to_string())).unwrap();
    let validator = Validator::new();
    let result = validator.validate_all(docs);

    assert!(
        result.is_ok(),
        "Basic validation should pass: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_validator_duplicate_facts() {
    let input = r#"doc person
fact name = "John"
fact name = "Jane""#;

    let docs = parse(input, Some("test.lemma".to_string())).unwrap();
    let validator = Validator::new();
    let result = validator.validate_all(docs);

    assert!(
        result.is_err(),
        "Duplicate facts should cause validation error"
    );
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Duplicate fact definition"));
    assert!(error.to_string().contains("name"));
}

#[test]
fn test_semantic_validator_duplicate_rules() {
    let input = r#"doc person
rule is_adult = age >= 18
rule is_adult = age >= 21"#;

    let docs = parse(input, Some("test.lemma".to_string())).unwrap();
    let validator = Validator::new();
    let result = validator.validate_all(docs);

    assert!(
        result.is_err(),
        "Duplicate rules should cause validation error"
    );
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Duplicate rule definition"));
    assert!(error.to_string().contains("is_adult"));
}

#[test]
fn test_semantic_validator_circular_dependency() {
    let input = r#"doc test
rule a = b?
rule b = a?"#;

    let docs = parse(input, Some("test.lemma".to_string())).unwrap();
    let validator = Validator::new();
    let result = validator.validate_all(docs);

    assert!(
        result.is_err(),
        "Circular dependency should cause validation error"
    );
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Circular dependency detected"));
    // The cycle can be detected as either "a -> b -> a" or "b -> a -> b" depending on processing order
    assert!(error.to_string().contains("a -> b -> a") || error.to_string().contains("b -> a -> b"));
}

#[test]
fn test_semantic_validator_reference_type_errors() {
    let input = r#"doc test
fact age = 25
rule is_adult = age >= 18
rule test1 = age?
rule test2 = is_adult"#;

    let docs = parse(input, Some("test.lemma".to_string())).unwrap();
    let validator = Validator::new();
    let result = validator.validate_all(docs);

    assert!(
        result.is_err(),
        "Reference type errors should cause validation error"
    );
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Reference error"));
}

#[test]
fn test_semantic_validator_multiple_documents() {
    let input = r#"doc person
fact name = "John"
fact age = 25

doc company
fact name = "Acme Corp"
fact employee = doc person"#;

    let docs = parse(input, Some("test.lemma".to_string())).unwrap();
    let validator = Validator::new();
    let result = validator.validate_all(docs);

    assert!(
        result.is_ok(),
        "Multiple documents should validate successfully: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_validator_invalid_document_reference() {
    let input = r#"doc person
fact name = "John"
fact contract = doc nonexistent"#;

    let docs = parse(input, Some("test.lemma".to_string())).unwrap();
    let validator = Validator::new();
    let result = validator.validate_all(docs);

    assert!(
        result.is_err(),
        "Invalid document reference should cause validation error"
    );
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Document reference error"));
    assert!(error.to_string().contains("nonexistent"));
}

#[test]
fn test_semantic_validator_fact_rule_name_conflict() {
    let input = r#"doc test
fact price = 100
rule price = 200"#;

    let docs = parse(input, Some("test.lemma".to_string())).unwrap();
    let validator = Validator::new();
    let result = validator.validate_all(docs);

    assert!(
        result.is_err(),
        "Fact and rule with same name should cause validation error"
    );
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Name conflict"));
    assert!(error.to_string().contains("price"));
}

#[test]
fn test_semantic_validator_fact_rule_name_conflict_usage() {
    let input = r#"doc test
fact price = 100
rule price = 200
rule total = price + 50"#;

    let docs = parse(input, Some("test.lemma".to_string())).unwrap();
    let validator = Validator::new();
    let result = validator.validate_all(docs);

    assert!(
        result.is_err(),
        "Should fail validation during definition, not usage"
    );
    let error = result.unwrap_err();
    assert!(error.to_string().contains("Name conflict") || error.to_string().contains("Duplicate"));
}
