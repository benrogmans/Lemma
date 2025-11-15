use crate::evaluator::context::EvaluationContext;
use crate::evaluator::expression::evaluate_expression;
use crate::evaluator::timeout::TimeoutTracker;
use crate::{
    ArithmeticComputation, Expression, ExpressionId, ExpressionKind, FactReference, LemmaDoc,
    LiteralValue, OperationResult, ResourceLimits,
};
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Helper to create an evaluation context for testing
fn create_test_context(facts: HashMap<FactReference, LiteralValue>) -> EvaluationContext<'static> {
    let docs = Box::leak(Box::new(HashMap::new()));
    let sources = Box::leak(Box::new(HashMap::new()));
    let doc = Box::leak(Box::new(LemmaDoc::new("test".to_string())));
    let limits = Box::leak(Box::new(ResourceLimits::default()));
    let timeout_tracker = Box::leak(Box::new(TimeoutTracker::new()));

    EvaluationContext::new(doc, docs, sources, facts, timeout_tracker, limits)
}

#[test]
fn test_evaluate_literal() {
    let mut context = create_test_context(HashMap::new());
    let test_doc = LemmaDoc::new("test".to_string());

    let expr = Expression::new(
        ExpressionKind::Literal(LiteralValue::Number(Decimal::from(42))),
        None,
        ExpressionId::new(0),
    );

    let result = evaluate_expression(&expr, &test_doc, &mut context, &[]).unwrap();
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(42)))
    );
}

#[test]
fn test_evaluate_fact_reference() {
    let mut facts = HashMap::new();
    facts.insert(
        FactReference {
            reference: vec!["price".to_string()],
        },
        LiteralValue::Number(Decimal::from(100)),
    );

    let mut context = create_test_context(facts);
    let test_doc = LemmaDoc::new("test".to_string());

    let expr = Expression::new(
        ExpressionKind::FactReference(FactReference {
            reference: vec!["price".to_string()],
        }),
        None,
        ExpressionId::new(0),
    );

    let result = evaluate_expression(&expr, &test_doc, &mut context, &[]).unwrap();
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(100)))
    );

    // Check operation recorded
    assert_eq!(context.operations.len(), 1);
}

#[test]
fn test_evaluate_simple_arithmetic() {
    let mut context = create_test_context(HashMap::new());
    let test_doc = LemmaDoc::new("test".to_string());

    // 10 + 5
    let expr = Expression::new(
        ExpressionKind::Arithmetic(
            Box::new(Expression::new(
                ExpressionKind::Literal(LiteralValue::Number(Decimal::from(10))),
                None,
                ExpressionId::new(0),
            )),
            ArithmeticComputation::Add,
            Box::new(Expression::new(
                ExpressionKind::Literal(LiteralValue::Number(Decimal::from(5))),
                None,
                ExpressionId::new(1),
            )),
        ),
        None,
        ExpressionId::new(2),
    );

    let result = evaluate_expression(&expr, &test_doc, &mut context, &[]).unwrap();
    assert_eq!(
        result,
        OperationResult::Value(LiteralValue::Number(Decimal::from(15)))
    );

    // Check operation recorded
    assert_eq!(context.operations.len(), 1);
}
