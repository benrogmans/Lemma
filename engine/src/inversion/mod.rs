//! World-based inverse reasoning for Lemma rules
//!
//! Determines what inputs produce desired outputs through world enumeration.
//! A "world" is a complete assignment of which branch is active for each rule.
//!
//! The main entry point is [`invert()`], which returns an [`InversionResponse`]
//! containing all valid solutions with their domains.

mod constraint;
mod derived;
mod domain;
mod solve;
mod target;
mod world;

pub use derived::{DerivedExpression, DerivedExpressionKind};
pub use domain::{extract_domains_from_constraint, Bound, Domain};
pub use target::{Target, TargetOp};
pub use world::World;

use crate::evaluation::operations::VetoType;
use crate::planning::semantics::{DataPath, Expression, LiteralValue, ValueKind};
use crate::planning::ExecutionPlan;
use crate::{Error, OperationResult};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::collections::{HashMap, HashSet};

use world::{WorldEnumerator, WorldSolution};

// ============================================================================
// Solution and Response types
// ============================================================================

/// A single solution from inversion
///
/// Contains the outcome for a solution. For data constraints,
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
    pub domains: Vec<HashMap<DataPath, Domain>>,
    /// Data that still need values (appear in conditions but aren't fully constrained)
    pub undetermined_data: Vec<DataPath>,
    /// True if all data are fully constrained to specific values
    pub is_determined: bool,
}

