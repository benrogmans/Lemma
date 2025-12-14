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
use std::sync::Arc;

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

    // 2. Normalize rule branches (account for last-wins semantics)
    let normalized_plan = reduced_plan.normalize_branches();

    // 3. Optimize the normalized plan (DNF expansion and simplification)
    let optimized_plan = normalized_plan.optimize();

    // 3. Build worlds on-demand from the optimized plan
    let mut builder = WorldBuilder::new(&optimized_plan);
    let rule_worlds = builder.build_worlds(rule_name)?;

    // 4. Filter worlds matching target
    let matching_worlds: Vec<&World> = rule_worlds
        .iter()
        .filter(|w| matches_target(&w.value, &target))
        .collect();

    // 5. For each world, solve algebraically
    let solutions: Vec<Solution> = matching_worlds
        .iter()
        .flat_map(|world| solve_world(world, &target))
        .collect();

    Ok(InversionResponse { solutions })
}

/// Negate a comparison operator
/// x > 0 becomes x <= 0, x == 0 becomes x != 0, etc.
fn negate_comparison(op: &ComparisonComputation) -> ComparisonComputation {
    match op {
        ComparisonComputation::LessThan => ComparisonComputation::GreaterThanOrEqual,
        ComparisonComputation::LessThanOrEqual => ComparisonComputation::GreaterThan,
        ComparisonComputation::GreaterThan => ComparisonComputation::LessThanOrEqual,
        ComparisonComputation::GreaterThanOrEqual => ComparisonComputation::LessThan,
        ComparisonComputation::Equal(n) => ComparisonComputation::NotEqual(*n),
        ComparisonComputation::NotEqual(n) => ComparisonComputation::Equal(*n),
    }
}

