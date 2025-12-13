//! Constraint solver for inversion
//!
//! Target-aware constraint solving for determining what inputs produce desired outputs.
//! Uses the computation module for constraint types and operations.

use crate::computation::{
    collect_domain_restrictions, reverse_comparison, ConstraintSet,
    DomainRestriction, FactConstraint, OperationResult, UnsatReason,
};
use crate::semantic::{
    ArithmeticComputation, BooleanValue, ComparisonComputation, EqualityNotation, Expression,
    ExpressionKind, FactPath, LiteralValue, NegationType,
};
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::Target;

/// Result of solving an equation
#[derive(Debug, Clone)]
pub enum SolveResult {
    /// Fully solved to concrete domains
    Solved {
        outcome: OperationResult,
        fact_constraints: HashMap<FactPath, FactConstraint>,
    },

    /// Partially solved — some constraints remain symbolic
    Partial {
        outcome: OperationResult,
        fact_constraints: HashMap<FactPath, FactConstraint>,
        remaining_constraints: Vec<Expression>,
        domain_restrictions: Vec<DomainRestriction>,
    },

    /// Contradiction detected — no valid solution
    Unsatisfiable { reason: UnsatReason },
}

// ============================================================================
// Helper functions
// ============================================================================

/// Flatten a DNF expression into a list of OR branches
///
/// Returns all top-level OR alternatives as separate expressions.
fn flatten_or(expression: Expression) -> Vec<Expression> {
    match expression.kind {
        ExpressionKind::LogicalOr(left, right) => {
            let mut branches = flatten_or(Arc::unwrap_or_clone(left));
            branches.extend(flatten_or(Arc::unwrap_or_clone(right)));
            branches
        }
        _ => vec![expression],
    }
}

// ============================================================================
// Main solver entry point
// ============================================================================

/// Solve an equation for the given target, preserving outcome information
///
/// This function extracts both constraints and outcomes from each branch.
/// When target is None, it skips apply_target to preserve all outcome information.
pub fn solve_with_target(equation: Expression, target: &Target) -> Vec<SolveResult> {
    // Flatten OR branches to get individual (condition ∧ result) branches
    let branches = flatten_or(equation);
    
    // Solve each branch, extracting outcome and checking target match
    let mut results: Vec<SolveResult> = Vec::new();
    for branch in branches {
        if let Some(result) = solve_branch_with_outcome(branch, target) {
            results.push(result);
        }
    }
    
    // If all branches were filtered or unsatisfiable, return single Unsatisfiable
    if results.is_empty() {
        return vec![SolveResult::Unsatisfiable {
            reason: UnsatReason::SimplifiedToFalse,
        }];
    }
    
    results
}

/// Solve a single branch while extracting its outcome
///
/// Returns None if the branch doesn't match the target (filtered out)
fn solve_branch_with_outcome(branch: Expression, target: &Target) -> Option<SolveResult> {
    // Try to extract (condition ∧ result) structure
    let (condition, result) = if let Some((cond, res)) = extract_condition_and_result(&branch) {
        // Normal case: explicit (condition ∧ result) structure
        (cond, res)
    } else {
        // Branch doesn't have AND structure
        // Check if it's a bare false literal - this means unsatisfiable (from simplification)
        if matches!(&branch.kind, ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False))) {
            return None;  // Filter out bare 'false' - it's unsatisfiable
        }
        
        // Treat branch as result with implicit true condition
        // Comparisons/logical expressions can be results that evaluate to boolean
        (
            Expression::new(
                ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
                None,
            ),
            branch,
            )
    };
    
    // Check if result is a simple literal/veto or needs algebraic solving
            match &result.kind {
        ExpressionKind::Literal(val) => {
            // Note: We don't filter false here, because if we got here,
            // false is a valid result value from a (condition ∧ false) branch
            
            // Simple case: result is already a literal value
            let outcome = OperationResult::Value(val.clone());
            
            // Check if outcome matches target (filtering logic)
            if !matches_target(&outcome, target) {
                return None;  // Filter out this branch
            }
            
            // Extract constraints from condition with the outcome
            let solve_result = solve_single_branch(condition, outcome);
            
            // Filter out unsatisfiable results
            match solve_result {
                SolveResult::Unsatisfiable { .. } => None,
                _ => Some(solve_result),
            }
        }
                ExpressionKind::Veto(veto) => {
            // Simple case: result is a veto
            let outcome = OperationResult::Veto(veto.message.clone());
            
            // Check if outcome matches target
            if !matches_target(&outcome, target) {
                return None;
            }
            
            let solve_result = solve_single_branch(condition, outcome);
            match solve_result {
                SolveResult::Unsatisfiable { .. } => None,
                _ => Some(solve_result),
            }
                }
        _ => {
            // Complex case: result is an expression that needs algebraic solving
            if let Some(OperationResult::Value(target_val)) = &target.outcome {
                // Special case: if result is a comparison/logical expr and target is boolean
                let result_condition = if matches!(target_val, LiteralValue::Boolean(_)) 
                    && (matches!(&result.kind, ExpressionKind::Comparison(_, _, _) | ExpressionKind::LogicalAnd(_, _) | ExpressionKind::LogicalOr(_, _) | ExpressionKind::LogicalNegation(_, _))) {
                    // Result is a boolean expression
                    match target_val {
                        LiteralValue::Boolean(BooleanValue::True) => {
                            // Target is true: use comparison as-is
                            result.clone()
                        }
                        LiteralValue::Boolean(BooleanValue::False) => {
                            // Target is false: negate the comparison
                            Expression::new(
                                ExpressionKind::LogicalNegation(Arc::new(result.clone()), NegationType::Not),
                    None,
                            )
            }
                        _ => unreachable!()
                    }
                } else {
                    // For arithmetic: create comparison result == target_value
            Expression::new(
                ExpressionKind::Comparison(
                            Arc::new(result.clone()),
                            ComparisonComputation::Equal(EqualityNotation::Symbol),
                            Arc::new(Expression::new(
                                ExpressionKind::Literal(target_val.clone()),
                        None,
                    )),
                ),
                None,
            )
                };
                
                // Combine condition with result_condition
                let full_condition = Expression::new(
                        ExpressionKind::LogicalAnd(
                        Arc::new(condition),
                        Arc::new(result_condition),
                        ),
                        None,
                    );
                
                let outcome = OperationResult::Value(target_val.clone());
                let solve_result = solve_single_branch(full_condition, outcome);
                
                match solve_result {
                    SolveResult::Unsatisfiable { .. } => None,
                    _ => Some(solve_result),
                }
            } else {
                // No target value to compare against - can't solve arithmetic without target
                None
            }
        }
    }
}

/// Extract condition and result from a branch expression
///
/// Branches have structure: (condition ∧ result)
fn extract_condition_and_result(branch: &Expression) -> Option<(Expression, Expression)> {
    match &branch.kind {
        ExpressionKind::LogicalAnd(cond, res) => {
            Some((Arc::unwrap_or_clone(cond.clone()), Arc::unwrap_or_clone(res.clone())))
        }
        _ => None,
    }
}


