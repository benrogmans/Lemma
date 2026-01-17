//! World-based inverse reasoning for Lemma rules
//!
//! Determines what inputs produce desired outputs through world enumeration.
//! A "world" is a complete assignment of which branch is active for each rule.
//!
//! The main entry point is [`invert()`], which returns an [`InversionResponse`]
//! containing all valid solutions with their domains.

mod constraint;
mod domain;
mod solve;
mod target;
mod world;

pub use domain::{extract_domains_from_constraint, Bound, Domain};
pub use target::{Target, TargetOp};
pub use world::World;

use crate::parsing::ast::Span;
use crate::planning::ExecutionPlan;
use crate::{
    Expression, ExpressionKind, FactPath, LemmaError, LemmaResult, LiteralValue, OperationResult,
    Value,
};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::collections::{HashMap, HashSet};

use world::{WorldEnumerator, WorldSolution};

// ============================================================================
// Solution and Response types
// ============================================================================

/// A single solution from inversion
///
/// Contains the outcome for a solution. For fact constraints,
/// use the corresponding entry in `InversionResponse.domains`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Solution {
    /// The outcome (value or veto)
    pub outcome: OperationResult,
    /// The world (branch assignment) that produced this solution
    pub world: World,
    /// For underdetermined systems: the expression that must equal the target
    /// e.g., for `total = price * quantity` with target 100, this would be `price * quantity`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<Expression>,
}

/// Response from inversion containing all valid solutions
#[derive(Debug, Clone)]
pub struct InversionResponse {
    /// All valid solutions
    pub solutions: Vec<Solution>,
    /// Domain constraints for each solution (indexed by solution index)
    pub domains: Vec<HashMap<FactPath, Domain>>,
    /// Facts that still need values (appear in conditions but aren't fully constrained)
    pub undetermined_facts: Vec<FactPath>,
    /// True if all facts are fully constrained to specific values
    pub is_determined: bool,
}

impl InversionResponse {
    /// Create a new inversion response, computing metadata from solutions and domains
    pub fn new(solutions: Vec<Solution>, domains: Vec<HashMap<FactPath, Domain>>) -> Self {
        let undetermined_facts = compute_undetermined_facts(&domains);
        let is_determined = compute_is_determined(&domains);
        Self {
            solutions,
            domains,
            undetermined_facts,
            is_determined,
        }
    }

    /// Check if the response is empty (no solutions)
    pub fn is_empty(&self) -> bool {
        self.solutions.is_empty()
    }

    /// Get the number of solutions
    pub fn len(&self) -> usize {
        self.solutions.len()
    }

    /// Iterate over solutions with their domains
    pub fn iter(&self) -> impl Iterator<Item = (&Solution, &HashMap<FactPath, Domain>)> {
        self.solutions.iter().zip(self.domains.iter())
    }

    /// Alias for undetermined_facts (backwards compatibility)
    pub fn free_variables(&self) -> &[FactPath] {
        &self.undetermined_facts
    }

    /// Alias for is_determined (backwards compatibility)
    pub fn is_fully_constrained(&self) -> bool {
        self.is_determined
    }
}

impl Serialize for InversionResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("InversionResponse", 4)?;
        state.serialize_field("solutions", &self.solutions)?;

        let domains_serializable: Vec<HashMap<String, String>> = self
            .domains
            .iter()
            .map(|d| {
                d.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            })
            .collect();
        state.serialize_field("domains", &domains_serializable)?;

        let undetermined_serializable: Vec<String> = self
            .undetermined_facts
            .iter()
            .map(|fp| fp.to_string())
            .collect();
        state.serialize_field("undetermined_facts", &undetermined_serializable)?;
        state.serialize_field("is_determined", &self.is_determined)?;
        state.end()
    }
}

// ============================================================================
// Main inversion function
// ============================================================================

