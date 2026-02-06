//! World enumeration for inversion
//!
//! A "world" is a complete assignment of which branch is active for each rule.
//! This module enumerates all valid worlds for a target rule.
//!
//! Also includes expression substitution and hydration utilities.

use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, Expression, ExpressionKind, FactPath,
    LiteralValue, MathematicalComputation, NegationType, RulePath, SemanticConversionTarget,
    Source,
};
use crate::planning::{ExecutableRule, ExecutionPlan};
use crate::{LemmaResult, OperationResult};
use serde::ser::{Serialize, SerializeMap, Serializer};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use super::constraint::Constraint;

/// A world assigns each rule to one of its branch indices
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    /// Cache: rule path -> executable rule (for quick lookup)
    rule_cache: HashMap<RulePath, &'a ExecutableRule>,
}

impl<'a> WorldEnumerator<'a> {
    /// Create a new world enumerator for a target rule
    pub fn new(plan: &'a ExecutionPlan, target_rule: &RulePath) -> LemmaResult<Self> {
        // Build rule lookup from execution plan
        let rule_map: HashMap<RulePath, &ExecutableRule> =
            plan.rules.iter().map(|r| (r.path.clone(), r)).collect();

        // Find all rules that the target rule depends on (transitively)
        let dependent_rules = collect_transitive_dependencies(target_rule, &rule_map)?;

        // Plan rules are already in topological order, so filter and preserve order
        let rules_in_order: Vec<RulePath> = plan
            .rules
            .iter()
            .filter(|r| dependent_rules.contains(&r.path))
            .map(|r| r.path.clone())
            .collect();

        // Build rule cache for quick lookup (only rules we need)
        let rule_cache: HashMap<RulePath, &ExecutableRule> = rules_in_order
            .iter()
            .filter_map(|path| rule_map.get(path).map(|r| (path.clone(), *r)))
            .collect();

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

                    // Get branch constraint
                    // For "last wins" semantics: all LATER branches must have FALSE conditions
                    let mut branch_constraint = if let Some(ref condition) = branch.condition {
                        // This branch's condition must be TRUE
                        let substituted_condition = substitute_rules_in_expression(
                            &Arc::new(condition.clone()),
                            &new_world,
                            self.plan,
                        )?;
                        let hydrated_condition = hydrate_facts_in_expression(
                            &Arc::new(substituted_condition),
                            self.plan,
                            provided_facts,
                        )?;
                        Constraint::from_expression(&hydrated_condition)?
                    } else {
                        // Default branch has no explicit condition
                        Constraint::True
                    };

                    // For "last wins": all LATER branches must NOT match
                    // (their conditions must be FALSE)
                    for later_branch in rule_node.branches.iter().skip(branch_idx + 1) {
                        if let Some(ref later_condition) = later_branch.condition {
                            let substituted_later = substitute_rules_in_expression(
                                &Arc::new(later_condition.clone()),
                                &new_world,
                                self.plan,
                            )?;
                            let hydrated_later = hydrate_facts_in_expression(
                                &Arc::new(substituted_later),
                                self.plan,
                                provided_facts,
                            )?;
                            let later_constraint = Constraint::from_expression(&hydrated_later)?;
                            // Later branch's condition must be FALSE
                            branch_constraint = branch_constraint.and(later_constraint.not());
                        }
                    }

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
        let target_rule_path = self
            .rules_in_order
            .last()
            .unwrap_or_else(|| unreachable!("BUG: no rules in order for world enumeration"));

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
                            &Arc::new(branch.result.clone()),
                            &world,
                            self.plan,
                        )?;

