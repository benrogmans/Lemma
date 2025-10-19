use crate::evaluator::context::EvaluationContext;
use crate::evaluator::expression::evaluate_expression;
use crate::{
    ArithmeticOperation, Expression, ExpressionId, ExpressionKind, FactReference, LemmaDoc,
    LiteralValue, OperationResult,
};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn test_evaluate_literal() {
    let docs = HashMap::new();
    let sources = HashMap::new();
    let doc = LemmaDoc::new("test".to_string());
    let facts = HashMap::new();
    let mut context = EvaluationContext::new(&doc, &docs, &sources, facts);

    let expr = Expression::new(
        ExpressionKind::Literal(LiteralValue::Number(Decimal::from(42))),
        None,
        ExpressionId::new(0),
    );

    let result = evaluate_expression(&expr, &mut context).unwrap();
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(42)))
    );
}

#[test]
fn test_evaluate_fact_reference() {
    let docs = HashMap::new();
    let sources = HashMap::new();
    let doc = LemmaDoc::new("test".to_string());

    let mut facts = HashMap::new();
    facts.insert(
        "price".to_string(),
        LiteralValue::Number(Decimal::from(100)),
    );

    let mut context = EvaluationContext::new(&doc, &docs, &sources, facts);

    let expr = Expression::new(
        ExpressionKind::FactReference(FactReference {
            reference: vec!["price".to_string()],
        }),
        None,
        ExpressionId::new(0),
    );

    let result = evaluate_expression(&expr, &mut context).unwrap();
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(100)))
    );

    // Check operation recorded
    assert_eq!(context.operations.len(), 1);
}

#[test]
fn test_evaluate_simple_arithmetic() {
    let docs = HashMap::new();
    let sources = HashMap::new();
    let doc = LemmaDoc::new("test".to_string());
    let facts = HashMap::new();
    let mut context = EvaluationContext::new(&doc, &docs, &sources, facts);

    // 10 + 5
    let expr = Expression::new(
        ExpressionKind::Arithmetic(
            Box::new(Expression::new(
                ExpressionKind::Literal(LiteralValue::Number(Decimal::from(10))),
                None,
                ExpressionId::new(0),
            )),
            ArithmeticOperation::Add,
            Box::new(Expression::new(
                ExpressionKind::Literal(LiteralValue::Number(Decimal::from(5))),
                None,
                ExpressionId::new(1),
            )),
        ),
        None,
        ExpressionId::new(2),
    );

    let result = evaluate_expression(&expr, &mut context).unwrap();
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(15)))
    );

    // Check operation recorded
    assert_eq!(context.operations.len(), 1);
}
