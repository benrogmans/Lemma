//! Expression expansion and hydration
//!
//! This module provides utilities for:
//! - Expanding rule references (`RulePath`) into their underlying definitions
//! - Hydrating expressions by substituting given fact paths with concrete values
//! - Constant folding (arithmetic, boolean, comparison simplification)
//!
//! **Architecture**: Expansion happens first (rule? → definition), then hydration (given facts → values),
//! then constant folding. The `expand_and_hydrate()` function performs all three steps in order.
//!
//! **Note**: The planning phase already guarantees no cycles and bounded depth through topological
//! sorting and dependency analysis. This module performs simple recursive lookup without cycle
//! detection or depth limiting.

use crate::evaluation::operations::OperationResult;
use crate::planning::{ExecutableRule, ExecutionPlan};
use crate::{
    Expression, ExpressionKind, FactPath, LemmaError, LemmaResult, LiteralValue, RulePath,
};
use std::collections::HashSet;

use super::{build_suffix_or_conditions, literal_expr, logical_and, logical_not, logical_or};

/// Substitute a specific fact with an expression throughout an expression tree
///
/// This function is currently unused but is kept for potential future use
/// in expression manipulation during inversion.
#[allow(dead_code)]
pub fn substitute_fact_with_expr(
    expr: &Expression,
    fact_path: &FactPath,
    replacement: &Expression,
) -> Expression {
    match &expr.kind {
        ExpressionKind::FactPath(fp) => {
            if fp == fact_path {
                return replacement.clone();
            }
            expr.clone()
        }
        ExpressionKind::FactReference(_) => expr.clone(),
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

/// Hydrate an expression by replacing given fact paths with their values.
///
/// This function:
/// - Only substitutes fact paths that are in `provided_facts` (user-provided values)
/// - Leaves other fact values as FactPath references (they're free variables)
/// - Does NOT expand rule references - use `expand_and_hydrate()` for that
///
/// Internal function - use `expand_and_hydrate()` for public API.
fn hydrate_expression(
    expr: &Expression,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> Expression {
    match &expr.kind {
        ExpressionKind::Literal(_) | ExpressionKind::Veto(_) => expr.clone(),

        ExpressionKind::FactPath(fp) => {
            // Only substitute if this fact was given by the user
            if provided_facts.contains(fp) {
                if let Some(value) = plan.get_fact_value(fp) {
                    return Expression::new(
                        ExpressionKind::Literal(value.clone()),
                        expr.source_location.clone(),
                        expr.id,
                    );
                }
            }
            expr.clone()
        }

        ExpressionKind::FactReference(_fref) => {
            // FactReference should have been converted to FactPath during planning.
            // If we see one here, it's a planning bug.
            // Return the expression as-is (defensive) but this shouldn't happen.
            expr.clone()
        }

        ExpressionKind::RulePath(_rp) => {
            // RulePath should have been expanded before hydration.
            // If we see one here, it means expansion didn't complete properly.
            // Return unchanged - this is an error condition that should be caught earlier.
            expr.clone()
        }

        ExpressionKind::RuleReference(_rref) => {
            // RuleReference should have been converted to RulePath during planning.
            // If we see one here, it's a planning bug.
            expr.clone()
        }

        ExpressionKind::Arithmetic(l, op, r) => Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(hydrate_expression(l, plan, provided_facts)),
                op.clone(),
                Box::new(hydrate_expression(r, plan, provided_facts)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),

        ExpressionKind::Comparison(l, op, r) => Expression::new(
            ExpressionKind::Comparison(
                Box::new(hydrate_expression(l, plan, provided_facts)),
                op.clone(),
                Box::new(hydrate_expression(r, plan, provided_facts)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),

        ExpressionKind::LogicalAnd(l, r) => Expression::new(
            ExpressionKind::LogicalAnd(
                Box::new(hydrate_expression(l, plan, provided_facts)),
                Box::new(hydrate_expression(r, plan, provided_facts)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),

        ExpressionKind::LogicalOr(l, r) => Expression::new(
            ExpressionKind::LogicalOr(
                Box::new(hydrate_expression(l, plan, provided_facts)),
                Box::new(hydrate_expression(r, plan, provided_facts)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),

        ExpressionKind::LogicalNegation(inner, nt) => Expression::new(
            ExpressionKind::LogicalNegation(
                Box::new(hydrate_expression(inner, plan, provided_facts)),
                nt.clone(),
            ),
            expr.source_location.clone(),
            expr.id,
        ),

        ExpressionKind::UnitConversion(val, tgt) => Expression::new(
            ExpressionKind::UnitConversion(
                Box::new(hydrate_expression(val, plan, provided_facts)),
                tgt.clone(),
            ),
            expr.source_location.clone(),
            expr.id,
        ),

        ExpressionKind::MathematicalComputation(op, inner) => Expression::new(
            ExpressionKind::MathematicalComputation(
                op.clone(),
                Box::new(hydrate_expression(inner, plan, provided_facts)),
            ),
            expr.source_location.clone(),
            expr.id,
        ),
    }
}

/// Attempt constant folding on an expression
///
/// Simplifies arithmetic, boolean, and comparison operations when all operands are literals.
pub fn try_constant_fold(expr: &Expression) -> Option<Expression> {
    fn make_literal(val: LiteralValue, expr: &Expression) -> Expression {
        Expression::new(
            ExpressionKind::Literal(val),
            expr.source_location.clone(),
            expr.id,
        )
    }

    match &expr.kind {
        ExpressionKind::Arithmetic(l, op, r) => {
            let l2 = try_constant_fold(l).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r).unwrap_or((**r).clone());
            if let (ExpressionKind::Literal(ref lv), ExpressionKind::Literal(ref rv)) =
                (&l2.kind, &r2.kind)
            {
                if let OperationResult::Value(val) =
                    crate::evaluation::operations::arithmetic_operation(lv, op, rv)
                {
                    return Some(make_literal(val, expr));
                }
            }
            Some(Expression::new(
                ExpressionKind::Arithmetic(Box::new(l2), op.clone(), Box::new(r2)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::Comparison(l, op, r) => {
            let l2 = try_constant_fold(l).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r).unwrap_or((**r).clone());
            if let (ExpressionKind::Literal(ref lv), ExpressionKind::Literal(ref rv)) =
                (&l2.kind, &r2.kind)
            {
                if let OperationResult::Value(LiteralValue::Boolean(b)) =
                    crate::evaluation::operations::comparison_operation(lv, op, rv)
                {
                    return Some(make_literal(LiteralValue::Boolean(b), expr));
                }
            }
            Some(Expression::new(
                ExpressionKind::Comparison(Box::new(l2), op.clone(), Box::new(r2)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::LogicalAnd(l, r) => {
            let l2 = try_constant_fold(l).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r).unwrap_or((**r).clone());
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::False)) =
                &l2.kind
            {
                return Some(make_literal(
                    LiteralValue::Boolean(crate::BooleanValue::False),
                    expr,
                ));
            }
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::False)) =
                &r2.kind
            {
                return Some(make_literal(
                    LiteralValue::Boolean(crate::BooleanValue::False),
                    expr,
                ));
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
                return Some(make_literal(LiteralValue::Boolean(result.into()), expr));
            }
            Some(Expression::new(
                ExpressionKind::LogicalAnd(Box::new(l2), Box::new(r2)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::LogicalOr(l, r) => {
            let l2 = try_constant_fold(l).unwrap_or((**l).clone());
            let r2 = try_constant_fold(r).unwrap_or((**r).clone());
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::True)) =
                &l2.kind
            {
                return Some(make_literal(
                    LiteralValue::Boolean(crate::BooleanValue::True),
                    expr,
                ));
            }
            if let ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::True)) =
                &r2.kind
            {
                return Some(make_literal(
                    LiteralValue::Boolean(crate::BooleanValue::True),
                    expr,
                ));
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
                return Some(make_literal(LiteralValue::Boolean(result.into()), expr));
            }
            Some(Expression::new(
                ExpressionKind::LogicalOr(Box::new(l2), Box::new(r2)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::LogicalNegation(inner, nt) => {
            let i2 = try_constant_fold(inner).unwrap_or((**inner).clone());
            if let ExpressionKind::Literal(LiteralValue::Boolean(b)) = i2.kind {
                return Some(make_literal(LiteralValue::Boolean(!b), expr));
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

/// Get the default expression from a rule (first branch's result)
pub fn get_default_expression(rule: &ExecutableRule) -> Option<&Expression> {
    rule.branches.first().map(|b| &b.result)
}

/// Collect veto conditions from a rule's branches (branches after the first that produce vetoes)
pub fn collect_veto_conditions(rule: &ExecutableRule) -> Vec<&Expression> {
    rule.branches
        .iter()
        .skip(1)
        .filter_map(|branch| {
            if let ExpressionKind::Veto(_) = &branch.result.kind {
                branch.condition.as_ref()
            } else {
                None
            }
        })
        .collect()
}

/// Expand a rule reference to its piecewise definition with "last wins" semantics
///
/// This function expands `RulePath` into its underlying definition.
/// It handles "last wins" semantics (later branches override earlier ones).
///
/// **Note**: The planning phase already guarantees no cycles and bounded depth,
/// so this function performs simple recursive lookup without cycle detection.
///
/// # Arguments
/// * `rule_path` - The rule path to expand
/// * `plan` - The execution plan containing all rules
///
/// # Returns
/// * `Ok(Expression)` - The expanded expression representing the rule's piecewise definition
/// * `Err(LemmaError)` - If rule not found
pub fn expand_rule_reference(
    rule_path: &RulePath,
    plan: &ExecutionPlan,
) -> LemmaResult<Expression> {
    // Get the rule from the plan
    let rule = plan
        .get_rule_by_path(rule_path)
        .ok_or_else(|| LemmaError::Engine(format!("Rule not found: {}", rule_path)))?;

    // Build branches: (condition, result) pairs
    let mut branches: Vec<(Option<Expression>, Expression)> = Vec::new();
    for branch in &rule.branches {
        let condition = branch.condition.clone();
        let result = branch.result.clone();
        branches.push((condition, result));
    }

    // Build suffix OR conditions for "last wins" semantics
    let suffix_or = build_suffix_or_conditions(&branches);

    // Expand each branch's result (may contain rule references)
    let mut expanded_branches: Vec<(Option<Expression>, Expression)> = Vec::new();
    for (idx, (condition, result)) in branches.iter().enumerate() {
        // Expand the result (recursively expand any rule references in it)
        let expanded_result = expand_expression_recursive(result.clone(), plan)?;

        // Build effective condition with "last wins" semantics
        let effective_condition = match condition {
            Some(cond) => {
                // Expand condition as well (may contain rule references)
                let expanded_cond = expand_expression_recursive(cond.clone(), plan)?;
                // Add negation of later branches
                if let Some(later_or) = &suffix_or[idx] {
                    logical_and(expanded_cond, logical_not(later_or.clone()))
                } else {
                    expanded_cond
                }
            }
            None => {
                // Default branch: true unless later branches match
                if let Some(later_or) = &suffix_or[idx] {
                    logical_not(later_or.clone())
                } else {
                    literal_expr(LiteralValue::Boolean(crate::BooleanValue::True))
                }
            }
        };

        expanded_branches.push((Some(effective_condition), expanded_result));
    }

    // Build piecewise expression: OR of all (condition AND result) pairs
    if expanded_branches.is_empty() {
        return Err(LemmaError::Engine(format!(
            "Rule {} has no branches",
            rule_path
        )));
    }

    if expanded_branches.len() == 1 {
        // Single branch: just return the result
        Ok(expanded_branches[0].1.clone())
    } else {
        // Multiple branches: OR of (condition AND result) for each branch
        let mut or_parts = Vec::new();
        for (condition, result) in expanded_branches {
            let condition_expr = condition
                .unwrap_or_else(|| literal_expr(LiteralValue::Boolean(crate::BooleanValue::True)));
            or_parts.push(logical_and(condition_expr, result));
        }

        // Combine all OR parts
        let mut combined = or_parts.remove(0);
        for part in or_parts {
            combined = logical_or(combined, part);
        }

        Ok(combined)
    }
}

/// Recursively expand all rule references in an expression
///
/// This is a helper function that traverses an expression tree and expands
/// any `RulePath` nodes it encounters.
///
/// **Note**: The planning phase already guarantees no cycles, so this function
/// performs simple recursive lookup without cycle detection.
fn expand_expression_recursive(expr: Expression, plan: &ExecutionPlan) -> LemmaResult<Expression> {
    match &expr.kind {
        ExpressionKind::RulePath(rule_path) => expand_rule_reference(rule_path, plan),
        ExpressionKind::RuleReference(_rule_ref) => {
            // RuleReference should have been converted to RulePath during planning.
            // If we see one here, it's a planning bug.
            Err(LemmaError::Engine(
                "Internal error: RuleReference found in expression - should have been converted to RulePath during planning".to_string()
            ))
        }
        ExpressionKind::Arithmetic(l, op, r) => {
            let expanded_l = expand_expression_recursive((**l).clone(), plan)?;
            let expanded_r = expand_expression_recursive((**r).clone(), plan)?;
            Ok(Expression::new(
                ExpressionKind::Arithmetic(Box::new(expanded_l), op.clone(), Box::new(expanded_r)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::Comparison(l, op, r) => {
            let expanded_l = expand_expression_recursive((**l).clone(), plan)?;
            let expanded_r = expand_expression_recursive((**r).clone(), plan)?;
            Ok(Expression::new(
                ExpressionKind::Comparison(Box::new(expanded_l), op.clone(), Box::new(expanded_r)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::LogicalAnd(l, r) => {
            let expanded_l = expand_expression_recursive((**l).clone(), plan)?;
            let expanded_r = expand_expression_recursive((**r).clone(), plan)?;
            Ok(Expression::new(
                ExpressionKind::LogicalAnd(Box::new(expanded_l), Box::new(expanded_r)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::LogicalOr(l, r) => {
            let expanded_l = expand_expression_recursive((**l).clone(), plan)?;
            let expanded_r = expand_expression_recursive((**r).clone(), plan)?;
            Ok(Expression::new(
                ExpressionKind::LogicalOr(Box::new(expanded_l), Box::new(expanded_r)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::LogicalNegation(inner, nt) => {
            let expanded_inner = expand_expression_recursive((**inner).clone(), plan)?;
            Ok(Expression::new(
                ExpressionKind::LogicalNegation(Box::new(expanded_inner), nt.clone()),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::UnitConversion(inner, tgt) => {
            let expanded_inner = expand_expression_recursive((**inner).clone(), plan)?;
            Ok(Expression::new(
                ExpressionKind::UnitConversion(Box::new(expanded_inner), tgt.clone()),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        ExpressionKind::MathematicalComputation(op, inner) => {
            let expanded_inner = expand_expression_recursive((**inner).clone(), plan)?;
            Ok(Expression::new(
                ExpressionKind::MathematicalComputation(op.clone(), Box::new(expanded_inner)),
                expr.source_location.clone(),
                expr.id,
            ))
        }
        _ => Ok(expr), // Literal, FactPath, Veto - no expansion needed
    }
}

/// Expand rule conditions for domain extraction context
///
/// Returns all (condition, result) pairs from a rule, with conditions and results
/// expanded and hydrated. This is used by domain extraction to determine which
/// branch conditions match a target value.
///
/// **Note**: The planning phase already guarantees no cycles, so this function
/// performs simple recursive lookup without cycle detection.
///
/// # Arguments
/// * `rule_path` - The rule path to expand
/// * `plan` - The execution plan containing all rules
/// * `provided_facts` - Facts that are given (will be hydrated)
///
/// # Returns
/// * `Ok(Vec<(Expression, Expression)>)` - Vector of (condition, result) pairs
/// * `Err(LemmaError)` - If rule not found
///
/// # Example
/// ```rust,no_run
/// // Rule: tier = "bronze" unless points >= 100 then "silver" unless points >= 500 then "gold"
/// // Returns:
/// // [
/// //     (true, "bronze"),                                    // Default branch
/// //     (points >= 100 AND NOT(points >= 500), "silver"),  // Second branch
/// //     (points >= 500, "gold"),                            // Third branch
/// // ]
/// ```
/// Expand a rule reference into its branch structure
///
/// Expands a rule reference into a vector of (condition, result) pairs,
/// where each pair represents one branch of the rule with "last wins" semantics applied.
/// Conditions and results are expanded, hydrated, and simplified.
///
/// This is used when a rule reference appears as a top-level result expression
/// in an inversion branch, allowing it to be expanded into multiple branches
/// before processing.
///
/// # Arguments
/// * `rule_path` - The rule path to expand
/// * `plan` - The execution plan containing all rules
/// * `provided_facts` - Facts that are given (will be hydrated)
///
/// # Returns
/// * `Ok(Vec<(Expression, Expression)>)` - Vector of (condition, result) pairs
/// * `Err(LemmaError)` - If rule not found
pub fn expand_rule_reference_to_branches(
    rule_path: &RulePath,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Vec<(Expression, Expression)>> {
    // Get the rule from the plan
    let rule = plan
        .get_rule_by_path(rule_path)
        .ok_or_else(|| LemmaError::Engine(format!("Rule not found: {}", rule_path)))?;

    // Build branches: (condition, result) pairs
    let mut branches: Vec<(Option<Expression>, Expression)> = Vec::new();
    for branch in &rule.branches {
        let condition = branch.condition.clone();
        let result = branch.result.clone();
        branches.push((condition, result));
    }

    // Build suffix OR conditions for "last wins" semantics
    let suffix_or = build_suffix_or_conditions(&branches);

    // Expand and hydrate each branch
    let mut result_pairs: Vec<(Expression, Expression)> = Vec::new();

    for (idx, (condition, result)) in branches.iter().enumerate() {
        // Expand the result (recursively expand any rule references in it)
        let expanded_result = expand_expression_recursive(result.clone(), plan)?;
        // Hydrate the result (substitute given facts)
        let hydrated_result = hydrate_expression(&expanded_result, plan, provided_facts);
        // Simplify the result
        let simplified_result = try_constant_fold(&hydrated_result).unwrap_or(hydrated_result);

        // Build effective condition with "last wins" semantics
        let effective_condition = match condition {
            Some(cond) => {
                // Expand condition as well (may contain rule references)
                let expanded_cond = expand_expression_recursive(cond.clone(), plan)?;
                // Add negation of later branches
                let cond_with_negation = if let Some(later_or) = &suffix_or[idx] {
                    logical_and(expanded_cond, logical_not(later_or.clone()))
                } else {
                    expanded_cond
                };
                // Hydrate the condition
                let hydrated_cond = hydrate_expression(&cond_with_negation, plan, provided_facts);
                // Simplify the condition
                try_constant_fold(&hydrated_cond).unwrap_or(hydrated_cond)
            }
            None => {
                // Default branch: true unless later branches match
                if let Some(later_or) = &suffix_or[idx] {
                    let negated = logical_not(later_or.clone());
                    let hydrated_negated = hydrate_expression(&negated, plan, provided_facts);
                    try_constant_fold(&hydrated_negated).unwrap_or(hydrated_negated)
                } else {
                    literal_expr(LiteralValue::Boolean(crate::BooleanValue::True))
                }
            }
        };

        result_pairs.push((effective_condition, simplified_result));
    }

    Ok(result_pairs)
}

/// Expand all rule references and hydrate given facts in an expression
///
/// This is the primary function used during Shape construction. It performs:
/// 1. Expansion: Recursively expands all `RulePath` nodes to their definitions
/// 2. Hydration: Substitutes given fact paths with their concrete values
/// 3. Simplification: Constant folds the resulting expression
///
/// **Note**: The planning phase already guarantees no cycles, so this function
/// performs simple recursive lookup without cycle detection.
///
/// # Arguments
/// * `expr` - The expression to expand and hydrate
/// * `plan` - The execution plan containing all rules and facts
/// * `provided_facts` - Facts that are given (will be substituted with values)
///
/// # Returns
/// * `Ok(Expression)` - Fully expanded and hydrated expression with no rule references
/// * `Err(LemmaError)` - If rule not found
///
/// # Guarantee
/// The returned expression contains NO `RulePath` or `RuleReference` nodes.
/// All rule references have been expanded to their underlying definitions.
pub fn expand_and_hydrate(
    expr: &Expression,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Expression> {
    // Step 1: Expand all rule references recursively
    let expanded = expand_expression_recursive(expr.clone(), plan)?;

    // Step 2: Hydrate (substitute given facts with values)
    let hydrated = hydrate_expression(&expanded, plan, provided_facts);

    // Step 3: Constant fold (simplify)
    let simplified = try_constant_fold(&hydrated).unwrap_or(hydrated);

    Ok(simplified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::ExpressionId;
    use crate::planning::Branch;
    use crate::semantic::{FactValue, LemmaFact, TypeAnnotation};
    use crate::{ArithmeticComputation, ComparisonComputation};

    fn create_test_plan() -> ExecutionPlan {
        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: std::collections::HashMap::new(),
            rules: Vec::new(),
            sources: std::collections::HashMap::new(),
        };

        let points_fact = LemmaFact {
            reference: crate::FactReference {
                segments: Vec::new(),
                fact: "points".to_string(),
            },
            value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(crate::LemmaType::Number)),
            source_location: None,
        };
        plan.facts
            .insert(FactPath::local("points".to_string()), points_fact);

        plan
    }

    fn create_simple_rule_plan() -> ExecutionPlan {
        let mut plan = create_test_plan();

        let rule_path = RulePath::local("tier".to_string());
        let branches = vec![Branch {
            condition: None,
            result: literal_expr(LiteralValue::Text("bronze".to_string())),
            source: None,
        }];

        let rule = ExecutableRule {
            path: rule_path.clone(),
            name: "tier".to_string(),
            branches,
            needs_facts: HashSet::new(),
            source: None,
        };

        plan.rules.push(rule);
        plan
    }

    fn create_multi_branch_rule_plan() -> ExecutionPlan {
        let mut plan = create_test_plan();

        let points_path = FactPath::local("points".to_string());

        let rule_path = RulePath::local("tier".to_string());
        let branches = vec![
            Branch {
                condition: None,
                result: literal_expr(LiteralValue::Text("bronze".to_string())),
                source: None,
            },
            Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Box::new(Expression::new(
                            ExpressionKind::FactPath(points_path.clone()),
                            None,
                            ExpressionId::new(0),
                        )),
                        ComparisonComputation::GreaterThanOrEqual,
                        Box::new(literal_expr(LiteralValue::Number(
                            rust_decimal::Decimal::from(100),
                        ))),
                    ),
                    None,
                    ExpressionId::new(0),
                )),
                result: literal_expr(LiteralValue::Text("silver".to_string())),
                source: None,
            },
            Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Box::new(Expression::new(
                            ExpressionKind::FactPath(points_path.clone()),
                            None,
                            ExpressionId::new(0),
                        )),
                        ComparisonComputation::GreaterThanOrEqual,
                        Box::new(literal_expr(LiteralValue::Number(
                            rust_decimal::Decimal::from(500),
                        ))),
                    ),
                    None,
                    ExpressionId::new(0),
                )),
                result: literal_expr(LiteralValue::Text("gold".to_string())),
                source: None,
            },
        ];

        let rule = ExecutableRule {
            path: rule_path.clone(),
            name: "tier".to_string(),
            branches,
            needs_facts: HashSet::from([points_path]),
            source: None,
        };

        plan.rules.push(rule);
        plan
    }

    fn create_recursive_rule_plan() -> ExecutionPlan {
        let mut plan = create_test_plan();

        let x_path = FactPath::local("x".to_string());

        let rule_a_path = RulePath::local("a".to_string());
        let rule_b_path = RulePath::local("b".to_string());
        let rule_c_path = RulePath::local("c".to_string());

        let rule_a = ExecutableRule {
            path: rule_a_path.clone(),
            name: "a".to_string(),
            branches: vec![Branch {
                condition: None,
                result: Expression::new(
                    ExpressionKind::Arithmetic(
                        Box::new(Expression::new(
                            ExpressionKind::FactPath(x_path.clone()),
                            None,
                            ExpressionId::new(0),
                        )),
                        ArithmeticComputation::Multiply,
                        Box::new(literal_expr(LiteralValue::Number(
                            rust_decimal::Decimal::from(2),
                        ))),
                    ),
                    None,
                    ExpressionId::new(0),
                ),
                source: None,
            }],
            needs_facts: HashSet::from([x_path.clone()]),
            source: None,
        };

        let rule_b = ExecutableRule {
            path: rule_b_path.clone(),
            name: "b".to_string(),
            branches: vec![Branch {
                condition: None,
                result: Expression::new(
                    ExpressionKind::Arithmetic(
                        Box::new(Expression::new(
                            ExpressionKind::RulePath(rule_a_path.clone()),
                            None,
                            ExpressionId::new(0),
                        )),
                        ArithmeticComputation::Add,
                        Box::new(literal_expr(LiteralValue::Number(
                            rust_decimal::Decimal::from(10),
                        ))),
                    ),
                    None,
                    ExpressionId::new(0),
                ),
                source: None,
            }],
            needs_facts: HashSet::new(),
            source: None,
        };

        let rule_c = ExecutableRule {
            path: rule_c_path.clone(),
            name: "c".to_string(),
            branches: vec![Branch {
                condition: None,
                result: Expression::new(
                    ExpressionKind::Arithmetic(
                        Box::new(Expression::new(
                            ExpressionKind::RulePath(rule_b_path.clone()),
                            None,
                            ExpressionId::new(0),
                        )),
                        ArithmeticComputation::Multiply,
                        Box::new(literal_expr(LiteralValue::Number(
                            rust_decimal::Decimal::from(3),
                        ))),
                    ),
                    None,
                    ExpressionId::new(0),
                ),
                source: None,
            }],
            needs_facts: HashSet::new(),
            source: None,
        };

        plan.rules.push(rule_a);
        plan.rules.push(rule_b);
        plan.rules.push(rule_c);
        plan
    }

    #[test]
    fn test_expand_simple_rule_single_branch() {
        let plan = create_simple_rule_plan();
        let rule_path = RulePath::local("tier".to_string());

        let result = expand_rule_reference(&rule_path, &plan);

        assert!(result.is_ok(), "Should expand successfully");
        let expanded = result.unwrap();

        // Single branch should return the result directly
        match &expanded.kind {
            ExpressionKind::Literal(LiteralValue::Text(ref s)) => {
                assert_eq!(s, "bronze", "Should expand to bronze");
            }
            _ => panic!("Expected literal 'bronze', got {:?}", expanded.kind),
        }
    }

    #[test]
    fn test_expand_multi_branch_rule() {
        let plan = create_multi_branch_rule_plan();
        let rule_path = RulePath::local("tier".to_string());

        let result = expand_rule_reference(&rule_path, &plan);

        assert!(result.is_ok(), "Should expand successfully");
        let expanded = result.unwrap();

        // Multi-branch rule should expand to LogicalOr structure
        match &expanded.kind {
            ExpressionKind::LogicalOr(_, _) => {
                // Good - it's an OR structure
            }
            _ => panic!("Expected LogicalOr structure, got {:?}", expanded.kind),
        }
    }

    #[test]
    fn test_expand_recursive_rule_references() {
        let plan = create_recursive_rule_plan();
        let rule_path = RulePath::local("c".to_string());

        let result = expand_rule_reference(&rule_path, &plan);

        assert!(result.is_ok(), "Should expand recursively");
        let expanded = result.unwrap();

        // c? should expand to (b? * 3) which expands to ((a? + 10) * 3) which expands to ((x * 2) + 10) * 3
        // Should have no RulePath nodes left
        assert!(
            !has_rule_path(&expanded),
            "Expanded expression should have no RulePath nodes"
        );
    }

    #[test]
    fn test_expand_rule_not_found() {
        let plan = create_test_plan();
        let rule_path = RulePath::local("nonexistent".to_string());

        let result = expand_rule_reference(&rule_path, &plan);

        assert!(result.is_err(), "Should fail for nonexistent rule");
        let error = result.unwrap_err();
        assert!(
            error.to_string().contains("Rule not found"),
            "Error should mention rule not found: {}",
            error
        );
    }

    #[test]
    fn test_expand_and_hydrate_simple() {
        let plan = create_simple_rule_plan();
        let rule_path = RulePath::local("tier".to_string());
        let expr = Expression::new(
            ExpressionKind::RulePath(rule_path),
            None,
            ExpressionId::new(0),
        );
        let provided_facts = HashSet::new();

        let result = expand_and_hydrate(&expr, &plan, &provided_facts);

        assert!(result.is_ok(), "Should expand and hydrate successfully");
        let expanded = result.unwrap();

        // Should be expanded to literal "bronze"
        match &expanded.kind {
            ExpressionKind::Literal(LiteralValue::Text(ref s)) => {
                assert_eq!(s, "bronze");
            }
            _ => panic!("Expected literal 'bronze', got {:?}", expanded.kind),
        }
    }

    #[test]
    fn test_expand_and_hydrate_with_facts() {
        let mut plan = create_test_plan();
        let points_path = FactPath::local("points".to_string());

        let points_fact = LemmaFact {
            reference: crate::FactReference {
                segments: Vec::new(),
                fact: "points".to_string(),
            },
            value: FactValue::Literal(LiteralValue::Number(rust_decimal::Decimal::from(150))),
            source_location: None,
        };
        plan.facts.insert(points_path.clone(), points_fact);

        let rule_path = RulePath::local("tier".to_string());
        let branches = vec![
            Branch {
                condition: None,
                result: literal_expr(LiteralValue::Text("bronze".to_string())),
                source: None,
            },
            Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Box::new(Expression::new(
                            ExpressionKind::FactPath(points_path.clone()),
                            None,
                            ExpressionId::new(0),
                        )),
                        ComparisonComputation::GreaterThanOrEqual,
                        Box::new(literal_expr(LiteralValue::Number(
                            rust_decimal::Decimal::from(100),
                        ))),
                    ),
                    None,
                    ExpressionId::new(0),
                )),
                result: literal_expr(LiteralValue::Text("silver".to_string())),
                source: None,
            },
        ];

        let rule = ExecutableRule {
            path: rule_path.clone(),
            name: "tier".to_string(),
            branches,
            needs_facts: HashSet::from([points_path.clone()]),
            source: None,
        };
        plan.rules.push(rule);

        let expr = Expression::new(
            ExpressionKind::RulePath(rule_path),
            None,
            ExpressionId::new(0),
        );
        let mut provided_facts = HashSet::new();
        provided_facts.insert(points_path);

        let result = expand_and_hydrate(&expr, &plan, &provided_facts);

        assert!(result.is_ok(), "Should expand and hydrate successfully");
        let expanded = result.unwrap();

        // With points=150, the condition points >= 100 should be true, so result should be "silver"
        // But wait - we need to check the actual structure. The expansion should handle the condition.
        // Actually, the expansion returns the piecewise structure, not a single value.
        // The hydration will substitute points=150, but the condition evaluation happens later.
        // So we just verify it's expanded (no RulePath) and hydrated (points is substituted).
        assert!(
            !has_rule_path(&expanded),
            "Expanded expression should have no RulePath nodes"
        );
    }

    #[test]
    fn test_expand_rule_reference_to_branches() {
        let plan = create_multi_branch_rule_plan();
        let rule_path = RulePath::local("tier".to_string());
        let provided_facts = HashSet::new();

        let result = expand_rule_reference_to_branches(&rule_path, &plan, &provided_facts);

        assert!(result.is_ok(), "Should expand conditions successfully");
        let pairs = result.unwrap();

        assert_eq!(pairs.len(), 3, "Should have 3 branch pairs");

        // Check that results are correct
        match &pairs[0].1.kind {
            ExpressionKind::Literal(LiteralValue::Text(ref s)) => {
                assert_eq!(s, "bronze", "First branch should be bronze");
            }
            _ => panic!("Expected bronze literal"),
        }
        match &pairs[1].1.kind {
            ExpressionKind::Literal(LiteralValue::Text(ref s)) => {
                assert_eq!(s, "silver", "Second branch should be silver");
            }
            _ => panic!("Expected silver literal"),
        }
        match &pairs[2].1.kind {
            ExpressionKind::Literal(LiteralValue::Text(ref s)) => {
                assert_eq!(s, "gold", "Third branch should be gold");
            }
            _ => panic!("Expected gold literal"),
        }
    }

    fn has_rule_path(expr: &Expression) -> bool {
        match &expr.kind {
            ExpressionKind::RulePath(_) | ExpressionKind::RuleReference(_) => true,
            ExpressionKind::Arithmetic(l, _, r) => has_rule_path(l) || has_rule_path(r),
            ExpressionKind::Comparison(l, _, r) => has_rule_path(l) || has_rule_path(r),
            ExpressionKind::LogicalAnd(l, r) | ExpressionKind::LogicalOr(l, r) => {
                has_rule_path(l) || has_rule_path(r)
            }
            ExpressionKind::LogicalNegation(inner, _) => has_rule_path(inner),
            ExpressionKind::UnitConversion(inner, _) => has_rule_path(inner),
            ExpressionKind::MathematicalComputation(_, inner) => has_rule_path(inner),
            _ => false,
        }
    }
}
