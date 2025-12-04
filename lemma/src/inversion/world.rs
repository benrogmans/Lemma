//! World enumeration for inversion
//!
//! A "world" is a complete assignment of which branch is active for each rule.
//! This module enumerates all valid worlds for a target rule.
//!
//! Also includes expression substitution and hydration utilities.

use crate::planning::graph::RuleNode;
use crate::planning::ExecutionPlan;
use crate::{
    ArithmeticComputation, BooleanValue, ComparisonComputation, Expression, ExpressionKind,
    FactPath, FactValue, LemmaError, LemmaResult, LiteralValue, OperationResult, RulePath,
};
use serde::ser::{Serialize, SerializeMap, Serializer};
use std::collections::{HashMap, HashSet, VecDeque};

use super::constraint::Constraint;

/// A world assigns each rule to one of its branch indices
#[derive(Debug, Clone, Default)]
pub struct World(HashMap<RulePath, usize>);

impl World {
    /// Create a new empty world
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Get the branch index for a rule
    pub fn get(&self, rule_path: &RulePath) -> Option<&usize> {
        self.0.get(rule_path)
    }

    /// Insert a branch assignment for a rule
    pub fn insert(&mut self, rule_path: RulePath, branch_idx: usize) -> Option<usize> {
        self.0.insert(rule_path, branch_idx)
    }

    /// Iterate over all branch assignments
    pub fn iter(&self) -> impl Iterator<Item = (&RulePath, &usize)> {
        self.0.iter()
    }
}

impl Serialize for World {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(&k.to_string(), v)?;
        }
        map.end()
    }
}

/// A solution from world enumeration with a resolved outcome
#[derive(Debug, Clone)]
pub struct WorldSolution {
    /// The world (branch assignment) that produced this solution
    pub world: World,
    /// The constraint under which this solution applies (facts only, no rule references)
    pub constraint: Constraint,
    /// The outcome (value or veto)
    pub outcome: OperationResult,
}

/// A solution from world enumeration with an arithmetic expression outcome
///
/// This represents cases where the outcome is a computed expression (like `price * 5`)
/// that couldn't be evaluated to a literal because it contains unknown facts.
/// These need algebraic solving to determine the input values.
#[derive(Debug, Clone)]
pub struct WorldArithmeticSolution {
    /// The world (branch assignment) that produced this solution
    pub world: World,
    /// The constraint under which this solution applies (facts only, no rule references)
    pub constraint: Constraint,
    /// The outcome expression (contains unknown facts)
    pub outcome_expression: Expression,
}

/// Result of world enumeration containing both literal and arithmetic solutions
#[derive(Debug, Clone)]
pub struct EnumerationResult {
    /// Solutions with literal outcomes (can be directly compared to target)
    pub literal_solutions: Vec<WorldSolution>,
    /// Solutions with arithmetic outcomes (need algebraic solving)
    pub arithmetic_solutions: Vec<WorldArithmeticSolution>,
}

/// Enumerates valid worlds for a target rule
pub struct WorldEnumerator<'a> {
    plan: &'a ExecutionPlan,
    /// Rules to process, in topological order (dependencies first)
    rules_in_order: Vec<RulePath>,
    /// Cache: rule path -> rule node (for quick lookup)
    rule_cache: HashMap<RulePath, &'a RuleNode>,
}

impl<'a> WorldEnumerator<'a> {
    /// Create a new world enumerator for a target rule
    pub fn new(plan: &'a ExecutionPlan, target_rule: &RulePath) -> LemmaResult<Self> {
        let graph = plan.graph();

        // Find all rules that the target rule depends on (transitively)
        let dependent_rules = collect_transitive_dependencies(target_rule, graph)?;

        // Order rules topologically (dependencies first)
        let rules_in_order = topological_sort(&dependent_rules, target_rule, graph)?;

        // Build rule cache for quick lookup
        let mut rule_cache = HashMap::new();
        for rule_path in &rules_in_order {
            if let Some(rule_node) = graph.rules().get(rule_path) {
                rule_cache.insert(rule_path.clone(), rule_node);
            }
        }

        Ok(Self {
            plan,
            rules_in_order,
            rule_cache,
        })
    }