/// Check if a world's value expression matches the target
fn matches_target(expr: &Expression, target: &Target) -> bool {
    match (&expr.kind, &target.outcome) {
        // Literal value cases
        (ExpressionKind::Literal(lit), Some(OperationResult::Value(target_val))) => {
            matches_comparison(lit, &target.op, target_val)
        }
        (ExpressionKind::Literal(_), Some(OperationResult::Veto(_))) => false,
        (ExpressionKind::Literal(_), None) => true,
        
        // Veto cases
        (ExpressionKind::Veto(veto), Some(OperationResult::Veto(target_veto))) => {
            match (target_veto, &veto.message) {
                (None, _) => true,
                (Some(target_msg), Some(veto_msg)) => target_msg == veto_msg,
                _ => false,
            }
        }
        (ExpressionKind::Veto(_), None) => true, // Any value includes vetos
        (ExpressionKind::Veto(_), Some(OperationResult::Value(_))) => false, // Veto doesn't match value target
        
        // Non-literal expressions need algebraic solving
        _ => true, // Return true - solve_world will handle the actual solving
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
                world.constraints.clone(),
            )]
        }

        // Case 2: Veto
        ExpressionKind::Veto(veto) => {
            vec![Solution::new(
                OperationResult::Veto(veto.message.clone()),
                world.constraints.clone(),
            )]
        }

        // Case 3: Comparison expression with boolean target
        ExpressionKind::Comparison(left, op, right) => {
            // Check if target is a boolean
            let target_bool = match &target.outcome {
                Some(OperationResult::Value(LiteralValue::Boolean(b))) => b,
                _ => return vec![], // Non-boolean target for comparison
            };

            // For comparison expressions, we need to extract constraints based on boolean target
            // If target is true, use the comparison as-is
            // If target is false, negate the comparison operator (x > 0 becomes x <= 0)
            let constraint_op = if *target_bool == crate::semantic::BooleanValue::True {
                op.clone()
            } else {
                // Negate the comparison: x > 0 becomes x <= 0
                negate_comparison(op)
            };

            // Extract constraints from the comparison
            let constraint_expr = Expression::new(
                ExpressionKind::Comparison(left.clone(), constraint_op, right.clone()),
                None,
            );

            let mut constraint_set = crate::algebra::constraints::ConstraintSet::new();
            crate::algebra::constraints::extract_constraints(&constraint_expr, &mut constraint_set);
            let mut solution_constraints = constraint_set.to_fact_constraints();

            // Merge with existing world constraints
            for (fact, constraint) in &world.constraints {
                if let Some(existing) = solution_constraints.get(fact) {
                    let intersection = existing.intersect(constraint);
                    if intersection.is_satisfiable() {
                        solution_constraints.insert(fact.clone(), intersection);
                    } else {
                        return vec![]; // Incompatible constraints
                    }
                } else {
                    solution_constraints.insert(fact.clone(), constraint.clone());
                }
            }

            vec![Solution::new(
                target.outcome.clone().unwrap(),
                solution_constraints,
            )]
        }

        // Case 4: Other expressions containing unknown facts - needs algebraic inversion
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
                if world
                    .constraints
                    .get(&unknown_fact)
                    .map_or(true, |c| c.contains(&inv_solution.value))
                {
                    let mut solution_constraints = world.constraints.clone();

                    // Merge solution's constraints
                    for (fact, constraint) in inv_solution.constraints {
                        solution_constraints.insert(fact, constraint);
                    }

                    // Add exact constraint for the solved fact
                    solution_constraints.insert(
                        unknown_fact.clone(),
                        FactConstraint::exact(inv_solution.value.clone()),
                    );

                    solutions.push(Solution::new(
                        OperationResult::Value(target_value.clone()),
                        solution_constraints,
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
fn find_unknown_fact(
    expr: &Expression,
    constraints: &HashMap<FactPath, FactConstraint>,
) -> Result<FactPath, LemmaError> {
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

    Err(LemmaError::Engine(
        "No unknown fact found in expression".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{Branch, ExecutableRule};
    use crate::semantic::{
        BooleanValue, Expression, ExpressionKind, FactReference, FactValue, LemmaFact,
        LiteralValue, RulePath,
    };
    use rust_decimal::Decimal;
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

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

    #[test]
    fn test_find_unknown_fact_in_arithmetic() {
        // Test that find_unknown_fact works for arithmetic expressions
        use crate::semantic::ExpressionKind;
        use std::sync::Arc;

        let original_price_path = FactPath::local("original_price".to_string());

        // Create expression: original_price - 20
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(original_price_path.clone()),
                    None,
                )),
                crate::semantic::ArithmeticComputation::Subtract,
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::from(20))),
                    None,
                )),
            ),
            None,
        );

        // Should find original_price as the unknown fact
        let unknown =
            find_unknown_fact(&expr, &HashMap::new()).expect("Should find original_price");
        assert_eq!(unknown, original_price_path);
    }

    #[test]
    fn test_invert_simple_subtraction() {
        // Test that invert_expression works for simple subtraction
        use crate::algebra::isolation::invert_expression;
        use crate::semantic::ExpressionKind;
        use std::sync::Arc;

        let original_price_path = FactPath::local("original_price".to_string());

        // Create expression: original_price - 20
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(original_price_path.clone()),
                    None,
                )),
                crate::semantic::ArithmeticComputation::Subtract,
                Arc::new(Expression::new(
                    ExpressionKind::Literal(LiteralValue::Number(Decimal::from(20))),
                    None,
                )),
            ),
            None,
        );

        // Solve: original_price - 20 = 80 for original_price
        let result = invert_expression(
            &expr,
            &original_price_path,
            &LiteralValue::Number(Decimal::from(80)),
        );

        assert!(!result.is_unsatisfiable(), "Should have a solution");
        assert_eq!(
            result.solutions.len(),
            1,
            "Should have exactly one solution"
        );
        assert_eq!(
            result.solutions[0].value,
            LiteralValue::Number(Decimal::from(100)),
            "original_price should be 100"
        );
    }

    #[test]
    fn test_solve_world_with_arithmetic_expression() {
        // Test solve_world directly with an arithmetic expression
        use crate::semantic::ExpressionKind;
        use std::sync::Arc;

        let original_price_path = FactPath::local("original_price".to_string());

        // Create world with value: original_price - 20
        let world = World {
            constraints: HashMap::new(),
            value: Expression::new(
                ExpressionKind::Arithmetic(
                    Arc::new(Expression::new(
                        ExpressionKind::FactPath(original_price_path.clone()),
                        None,
                    )),
                    crate::semantic::ArithmeticComputation::Subtract,
                    Arc::new(Expression::new(
                        ExpressionKind::Literal(LiteralValue::Number(Decimal::from(20))),
                        None,
                    )),
                ),
                None,
            ),
        };

        // Create target: final_price = 80
        let target = Target {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Value(LiteralValue::Number(Decimal::from(
                80,
            )))),
        };

        // Solve the world
        let solutions = solve_world(&world, &target);

        assert_eq!(solutions.len(), 1, "Should have exactly one solution");
        let solution = &solutions[0];

        // Verify the solution has original_price = 100
        let constraint = solution
            .fact_constraints
            .get(&original_price_path)
            .expect("Solution should have original_price constraint");
        match constraint {
            crate::algebra::constraints::FactConstraint::Enumeration(values) => {
                assert_eq!(values.len(), 1, "Should have exactly one value");
                assert_eq!(
                    values[0],
                    LiteralValue::Number(Decimal::from(100)),
                    "original_price should be 100"
                );
            }
            other => panic!("Expected Enumeration, got {:?}", other),
        }
    }

    #[test]
    fn test_invert_simple_rule_through_full_flow() {
        // Test the full inversion flow for a simple rule
        use crate::evaluation::Evaluator;
        use crate::planning::ExecutionPlan;

        let original_price_path = FactPath::local("original_price".to_string());
        let discount_path = FactPath::local("discount".to_string());

        // Create a simple rule: final_price = original_price - discount
        let rule = ExecutableRule {
            path: RulePath {
                segments: vec![],
                rule: "final_price".to_string(),
            },
            name: "final_price".to_string(),
            branches: vec![Branch {
                condition: literal_bool(true),
                optimized_condition: None,
                result: Expression::new(
                    ExpressionKind::Arithmetic(
                        Arc::new(Expression::new(
                            ExpressionKind::FactPath(original_price_path.clone()),
                            None,
                        )),
                        crate::semantic::ArithmeticComputation::Subtract,
                        Arc::new(Expression::new(
                            ExpressionKind::FactPath(discount_path.clone()),
                            None,
                        )),
                    ),
                    None,
                ),
                source: None,
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(original_price_path.clone());
                set.insert(discount_path.clone());
                set
            },
            source: None,
        };

        // Create plan with discount = 20
        let plan = ExecutionPlan::new(
            "test".to_string(),
            {
                let mut facts = HashMap::new();
                facts.insert(
                    discount_path.clone(),
                    LemmaFact::new(
                        FactReference::local("discount".to_string()),
                        FactValue::Literal(LiteralValue::Number(Decimal::from(20))),
                    ),
                );
                facts
            },
            vec![rule],
            crate::planning::graph::Graph::empty(),
        );

        // Apply symbolic evaluation
        let evaluator = Evaluator;
        let reduced_plan = evaluator.evaluate_symbolic(&plan);

        // Build worlds
        let mut builder = crate::inversion::WorldBuilder::new(&reduced_plan);
        let worlds = builder
            .build_worlds("final_price")
            .expect("build_worlds should succeed");

        assert_eq!(worlds.len(), 1, "Should have exactly one world");
        let world = &worlds[0];

        // Verify world value is simplified (should be original_price - 20)
        match &world.value.kind {
            ExpressionKind::Arithmetic(left, op, right) => {
                assert_eq!(*op, crate::semantic::ArithmeticComputation::Subtract);
                // Left should be original_price
                match &left.kind {
                    ExpressionKind::FactPath(path) => assert_eq!(path, &original_price_path),
                    other => panic!("Expected FactPath, got {:?}", other),
                }
                // Right should be literal 20
                match &right.kind {
                    ExpressionKind::Literal(LiteralValue::Number(n)) => {
                        assert_eq!(*n, Decimal::from(20));
                    }
                    other => panic!("Expected Literal(20), got {:?}", other),
                }
            }
            other => panic!("Expected Arithmetic, got {:?}", other),
        }

        // Now test inversion
        let target = Target {
            op: TargetOp::Eq,
            outcome: Some(OperationResult::Value(LiteralValue::Number(Decimal::from(
                80,
            )))),
        };

        let matching_worlds: Vec<&World> = worlds
            .iter()
            .filter(|w| matches_target(&w.value, &target))
            .collect();

        assert_eq!(matching_worlds.len(), 1, "Should have one matching world");

        let solutions: Vec<Solution> = matching_worlds
            .iter()
            .flat_map(|world| solve_world(world, &target))
            .collect();

        assert_eq!(solutions.len(), 1, "Should have exactly one solution");
        let solution = &solutions[0];

        // Verify the solution has original_price = 100
        let constraint = solution
            .fact_constraints
            .get(&original_price_path)
            .expect("Solution should have original_price constraint");
        match constraint {
            crate::algebra::constraints::FactConstraint::Enumeration(values) => {
                assert_eq!(values.len(), 1, "Should have exactly one value");
                assert_eq!(
                    values[0],
                    LiteralValue::Number(Decimal::from(100)),
                    "original_price should be 100"
                );
            }
            other => panic!("Expected Enumeration, got {:?}", other),
        }
    }
}