/// Check if an outcome matches the target criteria
fn matches_target(outcome: &OperationResult, target: &Target) -> bool {
    match &target.outcome {
        None => true,  // any outcome matches
        Some(OperationResult::Veto(None)) => {
            // any_veto: match any veto
            matches!(outcome, OperationResult::Veto(_))
        }
        Some(OperationResult::Veto(Some(target_msg))) => {
            // specific veto: match exact veto message
            matches!(outcome, OperationResult::Veto(Some(msg)) if msg == target_msg)
        }
        Some(OperationResult::Value(target_val)) => {
            // specific value: apply operator comparison
            match outcome {
                OperationResult::Value(outcome_val) => {
                    compare_with_operator(outcome_val, target_val, &target.op)
                }
                OperationResult::Veto(_) => false,  // veto doesn't match value target
            }
        }
    }
}

/// Compare two values with the given operator
fn compare_with_operator(left: &LiteralValue, right: &LiteralValue, op: &crate::inversion::TargetOp) -> bool {
    use crate::inversion::TargetOp;
    
    match op {
        TargetOp::Eq => left == right,
        TargetOp::Neq => left != right,
        TargetOp::Lt => {
            if let (LiteralValue::Number(l), LiteralValue::Number(r)) = (left, right) {
                l < r
            } else {
                false
            }
        }
        TargetOp::Lte => {
            if let (LiteralValue::Number(l), LiteralValue::Number(r)) = (left, right) {
                l <= r
            } else {
                false
            }
        }
        TargetOp::Gt => {
            if let (LiteralValue::Number(l), LiteralValue::Number(r)) = (left, right) {
                l > r
            } else {
                false
        }
    }
        TargetOp::Gte => {
            if let (LiteralValue::Number(l), LiteralValue::Number(r)) = (left, right) {
                l >= r
            } else {
                false
            }
        }
    }
}

/// Solve a single branch (conjunction of constraints)
///
/// This function handles a single AND-branch from the DNF form.
/// OR expressions should not appear here after DNF conversion.
fn solve_single_branch(expression: Expression, outcome: OperationResult) -> SolveResult {
    let mut constraint_set = ConstraintSet::new();

    // Check for trivial true
    if expression.is_boolean_true() {
        return SolveResult::Solved {
            outcome,
            fact_constraints: HashMap::new(),
        };
    }

    // Check for trivial false
    if expression.is_boolean_false() {
        return SolveResult::Unsatisfiable {
            reason: UnsatReason::SimplifiedToFalse,
        };
    }

    // Collect domain restrictions from the expression
    let restrictions = collect_domain_restrictions(&expression);
    for restriction in restrictions {
        constraint_set.add_restriction(restriction);
    }

    // Extract constraints from the expression
    extract_constraints(&expression, &mut constraint_set);

    // Check for contradictions
    if let Some(reason) = constraint_set.contradiction.take() {
        return SolveResult::Unsatisfiable { reason };
    }

    // Convert to result
    let fact_constraints = constraint_set.to_fact_constraints();

    if constraint_set.symbolic.is_empty() && constraint_set.restrictions.is_empty() {
        SolveResult::Solved { outcome, fact_constraints }
    } else {
        SolveResult::Partial {
            outcome,
            fact_constraints,
            remaining_constraints: constraint_set.symbolic,
            domain_restrictions: constraint_set.restrictions,
        }
    }
}

/// Extract constraints from an expression into the constraint set
fn extract_constraints(expression: &Expression, constraint_set: &mut ConstraintSet) {
    match &expression.kind {
        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)) => {
            // true contributes no constraints
        }

        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)) => {
            // false means unsatisfiable
            constraint_set.contradiction = Some(UnsatReason::SimplifiedToFalse);
        }

        ExpressionKind::LogicalAnd(left, right) => {
            extract_constraints(left, constraint_set);
            extract_constraints(right, constraint_set);
        }

        ExpressionKind::LogicalOr(left, right) => {
            // OR represents alternative branches - add as symbolic for now
            constraint_set.add_symbolic(expression.clone());
            let _ = (left, right);
        }

        ExpressionKind::Comparison(left, op, right) => {
            // Try to extract fact op literal (direct case)
            if let ExpressionKind::FactPath(fact_path) = &left.kind {
                if let ExpressionKind::Literal(value) = &right.kind {
                    constraint_set.add_comparison(fact_path.clone(), op, value.clone());
                    return;
                }
            }

            // Try reversed: literal op fact
            if let ExpressionKind::FactPath(fact_path) = &right.kind {
                if let ExpressionKind::Literal(value) = &left.kind {
                    let reversed_op = reverse_comparison(op);
                    constraint_set.add_comparison(fact_path.clone(), &reversed_op, value.clone());
                    return;
                }
            }

            // Try fact op fact (relational constraint)
            if let (ExpressionKind::FactPath(left_fact), ExpressionKind::FactPath(right_fact)) =
                (&left.kind, &right.kind)
            {
                constraint_set.add_relation(left_fact.clone(), op.clone(), right_fact.clone());
                return;
            }

            // Try algebraic isolation for arithmetic expressions
            match try_isolate_comparison(left, op, right) {
                IsolationResult::Isolated { fact, op, value } => {
                    constraint_set.add_comparison(fact, &op, value);
                    return;
                }
                IsolationResult::Unconstrained => {
                    // No constraint needed - expression is always true
                    return;
                }
                IsolationResult::Unsatisfiable(reason) => {
                    constraint_set.contradiction = Some(reason);
                    return;
                }
                IsolationResult::MultipleUnknowns(simplified) => {
                    // Add the simplified expression as symbolic
                    constraint_set.add_symbolic(simplified);
                    return;
                }
                IsolationResult::Symbolic => {
                    // Fall through to add as symbolic
                }
            }

            // Also try with reversed operands (literal op arithmetic_expr)
            if let ExpressionKind::Literal(_) = &left.kind {
                let reversed_op = reverse_comparison(op);
                match try_isolate_comparison(right, &reversed_op, left) {
                    IsolationResult::Isolated { fact, op, value } => {
                        constraint_set.add_comparison(fact, &op, value);
                        return;
                    }
                    IsolationResult::Unconstrained => {
                        return;
                    }
                    IsolationResult::Unsatisfiable(reason) => {
                        constraint_set.contradiction = Some(reason);
                        return;
                    }
                    IsolationResult::MultipleUnknowns(simplified) => {
                        constraint_set.add_symbolic(simplified);
                        return;
                    }
                    IsolationResult::Symbolic => {
                        // Fall through
                    }
                }
            }

            // Complex comparison - add as symbolic
            constraint_set.add_symbolic(expression.clone());
        }

        ExpressionKind::LogicalNegation(inner, _) => {
            // NOT(comparison) → opposite comparison
            if let ExpressionKind::Comparison(left, op, right) = &inner.kind {
                // Convert to opposite comparison
                let opposite_op = match op {
                    ComparisonComputation::Equal(notation) => ComparisonComputation::NotEqual(notation.clone()),
                    ComparisonComputation::NotEqual(notation) => ComparisonComputation::Equal(notation.clone()),
                    ComparisonComputation::LessThan => ComparisonComputation::GreaterThanOrEqual,
                    ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThan,
                    ComparisonComputation::GreaterThan => ComparisonComputation::LessThanOrEqual,
                    ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThan,
                };
                let opposite_comparison = Expression::new(
                    ExpressionKind::Comparison(left.clone(), opposite_op, right.clone()),
                    None,
                            );
                extract_constraints(&opposite_comparison, constraint_set);
                            return;
            }

            // NOT(fact) means fact == false
            if let ExpressionKind::FactPath(fact_path) = &inner.kind {
                constraint_set.add_comparison(
                    fact_path.clone(),
                    &ComparisonComputation::Equal(crate::semantic::EqualityNotation::Symbol),
                    LiteralValue::Boolean(BooleanValue::False),
                );
                return;
            }

            // Complex negation - add as symbolic
            constraint_set.add_symbolic(expression.clone());
        }

        ExpressionKind::FactPath(fact_path) => {
            // Bare fact reference means fact == true
                            constraint_set.add_comparison(
                                fact_path.clone(),
                &ComparisonComputation::Equal(crate::semantic::EqualityNotation::Symbol),
                LiteralValue::Boolean(BooleanValue::True),
            );
        }

        // Other expression types - add as symbolic
        _ => {
            constraint_set.add_symbolic(expression.clone());
        }
    }
}

