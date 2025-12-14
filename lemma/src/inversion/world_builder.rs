//! On-demand world building for inversion queries
//!
//! Builds "worlds" from rules - each world represents one consistent universe
//! with constraints and a value expression.

use super::World;
use crate::algebra::constraints::{extract_constraints, ConstraintSet, FactConstraint};
use crate::computation::OperationResult;
use crate::planning::ExecutionPlan;
use crate::semantic::{BooleanValue, Expression, ExpressionKind, FactPath, LiteralValue, RulePath};
use crate::{LemmaError, LemmaResult};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub struct WorldBuilder<'a> {
    /// Pre-reduced execution plan (evaluate_symbolic already called)
    plan: &'a ExecutionPlan,
    /// Cache to avoid rebuilding same rule's worlds
    cache: HashMap<RulePath, Vec<World>>,
}

impl<'a> WorldBuilder<'a> {
    /// Create WorldBuilder with pre-reduced and optimized plan
    ///
    /// The plan should have been:
    /// 1. Injected with known facts via with_typed_values()
    /// 2. Symbolically evaluated via evaluate_symbolic()
    /// 3. Optimized via optimize() for DNF structure
    pub fn new(plan: &'a ExecutionPlan) -> Self {
        Self {
            plan,
            cache: HashMap::new(),
        }
    }

    /// Build worlds for a rule (lazy, on-demand)
    ///
    /// Branches have already been symbolically evaluated and pruned.
    /// This extracts constraints and builds worlds from simplified branches.
    pub fn build_worlds(&mut self, rule_name: &str) -> LemmaResult<Vec<World>> {
        let rule = self
            .plan
            .get_rule(rule_name)
            .ok_or_else(|| LemmaError::Engine(format!("Rule not found: {}", rule_name)))?;
        let rule_path = rule.path.clone();

        // Check cache
        if let Some(cached) = self.cache.get(&rule_path) {
            return Ok(cached.clone());
        }

        let mut worlds = Vec::new();

        for branch in &rule.branches {
            // Branch already symbolically evaluated - use optimized_condition if available
            let condition = branch
                .optimized_condition
                .as_ref()
                .unwrap_or(&branch.condition);
            let result = &branch.result;

            // If condition is literal true, result applies unconditionally
            if matches!(
                &condition.kind,
                ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True))
            ) {
                worlds.push(World {
                    constraints: HashMap::new(),
                    value: result.clone(),
                });
                continue;
            }