                        let hydrated_result = hydrate_facts_in_expression(
                            &Arc::new(substituted_result),
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
    rule_map: &HashMap<RulePath, &ExecutableRule>,
) -> LemmaResult<HashSet<RulePath>> {
    let mut result = HashSet::new();
    let mut queue = VecDeque::new();

    queue.push_back(target_rule.clone());
    result.insert(target_rule.clone());

    while let Some(rule_path) = queue.pop_front() {
        if let Some(rule) = rule_map.get(&rule_path) {
            // Extract rule dependencies from branch expressions
            let dependencies = extract_rule_dependencies(rule);
            for dependency in dependencies {
                if result.insert(dependency.clone()) {
                    queue.push_back(dependency);
                }
            }
        }
    }

    Ok(result)
}

/// Extract rule paths referenced in an executable rule's expressions
fn extract_rule_dependencies(rule: &ExecutableRule) -> HashSet<RulePath> {
    let mut deps = HashSet::new();
    for branch in &rule.branches {
        if let Some(ref condition) = branch.condition {
            extract_rule_paths_from_expression(condition, &mut deps);
        }
        extract_rule_paths_from_expression(&branch.result, &mut deps);
    }
    deps
}

/// Recursively extract RulePath references from an expression
fn extract_rule_paths_from_expression(expr: &Expression, paths: &mut HashSet<RulePath>) {
    match &expr.kind {
        ExpressionKind::RulePath(rp) => {
            paths.insert(rp.clone());
        }
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right) => {
            extract_rule_paths_from_expression(left, paths);
            extract_rule_paths_from_expression(right, paths);
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            extract_rule_paths_from_expression(inner, paths);
        }
        ExpressionKind::Literal(_) | ExpressionKind::FactPath(_) | ExpressionKind::Veto(_) => {}
    }
}

// ============================================================================
// Expression substitution
// ============================================================================

