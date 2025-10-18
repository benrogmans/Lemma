use crate::parser::parse;
use crate::{ExpressionKind, LiteralValue};

#[test]
fn test_veto_in_unless_clauses() {
    let input = r#"doc test
rule is_adult = age >= 18 unless age < 0 then veto "Age must be 0 or higher""#;
    let result = parse(input, None);
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
    let result = parse(input, None);
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
    let result = parse(input, None);
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
    let result = parse(input, None);
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
        ExpressionKind::Literal(LiteralValue::Number(n)) => {
            assert_eq!(*n, rust_decimal::Decimal::new(100, 0));
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
        ("doc test\nrule test = 5 in invalidunit", "invalid unit"),
        ("doc test\nfact doc = 123", "reserved keyword as fact name"),
        (
            "doc test\nrule rule = true",
            "reserved keyword as rule name",
        ),
    ];

    for (input, description) in error_cases {
        let result = parse(input, None);
        assert!(
            result.is_err(),
            "Expected error for {} but got success",
            description
        );
    }
}
