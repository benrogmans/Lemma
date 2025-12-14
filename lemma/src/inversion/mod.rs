//! Constraint-based inverse reasoning for Lemma rules
//!
//! Determines what inputs produce desired outputs by building and solving constraint equations.
//!
//! The main entry point is [`invert()`], which returns an [`InversionResponse`]
//! containing all valid solutions with their domains.
//!
//! ## Algorithm
//!
//! 1. **Build equations**: During planning, equations are built and simplified.
//!
//! 2. **Apply target**: Transform equation with target constraint.
//!
//! 3. **Solve**: Constraint solving with function range checking, domain restrictions,
//!    and exact symbolic preservation.

mod response;
mod world;
mod world_builder;

pub use crate::algebra::constraints::{Bound, DomainRestriction, FactConstraint, UnsatReason};
pub use response::{InversionResponse, Solution};
pub use world::World;
pub use world_builder::WorldBuilder;

use crate::algebra::isolation::{collect_facts, invert_expression};
use crate::computation::OperationResult;
use crate::evaluation::Evaluator;
use crate::planning::ExecutionPlan;
use crate::semantic::{ComparisonComputation, Expression, ExpressionKind, FactPath, LiteralValue};
use crate::{LemmaError, LemmaResult};
use std::collections::HashMap;

/// Target specification for an inversion query
///
/// Use the `invert()` function's string operator for simpler API,
/// or construct a Target directly for Engine methods.
#[derive(Debug, Clone)]
pub struct Target {
    /// Comparison operator
    pub op: TargetOp,

    /// Desired outcome (value or veto), or None for any_value
    pub outcome: Option<OperationResult>,
}

impl Target {
    /// Create a target for equality with a specific value
    pub fn value(value: crate::LiteralValue) -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Value(value)),
        }
    }

    /// Create a target for any value (all possible outcomes)
    pub fn any_value() -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: None,
        }
    }

    /// Create a target that matches any veto outcome (regardless of message)
    pub fn any_veto() -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Veto(None)),
        }
    }

    /// Create a target that matches a veto with a specific message
    pub fn veto(message: String) -> Self {
        Self {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Veto(Some(message))),
        }
    }

    /// Create a target with a custom operator
    pub fn with_op(op: TargetOp, outcome: OperationResult) -> Self {
        Self {
            op,
            outcome: Some(outcome),
        }
    }
}

/// Comparison operators for targets
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOp {
    /// Equal to (=)
    Eq,
    /// Not equal to (!=)
    Neq,
    /// Less than (<)
    Lt,
    /// Less than or equal to (<=)
    Lte,
    /// Greater than (>)
    Gt,
    /// Greater than or equal to (>=)
    Gte,
}

impl TargetOp {
    /// Parse operator from string
    fn from_str(op: &str) -> Result<Self, LemmaError> {
        match op {
            "=" | "==" => Ok(TargetOp::Eq),
            "!=" | "<>" => Ok(TargetOp::Neq),
            "<" => Ok(TargetOp::Lt),
            "<=" => Ok(TargetOp::Lte),
            ">" => Ok(TargetOp::Gt),
            ">=" => Ok(TargetOp::Gte),
            _ => Err(LemmaError::Engine(format!(
                "Invalid comparison operator: {}. Expected one of: =, !=, <, <=, >, >=",
                op
            ))),
        }
    }
}

impl Target {
    /// Create a target from operator string and outcome
    pub fn from_str(operator: &str, outcome: Option<OperationResult>) -> LemmaResult<Self> {
        let op = TargetOp::from_str(operator)?;
        Ok(Target { op, outcome })
    }
}