/// Substitute rule references in an expression with their values in a given world
///
/// For each RulePath in the expression, looks up which branch is active in the world
/// and replaces the RulePath with the branch's result expression.
fn substitute_rules_in_expression(
    expr: &Arc<Expression>,
    world: &World,
    plan: &ExecutionPlan,
) -> LemmaResult<Expression> {
    enum WorkItem {
        Process(usize),
        BuildArithmetic(ArithmeticComputation, Source),
        BuildComparison(ComparisonComputation, Source),
        BuildLogicalAnd(Source),
        BuildLogicalOr(Source),
        BuildLogicalNegation(NegationType, Source),
        BuildUnitConversion(SemanticConversionTarget, Source),
        BuildMathematicalComputation(MathematicalComputation, Source),
        PopVisitedRules,
    }

    let mut expr_pool: Vec<Arc<Expression>> = Vec::new();
    let mut work_stack: Vec<WorkItem> = Vec::new();
    let mut result_pool: Vec<Expression> = Vec::new();
    let mut visited_rules_stack: Vec<HashSet<RulePath>> = vec![HashSet::new()];

    let root_idx = expr_pool.len();
    expr_pool.push(Arc::clone(expr));
    work_stack.push(WorkItem::Process(root_idx));

    while let Some(work) = work_stack.pop() {
        match work {
            WorkItem::Process(expr_idx) => {
                let e = &expr_pool[expr_idx];
                let source_loc = e.source_location.clone();

                match &e.kind {
                    ExpressionKind::RulePath(rule_path) => {
                        let visited = visited_rules_stack.last().expect("visited_rules_stack should never be empty when processing RulePath expressions");
                        if visited.contains(rule_path) {
                            unreachable!(
                                "BUG: circular rule reference detected during substitution: {}",
                                rule_path
                            );
                        }

                        if let Some(&branch_idx) = world.get(rule_path) {
                            if let Some(rule) = plan.get_rule_by_path(rule_path) {
                                if branch_idx < rule.branches.len() {
                                    let branch = &rule.branches[branch_idx];
                                    let mut new_visited = visited.clone();
                                    new_visited.insert(rule_path.clone());
                                    visited_rules_stack.push(new_visited);

                                    let sub_expr_idx = expr_pool.len();
                                    expr_pool.push(Arc::new(branch.result.clone()));
                                    work_stack.push(WorkItem::PopVisitedRules);
                                    work_stack.push(WorkItem::Process(sub_expr_idx));
                                    continue;
                                }
                            }
                        }
                        result_pool.push(Expression::new(
                            ExpressionKind::RulePath(rule_path.clone()),
                            source_loc,
                        ));
                    }
                    ExpressionKind::Arithmetic(left, op, right) => {
                        let op_clone = op.clone();
                        let left_arc = Arc::clone(left);
                        let right_arc = Arc::clone(right);

                        let left_idx = expr_pool.len();
                        expr_pool.push(left_arc);
                        let right_idx = expr_pool.len();
                        expr_pool.push(right_arc);

                        work_stack.push(WorkItem::BuildArithmetic(op_clone, source_loc));
                        work_stack.push(WorkItem::Process(right_idx));
                        work_stack.push(WorkItem::Process(left_idx));
                    }
                    ExpressionKind::Comparison(left, op, right) => {
                        let op_clone = op.clone();
                        let left_arc = Arc::clone(left);
                        let right_arc = Arc::clone(right);

                        let left_idx = expr_pool.len();
                        expr_pool.push(left_arc);
                        let right_idx = expr_pool.len();
                        expr_pool.push(right_arc);

                        work_stack.push(WorkItem::BuildComparison(op_clone, source_loc));
                        work_stack.push(WorkItem::Process(right_idx));
                        work_stack.push(WorkItem::Process(left_idx));
                    }
                    ExpressionKind::LogicalAnd(left, right) => {
                        let left_arc = Arc::clone(left);
                        let right_arc = Arc::clone(right);

                        let left_idx = expr_pool.len();
                        expr_pool.push(left_arc);
                        let right_idx = expr_pool.len();
                        expr_pool.push(right_arc);

                        work_stack.push(WorkItem::BuildLogicalAnd(source_loc));
                        work_stack.push(WorkItem::Process(right_idx));
                        work_stack.push(WorkItem::Process(left_idx));
                    }
                    ExpressionKind::LogicalOr(left, right) => {
                        let left_arc = Arc::clone(left);
                        let right_arc = Arc::clone(right);

                        let left_idx = expr_pool.len();
                        expr_pool.push(left_arc);
                        let right_idx = expr_pool.len();
                        expr_pool.push(right_arc);

                        work_stack.push(WorkItem::BuildLogicalOr(source_loc));
                        work_stack.push(WorkItem::Process(right_idx));
                        work_stack.push(WorkItem::Process(left_idx));
                    }
                    ExpressionKind::LogicalNegation(inner, neg_type) => {
                        let neg_type_clone = neg_type.clone();
                        let inner_arc = Arc::clone(inner);
                        let inner_idx = expr_pool.len();
                        expr_pool.push(inner_arc);
                        work_stack.push(WorkItem::BuildLogicalNegation(neg_type_clone, source_loc));
                        work_stack.push(WorkItem::Process(inner_idx));
                    }
                    ExpressionKind::UnitConversion(inner, unit) => {
                        let unit_clone = unit.clone();
                        let inner_arc = Arc::clone(inner);
                        let inner_idx = expr_pool.len();
                        expr_pool.push(inner_arc);
                        work_stack.push(WorkItem::BuildUnitConversion(unit_clone, source_loc));
                        work_stack.push(WorkItem::Process(inner_idx));
                    }
                    ExpressionKind::MathematicalComputation(func, inner) => {
                        let func_clone = func.clone();
                        let inner_arc = Arc::clone(inner);
                        let inner_idx = expr_pool.len();
                        expr_pool.push(inner_arc);
                        work_stack.push(WorkItem::BuildMathematicalComputation(
                            func_clone, source_loc,
                        ));
                        work_stack.push(WorkItem::Process(inner_idx));
                    }
                    ExpressionKind::Literal(lit) => {
                        result_pool.push(Expression::new(
                            ExpressionKind::Literal(lit.clone()),
                            source_loc,
                        ));
                    }
                    ExpressionKind::FactPath(fact_path) => {
                        result_pool.push(Expression::new(
                            ExpressionKind::FactPath(fact_path.clone()),
                            source_loc,
                        ));
                    }
                    ExpressionKind::Veto(veto) => {
                        result_pool.push(Expression::new(
                            ExpressionKind::Veto(veto.clone()),
                            source_loc,
                        ));
                    }
                }
            }
            WorkItem::BuildArithmetic(op, source_loc) => {
                let right = result_pool.pop().unwrap_or_else(|| {
                    unreachable!(
                        "BUG: missing right expression for Arithmetic during inversion hydration"
                    )
                });
                let left = result_pool.pop().unwrap_or_else(|| {
                    unreachable!(
                        "BUG: missing left expression for Arithmetic during inversion hydration"
                    )
                });
                result_pool.push(Expression::new(
                    ExpressionKind::Arithmetic(Arc::new(left), op, Arc::new(right)),
                    source_loc,
                ));
            }
            WorkItem::BuildComparison(op, source_loc) => {
                let right = result_pool.pop().unwrap_or_else(|| {
                    unreachable!(
                        "BUG: missing right expression for Comparison during inversion hydration"
                    )
                });
                let left = result_pool.pop().unwrap_or_else(|| {
                    unreachable!(
                        "BUG: missing left expression for Comparison during inversion hydration"
                    )
                });
                result_pool.push(Expression::new(
                    ExpressionKind::Comparison(Arc::new(left), op, Arc::new(right)),
                    source_loc,
                ));
            }
            WorkItem::BuildLogicalAnd(source_loc) => {
                let right = result_pool.pop().unwrap_or_else(|| {
                    unreachable!(
                        "BUG: missing right expression for LogicalAnd during inversion hydration"
                    )
                });
                let left = result_pool.pop().unwrap_or_else(|| {
                    unreachable!(
                        "BUG: missing left expression for LogicalAnd during inversion hydration"
                    )
                });
                result_pool.push(Expression::new(
                    ExpressionKind::LogicalAnd(Arc::new(left), Arc::new(right)),
                    source_loc,
                ));
            }
            WorkItem::BuildLogicalOr(source_loc) => {
                let right = result_pool.pop().unwrap_or_else(|| {
                    unreachable!(
                        "BUG: missing right expression for LogicalOr during inversion hydration"
                    )
                });
                let left = result_pool.pop().unwrap_or_else(|| {
                    unreachable!(
                        "BUG: missing left expression for LogicalOr during inversion hydration"
                    )
                });
                result_pool.push(Expression::new(
                    ExpressionKind::LogicalOr(Arc::new(left), Arc::new(right)),
                    source_loc,
                ));
            }
            WorkItem::BuildLogicalNegation(neg_type, source_loc) => {
                let inner = result_pool
                    .pop()
                    .expect("Internal error: missing expression for LogicalNegation");
                result_pool.push(Expression::new(
                    ExpressionKind::LogicalNegation(Arc::new(inner), neg_type),
                    source_loc,
                ));
            }
            WorkItem::BuildUnitConversion(unit, source_loc) => {
                let inner = result_pool
                    .pop()
                    .expect("Internal error: missing expression for UnitConversion");
                result_pool.push(Expression::new(
                    ExpressionKind::UnitConversion(Arc::new(inner), unit),
                    source_loc,
                ));
            }
            WorkItem::BuildMathematicalComputation(func, source_loc) => {
                let inner = result_pool
                    .pop()
                    .expect("Internal error: missing expression for MathematicalComputation");
                result_pool.push(Expression::new(
                    ExpressionKind::MathematicalComputation(func, Arc::new(inner)),
                    source_loc,
                ));
            }
            WorkItem::PopVisitedRules => {
                visited_rules_stack.pop();
            }
        }
    }

    Ok(result_pool
        .pop()
        .unwrap_or_else(|| unreachable!("BUG: no result from substitution")))
}