// ============================================================================
// Algebraic Isolation
// ============================================================================

/// Result of attempting to isolate a fact from an arithmetic expression
#[derive(Debug)]
enum IsolationResult {
    /// Successfully isolated: fact op value
    Isolated {
        fact: FactPath,
        op: ComparisonComputation,
        value: LiteralValue,
    },
    /// The constraint is satisfied for any value of the fact
    Unconstrained,
    /// The constraint can never be satisfied
    Unsatisfiable(UnsatReason),
    /// Multiple facts present - return simplified expression
    MultipleUnknowns(Expression),
    /// Cannot isolate (unsupported structure)
    Symbolic,
}

/// Collect all fact paths from an expression
fn collect_facts(expression: &Expression) -> HashSet<FactPath> {
    let mut facts = HashSet::new();
    collect_facts_recursive(expression, &mut facts);
    facts
}

fn collect_facts_recursive(expression: &Expression, facts: &mut HashSet<FactPath>) {
    match &expression.kind {
        ExpressionKind::FactPath(path) => {
            facts.insert(path.clone());
        }
        ExpressionKind::Arithmetic(left, _, right) => {
            collect_facts_recursive(left, facts);
            collect_facts_recursive(right, facts);
        }
        ExpressionKind::MathematicalComputation(_, inner) => {
            collect_facts_recursive(inner, facts);
        }
        ExpressionKind::Comparison(left, _, right) => {
            collect_facts_recursive(left, facts);
            collect_facts_recursive(right, facts);
        }
        ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
            collect_facts_recursive(left, facts);
            collect_facts_recursive(right, facts);
        }
        ExpressionKind::LogicalNegation(inner, _) => {
            collect_facts_recursive(inner, facts);
        }
        ExpressionKind::UnitConversion(inner, _) => {
            collect_facts_recursive(inner, facts);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::RulePath(_) => {}
        
        ExpressionKind::RuleReference(_) | ExpressionKind::FactReference(_) => {
            unreachable!(
                "bug: FactReference/RuleReference in inversion solver - \
                 should have been converted to FactPath/RulePath during graph building"
            )
        }
    }
}

/// Check if an expression contains a specific fact
fn contains_fact(expression: &Expression, target: &FactPath) -> bool {
    match &expression.kind {
        ExpressionKind::FactPath(path) => path == target,
        ExpressionKind::Arithmetic(left, _, right) => {
            contains_fact(left, target) || contains_fact(right, target)
        }
        ExpressionKind::MathematicalComputation(_, inner) => contains_fact(inner, target),
        ExpressionKind::UnitConversion(inner, _) => contains_fact(inner, target),
        _ => false,
    }
}

/// Try to algebraically isolate a fact from a comparison
///
/// Given `expr op literal`, attempts to solve for the single fact.
fn try_isolate_comparison(
    left: &Expression,
    op: &ComparisonComputation,
    right: &Expression,
) -> IsolationResult {
    // Check if right side is a literal
    let target_value = match &right.kind {
        ExpressionKind::Literal(value) => value.clone(),
        _ => return IsolationResult::Symbolic,
    };

    // Collect facts from left side
    let facts = collect_facts(left);

    match facts.len() {
        0 => {
            // No facts - this is a constant comparison, should have been reduced
            IsolationResult::Symbolic
        }
        1 => {
            // Single fact - try to isolate
            let fact_path = facts.into_iter().next().expect("checked len == 1");
            isolate_single_fact(left, &fact_path, op, &target_value)
        }
        _ => {
            // Multiple facts - try to simplify constants
            match try_simplify_constants(left, op, &target_value) {
                Some(simplified) => IsolationResult::MultipleUnknowns(simplified),
                None => IsolationResult::Symbolic,
            }
        }
    }
}

/// Isolate a single fact from an arithmetic expression
fn isolate_single_fact(
    expression: &Expression,
    target_fact: &FactPath,
    op: &ComparisonComputation,
    target_value: &LiteralValue,
) -> IsolationResult {
    match &expression.kind {
        ExpressionKind::FactPath(path) if path == target_fact => {
            // Already isolated
            IsolationResult::Isolated {
                fact: path.clone(),
                op: op.clone(),
                value: target_value.clone(),
            }
        }

        ExpressionKind::Arithmetic(left, arith_op, right) => {
            let left_has_fact = contains_fact(left, target_fact);
            let right_has_fact = contains_fact(right, target_fact);

            if left_has_fact && !right_has_fact {
                // Fact is on left: (fact_expr arith_op constant) cmp_op target
                isolate_from_left(left, arith_op, right, target_fact, op, target_value)
            } else if right_has_fact && !left_has_fact {
                // Fact is on right: (constant arith_op fact_expr) cmp_op target
                isolate_from_right(left, arith_op, right, target_fact, op, target_value)
            } else {
                // Both sides have the fact (non-linear) or neither (shouldn't happen)
                IsolationResult::Symbolic
            }
        }

        _ => IsolationResult::Symbolic,
    }
}

