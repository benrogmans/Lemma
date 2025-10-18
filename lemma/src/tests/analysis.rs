use crate::analysis::*;
use crate::{
    Expression, ExpressionKind, FactReference, FactType, FactValue, LemmaFact, LemmaRule,
    LemmaType, RuleReference,
};

#[test]
fn test_extract_fact_references() {
    let expr = Expression::new(
        ExpressionKind::FactReference(FactReference {
            reference: vec!["price".to_string()],
        }),
        None,
        crate::ExpressionId::new(0),
    );

    let refs = extract_references(&expr);
    assert_eq!(refs.facts.len(), 1);
    assert_eq!(refs.rules.len(), 0);
}

#[test]
fn test_extract_rule_references() {
    let expr = Expression::new(
        ExpressionKind::RuleReference(RuleReference {
            reference: vec!["total".to_string()],
        }),
        None,
        crate::ExpressionId::new(0),
    );

    let refs = extract_references(&expr);
    assert_eq!(refs.facts.len(), 0);
    assert_eq!(refs.rules.len(), 1);
}

#[test]
fn test_recursive_fact_finding() {
    // Setup: rule_c depends on rule_b which depends on rule_a which depends on fact_x
    // Should find that rule_c requires fact_x transitively

    let fact_x = LemmaFact::new(
        FactType::Local("x".to_string()),
        FactValue::TypeAnnotation(crate::TypeAnnotation::LemmaType(LemmaType::Number)),
    );

    let rule_a = LemmaRule {
        name: "a".to_string(),
        expression: Expression::new(
            ExpressionKind::FactReference(FactReference {
                reference: vec!["x".to_string()],
            }),
            None,
            crate::ExpressionId::new(0),
        ),
        unless_clauses: vec![],
        span: None,
    };

    let rule_b = LemmaRule {
        name: "b".to_string(),
        expression: Expression::new(
            ExpressionKind::RuleReference(RuleReference {
                reference: vec!["a".to_string()],
            }),
            None,
            crate::ExpressionId::new(1),
        ),
        unless_clauses: vec![],
        span: None,
    };

    let rule_c = LemmaRule {
        name: "c".to_string(),
        expression: Expression::new(
            ExpressionKind::RuleReference(RuleReference {
                reference: vec!["b".to_string()],
            }),
            None,
            crate::ExpressionId::new(2),
        ),
        unless_clauses: vec![],
        span: None,
    };

    let rules = vec![rule_a, rule_b, rule_c.clone()];
    let facts = vec![fact_x];

    let required = find_required_facts_recursive(&rule_c, &rules, &facts);

    assert!(
        required.contains("x"),
        "Should find fact x transitively through rule_a and rule_b"
    );
    assert_eq!(required.len(), 1);
}