    /// Enumerate all valid worlds for the target rule
    ///
    /// Returns an `EnumerationResult` containing:
    /// - `literal_solutions`: Worlds where the outcome is a concrete literal value
    /// - `arithmetic_solutions`: Worlds where the outcome is an arithmetic expression
    ///   containing unknown facts (needs algebraic solving)
    pub fn enumerate(
        &mut self,
        provided_facts: &HashSet<FactPath>,
    ) -> LemmaResult<EnumerationResult> {
        if self.rules_in_order.is_empty() {
            return Ok(EnumerationResult {
                literal_solutions: vec![],
                arithmetic_solutions: vec![],
            });
        }

        // Start with a single empty world and true constraint
        let mut current_worlds: Vec<(World, Constraint)> = vec![(World::new(), Constraint::True)];

        // Process each rule in topological order
        for rule_path in &self.rules_in_order.clone() {
            let rule_node = match self.rule_cache.get(rule_path) {
                Some(node) => *node,
                None => continue,
            };

            let mut next_worlds = Vec::new();

            for (world, accumulated_constraint) in current_worlds {
                // For each branch in this rule
                for (branch_idx, branch) in rule_node.branches.iter().enumerate() {
                    // Create new world with this branch assignment
                    let mut new_world = world.clone();
                    new_world.insert(rule_path.clone(), branch_idx);

                    // Substitute known rule values in the branch condition
                    let substituted_condition = substitute_rules_in_expression(
                        &branch.condition,
                        &new_world,
                        self.plan,
                    )?;

                    // Hydrate with provided facts
                    let hydrated_condition = hydrate_facts_in_expression(
                        &substituted_condition,
                        self.plan,
                        provided_facts,
                    )?;

                    // Convert to constraint
                    let branch_constraint = Constraint::from_expression(&hydrated_condition)?;

                    // Combine with accumulated constraint
                    let combined_constraint = accumulated_constraint.clone().and(branch_constraint);

                    // Simplify and check if satisfiable
                    let simplified = combined_constraint.simplify()?;

                    // Only keep if not contradictory
                    if !simplified.is_false() {
                        next_worlds.push((new_world, simplified));
                    }
                }
            }

            current_worlds = next_worlds;

            // Early exit if no valid worlds remain
            if current_worlds.is_empty() {
                break;
            }
        }

        // Convert to WorldSolutions and WorldArithmeticSolutions
        let target_rule_path = self.rules_in_order.last().ok_or_else(|| {
            LemmaError::Engine("No rules in order for world enumeration".to_string())
        })?;

        let mut literal_solutions = Vec::new();
        let mut arithmetic_solutions = Vec::new();

        for (world, constraint) in current_worlds {
            // Get the outcome from the target rule's branch
            if let Some(&branch_idx) = world.get(target_rule_path) {
                if let Some(rule_node) = self.rule_cache.get(target_rule_path) {
                    if branch_idx < rule_node.branches.len() {
                        let branch = &rule_node.branches[branch_idx];

                        // Substitute and hydrate the result expression
                        let substituted_result = substitute_rules_in_expression(
                            &branch.result,
                            &world,
                            self.plan,
                        )?;

                        let hydrated_result = hydrate_facts_in_expression(
                            &substituted_result,
                            self.plan,
                            provided_facts,
                        )?;

                        // Try to fold the result to a literal
                        let folded_result = try_constant_fold_expression(&hydrated_result)
                            .unwrap_or(hydrated_result.clone());

                        // Try to extract a literal value directly
                        if let Some(outcome) = extract_outcome(&folded_result) {
                            literal_solutions.push(WorldSolution {
                                world,
                                constraint,
                                outcome,
                            });
                        } else if is_boolean_expression(&folded_result) {
                            // For boolean expressions (comparisons, logical ops), create two solutions:
                            // one where the expression is true, one where it's false
                            let (true_solutions, false_solutions) =
                                create_boolean_expression_solutions(
                                    world,
                                    constraint,
                                    &folded_result,
                                )?;
                            literal_solutions.extend(true_solutions);
                            literal_solutions.extend(false_solutions);
                        } else if is_arithmetic_expression(&folded_result) {
                            // Arithmetic expression with unknown facts - needs algebraic solving
                            arithmetic_solutions.push(WorldArithmeticSolution {
                                world,
                                constraint,
                                outcome_expression: folded_result,
                            });
                        }
                        // Other expression types (rule references, etc.) are silently skipped
                        // as they indicate incomplete substitution
                    }
                }
            }
        }

        Ok(EnumerationResult {
            literal_solutions,
            arithmetic_solutions,
        })
    }
}

