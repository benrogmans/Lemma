//! Inverse reasoning for Lemma rules
//!
//! Determines what inputs produce desired outputs through symbolic manipulation.
//!
//! The main entry point is [`invert()`], which takes an execution plan, rule name,
//! and target outcome, and returns a [`Shape`] representing all valid solutions.

mod collapse;
mod expansion;
mod shape;
mod solver;
mod target;

pub use collapse::{shape_to_domains, Bound, Domain};
pub use shape::{BranchOutcome, InversionResponse, Shape, ShapeBranch, Solution};
pub use target::{Target, TargetOp};

use crate::parsing::ast::ExpressionId;
use crate::planning::{ExecutableRule, ExecutionPlan};
use crate::{
    Expression, ExpressionKind, FactPath, LemmaError, LemmaResult, LiteralValue, RulePath,
};
use std::collections::HashSet;

use crate::OperationResult;

fn is_boolean_false(expr: &Expression) -> bool {
    matches!(
        expr.kind,
        ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::False))
    )
}

fn expressions_semantically_equal(a: &Expression, b: &Expression) -> bool {
    match (&a.kind, &b.kind) {
        (ExpressionKind::Literal(lit_a), ExpressionKind::Literal(lit_b)) => lit_a == lit_b,
        (ExpressionKind::FactPath(path_a), ExpressionKind::FactPath(path_b)) => path_a == path_b,
        (ExpressionKind::RulePath(path_a), ExpressionKind::RulePath(path_b)) => path_a == path_b,
        (ExpressionKind::Arithmetic(l1, op1, r1), ExpressionKind::Arithmetic(l2, op2, r2)) => {
            op1 == op2
                && expressions_semantically_equal(l1, l2)
                && expressions_semantically_equal(r1, r2)
        }
        (ExpressionKind::LogicalAnd(l1, r1), ExpressionKind::LogicalAnd(l2, r2))
        | (ExpressionKind::LogicalOr(l1, r1), ExpressionKind::LogicalOr(l2, r2)) => {
            expressions_semantically_equal(l1, l2) && expressions_semantically_equal(r1, r2)
        }
        (ExpressionKind::Comparison(l1, op1, r1), ExpressionKind::Comparison(l2, op2, r2)) => {
            op1 == op2
                && expressions_semantically_equal(l1, l2)
                && expressions_semantically_equal(r1, r2)
        }
        (ExpressionKind::LogicalNegation(e1, _), ExpressionKind::LogicalNegation(e2, _)) => {
            expressions_semantically_equal(e1, e2)
        }
        (
            ExpressionKind::MathematicalComputation(op1, e1),
            ExpressionKind::MathematicalComputation(op2, e2),
        ) => op1 == op2 && expressions_semantically_equal(e1, e2),
        (
            ExpressionKind::UnitConversion(e1, target1),
            ExpressionKind::UnitConversion(e2, target2),
        ) => target1 == target2 && expressions_semantically_equal(e1, e2),
        (ExpressionKind::Veto(v1), ExpressionKind::Veto(v2)) => v1.message == v2.message,
        _ => false,
    }
}

/// Create a literal expression
pub(crate) fn literal_expr(val: LiteralValue) -> Expression {
    Expression::new(ExpressionKind::Literal(val), None, ExpressionId::new(0))
}

/// Create a logical AND expression
pub(crate) fn logical_and(a: Expression, b: Expression) -> Expression {
    Expression::new(
        ExpressionKind::LogicalAnd(Box::new(a), Box::new(b)),
        None,
        ExpressionId::new(0),
    )
}

/// Create a logical OR expression
pub(crate) fn logical_or(a: Expression, b: Expression) -> Expression {
    Expression::new(
        ExpressionKind::LogicalOr(Box::new(a), Box::new(b)),
        None,
        ExpressionId::new(0),
    )
}

/// Create a logical NOT expression
pub(crate) fn logical_not(a: Expression) -> Expression {
    Expression::new(
        ExpressionKind::LogicalNegation(Box::new(a), crate::NegationType::Not),
        None,
        ExpressionId::new(0),
    )
}