/// Invert a rule to find input domains that produce a desired outcome.
///
/// Given an execution plan and rule name, determines what values the unknown
/// facts must have to produce the target outcome.
///
/// The `provided_facts` set contains fact paths that are fixed (user-provided values).
/// Only these facts are substituted during hydration; other fact values remain as
/// undetermined facts for inversion.
///
/// Returns an [`InversionResponse`] containing all valid solutions.
pub fn invert(
    rule_name: &str,
    target: Target,
    plan: &ExecutionPlan,
    provided_facts: &HashSet<FactPath>,
) -> LemmaResult<InversionResponse> {
    let executable_rule = plan.get_rule(rule_name).ok_or_else(|| {
        LemmaError::engine(
            format!("Rule not found: {}.{}", plan.doc_name, rule_name),
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "<unknown>",
            std::sync::Arc::from(""),
            plan.doc_name.clone(),
            1,
            None::<String>,
        )
    })?;

    let rule_path = executable_rule.path.clone();

    // Enumerate all valid worlds for this rule
    let mut enumerator = WorldEnumerator::new(plan, &rule_path)?;
    let enumeration_result = enumerator.enumerate(provided_facts)?;

    // Build Solution objects with domains
    let mut solutions = Vec::new();
    let mut all_domains = Vec::new();

    // Process literal solutions (outcomes that are concrete values)
    let filtered_literal_solutions =
        filter_literal_solutions_by_target(enumeration_result.literal_solutions, &target);

    for world_solution in filtered_literal_solutions {
        let constraint_domains = extract_domains_from_constraint(&world_solution.constraint)?;

        let solution = Solution {
            outcome: world_solution.outcome,
            world: world_solution.world,
            shape: None,
        };

        solutions.push(solution);
        all_domains.push(constraint_domains);
    }

    // Process arithmetic solutions (outcomes that are expressions needing algebraic solving)
    if let Some(OperationResult::Value(target_value)) = &target.outcome {
        // For equality targets, try algebraic solving first
        let solved_indices: std::collections::HashSet<usize> = if target.op == TargetOp::Eq {
            let algebraic_solutions = solve::solve_arithmetic_batch(
                enumeration_result.arithmetic_solutions.clone(),
                target_value,
                provided_facts,
            );

            // Track which arithmetic solutions were successfully solved
            let indices: std::collections::HashSet<usize> = algebraic_solutions
                .iter()
                .map(|(ws, _, _)| {
                    enumeration_result
                        .arithmetic_solutions
                        .iter()
                        .position(|orig| orig.world == ws.world)
                        .unwrap_or(usize::MAX)
                })
                .collect();

            // Add algebraically solved solutions (only if solved values satisfy constraints)
            for (world_solution, solved_outcome, solved_domains) in algebraic_solutions {
                let constraint_domains =
                    extract_domains_from_constraint(&world_solution.constraint)?;

                // Check if solved values are compatible with constraint domains
                let mut is_valid = true;
                for (fact_path, solved_domain) in &solved_domains {
                    if let Some(constraint_domain) = constraint_domains.get(fact_path) {
                        // Check if the solved value is within the constraint domain
                        if let Domain::Enumeration(values) = solved_domain {
                            for value in values.iter() {
                                if !constraint_domain.contains(value) {
                                    is_valid = false;
                                    break;
                                }
                            }
                        }
                    }
                    if !is_valid {
                        break;
                    }
                }

                if !is_valid {
                    continue; // Skip this solution as solved value violates constraint
                }

                let solved_outcome_result = OperationResult::Value(solved_outcome);

                let mut combined_domains = constraint_domains;
                for (fact_path, domain) in solved_domains {
                    combined_domains.insert(fact_path, domain);
                }

                let solution = Solution {
                    outcome: solved_outcome_result,
                    world: world_solution.world,
                    shape: None,
                };

                solutions.push(solution);
                all_domains.push(combined_domains);
            }

            indices
        } else {
            std::collections::HashSet::new()
        };

        // For arithmetic solutions that couldn't be solved algebraically (multiple unknowns)
        // or for non-equality operators, add them with the shape representing the constraint
        for (idx, arith_solution) in enumeration_result.arithmetic_solutions.iter().enumerate() {
            if solved_indices.contains(&idx) {
                continue; // Already solved algebraically
            }

            // Add as underdetermined solution with shape
            let mut combined_domains = extract_domains_from_constraint(&arith_solution.constraint)?;

            // Extract unknown facts from the shape expression and add them as Unconstrained
            let unknown_facts =
                extract_fact_paths_from_expression(&arith_solution.outcome_expression);
            for fact_path in unknown_facts {
                // Only add if not already constrained and not a provided fact
                if !combined_domains.contains_key(&fact_path)
                    && !provided_facts.contains(&fact_path)
                {
                    combined_domains.insert(fact_path, Domain::Unconstrained);
                }
            }

            let solution = Solution {
                outcome: OperationResult::Value(target_value.clone()),
                world: arith_solution.world.clone(),
                shape: Some(arith_solution.outcome_expression.clone()),
            };

            solutions.push(solution);
            all_domains.push(combined_domains);
        }
    }

    Ok(InversionResponse::new(solutions, all_domains))
}

