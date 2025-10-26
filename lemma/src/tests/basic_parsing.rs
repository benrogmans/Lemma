use crate::parser::{parse, parse_facts};
use crate::FactType;

#[test]
fn test_parse_simple_document() {
    let input = r#"doc person
fact name = "John"
fact age = 25"#;
    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "person");
    assert_eq!(result[0].facts.len(), 2);
}

#[test]
fn test_parse_document_with_inheritance() {
    let input = r#"doc contracts/employment/jack
fact name = "Jack""#;
    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "contracts/employment/jack");
}

#[test]
fn test_parse_document_with_commentary() {
    let input = r#"doc person
"""
This is a markdown comment
with **bold** text
"""
fact name = "John""#;
    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].commentary.is_some());
    assert!(result[0].commentary.as_ref().unwrap().contains("**bold**"));
}

#[test]
fn test_parse_document_with_rule() {
    let input = r#"doc person
rule is_adult = age >= 18"#;
    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rules.len(), 1);
    assert_eq!(result[0].rules[0].name, "is_adult");
}

#[test]
fn test_parse_multiple_documents() {
    let input = r#"doc person
fact name = "John"

doc company
fact name = "Acme Corp""#;
    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].name, "person");
    assert_eq!(result[1].name, "company");
}

#[test]
fn test_parse_error_duplicate_fact_names() {
    let input = r#"doc person
fact name = "John"
fact name = "Jane""#;
    let result = parse(input, None, &crate::ResourceLimits::default());
    assert!(
        result.is_ok(),
        "Parser should succeed even with duplicate facts"
    );
}

#[test]
fn test_parse_error_duplicate_rule_names() {
    let input = r#"doc person
rule is_adult = age >= 18
rule is_adult = age >= 21"#;
    let result = parse(input, None, &crate::ResourceLimits::default());
    assert!(
        result.is_ok(),
        "Parser should succeed even with duplicate rules"
    );
}

#[test]
fn test_parse_error_malformed_input() {
    let input = "invalid syntax here";
    let result = parse(input, None, &crate::ResourceLimits::default());
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_input() {
    let input = "";
    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 0);
}

#[test]
fn test_parse_document_with_unless_clause() {
    let input = r#"doc person
rule is_active = service_started? and not service_ended?
unless maintenance_mode then false"#;
    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rules.len(), 1);
    assert_eq!(result[0].rules[0].unless_clauses.len(), 1);
}

#[test]
fn test_parse_workspace_file() {
    let input = r#"doc person
fact name = "John Doe"
rule adult = true"#;
    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "person");
    assert_eq!(result[0].facts.len(), 1);
    assert_eq!(result[0].rules.len(), 1);
    assert_eq!(result[0].rules[0].name, "adult");
}

#[test]
fn test_multiple_unless_clauses() {
    let input = r#"doc test
rule is_eligible = age >= 18 and have license
unless emergency_mode then true
unless system_override then accept"#;

    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rules.len(), 1);
    assert_eq!(result[0].rules[0].unless_clauses.len(), 2);
}

#[test]
fn test_multiple_rules_in_document() {
    let input = r#"doc test
rule is_adult = age >= 18
rule is_senior = age >= 65
rule is_minor = age < 18
rule can_vote = age >= 18 and is_citizen"#;

    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rules.len(), 4);
    assert_eq!(result[0].rules[0].name, "is_adult");
    assert_eq!(result[0].rules[1].name, "is_senior");
    assert_eq!(result[0].rules[2].name, "is_minor");
    assert_eq!(result[0].rules[3].name, "can_vote");
}

#[test]
fn test_mixing_facts_and_rules() {
    let input = r#"doc test
fact name = "John"
rule is_adult = age >= 18
fact age = 25
rule can_drink = age >= 21
fact status = "active"
rule is_eligible = is_adult and status == "active""#;

    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].facts.len(), 3);
    assert_eq!(result[0].rules.len(), 3);
}

#[test]
fn test_type_annotations_in_facts() {
    let input = r#"doc test
fact name = [text]
fact age = [number]
fact birth_date = [date]
fact is_active = [boolean]
fact pattern = [regex]
fact discount = [percentage]
fact weight = [weight]
fact height = [length]"#;

    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].facts.len(), 8);
}

#[test]
fn test_complex_unit_type_annotations() {
    let input = r#"doc test
fact volume = [volume]
fact duration = [duration]
fact temp = [temperature]
fact power = [power]
fact energy = [energy]
fact force = [force]
fact pressure = [pressure]
fact freq = [frequency]
fact data = [data_size]
fact price = [money]"#;

    let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].facts.len(), 10);
}

#[test]
fn test_whitespace_handling_comprehensive() {
    let test_cases = vec![
        ("doc test\nrule test = 2+3", "no spaces in arithmetic"),
        ("doc test\nrule test = age>=18", "no spaces in comparison"),
        (
            "doc test\nrule test = age >= 18 and salary>50000",
            "spaces around and keyword",
        ),
        (
            "doc test\nrule test = age  >=  18  and  salary  >  50000",
            "extra spaces",
        ),
        (
            "doc test\nrule test = \n  age >= 18 \n  and \n  salary > 50000",
            "newlines in expression",
        ),
    ];

    for (input, description) in test_cases {
        let result = parse(input, None, &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse {} ({}): {:?}",
            input,
            description,
            result.err()
        );
    }
}

#[test]
fn test_parse_facts_from_strings() {
    let result = parse_facts(&["x=5"]);
    assert!(
        result.is_ok(),
        "Failed to parse regular fact: {:?}",
        result.err()
    );
    let facts = result.unwrap();
    assert_eq!(facts.len(), 1);
    match &facts[0].fact_type {
        FactType::Local(name) => {
            assert_eq!(name, "x");
        }
        _ => panic!("Expected Local fact type"),
    }

    let result = parse_facts(&["test.x=5", "foo.bar=10"]);
    assert!(
        result.is_ok(),
        "Failed to parse fact overrides: {:?}",
        result.err()
    );
    let facts = result.unwrap();
    assert_eq!(facts.len(), 2);
    match &facts[0].fact_type {
        FactType::Foreign(override_ref) => {
            assert_eq!(override_ref.reference, vec!["test", "x"]);
        }
        _ => panic!("Expected Foreign fact type"),
    }
    match &facts[1].fact_type {
        FactType::Foreign(override_ref) => {
            assert_eq!(override_ref.reference, vec!["foo", "bar"]);
        }
        _ => panic!("Expected Foreign fact type"),
    }
}
