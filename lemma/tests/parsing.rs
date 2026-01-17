//! Integration tests for the parsing module
//!
//! Tests the parsing module end-to-end, including document parsing,
//! fact parsing, rule parsing, and error handling.

use lemma::{parse, ExpressionKind, Value};

#[test]
fn test_parse_simple_document() {
    let input = r#"doc person
fact name = "John"
fact age = 25"#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "person");
    assert_eq!(result[0].facts.len(), 2);
}

#[test]
fn test_parse_document_with_inheritance() {
    let input = r#"doc contracts/employment/jack
fact name = "Jack""#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
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
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].commentary.is_some());
    assert!(result[0].commentary.as_ref().unwrap().contains("**bold**"));
}

#[test]
fn test_parse_document_with_rule() {
    let input = r#"doc person
rule is_adult = age >= 18"#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
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
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].name, "person");
    assert_eq!(result[1].name, "company");
}

#[test]
fn test_parse_error_duplicate_fact_names() {
    let input = r#"doc person
fact name = "John"
fact name = "Jane""#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default());
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
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default());
    assert!(
        result.is_ok(),
        "Parser should succeed even with duplicate rules"
    );
}

#[test]
fn test_parse_error_malformed_input() {
    let input = "invalid syntax here";
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default());
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_input() {
    let input = "";
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 0);
}

#[test]
fn test_parse_document_with_unless_clause() {
    let input = r#"doc person
rule is_active = service_started? and not service_ended?
unless maintenance_mode then false"#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rules.len(), 1);
    assert_eq!(result[0].rules[0].unless_clauses.len(), 1);
}

#[test]
fn test_parse_workspace_file() {
    let input = r#"doc person
fact name = "John Doe"
rule adult = true"#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "person");
    assert_eq!(result[0].facts.len(), 1);
    assert_eq!(result[0].rules.len(), 1);
    assert_eq!(result[0].rules[0].name, "adult");
}

#[test]
fn test_multiple_unless_clauses() {
    let input = r#"doc test
rule is_eligible = age >= 18 and has_license
unless emergency_mode then true
unless system_override then accept"#;

    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
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

    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
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

    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
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
fact discount = [percent]
fact duration = [duration]"#;

    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].facts.len(), 6);
}

#[test]
fn test_complex_unit_type_annotations() {
    // After removing hardcoded units, only duration remains as a built-in type annotation
    let input = r#"doc test
fact duration = [duration]
fact number = [number]
fact text = [text]
fact date = [date]
fact boolean = [boolean]
fact percentage = [percent]"#;

    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].facts.len(), 6);
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
        let result = parse(input, "test.lemma", &lemma::ResourceLimits::default());
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
fn test_veto_in_unless_clauses() {
    let input = r#"doc test
rule is_adult = age >= 18 unless age < 0 then veto "Age must be 0 or higher""#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default());
    assert!(
        result.is_ok(),
        "Failed to parse single veto: {:?}",
        result.err()
    );

    let docs = result.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].rules.len(), 1);

    let rule = &docs[0].rules[0];
    assert_eq!(rule.name, "is_adult");
    assert_eq!(rule.unless_clauses.len(), 1);

    match &rule.unless_clauses[0].result.kind {
        ExpressionKind::Veto(veto) => {
            assert_eq!(veto.message, Some("Age must be 0 or higher".to_string()));
        }
        _ => panic!(
            "Expected veto expression, got {:?}",
            rule.unless_clauses[0].result
        ),
    }

    let input = r#"doc test
rule is_adult = age >= 18
  unless age > 150 then veto "Age cannot be over 150"
  unless age < 0 then veto "Age must be 0 or higher""#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default());
    assert!(
        result.is_ok(),
        "Failed to parse multiple vetoes: {:?}",
        result.err()
    );

    let docs = result.unwrap();
    let rule = &docs[0].rules[0];
    assert_eq!(rule.unless_clauses.len(), 2);

    match &rule.unless_clauses[0].result.kind {
        ExpressionKind::Veto(veto) => {
            assert_eq!(veto.message, Some("Age cannot be over 150".to_string()));
        }
        _ => panic!("Expected veto expression"),
    }

    match &rule.unless_clauses[1].result.kind {
        ExpressionKind::Veto(veto) => {
            assert_eq!(veto.message, Some("Age must be 0 or higher".to_string()));
        }
        _ => panic!("Expected veto expression"),
    }
}