// ============================================================================
// Helper functions
// ============================================================================

/// Filter literal solutions by the target outcome
fn filter_literal_solutions_by_target(
    solutions: Vec<WorldSolution>,
    target: &Target,
) -> Vec<WorldSolution> {
    let mut filtered = Vec::new();

    for solution in solutions {
        let matches = match (&target.outcome, &solution.outcome) {
            (None, _) => {
                // Target::any_value() - accept any outcome (including veto)
                true
            }
            (Some(OperationResult::Value(target_value)), OperationResult::Value(outcome_value)) => {
                // Specific value target, outcome is a value
                match target.op {
                    TargetOp::Eq => outcome_value == target_value,
                    TargetOp::Neq => outcome_value != target_value,
                    TargetOp::Lt => {
                        compare_values(outcome_value, target_value)
                            == Some(std::cmp::Ordering::Less)
                    }
                    TargetOp::Lte => {
                        let cmp = compare_values(outcome_value, target_value);
                        cmp == Some(std::cmp::Ordering::Less)
                            || cmp == Some(std::cmp::Ordering::Equal)
                    }
                    TargetOp::Gt => {
                        compare_values(outcome_value, target_value)
                            == Some(std::cmp::Ordering::Greater)
                    }
                    TargetOp::Gte => {
                        let cmp = compare_values(outcome_value, target_value);
                        cmp == Some(std::cmp::Ordering::Greater)
                            || cmp == Some(std::cmp::Ordering::Equal)
                    }
                }
            }
            (Some(OperationResult::Veto(target_msg)), OperationResult::Veto(outcome_msg)) => {
                // Veto target, outcome is a veto - check message match
                match target_msg {
                    None => true, // Target any veto
                    Some(t_msg) => outcome_msg.as_ref().map(|m| m == t_msg).unwrap_or(false),
                }
            }
            _ => false, // Mismatch between value/veto targets and outcomes
        };

        if matches {
            filtered.push(solution);
        }
    }

    filtered
}