// ============================================================================
// Dependency and topological sorting
// ============================================================================

/// Collect all rules that a target rule depends on (transitively)
fn collect_transitive_dependencies(
    target_rule: &RulePath,
    graph: &crate::planning::Graph,
) -> LemmaResult<HashSet<RulePath>> {
    let mut result = HashSet::new();
    let mut queue = VecDeque::new();

    queue.push_back(target_rule.clone());
    result.insert(target_rule.clone());

    while let Some(rule_path) = queue.pop_front() {
        if let Some(rule_node) = graph.rules().get(&rule_path) {
            for dependency in &rule_node.depends_on_rules {
                if result.insert(dependency.clone()) {
                    queue.push_back(dependency.clone());
                }
            }
        }
    }

    Ok(result)
}

/// Sort rules topologically (dependencies first)
fn topological_sort(
    rules: &HashSet<RulePath>,
    target_rule: &RulePath,
    graph: &crate::planning::Graph,
) -> LemmaResult<Vec<RulePath>> {
    // Build in-degree map (only for rules in our set)
    let mut in_degree: HashMap<RulePath, usize> = HashMap::new();
    let mut dependents: HashMap<RulePath, Vec<RulePath>> = HashMap::new();

    for rule_path in rules {
        in_degree.insert(rule_path.clone(), 0);
        dependents.insert(rule_path.clone(), Vec::new());
    }

    for rule_path in rules {
        if let Some(rule_node) = graph.rules().get(rule_path) {
            for dependency in &rule_node.depends_on_rules {
                if rules.contains(dependency) {
                    if let Some(degree) = in_degree.get_mut(rule_path) {
                        *degree += 1;
                    }
                    if let Some(deps) = dependents.get_mut(dependency) {
                        deps.push(rule_path.clone());
                    }
                }
            }
        }
    }

    // Process nodes with zero in-degree
    let mut queue = VecDeque::new();
    for (rule_path, degree) in &in_degree {
        if *degree == 0 {
            queue.push_back(rule_path.clone());
        }
    }

    let mut result = Vec::new();
    while let Some(rule_path) = queue.pop_front() {
        result.push(rule_path.clone());

        if let Some(dependent_rules) = dependents.get(&rule_path) {
            for dependent in dependent_rules {
                if let Some(degree) = in_degree.get_mut(dependent) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }
    }

    if result.len() != rules.len() {
        return Err(LemmaError::Engine(format!(
            "Circular dependency detected in rules: expected {} rules, got {}",
            rules.len(),
            result.len()
        )));
    }

    // Ensure target rule is last
    if let Some(target_pos) = result.iter().position(|r| r == target_rule) {
        let target = result.remove(target_pos);
        result.push(target);
    }

    Ok(result)
}

// ============================================================================
// Expression substitution
// ============================================================================

/// Substitute rule references in an expression with their values in a given world
///
/// For each RulePath in the expression, looks up which branch is active in the world
/// and replaces the RulePath with the branch's result expression.
fn substitute_rules_in_expression(
    expr: &Expression,
    world: &World,
    plan: &ExecutionPlan,
) -> LemmaResult<Expression> {
    let mut visited_rules: HashSet<RulePath> = HashSet::new();
    substitute_rules_recursive(expr, world, plan, &mut visited_rules)
}

fn substitute_rules_recursive(
    expr: &Expression,
    world: &World,
    plan: &ExecutionPlan,
    visited_rules: &mut HashSet<RulePath>,
) -> LemmaResult<Expression> {
    match &expr.kind {
        ExpressionKind::RulePath(rule_path) => {
            if visited_rules.contains(rule_path) {
                return Err(LemmaError::Engine(format!(
                    "Circular rule reference detected during substitution: {}",
                    rule_path
                )));
            }

            if let Some(&branch_idx) = world.get(rule_path) {
                if let Some(rule) = plan.get_rule_by_path(rule_path) {
                    if branch_idx < rule.branches.len() {
                        let branch = &rule.branches[branch_idx];

                        visited_rules.insert(rule_path.clone());
                        let result = substitute_rules_recursive(
                            &branch.result,
                            world,
                            plan,
                            visited_rules,
                        );
                        visited_rules.remove(rule_path);

                        return result;
                    }
                }
            }
            Ok(expr.clone())
        }

        ExpressionKind::RuleReference(_) => {
            Err(LemmaError::Engine(
                "RuleReference found during substitution - should have been converted to RulePath"
                    .to_string(),
            ))
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_sub = substitute_rules_recursive(left, world, plan, visited_rules)?;
            let right_sub = substitute_rules_recursive(right, world, plan, visited_rules)?;
            Ok(Expression::new(
                ExpressionKind::Arithmetic(Box::new(left_sub), op.clone(), Box::new(right_sub)),
                expr.source.clone(),
            ))
        }

        ExpressionKind::Comparison(left, op, right) => {
            let left_sub = substitute_rules_recursive(left, world, plan, visited_rules)?;
            let right_sub = substitute_rules_recursive(right, world, plan, visited_rules)?;
            Ok(Expression::new(
                ExpressionKind::Comparison(Box::new(left_sub), op.clone(), Box::new(right_sub)),
                expr.source.clone(),
            ))
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let left_sub = substitute_rules_recursive(left, world, plan, visited_rules)?;
            let right_sub = substitute_rules_recursive(right, world, plan, visited_rules)?;
            Ok(Expression::new(
                ExpressionKind::LogicalAnd(Box::new(left_sub), Box::new(right_sub)),
                expr.source.clone(),
            ))
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_sub = substitute_rules_recursive(left, world, plan, visited_rules)?;
            let right_sub = substitute_rules_recursive(right, world, plan, visited_rules)?;
            Ok(Expression::new(
                ExpressionKind::LogicalOr(Box::new(left_sub), Box::new(right_sub)),
                expr.source.clone(),
            ))
        }

        ExpressionKind::LogicalNegation(inner, neg_type) => {
            let inner_sub = substitute_rules_recursive(inner, world, plan, visited_rules)?;
            Ok(Expression::new(
                ExpressionKind::LogicalNegation(Box::new(inner_sub), neg_type.clone()),
                expr.source.clone(),
            ))
        }

        ExpressionKind::UnitConversion(inner, unit) => {
            let inner_sub = substitute_rules_recursive(inner, world, plan, visited_rules)?;
            Ok(Expression::new(
                ExpressionKind::UnitConversion(Box::new(inner_sub), unit.clone()),
                expr.source.clone(),
            ))
        }

        ExpressionKind::MathematicalComputation(func, inner) => {
            let inner_sub = substitute_rules_recursive(inner, world, plan, visited_rules)?;
            Ok(Expression::new(
                ExpressionKind::MathematicalComputation(func.clone(), Box::new(inner_sub)),
                expr.source.clone(),
            ))
        }

        // Leaf nodes and other expressions - return unchanged
        ExpressionKind::Literal(_)
        | ExpressionKind::FactPath(_)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::Veto(_) => Ok(expr.clone()),
    }
}

// ============================================================================
// Fact hydration
// ============================================================================

/// Hydrate fact references in an expression with their known values
///
/// For each FactPath in the expression, if the fact is in provided_facts,
/// replaces the FactPath with a Literal containing the fact's value.
fn hydrate_facts_in_expression(
    expr: &Expression,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Expression> {
    hydrate_facts_recursive(expr, plan, provided_facts)
}

fn hydrate_facts_recursive(
    expr: &Expression,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Expression> {
    match &expr.kind {
        ExpressionKind::FactPath(fact_path) => {
            if provided_facts.contains(fact_path) {
                if let Some(fact) = plan.facts.get(fact_path) {
                    if let FactValue::Literal(lit) = &fact.value {
                        return Ok(Expression::new(
                            ExpressionKind::Literal(lit.clone()),
                            expr.source.clone(),
                        ));
                    }
                }
            }
            Ok(expr.clone())
        }

        ExpressionKind::FactReference(_) => {
            Err(LemmaError::Engine(
                "FactReference found during hydration - should have been converted to FactPath"
                    .to_string(),
            ))
        }

        ExpressionKind::RuleReference(_) => {
            Err(LemmaError::Engine(
                "RuleReference found during hydration - should have been converted to RulePath"
                    .to_string(),
            ))
        }

        ExpressionKind::Arithmetic(left, op, right) => {
            let left_hyd = hydrate_facts_recursive(left, plan, provided_facts)?;
            let right_hyd = hydrate_facts_recursive(right, plan, provided_facts)?;
            Ok(Expression::new(
                ExpressionKind::Arithmetic(Box::new(left_hyd), op.clone(), Box::new(right_hyd)),
                expr.source.clone(),
            ))
        }

        ExpressionKind::Comparison(left, op, right) => {
            let left_hyd = hydrate_facts_recursive(left, plan, provided_facts)?;
            let right_hyd = hydrate_facts_recursive(right, plan, provided_facts)?;
            Ok(Expression::new(
                ExpressionKind::Comparison(Box::new(left_hyd), op.clone(), Box::new(right_hyd)),
                expr.source.clone(),
            ))
        }

        ExpressionKind::LogicalAnd(left, right) => {
            let left_hyd = hydrate_facts_recursive(left, plan, provided_facts)?;
            let right_hyd = hydrate_facts_recursive(right, plan, provided_facts)?;
            Ok(Expression::new(
                ExpressionKind::LogicalAnd(Box::new(left_hyd), Box::new(right_hyd)),
                expr.source.clone(),
            ))
        }

        ExpressionKind::LogicalOr(left, right) => {
            let left_hyd = hydrate_facts_recursive(left, plan, provided_facts)?;
            let right_hyd = hydrate_facts_recursive(right, plan, provided_facts)?;
            Ok(Expression::new(
                ExpressionKind::LogicalOr(Box::new(left_hyd), Box::new(right_hyd)),
                expr.source.clone(),
            ))
        }

        ExpressionKind::LogicalNegation(inner, neg_type) => {
            let inner_hyd = hydrate_facts_recursive(inner, plan, provided_facts)?;
            Ok(Expression::new(
                ExpressionKind::LogicalNegation(Box::new(inner_hyd), neg_type.clone()),
                expr.source.clone(),
            ))
        }

        ExpressionKind::UnitConversion(inner, unit) => {
            let inner_hyd = hydrate_facts_recursive(inner, plan, provided_facts)?;
            Ok(Expression::new(
                ExpressionKind::UnitConversion(Box::new(inner_hyd), unit.clone()),
                expr.source.clone(),
            ))
        }

        ExpressionKind::MathematicalComputation(func, inner) => {
            let inner_hyd = hydrate_facts_recursive(inner, plan, provided_facts)?;
            Ok(Expression::new(
                ExpressionKind::MathematicalComputation(func.clone(), Box::new(inner_hyd)),
                expr.source.clone(),
            ))
        }

        // Leaf nodes and other expressions - return unchanged
        ExpressionKind::Literal(_)
        | ExpressionKind::RulePath(_)
        | ExpressionKind::Veto(_) => Ok(expr.clone()),
    }
}

// ============================================================================
// Constant folding
// ============================================================================

/// Extract an outcome (value or veto) from an expression
fn extract_outcome(expr: &Expression) -> Option<OperationResult> {
    match &expr.kind {
        ExpressionKind::Literal(lit) => Some(OperationResult::Value(lit.clone())),
        ExpressionKind::Veto(ve) => Some(OperationResult::Veto(ve.message.clone())),
        _ => None,
    }
}

/// Check if an expression is a boolean-producing expression (comparison or logical)
fn is_boolean_expression(expr: &Expression) -> bool {
    matches!(
        &expr.kind,
        ExpressionKind::Comparison(_, _, _)
            | ExpressionKind::LogicalAnd(_, _)
            | ExpressionKind::LogicalOr(_, _)
            | ExpressionKind::LogicalNegation(_, _)
    )
}

/// Check if an expression is an arithmetic expression (contains arithmetic operations)
///
/// Returns true for expressions like `price * 5`, `x + y`, etc.
fn is_arithmetic_expression(expr: &Expression) -> bool {
    match &expr.kind {
        ExpressionKind::Arithmetic(_, _, _) => true,
        ExpressionKind::MathematicalComputation(_, _) => true,
        ExpressionKind::UnitConversion(inner, _) => is_arithmetic_expression(inner),
        ExpressionKind::FactPath(_) => true, // Lone fact is also solvable
        _ => false,
    }
}

/// For boolean expressions that can't be evaluated to a literal (e.g., `age > 18`),
/// create two solutions: one where the expression is true, one where it's false.
///
/// This allows inversion to work with rules like `rule of_age = age > 18`
fn create_boolean_expression_solutions(
    world: World,
    base_constraint: Constraint,
    boolean_expr: &Expression,
) -> LemmaResult<(Vec<WorldSolution>, Vec<WorldSolution>)> {
    // Convert boolean expression to constraint
    let expr_constraint = Constraint::from_expression(boolean_expr)?;

    // Solution where the boolean expression is true
    let true_constraint = base_constraint.clone().and(expr_constraint.clone());
    let simplified_true = true_constraint.simplify()?;

    let true_solutions = if !simplified_true.is_false() {
        vec![WorldSolution {
            world: world.clone(),
            constraint: simplified_true,
            outcome: OperationResult::Value(LiteralValue::Boolean(BooleanValue::True)),
        }]
    } else {
        vec![]
    };

    // Solution where the boolean expression is false
    let false_constraint = base_constraint.and(expr_constraint.not());
    let simplified_false = false_constraint.simplify()?;

    let false_solutions = if !simplified_false.is_false() {
        vec![WorldSolution {
            world,
            constraint: simplified_false,
            outcome: OperationResult::Value(LiteralValue::Boolean(BooleanValue::False)),
        }]
    } else {
        vec![]
    };

    Ok((true_solutions, false_solutions))
}

/// Attempt constant folding on an expression (simplified version for outcomes)
pub(crate) fn try_constant_fold_expression(expr: &Expression) -> Option<Expression> {
    match &expr.kind {
        ExpressionKind::Arithmetic(left, op, right) => {
            let left_folded = try_constant_fold_expression(left).unwrap_or((**left).clone());
            let right_folded = try_constant_fold_expression(right).unwrap_or((**right).clone());
            if let (ExpressionKind::Literal(ref left_val), ExpressionKind::Literal(ref right_val)) =
                (&left_folded.kind, &right_folded.kind)
            {
                if let Some(result) = evaluate_arithmetic(left_val, op, right_val) {
                    return Some(Expression::new(
                        ExpressionKind::Literal(result),
                        expr.source.clone(),
                    ));
                }
            }
            Some(Expression::new(
                ExpressionKind::Arithmetic(Box::new(left_folded), op.clone(), Box::new(right_folded)),
                expr.source.clone(),
            ))
        }
        ExpressionKind::Comparison(left, op, right) => {
            let left_folded = try_constant_fold_expression(left).unwrap_or((**left).clone());
            let right_folded = try_constant_fold_expression(right).unwrap_or((**right).clone());
            if let (ExpressionKind::Literal(ref left_val), ExpressionKind::Literal(ref right_val)) =
                (&left_folded.kind, &right_folded.kind)
            {
                if let Some(result) = evaluate_comparison(left_val, op, right_val) {
                    return Some(Expression::new(
                        ExpressionKind::Literal(LiteralValue::Boolean(result)),
                        expr.source.clone(),
                    ));
                }
            }
            Some(Expression::new(
                ExpressionKind::Comparison(Box::new(left_folded), op.clone(), Box::new(right_folded)),
                expr.source.clone(),
            ))
        }
        _ => None,
    }
}

/// Evaluate an arithmetic operation on two literals
///
/// Delegates to the computation module for consistent behavior
fn evaluate_arithmetic(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> Option<LiteralValue> {
    use crate::computation::{arithmetic_operation, OperationResult};

    match arithmetic_operation(left, op, right) {
        OperationResult::Value(lit) => Some(lit),
        OperationResult::Veto(_) => None,
    }
}

/// Evaluate a comparison operation on two literals
///
/// Delegates to the computation module for consistent behavior
fn evaluate_comparison(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> Option<BooleanValue> {
    use crate::computation::{comparison_operation, OperationResult};

    match comparison_operation(left, op, right) {
        OperationResult::Value(LiteralValue::Boolean(b)) => Some(b),
        _ => None,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::Span;
    use crate::Source;
    use rust_decimal::Decimal;

    fn test_source() -> Source {
        Source::new(
            "<test>",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "test",
        )
    }

    fn literal_expr(val: LiteralValue) -> Expression {
        Expression::new(ExpressionKind::Literal(val), test_source())
    }

    fn fact_expr(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::FactPath(FactPath::local(name.to_string())),
            test_source(),
        )
    }

    fn num(n: i64) -> LiteralValue {
        LiteralValue::Number(Decimal::from(n))
    }

    #[test]
    fn test_world_new() {
        let world = World::new();
        assert!(world.0.is_empty());
    }

    #[test]
    fn test_world_insert_and_get() {
        let mut world = World::new();
        let rule_path = RulePath {
            segments: vec![],
            rule: "test_rule".to_string(),
        };
        world.insert(rule_path.clone(), 2);
        assert_eq!(world.get(&rule_path), Some(&2));
    }

    #[test]
    fn test_hydrate_literal_unchanged() {
        let plan = ExecutionPlan::new("test".to_string(), HashMap::new(), Vec::new());
        let provided: HashSet<FactPath> = HashSet::new();

        let expr = literal_expr(num(42));
        let result = hydrate_facts_in_expression(&expr, &plan, &provided).unwrap();

        assert!(matches!(result.kind, ExpressionKind::Literal(LiteralValue::Number(_))));
    }

    #[test]
    fn test_hydrate_fact_not_provided() {
        let plan = ExecutionPlan::new("test".to_string(), HashMap::new(), Vec::new());
        let provided: HashSet<FactPath> = HashSet::new();

        let expr = fact_expr("age");
        let result = hydrate_facts_in_expression(&expr, &plan, &provided).unwrap();

        assert!(matches!(result.kind, ExpressionKind::FactPath(_)));
    }

    #[test]
    fn test_constant_fold_arithmetic() {
        let left = literal_expr(num(10));
        let right = literal_expr(num(5));
        let expr = Expression::new(
            ExpressionKind::Arithmetic(
                Box::new(left),
                ArithmeticComputation::Add,
                Box::new(right),
            ),
            test_source(),
        );

        let folded = try_constant_fold_expression(&expr).unwrap();
        
        if let ExpressionKind::Literal(LiteralValue::Number(n)) = folded.kind {
            assert_eq!(n, Decimal::from(15));
        } else {
            panic!("Expected literal number");
        }
    }

    #[test]
    fn test_constant_fold_comparison() {
        let left = literal_expr(num(10));
        let right = literal_expr(num(5));
        let expr = Expression::new(
            ExpressionKind::Comparison(
                Box::new(left),
                ComparisonComputation::GreaterThan,
                Box::new(right),
            ),
            test_source(),
        );

        let folded = try_constant_fold_expression(&expr).unwrap();
        
        if let ExpressionKind::Literal(LiteralValue::Boolean(b)) = folded.kind {
            assert_eq!(b, BooleanValue::True);
        } else {
            panic!("Expected literal boolean");
        }
    }
}