impl InversionResponse {
    /// Create a new inversion response, computing metadata from solutions and domains
    pub fn new(solutions: Vec<Solution>, domains: Vec<HashMap<DataPath, Domain>>) -> Self {
        let undetermined_data = compute_undetermined_data(&domains);
        let is_determined = compute_is_determined(&domains);
        Self {
            solutions,
            domains,
            undetermined_data,
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
    pub fn iter(&self) -> impl Iterator<Item = (&Solution, &HashMap<DataPath, Domain>)> {
        self.solutions.iter().zip(self.domains.iter())
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
            .undetermined_data
            .iter()
            .map(|fp| fp.to_string())
            .collect();
        state.serialize_field("undetermined_data", &undetermined_serializable)?;
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
/// data must have to produce the target outcome.
///
/// The `provided_data` set contains data paths that are fixed (user-provided values).
/// Only these data are substituted during hydration; other data values remain as
/// undetermined data for inversion.
///
/// Returns an [`InversionResponse`] containing all valid solutions.
pub fn invert(
    rule_name: &str,
    target: Target,
    plan: &ExecutionPlan,
    provided_data: &HashSet<DataPath>,
) -> Result<InversionResponse, Error> {
    let executable_rule = plan.get_rule(rule_name).ok_or_else(|| {
        Error::request(
            format!("Rule not found: {}.{}", plan.spec_name, rule_name),
            None::<String>,
        )
    })?;

    let rule_path = executable_rule.path.clone();

    // Enumerate all valid worlds for this rule
    let mut enumerator = WorldEnumerator::new(plan, &rule_path)?;
    let enumeration_result = enumerator.enumerate(provided_data)?;

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
                provided_data,
            );

            // Track which arithmetic solutions were successfully solved
            let indices: std::collections::HashSet<usize> = algebraic_solutions
                .iter()
                .filter_map(|(ws, _, _)| {
                    enumeration_result
                        .arithmetic_solutions
                        .iter()
                        .position(|orig| orig.world == ws.world)
                })
                .collect();

            // Add algebraically solved solutions (only if solved values satisfy constraints)
            for (world_solution, solved_outcome, solved_domains) in algebraic_solutions {
                let constraint_domains =
                    extract_domains_from_constraint(&world_solution.constraint)?;

                // Check if solved values are compatible with constraint domains
                let mut is_valid = true;
                for (data_path, solved_domain) in &solved_domains {
                    if let Some(constraint_domain) = constraint_domains.get(data_path) {
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

                let solved_outcome_result = OperationResult::Value(Box::new(solved_outcome));

                let mut combined_domains = constraint_domains;
                for (data_path, domain) in solved_domains {
                    combined_domains.insert(data_path, domain);
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

            // Extract unknown data from the shape expression and add them as Unconstrained
            let unknown_data =
                extract_data_paths_from_expression(&arith_solution.outcome_expression);
            for data_path in unknown_data {
                // Only add if not already constrained and not a provided data
                if !combined_domains.contains_key(&data_path) && !provided_data.contains(&data_path)
                {
                    combined_domains.insert(data_path, Domain::Unconstrained);
                }
            }

            let solution = Solution {
                outcome: OperationResult::Value(Box::new(target_value.as_ref().clone())),
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
                // Specific value target, outcome is a value.
                // Compare by semantic value only (ValueKind), not full LiteralValue,
                // because type metadata (e.g. LemmaType.name) may differ between the
                // target (constructed externally) and the outcome (from constant folding).
                match target.op {
                    TargetOp::Eq => outcome_value.value == target_value.value,
                    TargetOp::Neq => outcome_value.value != target_value.value,
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
            (Some(OperationResult::Veto(target_reason)), OperationResult::Veto(outcome_reason)) => {
                match target_reason {
                    VetoType::UserDefined { message: None } => true, // Target any veto
                    VetoType::UserDefined {
                        message: Some(ref t_msg),
                    } => matches!(
                        outcome_reason,
                        VetoType::UserDefined {
                            message: Some(ref o_msg)
                        }
                        if o_msg == t_msg
                    ),
                    _ => false,
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
        (ValueKind::Number(a_val), ValueKind::Number(b_val)) => Some(a_val.cmp(b_val)),
        (ValueKind::Ratio(a_val, _), ValueKind::Ratio(b_val, _)) => Some(a_val.cmp(b_val)),
        (ValueKind::Scale(a_val, _), ValueKind::Scale(b_val, _)) => Some(a_val.cmp(b_val)),
        (ValueKind::Duration(a_val, unit_a), ValueKind::Duration(b_val, unit_b)) => {
            if unit_a == unit_b {
                Some(a_val.cmp(b_val))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract all DataPath references from a derived expression
fn extract_data_paths_from_expression(expr: &Expression) -> Vec<DataPath> {
    let mut set = std::collections::HashSet::new();
    expr.collect_data_paths(&mut set);
    set.into_iter().collect()
}

/// Compute the list of undetermined data from all solution domains
fn compute_undetermined_data(all_domains: &[HashMap<DataPath, Domain>]) -> Vec<DataPath> {
    let mut undetermined: HashSet<DataPath> = HashSet::new();

    for solution_domains in all_domains {
        for (data_path, domain) in solution_domains {
            let is_determined = matches!(
                domain,
                Domain::Enumeration(values) if values.len() == 1
            );
            if !is_determined {
                undetermined.insert(data_path.clone());
            }
        }
    }

    let mut result: Vec<DataPath> = undetermined.into_iter().collect();
    result.sort_by_key(|a| a.to_string());
    result
}

/// Check if all data across all solutions are fully determined
fn compute_is_determined(all_domains: &[HashMap<DataPath, Domain>]) -> bool {
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
    use crate::parsing::ast::DateTimeValue;
    use crate::Engine;
    use rust_decimal::Decimal;
    use std::collections::HashMap;
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
    fn test_compute_undetermined_data_empty() {
        let domains: Vec<HashMap<DataPath, Domain>> = vec![];
        let undetermined = compute_undetermined_data(&domains);
        assert!(undetermined.is_empty());
    }

    #[test]
    fn test_compute_undetermined_data_single_value() {
        let mut domain_map = HashMap::new();
        domain_map.insert(
            DataPath::new(vec![], "age".to_string()),
            Domain::Enumeration(Arc::new(vec![LiteralValue::number(Decimal::from(25))])),
        );
        let domains = vec![domain_map];
        let undetermined = compute_undetermined_data(&domains);
        assert!(undetermined.is_empty());
    }

    #[test]
    fn test_compute_undetermined_data_range() {
        let mut domain_map = HashMap::new();
        domain_map.insert(
            DataPath::new(vec![], "age".to_string()),
            Domain::Range {
                min: Bound::Exclusive(Arc::new(LiteralValue::number(Decimal::from(18)))),
                max: Bound::Unbounded,
            },
        );
        let domains = vec![domain_map];
        let undetermined = compute_undetermined_data(&domains);
        assert_eq!(undetermined.len(), 1);
    }

    #[test]
    fn test_compute_is_determined_empty() {
        let domains: Vec<HashMap<DataPath, Domain>> = vec![];
        assert!(compute_is_determined(&domains));
    }

    #[test]
    fn test_compute_is_determined_true() {
        let mut domain_map = HashMap::new();
        domain_map.insert(
            DataPath::new(vec![], "age".to_string()),
            Domain::Enumeration(Arc::new(vec![LiteralValue::number(Decimal::from(25))])),
        );
        let domains = vec![domain_map];
        assert!(compute_is_determined(&domains));
    }

    #[test]
    fn test_compute_is_determined_false() {
        let mut domain_map = HashMap::new();
        domain_map.insert(
            DataPath::new(vec![], "age".to_string()),
            Domain::Range {
                min: Bound::Exclusive(Arc::new(LiteralValue::number(Decimal::from(18)))),
                max: Bound::Unbounded,
            },
        );
        let domains = vec![domain_map];
        assert!(!compute_is_determined(&domains));
    }

    #[test]
    fn test_invert_strict_rule_reference_expands_constraints() {
        // Regression-style test: rule references should be expanded during inversion,
        // and veto conditions should constrain the domains.
        let code = r#"
spec example
data x: number
rule base: x
  unless x > 3 then veto "too much"
  unless x < 0 then veto "too little"

rule another: base
  unless x > 5 then veto "way too much"
"#;

        let mut engine = Engine::new();
        engine
            .load(code, crate::SourceType::Labeled("test.lemma"))
            .unwrap();
        let now = DateTimeValue::now();

        let inv = engine
            .invert(
                "example",
                Some(&now),
                "another",
                Target::value(LiteralValue::number(3.into())),
                HashMap::new(),
            )
            .expect("inversion should succeed");

        assert!(!inv.is_empty(), "expected at least one solution");

        let x = DataPath::new(vec![], "x".to_string());
        let three = LiteralValue::number(3.into());

        // For target value 3, x must be exactly 3 (not just within a broad range).
        for (_solution, domains) in inv.iter() {
            let d = domains.get(&x).expect("domain for x should exist");
            assert!(
                d.contains(&three),
                "x domain should contain 3. Domain: {}",
                d
            );
        }
    }

    #[test]
    fn test_invert_strict_no_solution_when_value_is_blocked_by_veto() {
        let code = r#"
spec example
data x: number
rule base: x
  unless x > 3 then veto "too much"
  unless x < 0 then veto "too little"

rule another: base
  unless x > 5 then veto "way too much"
"#;

        let mut engine = Engine::new();
        engine
            .load(code, crate::SourceType::Labeled("test.lemma"))
            .unwrap();
        let now = DateTimeValue::now();

        let inv = engine
            .invert(
                "example",
                Some(&now),
                "another",
                Target::value(LiteralValue::number(7.into())),
                HashMap::new(),
            )
            .expect("inversion should succeed");

        assert!(
            inv.is_empty(),
            "Should have no solutions because another can never equal 7"
        );
    }

    #[test]
    fn test_invert_strict_veto_target_constrains_domain() {
        let code = r#"
spec example
data x: number
rule base: x
  unless x > 3 then veto "too much"
  unless x < 0 then veto "too little"

rule another: base
  unless x > 5 then veto "way too much"
"#;

        let mut engine = Engine::new();
        engine
            .load(code, crate::SourceType::Labeled("test.lemma"))
            .unwrap();
        let now = DateTimeValue::now();

        let inv = engine
            .invert(
                "example",
                Some(&now),
                "another",
                Target::veto(Some("way too much".to_string())),
                HashMap::new(),
            )
            .expect("inversion should succeed");

        assert!(!inv.is_empty(), "expected solutions for veto query");

        let x = DataPath::new(vec![], "x".to_string());
        let five = LiteralValue::number(5.into());
        let six = LiteralValue::number(6.into());

        for (solution, domains) in inv.iter() {
            assert_eq!(
                solution.outcome,
                OperationResult::Veto(VetoType::UserDefined {
                    message: Some("way too much".to_string()),
                }),
                "Expected solution outcome to be veto('way too much'), got: {:?}",
                solution.outcome
            );

            let d = domains.get(&x).expect("domain for x should exist");
            match d {
                Domain::Range { min, max } => {
                    assert!(
                        matches!(min, Bound::Exclusive(v) if v.as_ref() == &five),
                        "Expected min bound to be (5), got: {}",
                        d
                    );
                    assert!(
                        matches!(max, Bound::Unbounded),
                        "Expected max bound to be +inf, got: {}",
                        d
                    );
                }
                other => panic!("Expected range domain for x, got: {}", other),
            }
            assert!(
                !d.contains(&five),
                "x=5 should not be in veto('way too much') domain. Domain: {}",
                d
            );
            assert!(
                d.contains(&six),
                "x=6 should be in veto('way too much') domain. Domain: {}",
                d
            );
        }
    }

    #[test]
    fn test_invert_strict_any_veto_target_matches_all_veto_ranges() {
        let code = r#"
spec example
data x: number
rule base: x
  unless x > 3 then veto "too much"
  unless x < 0 then veto "too little"

rule another: base
  unless x > 5 then veto "way too much"
"#;

        let mut engine = Engine::new();
        engine
            .load(code, crate::SourceType::Labeled("test.lemma"))
            .unwrap();

        let now = DateTimeValue::now();
        let inv = engine
            .invert(
                "example",
                Some(&now),
                "another",
                Target::any_veto(),
                HashMap::new(),
            )
            .expect("inversion should succeed");

        assert!(!inv.is_empty(), "expected solutions for any-veto query");

        let x = DataPath::new(vec![], "x".to_string());
        let minus_one = LiteralValue::number((-1).into());
        let zero = LiteralValue::number(0.into());
        let two = LiteralValue::number(2.into());
        let three = LiteralValue::number(3.into());
        let four = LiteralValue::number(4.into());
        let five = LiteralValue::number(5.into());
        let six = LiteralValue::number(6.into());

        let mut saw_too_little = false;
        let mut saw_too_much = false;
        let mut saw_way_too_much = false;

        for (solution, domains) in inv.iter() {
            let d = domains.get(&x).expect("domain for x should exist");
            assert!(
                !d.contains(&two),
                "x=2 should not be in any-veto domain. Domain: {}",
                d
            );

            match &solution.outcome {
                OperationResult::Veto(VetoType::UserDefined {
                    message: Some(ref msg),
                }) if msg == "too little" => {
                    saw_too_little = true;

                    match d {
                        Domain::Range { min, max } => {
                            assert!(
                                matches!(min, Bound::Unbounded),
                                "Expected min bound to be -inf for 'too little', got: {}",
                                d
                            );
                            assert!(
                                matches!(max, Bound::Exclusive(v) if v.as_ref() == &zero),
                                "Expected max bound to be (0) for 'too little', got: {}",
                                d
                            );
                        }
                        other => panic!("Expected range domain for x, got: {}", other),
                    }

                    assert!(
                        d.contains(&minus_one),
                        "x=-1 should be in veto('too little') domain. Domain: {}",
                        d
                    );
                    assert!(
                        !d.contains(&zero),
                        "x=0 should not be in veto('too little') domain. Domain: {}",
                        d
                    );
                }
                OperationResult::Veto(VetoType::UserDefined {
                    message: Some(ref msg),
                }) if msg == "too much" => {
                    saw_too_much = true;

                    match d {
                        Domain::Range { min, max } => {
                            assert!(
                                matches!(min, Bound::Exclusive(v) if v.as_ref() == &three),
                                "Expected min bound to be (3) for 'too much', got: {}",
                                d
                            );
                            assert!(
                                matches!(max, Bound::Inclusive(v) if v.as_ref() == &five),
                                "Expected max bound to be [5] for 'too much', got: {}",
                                d
                            );
                        }
                        other => panic!("Expected range domain for x, got: {}", other),
                    }

                    assert!(
                        d.contains(&four),
                        "x=4 should be in veto('too much') domain. Domain: {}",
                        d
                    );
                    assert!(
                        d.contains(&five),
                        "x=5 should be in veto('too much') domain. Domain: {}",
                        d
                    );
                    assert!(
                        !d.contains(&three),
                        "x=3 should not be in veto('too much') domain. Domain: {}",
                        d
                    );
                    assert!(
                        !d.contains(&six),
                        "x=6 should not be in veto('too much') domain. Domain: {}",
                        d
                    );
                }
                OperationResult::Veto(VetoType::UserDefined {
                    message: Some(ref msg),
                }) if msg == "way too much" => {
                    saw_way_too_much = true;

                    match d {
                        Domain::Range { min, max } => {
                            assert!(
                                matches!(min, Bound::Exclusive(v) if v.as_ref() == &five),
                                "Expected min bound to be (5) for 'way too much', got: {}",
                                d
                            );
                            assert!(
                                matches!(max, Bound::Unbounded),
                                "Expected max bound to be +inf for 'way too much', got: {}",
                                d
                            );
                        }
                        other => panic!("Expected range domain for x, got: {}", other),
                    }

                    assert!(
                        d.contains(&six),
                        "x=6 should be in veto('way too much') domain. Domain: {}",
                        d
                    );
                    assert!(
                        !d.contains(&five),
                        "x=5 should not be in veto('way too much') domain. Domain: {}",
                        d
                    );
                }
                OperationResult::Veto(other) => {
                    panic!("Unexpected veto in any-veto results: {:?}", other)
                }
                OperationResult::Value(v) => {
                    panic!("Unexpected value result in any-veto results: {:?}", v)
                }
            }
        }

        assert!(
            saw_too_little,
            "Expected at least one veto('too little') solution"
        );
        assert!(
            saw_too_much,
            "Expected at least one veto('too much') solution"
        );
        assert!(
            saw_way_too_much,
            "Expected at least one veto('way too much') solution"
        );
    }
}