/// Isolate fact from: (fact_expr arith_op constant) cmp_op target
fn isolate_from_left(
    fact_expr: &Expression,
    arith_op: &ArithmeticComputation,
    constant_expr: &Expression,
    target_fact: &FactPath,
    cmp_op: &ComparisonComputation,
    target_value: &LiteralValue,
) -> IsolationResult {
    let constant = match &constant_expr.kind {
        ExpressionKind::Literal(LiteralValue::Number(n)) => *n,
        _ => return IsolationResult::Symbolic,
    };

    let target_num = match target_value {
        LiteralValue::Number(n) => *n,
        _ => return IsolationResult::Symbolic,
    };

    match arith_op {
        ArithmeticComputation::Add => {
            // (x + c) op t → x op (t - c)
            let new_target = target_num - constant;
            isolate_single_fact(
                fact_expr,
                target_fact,
                cmp_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Subtract => {
            // (x - c) op t → x op (t + c)
            let new_target = target_num + constant;
            isolate_single_fact(
                fact_expr,
                target_fact,
                cmp_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Multiply => {
            if constant == Decimal::ZERO {
                // x * 0 op t
                if cmp_op.is_equal() {
                    if target_num == Decimal::ZERO {
                        // x * 0 == 0 → true for any x
                        return IsolationResult::Unconstrained;
                    } else {
                        // x * 0 == non-zero → false
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("x * 0 cannot equal {}", target_num),
                        });
                    }
                } else {
                    // x * 0 > t, x * 0 < t, etc.
                    let zero = Decimal::ZERO;
                    let satisfiable = match cmp_op {
                        ComparisonComputation::GreaterThan => zero > target_num,
                        ComparisonComputation::GreaterThanOrEqual => zero >= target_num,
                        ComparisonComputation::LessThan => zero < target_num,
                        ComparisonComputation::LessThanOrEqual => zero <= target_num,
                        _ => return IsolationResult::Symbolic,
                    };
                    if satisfiable {
                        return IsolationResult::Unconstrained;
                    } else {
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("0 {} {} is always false", cmp_op.symbol(), target_num),
                        });
                    }
                }
            }

            // x * c op t → x op (t / c), flip if c < 0
            let new_target = target_num / constant;
            let new_op = if constant < Decimal::ZERO {
                flip_comparison(cmp_op)
            } else {
                cmp_op.clone()
            };
            isolate_single_fact(
                fact_expr,
                target_fact,
                &new_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Divide => {
            if constant == Decimal::ZERO {
                // x / 0 - division by zero, should have been caught earlier
                return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                    message: "Division by zero".to_string(),
                });
            }

            // x / c op t → x op (t * c), flip if c < 0
            let new_target = target_num * constant;
            let new_op = if constant < Decimal::ZERO {
                flip_comparison(cmp_op)
            } else {
                cmp_op.clone()
            };
            isolate_single_fact(
                fact_expr,
                target_fact,
                &new_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Modulo | ArithmeticComputation::Power => {
            // Not supported for simple isolation
            IsolationResult::Symbolic
        }
    }
}

/// Isolate fact from: (constant arith_op fact_expr) cmp_op target
fn isolate_from_right(
    constant_expr: &Expression,
    arith_op: &ArithmeticComputation,
    fact_expr: &Expression,
    target_fact: &FactPath,
    cmp_op: &ComparisonComputation,
    target_value: &LiteralValue,
) -> IsolationResult {
    let constant = match &constant_expr.kind {
        ExpressionKind::Literal(LiteralValue::Number(n)) => *n,
        _ => return IsolationResult::Symbolic,
    };

    let target_num = match target_value {
        LiteralValue::Number(n) => *n,
        _ => return IsolationResult::Symbolic,
    };

    match arith_op {
        ArithmeticComputation::Add => {
            // (c + x) op t → x op (t - c)
            let new_target = target_num - constant;
            isolate_single_fact(
                fact_expr,
                target_fact,
                cmp_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Subtract => {
            // (c - x) op t → -x op (t - c) → x op (c - t), flip comparison
            let new_target = constant - target_num;
            let new_op = flip_comparison(cmp_op);
            isolate_single_fact(
                fact_expr,
                target_fact,
                &new_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Multiply => {
            if constant == Decimal::ZERO {
                // 0 * x op t
                if cmp_op.is_equal() {
                    if target_num == Decimal::ZERO {
                        return IsolationResult::Unconstrained;
                    } else {
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("0 * x cannot equal {}", target_num),
                        });
                    }
                } else {
                    let zero = Decimal::ZERO;
                    let satisfiable = match cmp_op {
                        ComparisonComputation::GreaterThan => zero > target_num,
                        ComparisonComputation::GreaterThanOrEqual => zero >= target_num,
                        ComparisonComputation::LessThan => zero < target_num,
                        ComparisonComputation::LessThanOrEqual => zero <= target_num,
                        _ => return IsolationResult::Symbolic,
                    };
                    if satisfiable {
                        return IsolationResult::Unconstrained;
                    } else {
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("0 {} {} is always false", cmp_op.symbol(), target_num),
                        });
                    }
                }
            }

            // c * x op t → x op (t / c), flip if c < 0
            let new_target = target_num / constant;
            let new_op = if constant < Decimal::ZERO {
                flip_comparison(cmp_op)
            } else {
                cmp_op.clone()
            };
            isolate_single_fact(
                fact_expr,
                target_fact,
                &new_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Divide => {
            // c / x op t
            if target_num == Decimal::ZERO {
                if cmp_op.is_equal() {
                    if constant == Decimal::ZERO {
                        // 0 / x == 0 → true for any x != 0 (domain restriction)
                        // Return symbolic to preserve the domain restriction
                        return IsolationResult::Symbolic;
                    } else {
                        // c / x == 0 where c != 0 → impossible
                        return IsolationResult::Unsatisfiable(UnsatReason::ArithmeticContradiction {
                            message: format!("{} / x cannot equal 0", constant),
                        });
                    }
                }
            }

            // c / x op t → x op (c / t), flip comparison (dividing changes direction)
            // But we need t != 0
            if target_num == Decimal::ZERO {
                return IsolationResult::Symbolic;
            }

            let new_target = constant / target_num;
            // When dividing by positive target, direction depends on sign
            // c / x = t means x = c / t
            // For inequalities: c / x > t is complex, keep as symbolic for now
            if !cmp_op.is_equal() {
                return IsolationResult::Symbolic;
            }
            isolate_single_fact(
                fact_expr,
                target_fact,
                cmp_op,
                &LiteralValue::Number(new_target),
            )
        }

        ArithmeticComputation::Modulo | ArithmeticComputation::Power => {
            IsolationResult::Symbolic
        }
    }
}

/// Flip comparison direction (for negative multiplier/divisor or subtraction from constant)
fn flip_comparison(op: &ComparisonComputation) -> ComparisonComputation {
    match op {
        ComparisonComputation::Equal(notation) => ComparisonComputation::Equal(notation.clone()),
        ComparisonComputation::NotEqual(notation) => {
            ComparisonComputation::NotEqual(notation.clone())
        }
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThan,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThanOrEqual,
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThan,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThanOrEqual,
    }
}

/// Try to simplify constants in an expression with multiple unknowns
///
/// For `x + y + 10 == 100`, produce `x + y == 90`
fn try_simplify_constants(
    expression: &Expression,
    op: &ComparisonComputation,
    target_value: &LiteralValue,
) -> Option<Expression> {
    // Only handle equality for now
    if !op.is_equal() {
        return None;
    }

    let target_num = match target_value {
        LiteralValue::Number(n) => *n,
        _ => return None,
    };

    // Try to collect and combine constants from the expression
    let (facts_expr, constants_sum) = extract_constant_sum(expression)?;

    // New target: original target minus the constants
    let new_target = target_num - constants_sum;

    // Build simplified comparison: facts_expr == new_target
    Some(Expression::new(
            ExpressionKind::Comparison(
            Arc::new(facts_expr),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
            Arc::new(Expression::new(
                ExpressionKind::Literal(LiteralValue::Number(new_target)),
                None,
            )),
            ),
            None,
    ))
}

/// Extract the sum of constants from an additive expression
///
/// Returns (expression with constants removed, sum of constants)
/// Only handles simple additive chains for now.
fn extract_constant_sum(expression: &Expression) -> Option<(Expression, Decimal)> {
    match &expression.kind {
        ExpressionKind::Literal(LiteralValue::Number(_)) => {
            // Pure constant - shouldn't happen with multiple facts
            None
        }

        ExpressionKind::FactPath(_) => {
            // Single fact, no constants
            Some((expression.clone(), Decimal::ZERO))
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            match op {
                ArithmeticComputation::Add => {
                    // Check if right is a constant
                    if let ExpressionKind::Literal(LiteralValue::Number(n)) = &right.kind {
                        let (inner_expr, inner_sum) = extract_constant_sum(left)?;
                        return Some((inner_expr, inner_sum + *n));
                    }
                    // Check if left is a constant
                    if let ExpressionKind::Literal(LiteralValue::Number(n)) = &left.kind {
                        let (inner_expr, inner_sum) = extract_constant_sum(right)?;
                        return Some((inner_expr, inner_sum + *n));
                    }
                    // Both sides have facts - recurse on both
                    let (left_expr, left_sum) = extract_constant_sum(left)?;
                    let (right_expr, right_sum) = extract_constant_sum(right)?;
                    let combined = Expression::new(
                        ExpressionKind::Arithmetic(
                            Arc::new(left_expr),
                            ArithmeticComputation::Add,
                            Arc::new(right_expr),
            ),
            None,
        );
                    Some((combined, left_sum + right_sum))
                }

                ArithmeticComputation::Subtract => {
                    // x - c → (x, -c)
                    if let ExpressionKind::Literal(LiteralValue::Number(n)) = &right.kind {
                        let (inner_expr, inner_sum) = extract_constant_sum(left)?;
                        return Some((inner_expr, inner_sum - *n));
                    }
                    // c - x is more complex, skip for now
                    None
                }

                _ => None,
            }
        }

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::ConstraintSet;
    use crate::semantic::EqualityNotation;
    use rust_decimal::Decimal;

    fn literal_bool(value: bool) -> Expression {
        Expression::new(
            ExpressionKind::Literal(LiteralValue::Boolean(if value {
                BooleanValue::True
            } else {
                BooleanValue::False
            })),
            None,
        )
    }

    fn num(n: i64) -> LiteralValue {
        LiteralValue::Number(Decimal::from(n))
    }

    #[test]
    fn test_solve_trivial_true() {
        use crate::inversion::Target;

        // Proper equation structure: (true ∧ 1)
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(literal_bool(true)),
                Arc::new(num_expr(1)),
            ),
            None,
        );
        let target = Target::any_value();
        let results = solve_with_target(equation, &target);

        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], SolveResult::Solved { .. }));
    }

    #[test]
    fn test_solve_trivial_false() {
        use crate::inversion::Target;

        let equation = literal_bool(false);
        let target = Target::any_value();
        let results = solve_with_target(equation, &target);

        // All branches unsatisfiable returns single Unsatisfiable
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], SolveResult::Unsatisfiable { .. }));
    }

    #[test]
    fn test_solve_simple_comparison() {
        // Test solve_single_branch directly for isolated constraint solving
        let fact_path = FactPath::local("x".to_string());
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(ExpressionKind::FactPath(fact_path.clone()), None)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(num_expr(42)),
            ),
            None,
        );
        
        let outcome = OperationResult::Value(LiteralValue::Number(Decimal::from(42)));
        let result = solve_single_branch(condition, outcome);

        match result {
            SolveResult::Solved { outcome: _, fact_constraints } | SolveResult::Partial { outcome: _, fact_constraints, .. } => {
                assert!(fact_constraints.contains_key(&fact_path));
            }
            _ => panic!("Expected solved or partial result"),
        }
    }

    #[test]
    fn test_constraint_set_bounds() {
        let mut constraint_set = ConstraintSet::new();
        let fact = FactPath::local("x".to_string());

        constraint_set.add_comparison(
            fact.clone(),
            &ComparisonComputation::GreaterThanOrEqual,
            num(10),
        );

        constraint_set.add_comparison(fact.clone(), &ComparisonComputation::LessThan, num(100));

        assert!(constraint_set.contradiction.is_none());

        let bounds = constraint_set.facts.get(&fact).unwrap();
        assert!(matches!(bounds.min, Some((LiteralValue::Number(_), true))));
        assert!(matches!(bounds.max, Some((LiteralValue::Number(_), false))));
    }

    #[test]
    fn test_constraint_set_contradiction() {
        let mut constraint_set = ConstraintSet::new();
        let fact = FactPath::local("x".to_string());

        constraint_set.add_comparison(
            fact.clone(),
            &ComparisonComputation::GreaterThanOrEqual,
            num(100),
        );

        constraint_set.add_comparison(fact.clone(), &ComparisonComputation::LessThan, num(50));

        assert!(constraint_set.contradiction.is_some());
    }

    #[test]
    fn test_constraint_set_exact_value_contradiction() {
        let mut constraint_set = ConstraintSet::new();
        let fact = FactPath::local("x".to_string());

        constraint_set.add_comparison(
            fact.clone(),
            &ComparisonComputation::Equal(EqualityNotation::Symbol),
            num(10),
        );

        constraint_set.add_comparison(
            fact.clone(),
            &ComparisonComputation::Equal(EqualityNotation::Symbol),
            num(20),
        );

        assert!(matches!(
            constraint_set.contradiction,
            Some(UnsatReason::EnumContradiction { .. })
        ));
    }

    // ========================================================================
    // OR Handling Tests
    // ========================================================================

    /// `false ∨ (x > 10)` → Single solution: `x > 10`
    #[test]
    fn test_false_branch_filtered() {
        let fact_x = FactPath::local("x".to_string());

        // Wrap branches in proper (condition ∧ result) format
        // Branch 1: (false ∧ 1)
        // Branch 2: ((x > 10) ∧ 1)
        let branch1 = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(literal_bool(false)),
                Arc::new(num_expr(1)),
            ),
            None,
        );
        
        let branch2 = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(Expression::new(
                    ExpressionKind::Comparison(
                        Arc::new(Expression::new(
                            ExpressionKind::FactPath(fact_x.clone()),
                            None,
                        )),
                        ComparisonComputation::GreaterThan,
                        Arc::new(num_expr(10)),
                    ),
                    None,
                )),
                Arc::new(num_expr(1)),
            ),
            None,
        );
        
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(branch1), Arc::new(branch2)),
            None,
        );

        let target = Target::any_value();
        let results = solve_with_target(equation, &target);

        // Should have exactly 1 solution (false branch filtered out)
        assert_eq!(results.len(), 1, "false branch should be filtered, expected 1 solution");
        assert!(
            matches!(&results[0], SolveResult::Solved { .. } | SolveResult::Partial { .. }),
            "should have a valid solution"
        );
    }

    /// `false ∨ false` → Unsatisfiable
    #[test]
    fn test_all_false_unsatisfiable() {
        use crate::inversion::Target;

        // false ∨ false
        let equation = Expression::new(
            ExpressionKind::LogicalOr(
                Arc::new(literal_bool(false)),
                Arc::new(literal_bool(false)),
            ),
            None,
        );

        let target = Target::any_value();
        let results = solve_with_target(equation, &target);

        // Should return single Unsatisfiable
        assert_eq!(results.len(), 1, "all-false should return single result");
        assert!(
            matches!(&results[0], SolveResult::Unsatisfiable { .. }),
            "should be Unsatisfiable"
        );
    }

    /// `true ∨ (x > 10)` → Single solution: unconstrained (true absorbs)
    #[test]
    fn test_true_absorbs() {
        use crate::inversion::Target;

        let fact_x = FactPath::local("x".to_string());

        // (true ∧ 1) ∨ ((x > 10) ∧ 1)
        let branch1 = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(literal_bool(true)),
                Arc::new(num_expr(1)),
            ),
            None,
        );
        
        let branch2 = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(Expression::new(
            ExpressionKind::Comparison(
                        Arc::new(Expression::new(
                            ExpressionKind::FactPath(fact_x.clone()),
                            None,
                        )),
                        ComparisonComputation::GreaterThan,
                        Arc::new(num_expr(10)),
                    ),
                    None,
                )),
                Arc::new(num_expr(1)),
            ),
            None,
        );
        
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(branch1), Arc::new(branch2)),
            None,
        );

        let target = Target::any_value();
        let results = solve_with_target(equation, &target);

        // After reduction, `true ∨ X` becomes `true`, so we get one unconstrained solution
        // The reduce() function should simplify this
        assert!(
            results.len() >= 1,
            "should have at least one solution"
        );

        // At least one solution should be unconstrained (no fact constraints)
        let has_unconstrained = results.iter().any(|r| {
            matches!(r, SolveResult::Solved { outcome: _, fact_constraints } if fact_constraints.is_empty())
        });
        assert!(
            has_unconstrained,
            "true branch should produce unconstrained solution"
        );
    }

    /// Nested OR `(a ∨ b) ∨ c` → Three branches flattened
    ///
    /// Uses proper equation structure: (condition ∧ result) branches
    /// Each branch has condition (a==1, b==2, c==3) and result (true literal)
    #[test]
    fn test_nested_or_flattened() {
        use crate::inversion::Target;

        let fact_a = FactPath::local("a".to_string());
        let fact_b = FactPath::local("b".to_string());
        let fact_c = FactPath::local("c".to_string());

        // Branch 1: (a == 1) ∧ true
        let a_eq_1 = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(ExpressionKind::FactPath(fact_a.clone()), None)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(Expression::new(ExpressionKind::Literal(num(1)), None)),
            ),
            None,
        );
        let branch_a = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(a_eq_1),
                Arc::new(literal_bool(true)),
            ),
            None,
        );

        // Branch 2: (b == 2) ∧ true
        let b_eq_2 = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(ExpressionKind::FactPath(fact_b.clone()), None)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(Expression::new(ExpressionKind::Literal(num(2)), None)),
            ),
            None,
        );
        let branch_b = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(b_eq_2),
                Arc::new(literal_bool(true)),
            ),
            None,
        );

        // Branch 3: (c == 3) ∧ true
        let c_eq_3 = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(ExpressionKind::FactPath(fact_c.clone()), None)),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(Expression::new(ExpressionKind::Literal(num(3)), None)),
            ),
            None,
        );
        let branch_c = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(c_eq_3),
                Arc::new(literal_bool(true)),
            ),
            None,
        );

        // Nested: ((branch_a) ∨ (branch_b)) ∨ (branch_c)
        let a_or_b = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(branch_a), Arc::new(branch_b)),
            None,
        );
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(a_or_b), Arc::new(branch_c)),
            None,
        );

        // Target: result == true
        let target = Target::value(LiteralValue::Boolean(BooleanValue::True));
        let results = solve_with_target(equation, &target);

        // Should have 3 solutions (one for each branch)
        assert_eq!(
            results.len(),
            3,
            "nested OR should produce 3 solutions, got {}",
            results.len()
        );

        // Each solution should constrain exactly one fact
        for (i, result) in results.iter().enumerate() {
        match result {
                SolveResult::Solved { outcome: _, fact_constraints } | SolveResult::Partial { outcome: _, fact_constraints, .. } => {
                    assert!(
                        !fact_constraints.is_empty(),
                        "solution {} should have constraints",
                        i
                    );
                }
                SolveResult::Unsatisfiable { .. } => {
                    panic!("solution {} should not be unsatisfiable", i);
                }
            }
        }
    }

    /// OR inside AND: `x > 0 ∧ (y = 1 ∨ y = 2)` → Two solutions
    ///
    /// Uses proper equation structure with result value.
    /// The condition contains AND-over-OR which must be distributed.
    /// Test flatten_or produces correct number of branches
    #[test]
    fn test_flatten_or_basic() {
        let a = literal_bool(true);
        let b = literal_bool(false);
        let c = Expression::new(
            ExpressionKind::FactPath(FactPath::local("x".to_string())),
            None,
        );

        // Single expression
        let branches = flatten_or(a.clone());
        assert_eq!(branches.len(), 1);

        // Simple OR
        let or_expr = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(a.clone()), Arc::new(b.clone())),
            None,
        );
        let branches = flatten_or(or_expr);
        assert_eq!(branches.len(), 2);

        // Nested OR: (a ∨ b) ∨ c
        let nested = Expression::new(
            ExpressionKind::LogicalOr(
                Arc::new(Expression::new(
                    ExpressionKind::LogicalOr(Arc::new(a.clone()), Arc::new(b.clone())),
                    None,
                )),
                Arc::new(c),
            ),
            None,
        );
        let branches = flatten_or(nested);
        assert_eq!(branches.len(), 3);
    }

    // ========================================================================
    // Algebraic Solving Tests
    // ========================================================================

    fn fact_expr(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::FactPath(FactPath::local(name.to_string())),
            None,
        )
    }

    fn num_expr(n: i64) -> Expression {
        Expression::new(ExpressionKind::Literal(num(n)), None)
    }

    fn arith(left: Expression, op: ArithmeticComputation, right: Expression) -> Expression {
        Expression::new(
            ExpressionKind::Arithmetic(Arc::new(left), op, Arc::new(right)),
            None,
        )
    }

    /// `x + 5 == 10` → `x == 5`
    #[test]
    fn test_isolate_addition() {
        let fact_x = FactPath::local("x".to_string());

        // Test solve_single_branch directly with: (x + 5) == 10
        let x_plus_5 = arith(fact_expr("x"), ArithmeticComputation::Add, num_expr(5));
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(x_plus_5),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(num_expr(10)),
            ),
            None,
        );

        let outcome = OperationResult::Value(num(10));
        let result = solve_single_branch(condition, outcome);

        match result {
            SolveResult::Solved { outcome: _, fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(5), "x should equal 5");
                    }
                    _ => panic!("expected enumeration constraint, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", result),
        }
    }

    /// `x - 3 == 7` → `x == 10`
    #[test]
    fn test_isolate_subtraction() {
        let fact_x = FactPath::local("x".to_string());

        let x_minus_3 = arith(fact_expr("x"), ArithmeticComputation::Subtract, num_expr(3));
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(x_minus_3),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(num_expr(7)),
            ),
            None,
        );

        let outcome = OperationResult::Value(num(7));
        let result = solve_single_branch(condition, outcome);

        match result {
            SolveResult::Solved { outcome: _, fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(10), "x should equal 10");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", result),
        }
    }

    /// `x * 3 == 15` → `x == 5`
    #[test]
    fn test_isolate_multiplication() {
        let fact_x = FactPath::local("x".to_string());

        let x_times_3 = arith(fact_expr("x"), ArithmeticComputation::Multiply, num_expr(3));
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(x_times_3),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(num_expr(15)),
            ),
            None,
        );

        let outcome = OperationResult::Value(num(15));
        let result = solve_single_branch(condition, outcome);

        match result {
            SolveResult::Solved { outcome: _, fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(5), "x should equal 5");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", result),
        }
    }

    /// `x / 2 == 10` → `x == 20`
    #[test]
    fn test_isolate_division() {
        let fact_x = FactPath::local("x".to_string());

        let x_div_2 = arith(fact_expr("x"), ArithmeticComputation::Divide, num_expr(2));
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(x_div_2),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(num_expr(10)),
            ),
            None,
        );

        let outcome = OperationResult::Value(num(10));
        let result = solve_single_branch(condition, outcome);

        match result {
            SolveResult::Solved { outcome: _, fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(20), "x should equal 20");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", result),
        }
    }

    /// `10 - x == 3` → `x == 7`
    #[test]
    fn test_isolate_subtraction_from_constant() {
        let fact_x = FactPath::local("x".to_string());

        let ten_minus_x = arith(num_expr(10), ArithmeticComputation::Subtract, fact_expr("x"));
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(ten_minus_x),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(num_expr(3)),
            ),
            None,
        );

        let outcome = OperationResult::Value(num(3));
        let result = solve_single_branch(condition, outcome);

        match result {
            SolveResult::Solved { outcome: _, fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(7), "x should equal 7");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", result),
        }
    }

    /// `x * 0 == 5` → unsatisfiable
    #[test]
    fn test_multiply_by_zero_nonzero_target() {
        use crate::inversion::Target;

        let x_times_0 = arith(fact_expr("x"), ArithmeticComputation::Multiply, num_expr(0));
        let equation = Expression::new(
            ExpressionKind::LogicalAnd(Arc::new(literal_bool(true)), Arc::new(x_times_0)),
            None,
        );

        let target = Target::value(num(5));
        let results = solve_with_target(equation, &target);

        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], SolveResult::Unsatisfiable { .. }),
            "x * 0 == 5 should be unsatisfiable, got {:?}",
            results[0]
        );
    }

    /// `x * 0 == 0` → unconstrained
    #[test]
    fn test_multiply_by_zero_zero_target() {
        let x_times_0 = arith(fact_expr("x"), ArithmeticComputation::Multiply, num_expr(0));
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(x_times_0),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(num_expr(0)),
            ),
            None,
        );

        let outcome = OperationResult::Value(num(0));
        let result = solve_single_branch(condition, outcome);

        match result {
            SolveResult::Solved { outcome: _, fact_constraints } => {
                // Unconstrained means no constraints on x
                assert!(
                    fact_constraints.is_empty(),
                    "x * 0 == 0 should be unconstrained, got constraints: {:?}",
                    fact_constraints
                );
            }
            _ => panic!("expected Solved (unconstrained), got {:?}", result),
        }
    }

    /// `(x + 5) * 2 == 30` → `x == 10`
    #[test]
    fn test_nested_arithmetic() {
        let fact_x = FactPath::local("x".to_string());

        let x_plus_5 = arith(fact_expr("x"), ArithmeticComputation::Add, num_expr(5));
        let nested = arith(x_plus_5, ArithmeticComputation::Multiply, num_expr(2));
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(nested),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(num_expr(30)),
            ),
            None,
        );

        let outcome = OperationResult::Value(num(30));
        let result = solve_single_branch(condition, outcome);

        match result {
            SolveResult::Solved { outcome: _, fact_constraints } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Enumeration(values) => {
                        assert_eq!(values.len(), 1);
                        assert_eq!(values[0], num(10), "x should equal 10");
                    }
                    _ => panic!("expected enumeration, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved, got {:?}", result),
        }
    }

    /// `x * (-2) > 10` → `x < -5` (inequality flipping)
    #[test]
    fn test_inequality_with_negative_multiplier() {
        use crate::computation::Bound;

        let fact_x = FactPath::local("x".to_string());

        let x_times_neg2 = arith(fact_expr("x"), ArithmeticComputation::Multiply, num_expr(-2));
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(x_times_neg2),
                ComparisonComputation::GreaterThan,
                Arc::new(num_expr(10)),
            ),
            None,
        );

        let outcome = OperationResult::Value(num(10));
        let result = solve_single_branch(condition, outcome);

        match result {
            SolveResult::Solved { outcome: _, fact_constraints } | SolveResult::Partial { outcome: _, fact_constraints, .. } => {
                let constraint = fact_constraints.get(&fact_x).expect("should have x constraint");
                match constraint {
                    FactConstraint::Range { max, .. } => {
                        // x < -5 means max is Exclusive(-5)
                        match max {
                            Bound::Exclusive(val) => {
                                assert_eq!(*val, num(-5), "max should be -5");
                            }
                            _ => panic!("expected exclusive bound, got {:?}", max),
                        }
                    }
                    _ => panic!("expected range constraint, got {:?}", constraint),
                }
            }
            _ => panic!("expected Solved or Partial, got {:?}", result),
        }
    }

    /// Test multiple unknowns: `x + y + 10 == 100` → simplified constraint `x + y == 90`
    #[test]
    fn test_multiple_unknowns_constant_simplification() {
        let x_plus_y = arith(fact_expr("x"), ArithmeticComputation::Add, fact_expr("y"));
        let x_plus_y_plus_10 = arith(x_plus_y, ArithmeticComputation::Add, num_expr(10));
        let condition = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(x_plus_y_plus_10),
                ComparisonComputation::Equal(EqualityNotation::Symbol),
                Arc::new(num_expr(100)),
            ),
            None,
        );

        let outcome = OperationResult::Value(num(100));
        let result = solve_single_branch(condition, outcome);

        // Should have a partial result with simplified constraint
        match result {
            SolveResult::Partial {
                remaining_constraints,
                ..
            } => {
                // Should have a simplified constraint x + y == 90
                assert!(
                    !remaining_constraints.is_empty(),
                    "should have remaining constraint"
                );
                // The constraint should be a comparison with 90 on the right
                let constraint = &remaining_constraints[0];
                if let ExpressionKind::Comparison(_, _, right) = &constraint.kind {
                    if let ExpressionKind::Literal(LiteralValue::Number(n)) = &right.kind {
                        assert_eq!(*n, Decimal::from(90), "should have simplified to x + y == 90");
                    } else {
                        panic!("right side should be literal 90");
                    }
                } else {
                    panic!("should be a comparison expression");
                }
            }
            SolveResult::Solved { .. } => {
                // If it's fully solved, that's also acceptable
                // (means both facts are unconstrained which shouldn't happen here)
                panic!("should be Partial with simplified constraint, not Solved");
            }
            _ => panic!("expected Partial, got {:?}", result),
        }
    }

    /// Test collect_facts
    #[test]
    fn test_collect_facts() {
        let expr = arith(
            arith(fact_expr("x"), ArithmeticComputation::Add, num_expr(5)),
            ArithmeticComputation::Multiply,
            fact_expr("y"),
        );

        let facts = collect_facts(&expr);
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&FactPath::local("x".to_string())));
        assert!(facts.contains(&FactPath::local("y".to_string())));
    }

    /// Test contains_fact
    #[test]
    fn test_contains_fact() {
        let fact_x = FactPath::local("x".to_string());
        let fact_y = FactPath::local("y".to_string());
        let fact_z = FactPath::local("z".to_string());

        let expr = arith(fact_expr("x"), ArithmeticComputation::Add, num_expr(5));

        assert!(contains_fact(&expr, &fact_x));
        assert!(!contains_fact(&expr, &fact_y));
        assert!(!contains_fact(&expr, &fact_z));
    }

    // ========================================================================
    // Veto Target Integration Tests
    // ========================================================================

    /// Test that veto branches are eliminated when solving for a value target.
    /// Equation: (y >= 0 ∧ veto) ∨ (y < 0 ∧ 25)
    /// Target: x == 25
    /// Expected: veto branch eliminated, only y < 0 branch remains
    #[test]
    fn test_veto_branch_eliminated_for_value_target() {
        use crate::inversion::Target;
        use crate::semantic::VetoExpression;

        let fact_y = FactPath::local("y".to_string());

        // Branch 1: y >= 0 ∧ veto
        let y_gte_0 = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::GreaterThanOrEqual,
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let veto_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(y_gte_0),
                Arc::new(Expression::new(
                    ExpressionKind::Veto(VetoExpression { message: None }),
                    None,
                )),
            ),
            None,
        );

        // Branch 2: y < 0 ∧ 25
        let y_lt_0 = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::LessThan,
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let value_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(y_lt_0),
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::from(25))),
                    None,
                )),
            ),
            None,
        );

        // Equation: branch1 ∨ branch2
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(veto_branch), Arc::new(value_branch)),
            None,
        );

        let target = Target::value(LiteralValue::Number(Decimal::from(25)));
        let results = solve_with_target(equation, &target);

        // Veto branch should be eliminated, only value branch remains
        assert_eq!(
            results.len(),
            1,
            "should have exactly one solution (veto branch eliminated)"
        );
        assert!(
            !matches!(results[0], SolveResult::Unsatisfiable { .. }),
            "should find a valid solution"
        );
    }

    /// Test that any_veto target returns only veto branches.
    /// Equation: (y >= 0 ∧ veto "A") ∨ (y < 0 ∧ 25)
    /// Target: any_veto
    /// Expected: only y >= 0 branch remains (the veto branch)
    #[test]
    fn test_any_veto_target_returns_veto_branches() {
        use crate::inversion::Target;
        use crate::semantic::VetoExpression;

        let fact_y = FactPath::local("y".to_string());

        // Branch 1: y >= 0 ∧ veto "A"
        let y_gte_0 = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::GreaterThanOrEqual,
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let veto_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(y_gte_0),
                Arc::new(Expression::new(
                    ExpressionKind::Veto(VetoExpression {
                        message: Some("A".to_string()),
                    }),
                    None,
                )),
            ),
            None,
        );

        // Branch 2: y < 0 ∧ 25
        let y_lt_0 = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::LessThan,
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let value_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(y_lt_0),
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::from(25))),
                    None,
                )),
            ),
            None,
        );

        // Equation: branch1 ∨ branch2
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(veto_branch), Arc::new(value_branch)),
            None,
        );

        let target = Target::any_veto();
        let results = solve_with_target(equation, &target);

        // Value branch should be eliminated, only veto branch remains
        assert_eq!(
            results.len(),
            1,
            "should have exactly one solution (value branch eliminated)"
        );
        assert!(
            !matches!(results[0], SolveResult::Unsatisfiable { .. }),
            "should find a valid solution"
        );
    }

    /// Test that specific veto target matches only vetos with the correct message.
    /// Equation: (y < 0 ∧ veto "A") ∨ (y >= 0 ∧ veto "B")
    /// Target: veto("A")
    /// Expected: only y < 0 branch remains (matching message)
    #[test]
    fn test_specific_veto_target_matches_message() {
        use crate::inversion::Target;
        use crate::semantic::VetoExpression;

        let fact_y = FactPath::local("y".to_string());

        // Branch 1: y < 0 ∧ veto "A"
        let y_lt_0 = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::LessThan,
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let veto_a_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(y_lt_0),
                Arc::new(Expression::new(
                    ExpressionKind::Veto(VetoExpression {
                        message: Some("A".to_string()),
                    }),
                    None,
                )),
            ),
            None,
        );

        // Branch 2: y >= 0 ∧ veto "B"
        let y_gte_0 = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(fact_y.clone()),
                    None,
                )),
                ComparisonComputation::GreaterThanOrEqual,
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::ZERO)),
                    None,
                )),
            ),
            None,
        );
        let veto_b_branch = Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(y_gte_0),
                Arc::new(Expression::new(
                    ExpressionKind::Veto(VetoExpression {
                        message: Some("B".to_string()),
                    }),
                    None,
                )),
            ),
            None,
        );

        // Equation: branch1 ∨ branch2
        let equation = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(veto_a_branch), Arc::new(veto_b_branch)),
            None,
        );

        let target = Target::veto("A".to_string());
        let results = solve_with_target(equation, &target);

        // Veto "B" branch should be eliminated, only veto "A" branch remains
        assert_eq!(
            results.len(),
            1,
            "should have exactly one solution (non-matching veto branch eliminated)"
        );
        assert!(
            !matches!(results[0], SolveResult::Unsatisfiable { .. }),
            "should find a valid solution"
        );
    }
}