/// Compare two literal values for ordering
fn compare_values(a: &LiteralValue, b: &LiteralValue) -> Option<std::cmp::Ordering> {
    match (&a.value, &b.value) {
        (Value::Number(a_val), Value::Number(b_val)) => Some(a_val.cmp(b_val)),
        (Value::Ratio(a_val, _), Value::Ratio(b_val, _)) => Some(a_val.cmp(b_val)),
        (Value::Scale(a_val, _), Value::Scale(b_val, _)) => Some(a_val.cmp(b_val)),
        (Value::Duration(a_val, unit_a), Value::Duration(b_val, unit_b)) => {
            if unit_a == unit_b {
                Some(a_val.cmp(b_val))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract all FactPath references from an expression
fn extract_fact_paths_from_expression(expr: &Expression) -> Vec<FactPath> {
    let mut paths = Vec::new();
    collect_fact_paths(expr, &mut paths);
    paths
}

fn collect_fact_paths(expr: &Expression, paths: &mut Vec<FactPath>) {
    match &expr.kind {
        ExpressionKind::FactPath(fp) => {
            if !paths.contains(fp) {
                paths.push(fp.clone());
            }
        }
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right) => {
            collect_fact_paths(left, paths);
            collect_fact_paths(right, paths);
        }
        ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            collect_fact_paths(inner, paths);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Reference(_)
        | ExpressionKind::UnresolvedUnitLiteral(_, _)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::RuleReference(_)
        | ExpressionKind::RulePath(_) => {}
    }
}

/// Compute the list of undetermined facts from all solution domains
fn compute_undetermined_facts(all_domains: &[HashMap<FactPath, Domain>]) -> Vec<FactPath> {
    let mut undetermined: HashSet<FactPath> = HashSet::new();

    for solution_domains in all_domains {
        for (fact_path, domain) in solution_domains {
            let is_determined = matches!(
                domain,
                Domain::Enumeration(values) if values.len() == 1
            );
            if !is_determined {
                undetermined.insert(fact_path.clone());
            }
        }
    }

    let mut result: Vec<FactPath> = undetermined.into_iter().collect();
    result.sort_by_key(|a| a.to_string());
    result
}

/// Check if all facts across all solutions are fully determined
fn compute_is_determined(all_domains: &[HashMap<FactPath, Domain>]) -> bool {
    if all_domains.is_empty() {
        return true;
    }

    for solution_domains in all_domains {
        for domain in solution_domains.values() {
            let is_single_value = matches!(
                domain,
                Domain::Enumeration(values) if values.len() == 1
            );
            if !is_single_value {
                return false;
            }
        }
    }

    true
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::sync::Arc;

    #[test]
    fn test_format_target_eq() {
        let target = Target::value(LiteralValue::number(Decimal::from(42)));
        let formatted = target.format();
        assert_eq!(formatted, "= 42");
    }

    #[test]
    fn test_format_target_any() {
        let target = Target::any_value();
        let formatted = target.format();
        assert_eq!(formatted, "= any");
    }

    #[test]
    fn test_compute_undetermined_facts_empty() {
        let domains: Vec<HashMap<FactPath, Domain>> = vec![];
        let undetermined = compute_undetermined_facts(&domains);
        assert!(undetermined.is_empty());
    }

    #[test]
    fn test_compute_undetermined_facts_single_value() {
        let mut domain_map = HashMap::new();
        domain_map.insert(
            FactPath::local("age".to_string()),
            Domain::Enumeration(Arc::new(vec![LiteralValue::number(Decimal::from(25))])),
        );
        let domains = vec![domain_map];
        let undetermined = compute_undetermined_facts(&domains);
        assert!(undetermined.is_empty());
    }

    #[test]
    fn test_compute_undetermined_facts_range() {
        let mut domain_map = HashMap::new();
        domain_map.insert(
            FactPath::local("age".to_string()),
            Domain::Range {
                min: Bound::Exclusive(Arc::new(LiteralValue::number(Decimal::from(18)))),
                max: Bound::Unbounded,
            },
        );
        let domains = vec![domain_map];
        let undetermined = compute_undetermined_facts(&domains);
        assert_eq!(undetermined.len(), 1);
    }

    #[test]
    fn test_compute_is_determined_empty() {
        let domains: Vec<HashMap<FactPath, Domain>> = vec![];
        assert!(compute_is_determined(&domains));
    }

    #[test]
    fn test_compute_is_determined_true() {
        let mut domain_map = HashMap::new();
        domain_map.insert(
            FactPath::local("age".to_string()),
            Domain::Enumeration(Arc::new(vec![LiteralValue::number(Decimal::from(25))])),
        );
        let domains = vec![domain_map];
        assert!(compute_is_determined(&domains));
    }

    #[test]
    fn test_compute_is_determined_false() {
        let mut domain_map = HashMap::new();
        domain_map.insert(
            FactPath::local("age".to_string()),
            Domain::Range {
                min: Bound::Exclusive(Arc::new(LiteralValue::number(Decimal::from(18)))),
                max: Bound::Unbounded,
            },
        );
        let domains = vec![domain_map];
        assert!(!compute_is_determined(&domains));
    }
}
