//! On-demand world building for inversion queries
//!
//! Builds "worlds" from rules - each world represents one consistent universe
//! with constraints and a value expression.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use crate::algebra::constraints::{ConstraintSet, extract_constraints, FactConstraint};
use crate::planning::ExecutionPlan;
use crate::semantic::{
    BooleanValue, Expression, ExpressionKind, FactPath, LiteralValue, RulePath,
};
use crate::{LemmaError, LemmaResult};
use super::World;

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
        let rule = self.plan.get_rule(rule_name)
            .ok_or_else(|| LemmaError::Engine(format!("Rule not found: {}", rule_name)))?;
        let rule_path = rule.path.clone();
        
        // Check cache
        if let Some(cached) = self.cache.get(&rule_path) {
            return Ok(cached.clone());
        }
        
        let mut worlds = Vec::new();
        
        for branch in &rule.branches {
            // Branch already symbolically evaluated - use optimized_condition if available
            let condition = branch.optimized_condition.as_ref().unwrap_or(&branch.condition);
            let result = &branch.result;
            
            // If condition is literal true, result applies unconditionally
            if matches!(&condition.kind, 
                ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True))) {
                worlds.push(World {
                    constraints: HashMap::new(),
                    value: result.clone(),
                });
                continue;
            }
            
            // Extract constraints from condition (benefits from DNF optimization)
            let mut constraint_set = ConstraintSet::new();
            extract_constraints(condition, &mut constraint_set);
            let constraints = constraint_set.to_fact_constraints();
            
            // Check if result references other rules
            let rule_refs = extract_rule_references(result);
            
            if rule_refs.is_empty() {
                // Simple case: no rule dependencies in result
                worlds.push(World {
                    constraints,
                    value: result.clone(),
                });
            } else {
                // Complex case: recursively build referenced rule worlds
                let branch_worlds = self.build_with_references(
                    constraints,
                    result,
                    &rule_refs
                )?;
                worlds.extend(branch_worlds);
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
            let ref_worlds = self.build_worlds(&rule_ref.path.to_string())?;
            
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
}

// Helper structures

struct RuleReference {
    path: RulePath,
}

// Helper functions

/// Extract all rule references from an expression
fn extract_rule_references(expr: &Expression) -> Vec<RuleReference> {
    let paths = collect_rule_paths(expr);
    paths.into_iter()
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
fn substitute_rule_path(expr: &Expression, target: &RulePath, replacement: &Expression) -> Expression {
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