            // Skip branches with false conditions (should have been pruned, but check anyway)
            if matches!(
                &condition.kind,
                ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False))
            ) {
                continue;
            }

            // Handle OR expressions: create separate world for each OR branch
            // After DNF expansion, OR expressions represent alternative branches
            let or_branches = flatten_or_branches_for_worlds(condition);

            for or_branch in or_branches {
                // Check if condition contains rule references
                let condition_rule_refs = extract_rule_references(&or_branch);
                for rule_ref in &condition_rule_refs {
                }
                
                if condition_rule_refs.is_empty() {
                    // Simple case: no rule references in condition
                    // Extract constraints from this OR branch
                    let mut constraint_set = ConstraintSet::new();
                    extract_constraints(&or_branch, &mut constraint_set);
                    let constraints = constraint_set.to_fact_constraints();
                    for (fact, constraint) in &constraints {
                    }

                    // Check if result references other rules
                    let result_rule_refs = extract_rule_references(result);

                    if result_rule_refs.is_empty() {
                        // Simple case: no rule dependencies in result
                        worlds.push(World {
                            constraints,
                            value: result.clone(),
                        });
                    } else {
                        // Complex case: recursively build referenced rule worlds
                        let branch_worlds =
                            self.build_with_references(constraints, result, &result_rule_refs)?;
                        worlds.extend(branch_worlds);
                    }
                } else {
                    // Condition has rule references - need to expand them
                    // Build worlds for each referenced rule and substitute
                    let condition_worlds = self.build_condition_with_references(
                        &or_branch,
                        &condition_rule_refs,
                    )?;
                    
                    // For each expanded condition world, create a result world
                    for condition_world in condition_worlds {
                        // Merge condition constraints
                        let constraints = condition_world.constraints.clone();
                        for (fact, constraint) in &constraints {
                        }
                        
                        // Check if result references other rules
                        let result_rule_refs = extract_rule_references(result);
                        
                        if result_rule_refs.is_empty() {
                            worlds.push(World {
                                constraints,
                                value: result.clone(),
                            });
                        } else {
                            // Both condition and result have rule references
                            let branch_worlds = self.build_with_references(
                                constraints,
                                result,
                                &result_rule_refs,
                            )?;
                            worlds.extend(branch_worlds);
                        }
                    }
                }
            }
        }

        // Cache for future queries
        self.cache.insert(rule_path, worlds.clone());
        Ok(worlds)
    }

    /// Build worlds with rule references (cross-product with pruning)
    fn build_with_references(
        &mut self,
        base_constraints: HashMap<FactPath, FactConstraint>,
        result: &Expression,
        rule_refs: &[RuleReference],
    ) -> LemmaResult<Vec<World>> {
        let mut worlds = vec![World {
            constraints: base_constraints,
            value: result.clone(),
        }];

        // For each referenced rule, cross-product merge
        for rule_ref in rule_refs {
            // Recursively build worlds for referenced rule
            let ref_worlds = self.build_worlds(&rule_ref.path.rule)?;

            // Cross-product merge with pruning
            let mut new_worlds = Vec::new();
            for base_world in &worlds {
                for ref_world in &ref_worlds {
                    // Merge constraints; returns None if contradiction
                    if let Some(merged) = base_world.merge(ref_world, |base_val, ref_val| {
                        // Substitute rule reference in base_val with ref_val
                        substitute_rule_path(base_val, &rule_ref.path, ref_val)
                    }) {
                        new_worlds.push(merged);
                    }
                    // Contradictions are auto-pruned (merge returns None)
                }
            }
            worlds = new_worlds;
        }

        Ok(worlds)
    }

    /// Build worlds for a condition that contains rule references
    /// Expands rule references and extracts constraints from the expanded condition
    fn build_condition_with_references(
        &mut self,
        condition: &Expression,
        rule_refs: &[RuleReference],
    ) -> LemmaResult<Vec<World>> {
        // Start with empty constraints
        let mut worlds = vec![World {
            constraints: HashMap::new(),
            value: condition.clone(), // Placeholder - we'll extract constraints from this
        }];

        // For each referenced rule, cross-product merge
        for rule_ref in rule_refs {
            // Recursively build worlds for referenced rule
            let ref_worlds = self.build_worlds(&rule_ref.path.rule)?;
            for (i, ref_world) in ref_worlds.iter().enumerate() {
                for (fact, constraint) in &ref_world.constraints {
                }
            }

            // Cross-product merge with pruning
            let mut new_worlds = Vec::new();
            for base_world in &worlds {
                for ref_world in &ref_worlds {
                    // Substitute rule reference in condition with ref_world.value
                    let expanded_condition = substitute_rule_path(
                        &base_world.value,
                        &rule_ref.path,
                        &ref_world.value,
                    );

                    // Check if expanded condition is satisfied
                    // Evaluate the expression after substitution to see if it's true/false
                    let condition_eval = evaluate_condition_after_substitution(&expanded_condition);
                    
                    if condition_eval == Some(false) {
                        // Condition not satisfied - skip this world
                        continue;
                    }

                    // Merge constraints
                    let mut constraints = base_world.constraints.clone();
                    let mut has_contradiction = false;
                    
                    match condition_eval {
                        Some(true) => {
                            // Condition fully satisfied - merge ref_world constraints
                            // (these come from the rule that produced the matching value)
                            for (fact, constraint) in &ref_world.constraints {
                                if let Some(existing) = constraints.get(fact) {
                                    let intersection = existing.intersect(constraint);
                                    if intersection.is_satisfiable() {
                                        constraints.insert(fact.clone(), intersection);
                                    } else {
                                        has_contradiction = true;
                                        break;
                                    }
                                } else {
                                    constraints.insert(fact.clone(), constraint.clone());
                                }
                            }
                        }
                        None => {
                            // Condition still has unknowns - extract constraints from expanded condition
                            let mut constraint_set = ConstraintSet::new();
                            extract_constraints(&expanded_condition, &mut constraint_set);
                            let extracted_constraints = constraint_set.to_fact_constraints();

                            // Merge extracted constraints
                            for (fact, constraint) in extracted_constraints {
                                if let Some(existing) = constraints.get(&fact) {
                                    let intersection = existing.intersect(&constraint);
                                    if intersection.is_satisfiable() {
                                        constraints.insert(fact, intersection);
                                    } else {
                                        has_contradiction = true;
                                        break;
                                    }
                                } else {
                                    constraints.insert(fact, constraint);
                                }
                            }

                            // Also merge ref_world constraints
                            for (fact, constraint) in &ref_world.constraints {
                                if let Some(existing) = constraints.get(fact) {
                                    let intersection = existing.intersect(constraint);
                                    if intersection.is_satisfiable() {
                                        constraints.insert(fact.clone(), intersection);
                                    } else {
                                        has_contradiction = true;
                                        break;
                                    }
                                } else {
                                    constraints.insert(fact.clone(), constraint.clone());
                                }
                            }
                        }
                        Some(false) => {
                            // Should have been skipped above, but handle it anyway
                            continue;
                        }
                    }

                    if has_contradiction {
                        continue;
                    }

                    for (fact, constraint) in &constraints {
                    }

                    new_worlds.push(World {
                        constraints,
                        value: expanded_condition, // Store expanded condition for potential further expansion
                    });
                }
            }
            worlds = new_worlds;
        }

        Ok(worlds)
    }
}