/// Invert a rule to find input fact values that produce a desired outcome.
///
/// Given an execution plan and rule name, determines what values the unknown
/// facts must have to produce the target outcome.
///
/// # Arguments
///
/// * `plan` - The execution plan containing the rule
/// * `rule_name` - Name of the rule to invert (local rule in main document)
/// * `operator` - Comparison operator: "=", "!=", "<", "<=", ">", ">="
/// * `outcome` - Desired result, or None for any_value (returns all possible outcomes)
///
/// # Returns
///
/// An `InversionResponse` containing all valid solutions, each with:
/// - The outcome it produces
/// - Fact constraints that produce that outcome
///
/// # Examples
///
/// ```ignore
/// // Find inputs where rate == 15%
/// let response = invert(&plan, "rate", "=", Some(OperationResult::Value(fifteen_percent)))?;
///
/// // Find all possible outcomes
/// let response = invert(&plan, "rate", "=", None)?;
///
/// // Find inputs where discount > 100
/// let response = invert(&plan, "discount", ">", Some(OperationResult::Value(hundred)))?;
///
/// // Find inputs that cause veto
/// let response = invert(&plan, "can_ship", "=", Some(OperationResult::Veto(None)))?;
/// ```
pub fn invert(
    plan: &ExecutionPlan,
    rule_name: &str,
    operator: &str,
    outcome: Option<OperationResult>,
) -> LemmaResult<InversionResponse> {
    let target = Target::from_str(operator, outcome)?;
    
    // 1. Apply symbolic evaluation to reduce the plan (substitute known facts, prune branches)
    let evaluator = Evaluator;
    let reduced_plan = evaluator.evaluate_symbolic(plan);
    
    // 2. Optimize the reduced plan (DNF expansion and simplification)
    let optimized_plan = reduced_plan.optimize();
    
    // 3. Build worlds on-demand from the optimized plan
    let mut builder = WorldBuilder::new(&optimized_plan);
    let rule_worlds = builder.build_worlds(rule_name)?;
    
    // 4. Filter worlds matching target
    let matching_worlds: Vec<&World> = rule_worlds.iter()
        .filter(|w| matches_target(&w.value, &target))
        .collect();
    
    // 5. For each world, solve algebraically
    let solutions: Vec<Solution> = matching_worlds.iter()
        .flat_map(|world| solve_world(world, &target))
        .collect();
    
    Ok(InversionResponse { solutions })
}