// ============================================================================
// Fact hydration
// ============================================================================

/// Hydrate fact references in an expression with their known values
///
/// For each FactPath in the expression, if the fact is in provided_facts,
/// replaces the FactPath with a Literal containing the fact's value.
fn hydrate_facts_in_expression(
    expr: &Arc<Expression>,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<Expression> {
    enum WorkItem {
        Process(usize),
        BuildArithmetic(ArithmeticComputation, Source),
        BuildComparison(ComparisonComputation, Source),
        BuildLogicalAnd(Source),
        BuildLogicalOr(Source),
        BuildLogicalNegation(NegationType, Source),
        BuildUnitConversion(SemanticConversionTarget, Source),
        BuildMathematicalComputation(MathematicalComputation, Source),
    }

    let mut expr_pool: Vec<Arc<Expression>> = Vec::new();
    let mut work_stack: Vec<WorkItem> = Vec::new();
    let mut result_pool: Vec<Expression> = Vec::new();

    let root_idx = expr_pool.len();
    expr_pool.push(Arc::clone(expr));
    work_stack.push(WorkItem::Process(root_idx));

    while let Some(work) = work_stack.pop() {
        match work {
            WorkItem::Process(expr_idx) => {
                let (source_loc, expr_kind_ref) = {
                    let e = &expr_pool[expr_idx];
                    (e.source_location.clone(), &e.kind)
                };

                match expr_kind_ref {
                    ExpressionKind::FactPath(fact_path) => {
                        if provided_facts.contains(fact_path) {
                            if let Some(lit) = plan.facts.get(fact_path).and_then(|d| d.value()) {
                                result_pool.push(Expression::new(
                                    ExpressionKind::Literal(Box::new(lit.clone())),
                                    source_loc,
                                ));
                                continue;
                            }
                        }
                        result_pool.push(Expression::new(
                            ExpressionKind::FactPath(fact_path.clone()),
                            source_loc,
                        ));
                    }
                    ExpressionKind::Arithmetic(left, op, right) => {
                        let op_clone = op.clone();
                        let left_arc = Arc::clone(left);
                        let right_arc = Arc::clone(right);

                        let left_idx = expr_pool.len();
                        expr_pool.push(left_arc);
                        let right_idx = expr_pool.len();
                        expr_pool.push(right_arc);

                        work_stack.push(WorkItem::BuildArithmetic(op_clone, source_loc));
                        work_stack.push(WorkItem::Process(right_idx));
                        work_stack.push(WorkItem::Process(left_idx));
                    }
                    ExpressionKind::Comparison(left, op, right) => {
                        let op_clone = op.clone();
                        let left_arc = Arc::clone(left);
                        let right_arc = Arc::clone(right);

                        let left_idx = expr_pool.len();
                        expr_pool.push(left_arc);
                        let right_idx = expr_pool.len();
                        expr_pool.push(right_arc);

                        work_stack.push(WorkItem::BuildComparison(op_clone, source_loc));
                        work_stack.push(WorkItem::Process(right_idx));
                        work_stack.push(WorkItem::Process(left_idx));
                    }
                    ExpressionKind::LogicalAnd(left, right) => {
                        let left_arc = Arc::clone(left);
                        let right_arc = Arc::clone(right);

                        let left_idx = expr_pool.len();
                        expr_pool.push(left_arc);
                        let right_idx = expr_pool.len();
                        expr_pool.push(right_arc);

                        work_stack.push(WorkItem::BuildLogicalAnd(source_loc));
                        work_stack.push(WorkItem::Process(right_idx));
                        work_stack.push(WorkItem::Process(left_idx));
                    }
                    ExpressionKind::LogicalOr(left, right) => {
                        let left_arc = Arc::clone(left);
                        let right_arc = Arc::clone(right);

                        let left_idx = expr_pool.len();
                        expr_pool.push(left_arc);
                        let right_idx = expr_pool.len();
                        expr_pool.push(right_arc);

                        work_stack.push(WorkItem::BuildLogicalOr(source_loc));
                        work_stack.push(WorkItem::Process(right_idx));
                        work_stack.push(WorkItem::Process(left_idx));
                    }
                    ExpressionKind::LogicalNegation(inner, neg_type) => {
                        let neg_type_clone = neg_type.clone();
                        let inner_arc = Arc::clone(inner);
                        let inner_idx = expr_pool.len();
                        expr_pool.push(inner_arc);
                        work_stack.push(WorkItem::BuildLogicalNegation(neg_type_clone, source_loc));
                        work_stack.push(WorkItem::Process(inner_idx));
                    }
                    ExpressionKind::UnitConversion(inner, unit) => {
                        let unit_clone = unit.clone();
                        let inner_arc = Arc::clone(inner);
                        let inner_idx = expr_pool.len();
                        expr_pool.push(inner_arc);
                        work_stack.push(WorkItem::BuildUnitConversion(unit_clone, source_loc));
                        work_stack.push(WorkItem::Process(inner_idx));
                    }
                    ExpressionKind::MathematicalComputation(func, inner) => {
                        let func_clone = func.clone();
                        let inner_arc = Arc::clone(inner);
                        let inner_idx = expr_pool.len();
                        expr_pool.push(inner_arc);
                        work_stack.push(WorkItem::BuildMathematicalComputation(
                            func_clone, source_loc,
                        ));
                        work_stack.push(WorkItem::Process(inner_idx));
                    }
                    ExpressionKind::Literal(lit) => {
                        result_pool.push(Expression::new(
                            ExpressionKind::Literal(lit.clone()),
                            source_loc,
                        ));
                    }
                    ExpressionKind::RulePath(rule_path) => {
                        result_pool.push(Expression::new(
                            ExpressionKind::RulePath(rule_path.clone()),
                            source_loc,
                        ));
                    }
                    ExpressionKind::Veto(veto) => {
                        result_pool.push(Expression::new(
                            ExpressionKind::Veto(veto.clone()),
                            source_loc,
                        ));
                    }
                }
            }
            WorkItem::BuildArithmetic(op, source_loc) => {
                let right = result_pool.pop().unwrap_or_else(|| {
                    unreachable!("BUG: missing right expression for Arithmetic")
                });
                let left = result_pool
                    .pop()
                    .unwrap_or_else(|| unreachable!("BUG: missing left expression for Arithmetic"));
                result_pool.push(Expression::new(
                    ExpressionKind::Arithmetic(Arc::new(left), op, Arc::new(right)),
                    source_loc,
                ));
            }
            WorkItem::BuildComparison(op, source_loc) => {
                let right = result_pool.pop().unwrap_or_else(|| {
                    unreachable!("BUG: missing right expression for Comparison")
                });
                let left = result_pool
                    .pop()
                    .unwrap_or_else(|| unreachable!("BUG: missing left expression for Comparison"));
                result_pool.push(Expression::new(
                    ExpressionKind::Comparison(Arc::new(left), op, Arc::new(right)),
                    source_loc,
                ));
            }
            WorkItem::BuildLogicalAnd(source_loc) => {
                let right = result_pool.pop().unwrap_or_else(|| {
                    unreachable!("BUG: missing right expression for LogicalAnd")
                });
                let left = result_pool
                    .pop()
                    .unwrap_or_else(|| unreachable!("BUG: missing left expression for LogicalAnd"));
                result_pool.push(Expression::new(
                    ExpressionKind::LogicalAnd(Arc::new(left), Arc::new(right)),
                    source_loc,
                ));
            }
            WorkItem::BuildLogicalOr(source_loc) => {
                let right = result_pool
                    .pop()
                    .unwrap_or_else(|| unreachable!("BUG: missing right expression for LogicalOr"));
                let left = result_pool
                    .pop()
                    .unwrap_or_else(|| unreachable!("BUG: missing left expression for LogicalOr"));
                result_pool.push(Expression::new(
                    ExpressionKind::LogicalOr(Arc::new(left), Arc::new(right)),
                    source_loc,
                ));
            }
            WorkItem::BuildLogicalNegation(neg_type, source_loc) => {
                let inner = result_pool
                    .pop()
                    .expect("Internal error: missing expression for LogicalNegation");
                result_pool.push(Expression::new(
                    ExpressionKind::LogicalNegation(Arc::new(inner), neg_type),
                    source_loc,
                ));
            }
            WorkItem::BuildUnitConversion(unit, source_loc) => {
                let inner = result_pool
                    .pop()
                    .expect("Internal error: missing expression for UnitConversion");
                result_pool.push(Expression::new(
                    ExpressionKind::UnitConversion(Arc::new(inner), unit),
                    source_loc,
                ));
            }
            WorkItem::BuildMathematicalComputation(func, source_loc) => {
                let inner = result_pool
                    .pop()
                    .expect("Internal error: missing expression for MathematicalComputation");
                result_pool.push(Expression::new(
                    ExpressionKind::MathematicalComputation(func, Arc::new(inner)),
                    source_loc,
                ));
            }
        }
    }

    Ok(result_pool
        .pop()
        .expect("Internal error: no result from hydration"))
}

// ============================================================================
// Constant folding
// ============================================================================

/// Extract an outcome (value or veto) from an expression
fn extract_outcome(expr: &Expression) -> Option<OperationResult> {
    match &expr.kind {
        ExpressionKind::Literal(lit) => {
            Some(OperationResult::Value(Box::new(lit.as_ref().clone())))
        }
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
            outcome: OperationResult::Value(Box::new(LiteralValue::from_bool(true))),
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
            outcome: OperationResult::Value(Box::new(LiteralValue::from_bool(false))),
        }]
    } else {
        vec![]
    };

    Ok((true_solutions, false_solutions))
}