// Helper structures

struct RuleReference {
    path: RulePath,
}

// Helper functions

/// Evaluate a condition expression after rule reference substitution
/// Returns Some(true/false) if the expression can be fully evaluated,
/// None if it still contains unknowns
fn evaluate_condition_after_substitution(expr: &Expression) -> Option<bool> {
    match &expr.kind {
        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)) => Some(true),
        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False)) => Some(false),
        ExpressionKind::Comparison(left, op, right) => {
            // Evaluate comparison: left op right
            match (&left.kind, &right.kind) {
                (ExpressionKind::Literal(left_val), ExpressionKind::Literal(right_val)) => {
                    use crate::computation::comparison::comparison_operation;
                    match comparison_operation(left_val, op, right_val) {
                        OperationResult::Value(LiteralValue::Boolean(BooleanValue::True)) => Some(true),
                        OperationResult::Value(LiteralValue::Boolean(BooleanValue::False)) => Some(false),
                        _ => None,
                    }
                }
                _ => None, // Still has unknowns
            }
        }
        ExpressionKind::LogicalAnd(left, right) => {
            let left_val = evaluate_condition_after_substitution(left)?;
            let right_val = evaluate_condition_after_substitution(right)?;
            Some(left_val && right_val)
        }
        ExpressionKind::LogicalOr(left, right) => {
            let left_val = evaluate_condition_after_substitution(left)?;
            let right_val = evaluate_condition_after_substitution(right)?;
            Some(left_val || right_val)
        }
        ExpressionKind::LogicalNegation(inner, _) => {
            let inner_val = evaluate_condition_after_substitution(inner)?;
            Some(!inner_val)
        }
        _ => None, // Complex expression - can't fully evaluate
    }
}

/// Flatten OR expression into list of branches for world building
/// After DNF expansion, OR expressions represent alternative branches
/// Each branch should get its own world
fn flatten_or_branches_for_worlds(expr: &Expression) -> Vec<Expression> {
    match &expr.kind {
        ExpressionKind::LogicalOr(left, right) => {
            let mut branches = flatten_or_branches_for_worlds(left);
            branches.extend(flatten_or_branches_for_worlds(right));
            branches
        }
        _ => vec![expr.clone()],
    }
}

/// Extract all rule references from an expression
fn extract_rule_references(expr: &Expression) -> Vec<RuleReference> {
    let paths = collect_rule_paths(expr);
    paths
        .into_iter()
        .map(|path| RuleReference { path })
        .collect()
}

/// Collect all rule paths from an expression (similar to algebra::isolation::collect_facts)
fn collect_rule_paths(expr: &Expression) -> HashSet<RulePath> {
    let mut paths = HashSet::new();
    collect_rule_paths_recursive(expr, &mut paths);
    paths
}

fn collect_rule_paths_recursive(expr: &Expression, paths: &mut HashSet<RulePath>) {
    match &expr.kind {
        ExpressionKind::RulePath(path) => {
            paths.insert(path.clone());
        }
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right) => {
            collect_rule_paths_recursive(left, paths);
            collect_rule_paths_recursive(right, paths);
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner)
        | ExpressionKind::UnitConversion(inner, _) => {
            collect_rule_paths_recursive(inner, paths);
        }
        _ => {}
    }
}