#[test]
fn test_veto_without_message() {
    let input = r#"doc test
rule adult = age >= 18 unless age > 150 then veto"#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default());
    assert!(
        result.is_ok(),
        "Failed to parse veto without message: {:?}",
        result.err()
    );

    let docs = result.unwrap();
    let rule = &docs[0].rules[0];
    assert_eq!(rule.unless_clauses.len(), 1);

    match &rule.unless_clauses[0].result.kind {
        ExpressionKind::Veto(veto) => {
            assert_eq!(veto.message, None);
        }
        _ => panic!("Expected veto expression"),
    }
}

#[test]
fn test_mixed_veto_and_regular_unless() {
    let input = r#"doc test
rule adjusted_age = age + 1
  unless age < 0 then veto "Invalid age"
  unless age > 100 then 100"#;
    let result = parse(input, "test.lemma", &lemma::ResourceLimits::default());
    assert!(
        result.is_ok(),
        "Failed to parse mixed unless: {:?}",
        result.err()
    );

    let docs = result.unwrap();
    let rule = &docs[0].rules[0];
    assert_eq!(rule.unless_clauses.len(), 2);

    match &rule.unless_clauses[0].result.kind {
        ExpressionKind::Veto(veto) => {
            assert_eq!(veto.message, Some("Invalid age".to_string()));
        }
        _ => panic!("Expected veto expression"),
    }

    match &rule.unless_clauses[1].result.kind {
        ExpressionKind::Literal(lit) => {
            if let Value::Number(n) = &lit.value {
                assert_eq!(*n, rust_decimal::Decimal::new(100, 0));
            } else {
                panic!("Expected literal number");
            }
        }
        _ => panic!("Expected literal number"),
    }
}

#[test]
fn test_error_cases_comprehensive() {
    let error_cases = vec![
        (
            "doc test\nfact name = \"unclosed string",
            "unclosed string literal",
        ),
        ("doc test\nrule test = 2 + + 3", "double operator"),
        ("doc test\nrule test = (2 + 3", "unclosed parenthesis"),
        ("doc test\nrule test = 2 + 3)", "extra closing paren"),
        // Note: "invalid unit" now parses as a user-defined unit (validated during planning)
        ("doc test\nfact doc = 123", "reserved keyword as fact name"),
        (
            "doc test\nrule rule = true",
            "reserved keyword as rule name",
        ),
    ];

    for (input, description) in error_cases {
        let result = parse(input, "test.lemma", &lemma::ResourceLimits::default());
        assert!(
            result.is_err(),
            "Expected error for {} but got success",
            description
        );
    }
}

#[test]
fn test_duration_literals_in_rules() {
    // After removing hardcoded units, only duration units remain as built-in
    let test_cases = vec![
        ("2 years", "years"),
        ("6 months", "months"),
        ("52 weeks", "weeks"),
        ("365 days", "days"),
        ("24 hours", "hours"),
        ("60 minutes", "minutes"),
        ("3600 seconds", "seconds"),
        ("1000 milliseconds", "milliseconds"),
        ("500000 microseconds", "microseconds"),
        ("50 percent", "percent"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, "test.lemma", &lemma::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse literal {} ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}

#[test]
fn test_comparison_with_unit_conversions() {
    // After removing hardcoded units, only duration conversions remain as built-in
    let test_cases = vec![
        (
            "(duration in hours) > 2",
            "duration conversion in comparison with parens",
        ),
        (
            "(meeting_time in minutes) >= 30",
            "duration conversion with gte",
        ),
        (
            "(project_length in days) < 100",
            "duration conversion with lt",
        ),
        (
            "(delay in seconds) == 60",
            "duration conversion with equality",
        ),
        (
            "(1 hours) > (30 minutes)",
            "duration conversions on both sides",
        ),
        (
            "duration in hours > 2",
            "duration conversion without parens",
        ),
        (
            "meeting_time in seconds > 3600",
            "variable duration conversion in comparison",
        ),
        (
            "project_length in days > deadline_days",
            "two variables with duration conversion",
        ),
        (
            "duration in hours >= 1 and duration in hours <= 8",
            "multiple duration comparisons",
        ),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, "test.lemma", &lemma::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Failed to parse {} ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}
