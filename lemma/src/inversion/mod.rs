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

use crate::planning::ExecutionPlan;
use crate::{
    Expression, ExpressionKind, FactPath, LemmaError, LemmaResult, LiteralValue, OperationResult,
    RulePath,
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

    /// Iterate over solutions
    pub fn iter(&self) -> impl Iterator<Item = &Solution> {
        self.solutions.iter()
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
        LemmaError::Engine(format!("Rule not found: {}.{}", plan.doc_name, rule_name))
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
        };

        solutions.push(solution);
        all_domains.push(constraint_domains);
    }

    // Process arithmetic solutions (outcomes that are expressions needing algebraic solving)
    if let Some(OperationResult::Value(target_value)) = &target.outcome {
        if target.op == TargetOp::Eq {
            let algebraic_solutions = solve::solve_arithmetic_batch(
                enumeration_result.arithmetic_solutions,
                target_value,
                provided_facts,
            );

            for (world_solution, solved_outcome, solved_domains) in algebraic_solutions {
                let solved_outcome_result = OperationResult::Value(solved_outcome);

                let mut combined_domains =
                    extract_domains_from_constraint(&world_solution.constraint)?;
                for (fact_path, domain) in solved_domains {
                    combined_domains.insert(fact_path, domain);
                }

                let solution = Solution {
                    outcome: solved_outcome_result,
                    world: world_solution.world,
                };

                solutions.push(solution);
                all_domains.push(combined_domains);
            }
        }
    }

    if solutions.is_empty() {
        return Err(build_no_solution_error(&rule_path, &target, plan)?);
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
    match (a, b) {
        (LiteralValue::Number(a), LiteralValue::Number(b)) => Some(a.cmp(b)),
        (LiteralValue::Percentage(a), LiteralValue::Percentage(b)) => Some(a.cmp(b)),
        (LiteralValue::Unit(a), LiteralValue::Unit(b)) => {
            if a.same_category(b) {
                Some(a.value().cmp(&b.value()))
            } else {
                None
            }
        }
        (LiteralValue::Number(a), LiteralValue::Unit(b)) => Some(a.cmp(&b.value())),
        (LiteralValue::Unit(a), LiteralValue::Number(b)) => Some(a.value().cmp(b)),
        _ => None,
    }
}

/// Build error message when no solution is found
fn build_no_solution_error(
    rule_path: &RulePath,
    target: &Target,
    plan: &ExecutionPlan,
) -> LemmaResult<LemmaError> {
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

    // Collect available outcomes from the rule
    let mut available_outcomes = Vec::new();
    if let Some(rule) = plan.get_rule_by_path(rule_path) {
        for branch in &rule.branches {
            let outcome_desc = format_outcome_description(&branch.result);
            if !available_outcomes.contains(&outcome_desc) {
                available_outcomes.push(outcome_desc);
            }
        }
    }

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

    Ok(LemmaError::Engine(error_msg))
}

/// Format an outcome expression for error messages
fn format_outcome_description(outcome: &Expression) -> String {
    match &outcome.kind {
        ExpressionKind::Veto(ve) => {
            if let Some(msg) = &ve.message {
                format!("veto '{}'", msg)
            } else {
                "veto".to_owned()
            }
        }
        ExpressionKind::Literal(lit) => format!("value {}", lit),
        _ => "computed value".to_owned(),
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

    #[test]
    fn test_format_target_eq() {
        let target = Target::value(LiteralValue::Number(Decimal::from(42)));
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
            Domain::Enumeration(vec![LiteralValue::Number(Decimal::from(25))]),
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
                min: Bound::Exclusive(LiteralValue::Number(Decimal::from(18))),
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
            Domain::Enumeration(vec![LiteralValue::Number(Decimal::from(25))]),
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
                min: Bound::Exclusive(LiteralValue::Number(Decimal::from(18))),
                max: Bound::Unbounded,
            },
        );
        let domains = vec![domain_map];
        assert!(!compute_is_determined(&domains));
    }
}