/// Invert a rule to find input domains that produce a desired outcome.
///
/// Given an execution plan and rule name, determines what values the unknown
/// facts must have to produce the target outcome.
///
/// The `provided_facts` set contains fact paths that are fixed (user-provided values).
/// Only these facts are substituted during hydration; other fact values remain as
/// free variables for inversion.
///
/// Returns a [`Shape`] representing all valid solutions as a piecewise function.
pub fn invert(
    rule_name: &str,
    target: Target,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Shape> {
    let executable_rule = plan.get_rule(rule_name).ok_or_else(|| {
        LemmaError::Engine(format!("Rule not found: {}.{}", plan.doc_name, rule_name))
    })?;

    let rule_path_string = executable_rule.path.to_string();

    let all_branches = build_inversion_branches(executable_rule);

    let mut expanded_branches = Vec::new();
    for (condition, result) in all_branches {
        let branches = expand_result_rule_references(condition, result, plan, provided_facts)?;
        expanded_branches.extend(branches);
    }

    let branches_with_options: Vec<(Option<Expression>, Expression)> = expanded_branches
        .iter()
        .map(|(cond, result)| (Some(cond.clone()), result.clone()))
        .collect();
    let suffix_or = build_suffix_or_conditions(&branches_with_options);

    let mut branches_out = Vec::new();
    let mut available_outcomes = Vec::new();

    for (idx, (raw_condition, raw_result)) in expanded_branches.iter().enumerate() {
        let mut effective_condition = raw_condition.clone();
        if let Some(later_or) = &suffix_or[idx] {
            effective_condition = logical_and(effective_condition, logical_not(later_or.clone()));
        }

        let expanded_and_hydrated_condition =
            expansion::expand_and_hydrate(&effective_condition, plan, provided_facts)?;
        let outcome = match &raw_result.kind {
            ExpressionKind::Veto(ve) => BranchOutcome::Veto(ve.message.clone()),
            _ => {
                let expanded_and_hydrated_result =
                    expansion::expand_and_hydrate(raw_result, plan, provided_facts)?;
                BranchOutcome::Value(expanded_and_hydrated_result)
            }
        };

        if !is_boolean_false(&expanded_and_hydrated_condition) {
            let outcome_desc = match &outcome {
                BranchOutcome::Value(expr) => {
                    if let ExpressionKind::Literal(lit) = &expr.kind {
                        format!("value {}", lit)
                    } else {
                        "computed value".to_owned()
                    }
                }
                BranchOutcome::Veto(Some(msg)) => format!("veto '{}'", msg),
                BranchOutcome::Veto(None) => "veto".to_owned(),
            };
            available_outcomes.push(outcome_desc);
        }

        if let Some(branch) = filter_branch(
            expanded_and_hydrated_condition,
            outcome,
            &target,
            plan,
            provided_facts,
        )? {
            branches_out.push(branch);
        }
    }

    if branches_out.is_empty() {
        return Err(build_no_solution_error(
            &rule_path_string,
            &target,
            &available_outcomes,
        ));
    }

    let unified_branches = unify_branches(branches_out);

    let mut free_vars = collect_free_vars_piecewise(&unified_branches, plan);
    dedup_and_remove_given(&mut free_vars, provided_facts);

    Ok(Shape::new(unified_branches, free_vars))
}

/// Expand rule references in result expressions into multiple branches
///
/// If the result expression is a top-level RulePath, expands it into multiple branches.
/// Otherwise, returns a single branch.
///
/// This allows rule references in results (e.g., `rule another = base?`) to be
/// expanded into multiple branches before processing, avoiding LogicalOr structures
/// in the main inversion loop.
fn expand_result_rule_references(
    condition: Expression,
    result: Expression,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Vec<(Expression, Expression)>> {
    match &result.kind {
        ExpressionKind::RulePath(rule_path) => {
            let expanded_branches =
                expansion::expand_rule_reference_to_branches(rule_path, plan, provided_facts)?;
            let mut combined_branches = Vec::new();
            for (base_condition, base_result) in expanded_branches {
                let combined_condition = logical_and(condition.clone(), base_condition);
                combined_branches.push((combined_condition, base_result));
            }
            Ok(combined_branches)
        }
        _ => Ok(vec![(condition, result)]),
    }
}

/// Build inversion branches from an executable rule
///
/// Converts ExecutableRule.branches into (condition, result) pairs.
/// First branch has condition=true (default), subsequent branches have their conditions.
fn build_inversion_branches(rule: &ExecutableRule) -> Vec<(Expression, Expression)> {
    let mut all_branches: Vec<(Expression, Expression)> = Vec::new();

    for (idx, branch) in rule.branches.iter().enumerate() {
        let condition = if idx == 0 {
            literal_expr(LiteralValue::Boolean(crate::BooleanValue::True))
        } else {
            branch
                .condition
                .clone()
                .unwrap_or_else(|| literal_expr(LiteralValue::Boolean(crate::BooleanValue::True)))
        };
        all_branches.push((condition, branch.result.clone()));
    }

    all_branches
}

/// Build suffix OR conditions for branch exclusivity
///
/// For each branch i, suffix_or[i] is the OR of all conditions from branches i+1 to end.
/// This is used to ensure "last matching wins" semantics.
///
/// Works with branches where conditions can be None (default branch = true).
pub(crate) fn build_suffix_or_conditions(
    branches: &[(Option<Expression>, Expression)],
) -> Vec<Option<Expression>> {
    let mut suffix_or: Vec<Option<Expression>> = vec![None; branches.len()];
    let mut acc: Option<Expression> = None;

    for i in (0..branches.len()).rev() {
        suffix_or[i] = acc.clone();
        let cond = branches[i].0.as_ref();
        if let Some(condition) = cond {
            acc = Some(match acc {
                None => condition.clone(),
                Some(prev) => logical_or(condition.clone(), prev),
            });
        }
    }

    suffix_or
}

/// Build error message when no solution is found
fn build_no_solution_error(
    rule_path: &str,
    target: &Target,
    available_outcomes: &[String],
) -> LemmaError {
    let target_desc = match &target.outcome {
        None => "any value".to_owned(),
        Some(OperationResult::Value(v)) => format!("value {}", v),
        Some(OperationResult::Veto(Some(msg))) => format!("veto '{}'", msg),
        Some(OperationResult::Veto(None)) => "any veto".to_owned(),
    };

    let op_str = match target.op {
        TargetOp::Eq => "=",
        TargetOp::Neq => "!=",
        TargetOp::Lt => "<",
        TargetOp::Lte => "<=",
        TargetOp::Gt => ">",
        TargetOp::Gte => ">=",
    };

    let mut error_msg = format!(
        "Cannot invert rule '{}' for target {} {}.\n",
        rule_path, op_str, target_desc
    );

    if !available_outcomes.is_empty() {
        error_msg.push_str("This rule can produce:\n");
        for (i, outcome) in available_outcomes.iter().enumerate() {
            error_msg.push_str(&format!("  {}: {}\n", i + 1, outcome));
        }
    } else {
        error_msg.push_str("No branches in this rule can be satisfied with the given facts.");
    }

    LemmaError::Engine(error_msg)
}

/// Filter a branch based on the target outcome
fn filter_branch(
    hydrated_condition: Expression,
    outcome: BranchOutcome,
    target: &Target,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Option<ShapeBranch>> {
    match (&outcome, &target.outcome) {
        (BranchOutcome::Value(_value_expr), None) => {
            let simplified_condition =
                solver::simplify_boolean(&hydrated_condition, &expressions_semantically_equal)?;
            if is_boolean_false(&simplified_condition) {
                Ok(None)
            } else {
                Ok(Some(ShapeBranch {
                    condition: simplified_condition,
                    outcome,
                }))
            }
        }
        (BranchOutcome::Value(ref value_expr), Some(OperationResult::Value(_))) => {
            let mut guard = build_value_target_guard(value_expr, target);

            if let ExpressionKind::Comparison(lhs, op, rhs) = &guard.kind {
                if matches!(op, crate::ComparisonComputation::Equal) {
                    if let ExpressionKind::RulePath(rp) = &lhs.kind {
                        if let Some(referenced_rule) = plan.get_rule_by_path(rp) {
                            if let Some(default_expr) =
                                expansion::get_default_expression(referenced_rule)
                            {
                                let inner_expr = expansion::expand_and_hydrate(
                                    default_expr,
                                    plan,
                                    provided_facts,
                                )?;
                                guard = Expression::new(
                                    ExpressionKind::Comparison(
                                        Box::new(inner_expr),
                                        op.clone(),
                                        Box::new((**rhs).clone()),
                                    ),
                                    None,
                                    ExpressionId::new(0),
                                );

                                let veto_conditions =
                                    expansion::collect_veto_conditions(referenced_rule);
                                if !veto_conditions.is_empty() {
                                    let hydrated_veto_conds: Vec<Expression> = veto_conditions
                                        .into_iter()
                                        .map(|c| {
                                            expansion::expand_and_hydrate(c, plan, provided_facts)
                                        })
                                        .collect::<LemmaResult<Vec<Expression>>>()?;

                                    let combined_veto_conditions = hydrated_veto_conds
                                        .into_iter()
                                        .reduce(logical_or)
                                        .expect("veto_conditions was non-empty");

                                    let veto_guard = logical_not(combined_veto_conditions);
                                    let extended_condition =
                                        logical_and(hydrated_condition.clone(), veto_guard);
                                    let simplified_condition = solver::simplify_boolean(
                                        &extended_condition,
                                        &expressions_semantically_equal,
                                    )?;

                                    if is_boolean_false(&simplified_condition) {
                                        return Ok(None);
                                    }

                                    let hydrated_guard = expansion::expand_and_hydrate(
                                        &guard,
                                        plan,
                                        provided_facts,
                                    )?;
                                    let conjunction =
                                        logical_and(simplified_condition, hydrated_guard);
                                    let simplified_conjunction = solver::simplify_boolean(
                                        &conjunction,
                                        &expressions_semantically_equal,
                                    )?;

                                    if is_boolean_false(&simplified_conjunction) {
                                        return Ok(None);
                                    }

                                    return Ok(Some(ShapeBranch {
                                        condition: simplified_conjunction,
                                        outcome,
                                    }));
                                }
                            }
                        }
                    }
                }
            }

            let hydrated_guard = expansion::expand_and_hydrate(&guard, plan, provided_facts)?;
            let conjunction = logical_and(hydrated_condition, hydrated_guard);
            let simplified_conjunction =
                solver::simplify_boolean(&conjunction, &expressions_semantically_equal)?;

            if is_boolean_false(&simplified_conjunction) {
                Ok(None)
            } else {
                Ok(Some(ShapeBranch {
                    condition: simplified_conjunction,
                    outcome,
                }))
            }
        }
        (BranchOutcome::Veto(msg), Some(OperationResult::Veto(query_msg))) => {
            let matches = match (query_msg, msg) {
                (None, _) => true,
                (Some(q), Some(m)) => q == m,
                _ => false,
            };
            if !matches {
                return Ok(None);
            }
            let simplified_condition =
                solver::simplify_boolean(&hydrated_condition, &expressions_semantically_equal)?;
            if is_boolean_false(&simplified_condition) {
                Ok(None)
            } else {
                Ok(Some(ShapeBranch {
                    condition: simplified_condition,
                    outcome,
                }))
            }
        }
        _ => Ok(None),
    }
}

fn build_value_target_guard(expr: &Expression, target: &Target) -> Expression {
    let rhs = match &target.outcome {
        Some(OperationResult::Value(v)) => literal_expr(v.clone()),
        _ => unreachable!("build_value_target_guard called with non-value target"),
    };
    let op = match target.op {
        TargetOp::Eq => crate::ComparisonComputation::Equal,
        TargetOp::Neq => crate::ComparisonComputation::NotEqual,
        TargetOp::Lt => crate::ComparisonComputation::LessThan,
        TargetOp::Lte => crate::ComparisonComputation::LessThanOrEqual,
        TargetOp::Gt => crate::ComparisonComputation::GreaterThan,
        TargetOp::Gte => crate::ComparisonComputation::GreaterThanOrEqual,
    };
    Expression::new(
        ExpressionKind::Comparison(Box::new(expr.clone()), op, Box::new(rhs)),
        None,
        ExpressionId::new(0),
    )
}

fn unify_branches(branches: Vec<ShapeBranch>) -> Vec<ShapeBranch> {
    if branches.is_empty() {
        return branches;
    }

    let mut result = Vec::new();
    let mut processed = vec![false; branches.len()];

    for i in 0..branches.len() {
        if processed[i] {
            continue;
        }

        let mut matching_indices = vec![i];
        for j in (i + 1)..branches.len() {
            if !processed[j] && outcomes_equal(&branches[i].outcome, &branches[j].outcome) {
                matching_indices.push(j);
                processed[j] = true;
            }
        }
        processed[i] = true;

        let unified_condition = if matching_indices.len() == 1 {
            branches[i].condition.clone()
        } else {
            let or_expr = matching_indices.iter().skip(1).fold(
                branches[matching_indices[0]].condition.clone(),
                |acc, &idx| logical_or(acc, branches[idx].condition.clone()),
            );
            solver::simplify_or_expression(&or_expr, &expressions_semantically_equal)
        };

        result.push(ShapeBranch {
            condition: unified_condition,
            outcome: branches[i].outcome.clone(),
        });
    }

    result
}

fn outcomes_equal(a: &BranchOutcome, b: &BranchOutcome) -> bool {
    match (a, b) {
        (BranchOutcome::Veto(msg_a), BranchOutcome::Veto(msg_b)) => msg_a == msg_b,
        (BranchOutcome::Value(expr_a), BranchOutcome::Value(expr_b)) => {
            expressions_semantically_equal(expr_a, expr_b)
        }
        _ => false,
    }
}

fn extract_references(expr: &Expression) -> (HashSet<FactPath>, HashSet<RulePath>) {
    let mut fact_refs = HashSet::new();
    let mut rule_refs = HashSet::new();
    collect_references(expr, &mut fact_refs, &mut rule_refs);
    (fact_refs, rule_refs)
}

fn collect_references(
    expr: &Expression,
    fact_refs: &mut HashSet<FactPath>,
    rule_refs: &mut HashSet<RulePath>,
) {
    match &expr.kind {
        ExpressionKind::FactPath(fact_path) => {
            fact_refs.insert(fact_path.clone());
        }
        ExpressionKind::RulePath(rule_path) => {
            rule_refs.insert(rule_path.clone());
        }
        ExpressionKind::Arithmetic(left, _op, right) => {
            collect_references(left, fact_refs, rule_refs);
            collect_references(right, fact_refs, rule_refs);
        }
        ExpressionKind::Comparison(left, _op, right) => {
            collect_references(left, fact_refs, rule_refs);
            collect_references(right, fact_refs, rule_refs);
        }
        ExpressionKind::LogicalAnd(left, right) => {
            collect_references(left, fact_refs, rule_refs);
            collect_references(right, fact_refs, rule_refs);
        }
        ExpressionKind::LogicalOr(left, right) => {
            collect_references(left, fact_refs, rule_refs);
            collect_references(right, fact_refs, rule_refs);
        }
        ExpressionKind::LogicalNegation(inner, _negation_type) => {
            collect_references(inner, fact_refs, rule_refs);
        }
        ExpressionKind::UnitConversion(value, _target) => {
            collect_references(value, fact_refs, rule_refs);
        }
        ExpressionKind::MathematicalComputation(_op, operand) => {
            collect_references(operand, fact_refs, rule_refs);
        }
        ExpressionKind::Literal(_) | ExpressionKind::Veto(_) => {}
        ExpressionKind::FactReference(_) | ExpressionKind::RuleReference(_) => {
            unreachable!("FactReference and RuleReference should not appear after expansion")
        }
    }
}

fn collect_free_vars_piecewise(branches: &[ShapeBranch], plan: &ExecutionPlan) -> Vec<FactPath> {
    let mut vars = Vec::new();
    for br in branches {
        vars.extend(collect_free_vars_expr(&br.condition, plan));
        if let BranchOutcome::Value(expr) = &br.outcome {
            vars.extend(collect_free_vars_expr(expr, plan));
        }
    }
    vars
}

fn collect_free_vars_expr(expr: &Expression, plan: &ExecutionPlan) -> Vec<FactPath> {
    let mut result = Vec::new();
    let (fact_refs, rule_refs) = extract_references(expr);

    for path in fact_refs {
        result.push(path);
    }

    if !rule_refs.is_empty() {
        for rule_path in rule_refs {
            if let Some(referenced_rule) = plan.get_rule_by_path(&rule_path) {
                for branch in &referenced_rule.branches {
                    if let Some(condition) = &branch.condition {
                        result.extend(collect_free_vars_expr(condition, plan));
                    }
                    result.extend(collect_free_vars_expr(&branch.result, plan));
                }
            }
        }
    }

    result
}

fn dedup_and_remove_given(vars: &mut Vec<FactPath>, provided_facts: &HashSet<FactPath>) {
    vars.sort_by(|a, b| {
        let a_facts: Vec<String> = a.segments.iter().map(|s| s.fact.clone()).collect();
        let b_facts: Vec<String> = b.segments.iter().map(|s| s.fact.clone()).collect();
        a_facts.cmp(&b_facts).then(a.fact.cmp(&b.fact))
    });
    vars.dedup();
    vars.retain(|fact_path| !provided_facts.contains(fact_path));
}
