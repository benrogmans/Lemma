//! Expression hydration and constant folding
//!
//! This module provides utilities for:
//! - Substituting fact references with concrete values
//! - Expanding rule references when appropriate
//! - Constant folding (arithmetic, boolean, comparison simplification)

use crate::{Expression, ExpressionKind, LiteralValue};
use std::collections::HashMap;

/// Substitute a specific fact with an expression throughout an expression tree
pub fn substitute_fact_with_expr(
    expr: &Expression,
    fact_path: &crate::FactReference,
    replacement: &Expression,
) -> Expression {
    use ExpressionKind;
    match &expr.kind {
        ExpressionKind::FactReference(fr) => {
            if fr.reference == fact_path.reference {
                return replacement.clone();
            }
            expr.clone()
        }
        ExpressionKind::Arithmetic(l, op, r) => Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(substitute_fact_with_expr(l, fact_path, replacement)),
                op.clone(),
                Box::new(substitute_fact_with_expr(r, fact_path, replacement)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::Comparison(l, op, r) => Expression::new(
            ExpressionKind::Comparison(
                Box::new(substitute_fact_with_expr(l, fact_path, replacement)),
                op.clone(),
                Box::new(substitute_fact_with_expr(r, fact_path, replacement)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::LogicalAnd(l, r) => Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(substitute_fact_with_expr(l, fact_path, replacement)),
                Box::new(substitute_fact_with_expr(r, fact_path, replacement)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::LogicalOr(l, r) => Expression::new(
            ExpressionKind::LogicalOr(
                Box::new(substitute_fact_with_expr(l, fact_path, replacement)),
                Box::new(substitute_fact_with_expr(r, fact_path, replacement)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::LogicalNegation(inner, nt) => Expression::new(
            ExpressionKind::LogicalNegation(
                Box::new(substitute_fact_with_expr(inner, fact_path, replacement)),
                nt.clone(),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::UnitConversion(inner, tgt) => Expression::new(
            ExpressionKind::UnitConversion(
                Box::new(substitute_fact_with_expr(inner, fact_path, replacement)),
                tgt.clone(),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::MathematicalComputation(op, inner) => Expression::new(
            ExpressionKind::MathematicalComputation(
                op.clone(),
                Box::new(substitute_fact_with_expr(inner, fact_path, replacement)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        _ => expr.clone(),
    }
}

/// Hydrate an expression by replacing fact references with their values
///
/// This function:
/// - Substitutes fact references with values from `given`
/// - Expands simple rule references when appropriate
/// - Handles both qualified (doc.fact) and local (fact) references
pub fn hydrate_expression<'a, F, G>(
    expr: &Expression,
    doc_name: &str,
    given: &HashMap<String, LiteralValue>,
    get_rule: &F,
    is_simple: &G,
) -> Expression
where
    F: Fn(&[String]) -> Option<&'a crate::LemmaRule>,
    G: Fn(&Expression, &HashMap<String, LiteralValue>) -> bool,
{
    use ExpressionKind;
    match &expr.kind {
        ExpressionKind::Literal(_) | ExpressionKind::Veto(_) => expr.clone(),
        ExpressionKind::FactReference(fref) => {
            // Build keys to try: fully-qualified and local
            let local = fref.reference.join(".");
            let qualified = if fref.reference.len() > 1 {
                local.clone()
            } else {
                format!("{}.{}", doc_name, local)
            };
            if let Some(val) = given.get(&qualified).or_else(|| given.get(&local)) {
                Expression::new(
                    ExpressionKind::Literal(val.clone()),
                    expr.source_location.clone(),
                    expr.id,
                )
            } else {
                expr.clone()
            }
        }
        ExpressionKind::RuleReference(rule_ref) => {
            let rule_ref_qualified: Vec<String> = if rule_ref.reference.len() > 1 {
                rule_ref.reference.clone()
            } else {
                vec![doc_name.to_owned(), rule_ref.reference[0].clone()]
            };

            // Look up the rule
            if let Some(referenced_rule) = get_rule(&rule_ref_qualified) {
                // Only expand if: no branches (simple rule)
                if referenced_rule.unless_clauses.is_empty() {
                    // Recursively hydrate the rule's expression with current context
                    let hydrated = hydrate_expression(
                        &referenced_rule.expression,
                        doc_name,
                        given,
                        get_rule,
                        is_simple,
                    );

                    // Check if the hydrated result is "simple enough" to expand
                    if is_simple(&hydrated, given) {
                        return hydrated;
                    }
                    // Otherwise it has unresolved dependencies - keep symbolic
                }
                // If has branches (piecewise) - keep symbolic
            }

            // Can't simplify, keep the rule reference
            expr.clone()
        }
        ExpressionKind::Arithmetic(l, op, r) => Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(hydrate_expression(l, doc_name, given, get_rule, is_simple)),
                op.clone(),
                Box::new(hydrate_expression(r, doc_name, given, get_rule, is_simple)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::Comparison(l, op, r) => Expression::new(
            ExpressionKind::Comparison(
                Box::new(hydrate_expression(l, doc_name, given, get_rule, is_simple)),
                op.clone(),
                Box::new(hydrate_expression(r, doc_name, given, get_rule, is_simple)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::LogicalAnd(l, r) => Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(hydrate_expression(l, doc_name, given, get_rule, is_simple)),
                Box::new(hydrate_expression(r, doc_name, given, get_rule, is_simple)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::LogicalOr(l, r) => Expression::new(
            ExpressionKind::LogicalOr(
                Box::new(hydrate_expression(l, doc_name, given, get_rule, is_simple)),
                Box::new(hydrate_expression(r, doc_name, given, get_rule, is_simple)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::LogicalNegation(inner, nt) => Expression::new(
            ExpressionKind::LogicalNegation(
                Box::new(hydrate_expression(
                    inner, doc_name, given, get_rule, is_simple,
                )),
                nt.clone(),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::UnitConversion(val, tgt) => Expression::new(
            ExpressionKind::UnitConversion(
                Box::new(hydrate_expression(
                    val, doc_name, given, get_rule, is_simple,
                )),
                tgt.clone(),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::MathematicalComputation(op, inner) => Expression::new(
            ExpressionKind::MathematicalComputation(
                op.clone(),
                Box::new(hydrate_expression(
                    inner, doc_name, given, get_rule, is_simple,
                )),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
        ExpressionKind::FactHasAnyValue(fref) => {
            // If a given fact is present, this reduces to true; otherwise keep symbolic
            let local = fref.reference.join(".");
            let qualified = if fref.reference.len() > 1 {
                local.clone()
            } else {
                format!("{}.{}", doc_name, local)
            };
            if given.contains_key(&qualified) || given.contains_key(&local) {
                Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::True)),
                    expr.source_location.clone(),
                    expr.id,
                )
            } else {
                expr.clone()
            }
        }
    }
}

/// Check if an expression is simple enough to expand inline
///
/// Returns true for:
/// - Literals (constants)
/// - Simple arithmetic with literals only
/// - Expressions with no fact or rule references
pub fn is_simple_for_expansion(expr: &Expression, _given: &HashMap<String, LiteralValue>) -> bool {
    use ExpressionKind;
    match &expr.kind {
        // Literals are always simple
        ExpressionKind::Literal(_) => true,

        // Arithmetic is simple if both operands are simple
        ExpressionKind::Arithmetic(l, _, r) => {
            is_simple_for_expansion(l, _given) && is_simple_for_expansion(r, _given)
        }

        // Unit conversions are simple if the inner expression is simple
        ExpressionKind::UnitConversion(inner, _) => is_simple_for_expansion(inner, _given),

        // Mathematical operators (abs, etc.) are simple if inner is simple
        ExpressionKind::MathematicalComputation(_, inner) => is_simple_for_expansion(inner, _given),

        // Fact references and rule references are NOT simple - keep symbolic
        ExpressionKind::FactReference(_) | ExpressionKind::RuleReference(_) => false,

        // Comparisons, logical ops, vetos are NOT simple for expansion
        _ => false,
    }
}

/// Attempt constant folding on an expression
///
/// Simplifies arithmetic, boolean, and comparison operations when all operands are literals.
pub fn try_constant_fold<F>(expr: &Expression, make_literal: &F) -> Option<Expression>
where
    F: Fn(LiteralValue) -> Expression,
{
    use ExpressionKind;
    match &expr.kind {
        ExpressionKind::Arithmetic(l, op, r) => {
            let l2 = try_constant_fold(l, make_literal).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r, make_literal).unwrap_or((**r).clone());
            if let (ExpressionKind::Literal(ref lv), ExpressionKind::Literal(ref rv)) =
                (&l2.kind, &r2.kind)
            {
                if let Ok(val) = crate::evaluator::operations::arithmetic_operation(lv, op, rv) {
                    return Some(make_literal(val));
                }
            }
            Some(Expression::new(
                ExpressionKind::Arithmetic(Box::new(l2), op.clone(), Box::new(r2)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::Comparison(l, op, r) => {
            let l2 = try_constant_fold(l, make_literal).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r, make_literal).unwrap_or((**r).clone());
            if let (ExpressionKind::Literal(ref lv), ExpressionKind::Literal(ref rv)) =
                (&l2.kind, &r2.kind)
            {
                if let Ok(b) = crate::evaluator::operations::comparison_operation(lv, op, rv) {
                    return Some(make_literal(LiteralValue::Boolean(b.into())));
                }
            }
            Some(Expression::new(
                ExpressionKind::Comparison(Box::new(l2), op.clone(), Box::new(r2)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::LogicalAnd(l, r) => {
            let l2 = try_constant_fold(l, make_literal).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r, make_literal).unwrap_or((**r).clone());
            // Short-circuit identities
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::False)) =
                &l2.kind
            {
                return Some(make_literal(LiteralValue::Boolean(
                    crate::BooleanValue::False,
                )));
            }
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::False)) =
                &r2.kind
            {
                return Some(make_literal(LiteralValue::Boolean(
                    crate::BooleanValue::False,
                )));
            }
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::True)) =
                &l2.kind
            {
                return Some(r2);
            }
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::True)) =
                &r2.kind
            {
                return Some(l2);
            }
            if let (
                ExpressionKind::Literal(LiteralValue::Boolean(lb)),
                ExpressionKind::Literal(LiteralValue::Boolean(rb)),
            ) = (&l2.kind, &r2.kind)
            {
                let result = lb.into() && rb.into();
                return Some(make_literal(LiteralValue::Boolean(result.into())));
            }
            Some(Expression::new(
                ExpressionKind::LogicalAnd(Box::new(l2), Box::new(r2)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::LogicalOr(l, r) => {
            let l2 = try_constant_fold(l, make_literal).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r, make_literal).unwrap_or((**r).clone());
            // Short-circuit identities
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::True)) =
                &l2.kind
            {
                return Some(make_literal(LiteralValue::Boolean(
                    crate::BooleanValue::True,
                )));
            }
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::True)) =
                &r2.kind
            {
                return Some(make_literal(LiteralValue::Boolean(
                    crate::BooleanValue::True,
                )));
            }
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::False)) =
                &l2.kind
            {
                return Some(r2);
            }
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::False)) =
                &r2.kind
            {
                return Some(l2);
            }
            if let (
                ExpressionKind::Literal(LiteralValue::Boolean(lb)),
                ExpressionKind::Literal(LiteralValue::Boolean(rb)),
            ) = (&l2.kind, &r2.kind)
            {
                let result = lb.into() || rb.into();
                return Some(make_literal(LiteralValue::Boolean(result.into())));
            }
            Some(Expression::new(
                ExpressionKind::LogicalOr(Box::new(l2), Box::new(r2)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::LogicalNegation(inner, nt) => {
            let i2 = try_constant_fold(inner, make_literal).unwrap_or((**inner).clone());
            if let ExpressionKind::Literal(LiteralValue::Boolean(b)) = i2.kind {
                return Some(make_literal(LiteralValue::Boolean(!b)));
            }
            Some(Expression::new(
                ExpressionKind::LogicalNegation(Box::new(i2), nt.clone()),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        _ => None,
    }
}

/// Hydrate and simplify an expression in one step
pub fn hydrate_and_simplify<'a, F, G, H>(
    expr: &Expression,
    doc_name: &str,
    given: &HashMap<String, LiteralValue>,
    get_rule: &F,
    is_simple: &G,
    make_literal: &H,
) -> Expression
where
    F: Fn(&[String]) -> Option<&'a crate::LemmaRule>,
    G: Fn(&Expression, &HashMap<String, LiteralValue>) -> bool,
    H: Fn(LiteralValue) -> Expression,
{
    let h = hydrate_expression(expr, doc_name, given, get_rule, is_simple);
    try_constant_fold(&h, make_literal).unwrap_or(h)
}
