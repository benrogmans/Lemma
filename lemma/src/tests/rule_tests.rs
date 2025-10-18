use crate::evaluator::context::EvaluationContext;
use crate::evaluator::rules::evaluate_rule;
use crate::{
    Expression, ExpressionId, ExpressionKind, LemmaDoc, LemmaRule, LiteralValue, OperationResult,
    UnlessClause,
};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn test_evaluate_rule_no_unless() {
    let docs = HashMap::new();
    let sources = HashMap::new();
    let doc = LemmaDoc::new("test".to_string());
    let facts = HashMap::new();
    let mut context = EvaluationContext::new(&doc, &docs, &sources, facts);

    let rule = LemmaRule {
        name: "test_rule".to_string(),
        expression: Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(Decimal::from(42))),
            None,
            ExpressionId::new(0),
        ),
        unless_clauses: vec![],
        span: None,
    };

    let result = evaluate_rule(&rule, &mut context).unwrap();
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(42)))
    );
}

#[test]
fn test_evaluate_rule_with_unless_no_match() {
    let docs = HashMap::new();
    let sources = HashMap::new();
    let doc = LemmaDoc::new("test".to_string());
    let facts = HashMap::new();
    let mut context = EvaluationContext::new(&doc, &docs, &sources, facts);

    let rule = LemmaRule {
        name: "test_rule".to_string(),
        expression: Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(Decimal::from(100))),
            None,
            ExpressionId::new(0),
        ),
        unless_clauses: vec![UnlessClause {
            condition: Expression::new(
                ExpressionKind::Literal(LiteralValue::Boolean(false)),
                None,
                ExpressionId::new(1),
            ),
            result: Expression::new(
                ExpressionKind::Literal(LiteralValue::Number(Decimal::from(200))),
                None,
                ExpressionId::new(2),
            ),
            span: None,
        }],
        span: None,
    };

    let result = evaluate_rule(&rule, &mut context).unwrap();
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(100)))
    );
}

#[test]
fn test_evaluate_rule_with_unless_match() {
    let docs = HashMap::new();
    let sources = HashMap::new();
    let doc = LemmaDoc::new("test".to_string());
    let facts = HashMap::new();
    let mut context = EvaluationContext::new(&doc, &docs, &sources, facts);

    let rule = LemmaRule {
        name: "test_rule".to_string(),
        expression: Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(Decimal::from(100))),
            None,
            ExpressionId::new(0),
        ),
        unless_clauses: vec![UnlessClause {
            condition: Expression::new(
                ExpressionKind::Literal(LiteralValue::Boolean(true)),
                None,
                ExpressionId::new(1),
            ),
            result: Expression::new(
                ExpressionKind::Literal(LiteralValue::Number(Decimal::from(200))),
                None,
                ExpressionId::new(2),
            ),
            span: None,
        }],
        span: None,
    };

    let result = evaluate_rule(&rule, &mut context).unwrap();
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(200)))
    );
}

#[test]
fn test_evaluate_rule_last_matching_wins() {
    let docs = HashMap::new();
    let sources = HashMap::new();
    let doc = LemmaDoc::new("test".to_string());
    let facts = HashMap::new();
    let mut context = EvaluationContext::new(&doc, &docs, &sources, facts);

    let rule = LemmaRule {
        name: "test_rule".to_string(),
        expression: Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(Decimal::from(100))),
            None,
            ExpressionId::new(0),
        ),
        unless_clauses: vec![
            UnlessClause {
                condition: Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(true)),
                    None,
                    ExpressionId::new(1),
                ),
                result: Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::from(200))),
                    None,
                    ExpressionId::new(2),
                ),
                span: None,
            },
            UnlessClause {
                condition: Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(true)),
                    None,
                    ExpressionId::new(3),
                ),
                result: Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::from(300))),
                    None,
                    ExpressionId::new(4),
                ),
                span: None,
            },
        ],
        span: None,
    };

    let result = evaluate_rule(&rule, &mut context).unwrap();
    // Last unless clause wins
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(300)))
    );
}