/// Substitute a rule path with its value expression
fn substitute_rule_path(
    expr: &Expression,
    target: &RulePath,
    replacement: &Expression,
) -> Expression {
    match &expr.kind {
        ExpressionKind::RulePath(path) if path == target => replacement.clone(),
        ExpressionKind::Arithmetic(left, op, right) => Expression::new(
            ExpressionKind::Arithmetic(
                Arc::new(substitute_rule_path(left, target, replacement)),
                op.clone(),
                Arc::new(substitute_rule_path(right, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::Comparison(left, op, right) => Expression::new(
            ExpressionKind::Comparison(
                Arc::new(substitute_rule_path(left, target, replacement)),
                op.clone(),
                Arc::new(substitute_rule_path(right, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::LogicalAnd(left, right) => Expression::new(
            ExpressionKind::LogicalAnd(
                Arc::new(substitute_rule_path(left, target, replacement)),
                Arc::new(substitute_rule_path(right, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::LogicalOr(left, right) => Expression::new(
            ExpressionKind::LogicalOr(
                Arc::new(substitute_rule_path(left, target, replacement)),
                Arc::new(substitute_rule_path(right, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::LogicalNegation(inner, style) => Expression::new(
            ExpressionKind::LogicalNegation(
                Arc::new(substitute_rule_path(inner, target, replacement)),
                style.clone(),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::MathematicalComputation(op, inner) => Expression::new(
            ExpressionKind::MathematicalComputation(
                op.clone(),
                Arc::new(substitute_rule_path(inner, target, replacement)),
            ),
            expr.source.clone(),
        ),
        ExpressionKind::UnitConversion(inner, target_unit) => Expression::new(
            ExpressionKind::UnitConversion(
                Arc::new(substitute_rule_path(inner, target, replacement)),
                target_unit.clone(),
            ),
            expr.source.clone(),
        ),
        _ => expr.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inversion::invert;
    use crate::planning::{Branch, ExecutableRule};
    use crate::semantic::{
        BooleanValue, Expression, ExpressionKind, FactPath, LiteralValue, RulePath,
    };
    use std::collections::HashMap;
    use std::sync::Arc;

    fn create_test_plan_with_simplified_condition() -> ExecutionPlan {
        // Create a plan with a rule that has a simplified condition:
        // discount_code is "SAVE30" and (tag1 is "yes" or tag2 is "yes")
        // This simulates the optimized condition after simplification

        let discount_code_path = FactPath::local("discount_code".to_string());
        let tag1_path = FactPath::local("tag1".to_string());
        let tag2_path = FactPath::local("tag2".to_string());

        let save30 = LiteralValue::Text("SAVE30".to_string());
        let yes = LiteralValue::Text("yes".to_string());

        // Simplified condition: discount_code is "SAVE30" and (tag1 is "yes" or tag2 is "yes")
        let discount_expr = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(discount_code_path.clone()),
                    None,
                )),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(Expression::new(
                    ExpressionKind::Literal(save30.clone()),
                    None,
                )),
            ),
            None,
        );

        let tag1_expr = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(tag1_path.clone()),
                    None,
                )),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(Expression::new(ExpressionKind::Literal(yes.clone()), None)),
            ),
            None,
        );

        let tag2_expr = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(tag2_path.clone()),
                    None,
                )),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(Expression::new(ExpressionKind::Literal(yes.clone()), None)),
            ),
            None,
        );

        let or_expr = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(tag1_expr), Arc::new(tag2_expr)),
            None,
        );

        let simplified_condition = Expression::new(
            ExpressionKind::LogicalAnd(Arc::new(discount_expr), Arc::new(or_expr)),
            None,
        );

        let result_expr = Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(rust_decimal::Decimal::from(1))),
            None,
        );

        let rule = ExecutableRule {
            path: RulePath {
                segments: vec![],
                rule: "target".to_string(),
            },
            name: "target".to_string(),
            branches: vec![Branch {
                condition: Expression::new(
                    ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
                    None,
                ),
                optimized_condition: Some(simplified_condition),
                result: result_expr,
                source: None,
            }],
            needs_facts: {
                let mut set = std::collections::HashSet::new();
                set.insert(discount_code_path.clone());
                set.insert(tag1_path.clone());
                set.insert(tag2_path.clone());
                set
            },
            source: None,
        };

        ExecutionPlan::new(
            "test_doc".to_string(),
            HashMap::new(),
            vec![rule],
            crate::planning::graph::Graph::empty(),
        )
    }

    #[test]
    fn test_world_builder_extracts_discount_code_constraint() {
        // Test that WorldBuilder correctly extracts discount_code constraint
        // from a simplified condition that contains AND with OR
        let plan = create_test_plan_with_simplified_condition();
        let mut builder = WorldBuilder::new(&plan);

        let worlds = builder
            .build_worlds("target")
            .expect("build_worlds should succeed");

        assert!(!worlds.is_empty(), "Should have at least one world");

        let discount_code_path = FactPath::local("discount_code".to_string());
        let save30 = LiteralValue::Text("SAVE30".to_string());

        // Verify all worlds have discount_code constraint
        for (idx, world) in worlds.iter().enumerate() {
            let discount_constraint = world.constraints.get(&discount_code_path).expect(&format!(
                "World {} should have discount_code constraint",
                idx
            ));

            match discount_constraint {
                FactConstraint::Enumeration(values) => {
                    assert!(
                        values.contains(&save30),
                        "World {}: discount_code should be constrained to 'SAVE30', got {:?}",
                        idx,
                        values
                    );
                }
                FactConstraint::Unconstrained => {
                    panic!("World {}: discount_code should not be Unconstrained", idx);
                }
                other => {
                    panic!(
                        "World {}: discount_code should be Enumeration, got {:?}",
                        idx, other
                    );
                }
            }
        }
    }

    #[test]
    fn test_world_builder_handles_or_in_condition() {
        // Test that WorldBuilder correctly handles OR expressions in conditions
        // The OR part should be preserved but not extracted as direct constraints
        let plan = create_test_plan_with_simplified_condition();
        let mut builder = WorldBuilder::new(&plan);

        let worlds = builder
            .build_worlds("target")
            .expect("build_worlds should succeed");

        assert!(!worlds.is_empty(), "Should have at least one world");

        // Verify tag constraints are NOT directly extracted (OR is symbolic)
        // But the world value should contain the OR expression
        for (idx, world) in worlds.iter().enumerate() {
            // Tags should not have direct constraints (OR is handled symbolically)
            // This is expected behavior - OR expressions are preserved in the value
            // but not extracted as direct fact constraints

            // The world value should contain the OR expression
            // We can't easily check this without more complex expression matching,
            // but we can verify the world structure is valid
            assert!(
                !world.constraints.is_empty(),
                "World {} should have some constraints",
                idx
            );

            // Verify discount_code is present (from AND part)
            let discount_code_path = FactPath::local("discount_code".to_string());
            assert!(
                world.constraints.contains_key(&discount_code_path),
                "World {} should have discount_code constraint",
                idx
            );
        }
    }

    #[test]
    fn test_world_builder_with_unless_clause_simplification() {
        // Test the full flow: rule with unless clause → symbolic eval → optimize → world building
        // This mimics the bdd_partial_simplification integration test scenario

        use crate::evaluation::Evaluator;
        use crate::semantic::{BooleanValue, Expression, ExpressionKind, LiteralValue};
        use std::sync::Arc;

        let discount_code_path = FactPath::local("discount_code".to_string());
        let member_level_path = FactPath::local("member_level".to_string());
        let tag1_path = FactPath::local("tag1".to_string());

        let save30 = LiteralValue::Text("SAVE30".to_string());
        let platinum = LiteralValue::Text("platinum".to_string());
        let yes = LiteralValue::Text("yes".to_string());

        // Create the unless condition:
        // ((discount_code is "SAVE30" and member_level is "platinum")
        //  or (discount_code is "SAVE30" and not (member_level is "platinum")))
        //  and tag1 is "yes"
        let discount_eq = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(discount_code_path.clone()),
                    None,
                )),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(Expression::new(
                    ExpressionKind::Literal(save30.clone()),
                    None,
                )),
            ),
            None,
        );

        let member_eq = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(member_level_path.clone()),
                    None,
                )),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(Expression::new(
                    ExpressionKind::Literal(platinum.clone()),
                    None,
                )),
            ),
            None,
        );

        let member_neq = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(member_level_path.clone()),
                    None,
                )),
                crate::semantic::ComparisonComputation::NotEqual(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(Expression::new(
                    ExpressionKind::Literal(platinum.clone()),
                    None,
                )),
            ),
            None,
        );

        let tag1_eq = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(tag1_path.clone()),
                    None,
                )),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(Expression::new(ExpressionKind::Literal(yes.clone()), None)),
            ),
            None,
        );

        // (discount_code is "SAVE30" and member_level is "platinum")
        let branch1 = Expression::new(
            ExpressionKind::LogicalAnd(Arc::new(discount_eq.clone()), Arc::new(member_eq)),
            None,
        );

        // (discount_code is "SAVE30" and not (member_level is "platinum"))
        let branch2 = Expression::new(
            ExpressionKind::LogicalAnd(Arc::new(discount_eq.clone()), Arc::new(member_neq)),
            None,
        );

        // (branch1 or branch2)
        let or_expr = Expression::new(
            ExpressionKind::LogicalOr(Arc::new(branch1), Arc::new(branch2)),
            None,
        );

        // (or_expr and tag1 is "yes")
        let unless_condition = Expression::new(
            ExpressionKind::LogicalAnd(Arc::new(or_expr), Arc::new(tag1_eq)),
            None,
        );

        // Create rule: target = 0 unless (condition) then 1
        let default_result = Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(rust_decimal::Decimal::from(0))),
            None,
        );

        let unless_result = Expression::new(
            ExpressionKind::Literal(LiteralValue::Number(rust_decimal::Decimal::from(1))),
            None,
        );

        let rule = ExecutableRule {
            path: RulePath {
                segments: vec![],
                rule: "target".to_string(),
            },
            name: "target".to_string(),
            branches: vec![
                // Default branch: condition = true, result = 0
                Branch {
                    condition: Expression::new(
                        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
                        None,
                    ),
                    optimized_condition: None,
                    result: default_result,
                    source: None,
                },
                // Unless branch: condition = unless_condition, result = 1
                Branch {
                    condition: unless_condition.clone(),
                    optimized_condition: None,
                    result: unless_result,
                    source: None,
                },
            ],
            needs_facts: {
                let mut set = std::collections::HashSet::new();
                set.insert(discount_code_path.clone());
                set.insert(member_level_path.clone());
                set.insert(tag1_path.clone());
                set
            },
            source: None,
        };

        let plan = ExecutionPlan::new(
            "test_doc".to_string(),
            HashMap::new(),
            vec![rule],
            crate::planning::graph::Graph::empty(),
        );

        // Debug: Check original condition
        let original_rule = plan.get_rule("target").expect("Rule should exist");
        let original_unless_branch = original_rule
            .branches
            .get(1)
            .expect("Unless branch should exist");

        // Step 1: Apply symbolic evaluation
        let evaluator = Evaluator;
        let reduced_plan = evaluator.evaluate_symbolic(&plan);

        // Debug: Check what the condition looks like after symbolic evaluation
        let reduced_rule = reduced_plan.get_rule("target").expect("Rule should exist");
        let reduced_unless_branch = reduced_rule
            .branches
            .get(1)
            .expect("Unless branch should exist");

        // Step 2: Apply optimization (this should simplify the unless condition)
        let optimized_plan = reduced_plan.optimize();

        // Step 3: Build worlds
        let mut builder = WorldBuilder::new(&optimized_plan);
        let worlds = builder
            .build_worlds("target")
            .expect("build_worlds should succeed");

        // Find worlds where result = 1 (the unless branch)
        let unless_worlds: Vec<&World> = worlds.iter()
            .filter(|w| matches!(&w.value.kind, ExpressionKind::Literal(LiteralValue::Number(n)) if *n == rust_decimal::Decimal::from(1)))
            .collect();

        assert!(
            !unless_worlds.is_empty(),
            "Should have at least one world with result = 1"
        );

        // Verify all unless worlds have discount_code constraint
        for (idx, world) in unless_worlds.iter().enumerate() {
            let discount_constraint = world.constraints.get(&discount_code_path).expect(&format!(
                "Unless world {} should have discount_code constraint after simplification",
                idx
            ));

            match discount_constraint {
                FactConstraint::Enumeration(values) => {
                    assert!(
                        values.contains(&save30),
                        "Unless world {}: discount_code should be constrained to 'SAVE30' after simplification (A&B)|(A&!B) => A, got {:?}",
                        idx, values
                    );
                }
                FactConstraint::Unconstrained => {
                    panic!("Unless world {}: discount_code should not be Unconstrained after simplification", idx);
                }
                other => {
                    panic!(
                        "Unless world {}: discount_code should be Enumeration, got {:?}",
                        idx, other
                    );
                }
            }
        }
    }

    #[test]
    fn test_full_inversion_flow_with_simplified_condition() {
        // Test the full flow: rule → symbolic eval → optimize → world building → solve_world
        // This verifies that constraints are preserved through the entire flow

        use crate::evaluation::Evaluator;
        use crate::semantic::{BooleanValue, Expression, ExpressionKind, LiteralValue};
        use std::sync::Arc;

        let discount_code_path = FactPath::local("discount_code".to_string());
        let tag1_path = FactPath::local("tag1".to_string());

        let save30 = LiteralValue::Text("SAVE30".to_string());
        let yes = LiteralValue::Text("yes".to_string());

        // Create simplified condition: discount_code is "SAVE30" and tag1 is "yes"
        // (This simulates what optimization should produce)
        let discount_expr = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(discount_code_path.clone()),
                    None,
                )),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(Expression::new(
                    ExpressionKind::Literal(save30.clone()),
                    None,
                )),
            ),
            None,
        );

        let tag1_expr = Expression::new(
            ExpressionKind::Comparison(
                Arc::new(Expression::new(
                    ExpressionKind::FactPath(tag1_path.clone()),
                    None,
                )),
                crate::semantic::ComparisonComputation::Equal(
                    crate::semantic::EqualityNotation::Symbol,
                ),
                Arc::new(Expression::new(ExpressionKind::Literal(yes.clone()), None)),
            ),
            None,
        );

        let simplified_condition = Expression::new(
            ExpressionKind::LogicalAnd(Arc::new(discount_expr), Arc::new(tag1_expr)),
            None,
        );

        // Create rule with unless clause
        let rule = ExecutableRule {
            path: RulePath {
                segments: vec![],
                rule: "target".to_string(),
            },
            name: "target".to_string(),
            branches: vec![
                Branch {
                    condition: Expression::new(
                        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True)),
                        None,
                    ),
                    optimized_condition: None,
                    result: Expression::new(
                        ExpressionKind::Literal(LiteralValue::Number(rust_decimal::Decimal::from(
                            0,
                        ))),
                        None,
                    ),
                    source: None,
                },
                Branch {
                    condition: simplified_condition.clone(),
                    optimized_condition: Some(simplified_condition),
                    result: Expression::new(
                        ExpressionKind::Literal(LiteralValue::Number(rust_decimal::Decimal::from(
                            1,
                        ))),
                        None,
                    ),
                    source: None,
                },
            ],
            needs_facts: {
                let mut set = std::collections::HashSet::new();
                set.insert(discount_code_path.clone());
                set.insert(tag1_path.clone());
                set
            },
            source: None,
        };

        let plan = ExecutionPlan::new(
            "test_doc".to_string(),
            HashMap::new(),
            vec![rule],
            crate::planning::graph::Graph::empty(),
        );

        // Apply symbolic evaluation and optimization
        let evaluator = Evaluator;
        let reduced_plan = evaluator.evaluate_symbolic(&plan);
        let optimized_plan = reduced_plan.optimize();

        // Build worlds
        let mut builder = WorldBuilder::new(&optimized_plan);
        let worlds = builder
            .build_worlds("target")
            .expect("build_worlds should succeed");

        // Find world with result = 1
        let unless_world = worlds.iter()
            .find(|w| matches!(&w.value.kind, ExpressionKind::Literal(LiteralValue::Number(n)) if *n == rust_decimal::Decimal::from(1)))
            .expect("Should have world with result = 1");

        // Verify world has discount_code constraint
        assert!(
            unless_world.constraints.contains_key(&discount_code_path),
            "World should have discount_code constraint"
        );

        // Test through public invert API
        let response = invert(
            &optimized_plan,
            "target",
            "=",
            Some(crate::computation::OperationResult::Value(
                LiteralValue::Number(rust_decimal::Decimal::from(1)),
            )),
        )
        .expect("invert should succeed");

        assert!(
            !response.solutions.is_empty(),
            "Should have at least one solution"
        );

        // Verify all solutions have discount_code constraint
        for (idx, solution) in response.solutions.iter().enumerate() {
            let discount_constraint =
                solution
                    .fact_constraints
                    .get(&discount_code_path)
                    .expect(&format!(
                        "Solution {} should have discount_code constraint",
                        idx
                    ));

            match discount_constraint {
                crate::algebra::constraints::FactConstraint::Enumeration(values) => {
                    assert!(
                        values.contains(&save30),
                        "Solution {}: discount_code should be 'SAVE30', got {:?}",
                        idx,
                        values
                    );
                }
                other => {
                    panic!(
                        "Solution {}: discount_code should be Enumeration, got {:?}",
                        idx, other
                    );
                }
            }
        }
    }

    #[test]
    fn test_full_flow_with_engine_api() {
        // Test using Engine API to create rule from source (like integration test)
        use crate::Engine;

        let mut code =
            String::from("doc test\n\nfact discount_code = [text]\nfact member_level = [text]\n");

        // Add multiple tags to test with larger OR expression
        let n_extra = 5; // Smaller than integration test for speed, but tests the pattern
        for i in 1..=n_extra {
            code.push_str(&format!("fact tag{} = [text]\n", i));
        }

        code.push_str("\nrule target = 0\n  unless ((discount_code is \"SAVE30\" and member_level is \"platinum\") or (discount_code is \"SAVE30\" and not (member_level is \"platinum\"))) and (");
        for i in 1..=n_extra {
            if i > 1 {
                code.push_str(" or ");
            }
            code.push_str(&format!("tag{} is \"yes\"", i));
        }
        code.push_str(") then 1\n");

        let mut engine = Engine::new();
        engine.add_lemma_code(&code, "gen").unwrap();

        let discount_code_path = FactPath::local("discount_code".to_string());
        let save30 = crate::semantic::LiteralValue::Text("SAVE30".to_string());

        let response = engine
            .invert_strict(
                "test",
                "target",
                "=",
                Some(crate::computation::OperationResult::Value(
                    crate::LiteralValue::number(1),
                )),
                std::collections::HashMap::new(),
            )
            .expect("invert should succeed");

        assert!(
            !response.solutions.is_empty(),
            "Should have at least one solution"
        );

        // Verify all solutions have discount_code constraint
        for (idx, solution) in response.solutions.iter().enumerate() {
            if !solution.fact_constraints.contains_key(&discount_code_path) {
                panic!(
                    "Solution {} should have discount_code constraint, but it's missing.\n\
                     Solution constraints: {:?}\n\
                     All constraint keys: {:?}",
                    idx,
                    solution.fact_constraints,
                    solution.fact_constraints.keys().collect::<Vec<_>>()
                );
            }

            let discount_constraint = solution
                .fact_constraints
                .get(&discount_code_path)
                .expect("Should exist after check above");

            match discount_constraint {
                crate::algebra::constraints::FactConstraint::Enumeration(values) => {
                    assert!(
                        values.contains(&save30),
                        "Solution {}: discount_code should be 'SAVE30', got {:?}",
                        idx,
                        values
                    );
                }
                other => {
                    panic!(
                        "Solution {}: discount_code should be Enumeration, got {:?}",
                        idx, other
                    );
                }
            }
        }
    }

    #[test]
    fn test_simple_rule_without_unless() {
        // Test simple rule with no unless clauses - should create one world
        use crate::Engine;

        let code = r#"
            doc test
            fact original_price = [number]
            fact discount = [number]
            
            rule final_price = original_price - discount
        "#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test").unwrap();

        let mut provided_values = std::collections::HashMap::new();
        provided_values.insert("discount".to_string(), crate::LiteralValue::number(20));

        let response = engine
            .invert_strict(
                "test",
                "final_price",
                "=",
                Some(crate::computation::OperationResult::Value(
                    crate::LiteralValue::number(80),
                )),
                provided_values,
            )
            .expect("invert should succeed");

        assert_eq!(
            response.solutions.len(),
            1,
            "Simple rule should have exactly 1 solution"
        );

        // Verify the solution has original_price constraint
        let original_price_path = FactPath::local("original_price".to_string());
        let solution = response.solutions.first().expect("Should have solution");
        assert!(
            solution.fact_constraints.contains_key(&original_price_path),
            "Solution should have original_price constraint"
        );
    }

    #[test]
    fn test_rule_reference_in_condition_expands_constraints() {
        // Test that rule references in conditions are expanded and their constraints are included
        use crate::Engine;
        use crate::inversion::invert;
        
        let code = r#"
            doc test
            fact points = [number]
            
            rule tier = "bronze"
              unless points >= 100 then "silver"
            
            rule rate = 5%
              unless tier? == "silver" then 10%
        "#;
        
        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test").unwrap();
        
        // Invert for rate = 10%
        let response = engine
            .invert_strict(
                "test",
                "rate",
                "=",
                Some(crate::computation::OperationResult::Value(
                    crate::LiteralValue::Percentage(rust_decimal::Decimal::from(10))
                )),
                std::collections::HashMap::new(),
            )
            .expect("invert should succeed");
        
        assert!(!response.solutions.is_empty(), "Should have at least one solution");
        
        // Verify solution has points constraint from tier rule
        let points_path = FactPath::local("points".to_string());
        let has_points_constraint = response.solutions.iter()
            .any(|s| s.fact_constraints.contains_key(&points_path));
        
        assert!(
            has_points_constraint,
            "Solution should have points constraint from tier rule expansion"
        );
    }
}