/// Attempt constant folding on an expression (simplified version for outcomes)
pub(crate) fn try_constant_fold_expression(expr: &Expression) -> Option<Expression> {
    match &expr.kind {
        ExpressionKind::Literal(_) => Some(expr.clone()),
        ExpressionKind::Arithmetic(left, op, right) => {
            let left_folded = try_constant_fold_expression(left).unwrap_or((**left).clone());
            let right_folded = try_constant_fold_expression(right).unwrap_or((**right).clone());
            if let (ExpressionKind::Literal(ref left_val), ExpressionKind::Literal(ref right_val)) =
                (&left_folded.kind, &right_folded.kind)
            {
                if let Some(result) = evaluate_arithmetic(left_val.as_ref(), op, right_val.as_ref())
                {
                    return Some(Expression::new(
                        ExpressionKind::Literal(Box::new(result)),
                        expr.source_location.clone(),
                    ));
                }
            }
            Some(Expression::new(
                ExpressionKind::Arithmetic(
                    Arc::new(left_folded),
                    op.clone(),
                    Arc::new(right_folded),
                ),
                expr.source_location.clone(),
            ))
        }
        ExpressionKind::Comparison(left, op, right) => {
            let left_folded = try_constant_fold_expression(left).unwrap_or((**left).clone());
            let right_folded = try_constant_fold_expression(right).unwrap_or((**right).clone());
            if let (ExpressionKind::Literal(ref left_val), ExpressionKind::Literal(ref right_val)) =
                (&left_folded.kind, &right_folded.kind)
            {
                if let Some(result) = evaluate_comparison(left_val.as_ref(), op, right_val.as_ref())
                {
                    return Some(Expression::new(
                        ExpressionKind::Literal(Box::new(LiteralValue::from_bool(result))),
                        expr.source_location.clone(),
                    ));
                }
            }
            Some(Expression::new(
                ExpressionKind::Comparison(
                    Arc::new(left_folded),
                    op.clone(),
                    Arc::new(right_folded),
                ),
                expr.source_location.clone(),
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
    use crate::computation::arithmetic_operation;

    match arithmetic_operation(left, op, right) {
        OperationResult::Value(lit) => Some(lit.as_ref().clone()),
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
) -> Option<bool> {
    use crate::computation::comparison_operation;
    use crate::planning::semantics::ValueKind;

    match comparison_operation(left, op, right) {
        OperationResult::Value(lit) => match &lit.value {
            ValueKind::Boolean(b) => Some(*b),
            _ => None,
        },
        _ => None,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::semantics::ValueKind;
    use rust_decimal::Decimal;

    fn literal_expr(val: LiteralValue) -> Expression {
        Expression::new(
            ExpressionKind::Literal(Box::new(val)),
            crate::inversion::synthetic_source(),
        )
    }

    fn fact_expr(name: &str) -> Expression {
        Expression::new(
            ExpressionKind::FactPath(FactPath::new(vec![], name.to_string())),
            crate::inversion::synthetic_source(),
        )
    }

    fn num(n: i64) -> LiteralValue {
        LiteralValue::number(Decimal::from(n))
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

    fn empty_plan() -> ExecutionPlan {
        ExecutionPlan {
            doc_name: "test".to_string(),
            facts: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        }
    }

    #[test]
    fn test_hydrate_literal_unchanged() {
        let plan = empty_plan();
        let provided: HashSet<FactPath> = HashSet::new();

        let expr = literal_expr(num(42));
        let result = hydrate_facts_in_expression(&Arc::new(expr), &plan, &provided).unwrap();

        if let ExpressionKind::Literal(lit) = &result.kind {
            assert!(matches!(&lit.value, ValueKind::Number(_)));
        } else {
            panic!("Expected literal number");
        }
    }

    #[test]
    fn test_hydrate_fact_not_provided() {
        let plan = empty_plan();
        let provided: HashSet<FactPath> = HashSet::new();

        let expr = fact_expr("age");
        let result = hydrate_facts_in_expression(&Arc::new(expr), &plan, &provided).unwrap();

        assert!(matches!(result.kind, ExpressionKind::FactPath(_)));
    }

    #[test]
    fn test_constant_fold_arithmetic() {
        let left = literal_expr(num(10));
        let right = literal_expr(num(5));
        let expr = Expression::new(
            ExpressionKind::Arithmetic(Arc::new(left), ArithmeticComputation::Add, Arc::new(right)),
            crate::inversion::synthetic_source(),
        );

        let folded = try_constant_fold_expression(&expr).unwrap();

        if let ExpressionKind::Literal(lit) = &folded.kind {
            if let ValueKind::Number(n) = &lit.value {
                assert_eq!(*n, Decimal::from(15));
            } else {
                panic!("Expected literal number");
            }
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
                Arc::new(left),
                ComparisonComputation::GreaterThan,
                Arc::new(right),
            ),
            crate::inversion::synthetic_source(),
        );

        let folded = try_constant_fold_expression(&expr).unwrap();

        if let ExpressionKind::Literal(lit) = &folded.kind {
            if let ValueKind::Boolean(b) = &lit.value {
                assert!(*b);
            } else {
                panic!("Expected literal boolean");
            }
        } else {
            panic!("Expected literal boolean");
        }
    }
}
