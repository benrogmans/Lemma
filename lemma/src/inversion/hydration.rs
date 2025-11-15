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
    use ExpressionKind as EK;
    match &expr.kind {
        EK::FactReference(fr) => {
            if fr.reference == fact_path.reference {
                return replacement.clone();
            }
            expr.clone()
        }
        EK::Arithmetic(l, op, r) => Expression::new(
            EK::Arithmetic(
                Box::new(substitute_fact_with_expr(l, fact_path, replacement)),
                op.clone(),
                Box::new(substitute_fact_with_expr(r, fact_path, replacement)),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::Comparison(l, op, r) => Expression::new(
            EK::Comparison(
                Box::new(substitute_fact_with_expr(l, fact_path, replacement)),
                op.clone(),
                Box::new(substitute_fact_with_expr(r, fact_path, replacement)),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::LogicalAnd(l, r) => Expression::new(
            EK::LogicalAnd(
                Box::new(substitute_fact_with_expr(l, fact_path, replacement)),
                Box::new(substitute_fact_with_expr(r, fact_path, replacement)),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::LogicalOr(l, r) => Expression::new(
            EK::LogicalOr(
                Box::new(substitute_fact_with_expr(l, fact_path, replacement)),
                Box::new(substitute_fact_with_expr(r, fact_path, replacement)),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::LogicalNegation(inner, nt) => Expression::new(
            EK::LogicalNegation(
                Box::new(substitute_fact_with_expr(inner, fact_path, replacement)),
                nt.clone(),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::UnitConversion(inner, tgt) => Expression::new(
            EK::UnitConversion(
                Box::new(substitute_fact_with_expr(inner, fact_path, replacement)),
                tgt.clone(),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::MathematicalComputation(op, inner) => Expression::new(
            EK::MathematicalComputation(
                op.clone(),
                Box::new(substitute_fact_with_expr(inner, fact_path, replacement)),
            ),
            expr.span.clone(),
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
    use ExpressionKind as EK;
    match &expr.kind {
        EK::Literal(_) | EK::Veto(_) => expr.clone(),
        EK::FactReference(fref) => {
            // Build keys to try: fully-qualified and local
            let local = fref.reference.join(".");
            let qualified = if fref.reference.len() > 1 {
                local.clone()
            } else {
                format!("{}.{}", doc_name, local)
            };
            if let Some(val) = given.get(&qualified).or_else(|| given.get(&local)) {
                Expression::new(EK::Literal(val.clone()), expr.span.clone(), expr.id)
            } else {
                expr.clone()
            }
        }
        EK::RuleReference(rule_ref) => {
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
        EK::Arithmetic(l, op, r) => Expression::new(
            EK::Arithmetic(
                Box::new(hydrate_expression(l, doc_name, given, get_rule, is_simple)),
                op.clone(),
                Box::new(hydrate_expression(r, doc_name, given, get_rule, is_simple)),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::Comparison(l, op, r) => Expression::new(
            EK::Comparison(
                Box::new(hydrate_expression(l, doc_name, given, get_rule, is_simple)),
                op.clone(),
                Box::new(hydrate_expression(r, doc_name, given, get_rule, is_simple)),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::LogicalAnd(l, r) => Expression::new(
            EK::LogicalAnd(
                Box::new(hydrate_expression(l, doc_name, given, get_rule, is_simple)),
                Box::new(hydrate_expression(r, doc_name, given, get_rule, is_simple)),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::LogicalOr(l, r) => Expression::new(
            EK::LogicalOr(
                Box::new(hydrate_expression(l, doc_name, given, get_rule, is_simple)),
                Box::new(hydrate_expression(r, doc_name, given, get_rule, is_simple)),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::LogicalNegation(inner, nt) => Expression::new(
            EK::LogicalNegation(
                Box::new(hydrate_expression(
                    inner, doc_name, given, get_rule, is_simple,
                )),
                nt.clone(),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::UnitConversion(val, tgt) => Expression::new(
            EK::UnitConversion(
                Box::new(hydrate_expression(
                    val, doc_name, given, get_rule, is_simple,
                )),
                tgt.clone(),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::MathematicalComputation(op, inner) => Expression::new(
            EK::MathematicalComputation(
                op.clone(),
                Box::new(hydrate_expression(
                    inner, doc_name, given, get_rule, is_simple,
                )),
            ),
            expr.span.clone(),
            expr.id,
        ),
        EK::FactHasAnyValue(fref) => {
            // If a given fact is present, this reduces to true; otherwise keep symbolic
            let local = fref.reference.join(".");
            let qualified = if fref.reference.len() > 1 {
                local.clone()
            } else {
                format!("{}.{}", doc_name, local)
            };
            if given.contains_key(&qualified) || given.contains_key(&local) {
                Expression::new(
                    EK::Literal(LiteralValue::Boolean(true)),
                    expr.span.clone(),
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
    use ExpressionKind as EK;
    match &expr.kind {
        // Literals are always simple
        EK::Literal(_) => true,

        // Arithmetic is simple if both operands are simple
        EK::Arithmetic(l, _, r) => {
            is_simple_for_expansion(l, _given) && is_simple_for_expansion(r, _given)
        }

        // Unit conversions are simple if the inner expression is simple
        EK::UnitConversion(inner, _) => is_simple_for_expansion(inner, _given),

        // Mathematical operators (abs, etc.) are simple if inner is simple
        EK::MathematicalComputation(_, inner) => is_simple_for_expansion(inner, _given),

        // Fact references and rule references are NOT simple - keep symbolic
        EK::FactReference(_) | EK::RuleReference(_) => false,

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
    use ExpressionKind as EK;
    match &expr.kind {
        EK::Arithmetic(l, op, r) => {
            let l2 = try_constant_fold(l, make_literal).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r, make_literal).unwrap_or((**r).clone());
            if let (EK::Literal(ref lv), EK::Literal(ref rv)) = (&l2.kind, &r2.kind) {
                if let Ok(val) = crate::evaluator::operations::arithmetic_operation(lv, op, rv) {
                    return Some(make_literal(val));
                }
            }
            Some(Expression::new(
                EK::Arithmetic(Box::new(l2), op.clone(), Box::new(r2)),
                expr.span.clone(),
                expr.id,
            ))
        }
        EK::Comparison(l, op, r) => {
            let l2 = try_constant_fold(l, make_literal).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r, make_literal).unwrap_or((**r).clone());
            if let (EK::Literal(ref lv), EK::Literal(ref rv)) = (&l2.kind, &r2.kind) {
                if let Ok(b) = crate::evaluator::operations::comparison_operation(lv, op, rv) {
                    return Some(make_literal(LiteralValue::Boolean(b)));
                }
            }
            Some(Expression::new(
                EK::Comparison(Box::new(l2), op.clone(), Box::new(r2)),
                expr.span.clone(),
                expr.id,
            ))
        }
        EK::LogicalAnd(l, r) => {
            let l2 = try_constant_fold(l, make_literal).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r, make_literal).unwrap_or((**r).clone());
            // Short-circuit identities
            if let EK::Literal(LiteralValue::Boolean(false)) = &l2.kind {
                return Some(make_literal(LiteralValue::Boolean(false)));
            }
            if let EK::Literal(LiteralValue::Boolean(false)) = &r2.kind {
                return Some(make_literal(LiteralValue::Boolean(false)));
            }
            if let EK::Literal(LiteralValue::Boolean(true)) = &l2.kind {
                return Some(r2);
            }
            if let EK::Literal(LiteralValue::Boolean(true)) = &r2.kind {
                return Some(l2);
            }
            if let (
                EK::Literal(LiteralValue::Boolean(lb)),
                EK::Literal(LiteralValue::Boolean(rb)),
            ) = (&l2.kind, &r2.kind)
            {
                return Some(make_literal(LiteralValue::Boolean(*lb && *rb)));
            }
            Some(Expression::new(
                EK::LogicalAnd(Box::new(l2), Box::new(r2)),
                expr.span.clone(),
                expr.id,
            ))
        }
        EK::LogicalOr(l, r) => {
            let l2 = try_constant_fold(l, make_literal).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r, make_literal).unwrap_or((**r).clone());
            // Short-circuit identities
            if let EK::Literal(LiteralValue::Boolean(true)) = &l2.kind {
                return Some(make_literal(LiteralValue::Boolean(true)));
            }
            if let EK::Literal(LiteralValue::Boolean(true)) = &r2.kind {
                return Some(make_literal(LiteralValue::Boolean(true)));
            }
            if let EK::Literal(LiteralValue::Boolean(false)) = &l2.kind {
                return Some(r2);
            }
            if let EK::Literal(LiteralValue::Boolean(false)) = &r2.kind {
                return Some(l2);
            }
            if let (
                EK::Literal(LiteralValue::Boolean(lb)),
                EK::Literal(LiteralValue::Boolean(rb)),
            ) = (&l2.kind, &r2.kind)
            {
                return Some(make_literal(LiteralValue::Boolean(*lb || *rb)));
            }
            Some(Expression::new(
                EK::LogicalOr(Box::new(l2), Box::new(r2)),
                expr.span.clone(),
                expr.id,
            ))
        }
        EK::LogicalNegation(inner, nt) => {
            let i2 = try_constant_fold(inner, make_literal).unwrap_or((**inner).clone());
            if let EK::Literal(LiteralValue::Boolean(b)) = i2.kind {
                return Some(make_literal(LiteralValue::Boolean(!b)));
            }
            Some(Expression::new(
                EK::LogicalNegation(Box::new(i2), nt.clone()),
                expr.span.clone(),
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