/// Check if a world's value expression matches the target
fn matches_target(expr: &Expression, target: &Target) -> bool {
    match &expr.kind {
        ExpressionKind::Literal(lit) => {
            match &target.outcome {
                Some(OperationResult::Value(target_val)) => {
                    matches_comparison(lit, &target.op, target_val)
                }
                Some(OperationResult::Veto(_)) => false,
                None => true,
            }
        }
        ExpressionKind::Veto(veto) => {
            match &target.outcome {
                Some(OperationResult::Veto(target_veto)) => {
                    match (target_veto, &veto.message) {
                        (None, _) => true,
                        (Some(target_msg), Some(veto_msg)) => target_msg == veto_msg,
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        _ => {
            // For non-literal expressions, we need to solve algebraically
            // Return true here - solve_world will handle the actual solving
            true
        }
    }
}

/// Compare a literal value with a target value using the given operator
fn matches_comparison(value: &LiteralValue, op: &TargetOp, target: &LiteralValue) -> bool {
    use crate::computation::comparison::comparison_operation;
    use crate::semantic::EqualityNotation;
    
    let cmp_op = match op {
        TargetOp::Eq => ComparisonComputation::Equal(EqualityNotation::Symbol),
        TargetOp::Neq => ComparisonComputation::NotEqual(EqualityNotation::Symbol),
        TargetOp::Lt => ComparisonComputation::LessThan,
        TargetOp::Lte => ComparisonComputation::LessThanOrEqual,
        TargetOp::Gt => ComparisonComputation::GreaterThan,
        TargetOp::Gte => ComparisonComputation::GreaterThanOrEqual,
    };
    
    match comparison_operation(value, &cmp_op, target) {
        OperationResult::Value(LiteralValue::Boolean(crate::semantic::BooleanValue::True)) => true,
        _ => false,
    }
}

/// Solve a single world
fn solve_world(world: &World, target: &Target) -> Vec<Solution> {
    // World.value is now an Expression (not Value enum)
    match &world.value.kind {
        // Case 1: Literal value
        ExpressionKind::Literal(lit) => {
            // Already matches (pre-filtered)
            vec![Solution::new(
                OperationResult::Value(lit.clone()),
                world.constraints.clone()
            )]
        }
        
        // Case 2: Veto
        ExpressionKind::Veto(veto) => {
            vec![Solution::new(
                OperationResult::Veto(veto.message.clone()),
                world.constraints.clone()
            )]
        }
        
        // Case 3: Expression containing unknown facts - needs algebraic inversion
        _ => {
            // Extract target value
            let target_value = match &target.outcome {
                Some(OperationResult::Value(val)) => val.clone(),
                _ => return vec![],
            };
            
            // Find which fact needs to be solved for
            let unknown_fact = match find_unknown_fact(&world.value, &world.constraints) {
                Ok(fact) => fact,
                Err(_) => return vec![],
            };
            
            // Use algebraic inversion (handles linear + non-linear, multiple solutions)
            let inversion_result = invert_expression(&world.value, &unknown_fact, &target_value);
            
            let mut solutions = Vec::new();
            for inv_solution in inversion_result.solutions {
                // Verify solution satisfies world constraints
                if world.constraints.get(&unknown_fact).map_or(true, |c| c.contains(&inv_solution.value)) {
                    let mut solution_constraints = world.constraints.clone();
                    
                    // Merge solution's constraints
                    for (fact, constraint) in inv_solution.constraints {
                        solution_constraints.insert(fact, constraint);
                    }
                    
                    // Add exact constraint for the solved fact
                    solution_constraints.insert(
                        unknown_fact.clone(),
                        FactConstraint::exact(inv_solution.value.clone())
                    );
                    
                    solutions.push(Solution::new(
                        OperationResult::Value(target_value.clone()),
                        solution_constraints
                    ));
                }
                // else: solution outside valid range, skip
            }
            
            // Return all valid solutions (empty if none work or inversion failed)
            solutions
        }
    }
}

/// Find an unknown fact in an expression that needs to be solved for
fn find_unknown_fact(expr: &Expression, constraints: &HashMap<FactPath, FactConstraint>) -> Result<FactPath, LemmaError> {
    // Find facts in expression that don't have exact values in constraints
    let facts_in_expr = collect_facts(expr);
    
    for fact in facts_in_expr {
        if let Some(constraint) = constraints.get(&fact) {
            if !constraint.is_exact() {
                return Ok(fact);
            }
        } else {
            return Ok(fact);
        }
    }
    
    Err(LemmaError::Engine("No unknown fact found in expression".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{Branch, ExecutableRule};
    use crate::semantic::{BooleanValue, Expression, ExpressionKind, LiteralValue, RulePath};
    use rust_decimal::Decimal;
    use std::collections::{HashMap, HashSet};

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

    fn literal_num(n: i64) -> Expression {
        Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(Decimal::from(n))),
            None,
        )
    }

    fn rule_path(name: &str) -> RulePath {
        RulePath::local(name.to_string())
    }

    #[test]
    fn test_target_op_from_str() {
        assert_eq!(TargetOp::from_str("=").unwrap(), TargetOp::Eq);
        assert_eq!(TargetOp::from_str("==").unwrap(), TargetOp::Eq);
        assert_eq!(TargetOp::from_str("!=").unwrap(), TargetOp::Neq);
        assert_eq!(TargetOp::from_str("<").unwrap(), TargetOp::Lt);
        assert_eq!(TargetOp::from_str("<=").unwrap(), TargetOp::Lte);
        assert_eq!(TargetOp::from_str(">").unwrap(), TargetOp::Gt);
        assert_eq!(TargetOp::from_str(">=").unwrap(), TargetOp::Gte);
        assert!(TargetOp::from_str("invalid").is_err());
    }

    #[test]
    fn test_invert_simple_rule() {
        // Create a simple rule: result = 42 (always)
        let rule = ExecutableRule {
            path: rule_path("test"),
            name: "test".to_string(),
            branches: vec![Branch {
                condition: literal_bool(true),
                optimized_condition: None,
                result: literal_num(42),
                source: None,
            }],
            needs_facts: HashSet::new(),
            source: None,
        };

        let plan = ExecutionPlan::new(
            "test_doc".to_string(),
            HashMap::new(),
            vec![rule],
            crate::planning::graph::Graph::empty(),
        );

        // Invert: find inputs where test == 42
        let response = invert(
            &plan,
            "test",
            "=",
            Some(OperationResult::Value(LiteralValue::Number(Decimal::from(
                42,
            )))),
        )
        .unwrap();

        // Should have one solution with no constraints (any input works)
        assert!(response.has_solutions());
    }

    #[test]
    fn test_invert_rule_not_found() {
        let plan = ExecutionPlan::new(
            "test_doc".to_string(),
            HashMap::new(),
            vec![],
            crate::planning::graph::Graph::empty(),
        );

        let result = invert(
            &plan,
            "nonexistent",
            "=",
            Some(OperationResult::Value(LiteralValue::Number(Decimal::from(
                42,
            )))),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_invert_invalid_operator() {
        let rule = ExecutableRule {
            path: rule_path("test"),
            name: "test".to_string(),
            branches: vec![Branch {
                condition: literal_bool(true),
                optimized_condition: None,
                result: literal_num(42),
                source: None,
            }],
            needs_facts: HashSet::new(),
            source: None,
        };

        let plan = ExecutionPlan::new(
            "test_doc".to_string(),
            HashMap::new(),
            vec![rule],
            crate::planning::graph::Graph::empty(),
        );

        let result = invert(
            &plan,
            "test",
            "~=", // Invalid operator
            Some(OperationResult::Value(LiteralValue::Number(Decimal::from(
                42,
            )))),
        );

        assert!(result.is_err());
    }
}
