//! Constraint-based inverse reasoning for Lemma rules
//!
//! Determines what inputs produce desired outputs by building and solving constraint equations.
//!
//! The main entry point is [`invert()`], which returns an [`InversionResponse`]
//! containing all valid solutions with their domains.

mod constraint;
mod domain;
mod target;

pub use domain::{extract_fact_constraints_from_rule_constraint, Bound, FactConstraint};
pub use target::{Target, TargetOp};

use crate::planning::ExecutionPlan;
use crate::{FactPath, LemmaError, LemmaResult, OperationResult};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::collections::{HashMap, HashSet};

/// A single solution from inversion
///
/// Contains the outcome for a solution. For fact constraints,
/// use the corresponding entry in `InversionResponse.domains`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Solution {
    /// The outcome (value or veto)
    pub outcome: OperationResult,
}

/// Response from inversion containing all valid solutions
#[derive(Debug, Clone)]
pub struct InversionResponse {
    /// All valid solutions
    pub solutions: Vec<Solution>,
    /// Fact constraints for each solution (indexed by solution index)
    pub domains: Vec<HashMap<FactPath, FactConstraint>>,
    /// Facts that still need values (appear in conditions but aren't fully constrained)
    pub undetermined_facts: Vec<FactPath>,
    /// True if all facts are fully constrained to specific values
    pub is_determined: bool,
}

impl InversionResponse {
    /// Create a new inversion response, computing metadata from solutions and domains
    pub fn new(solutions: Vec<Solution>, domains: Vec<HashMap<FactPath, FactConstraint>>) -> Self {
        let mut undetermined: HashSet<FactPath> = HashSet::new();
        for solution_domains in &domains {
            for (fact_path, domain) in solution_domains {
                let is_determined = matches!(
                    domain,
                    FactConstraint::Enumeration(values) if values.len() == 1
                );
                if !is_determined {
                    undetermined.insert(fact_path.clone());
                }
            }
        }
        let mut undetermined_facts: Vec<FactPath> = undetermined.into_iter().collect();
        undetermined_facts.sort_by_key(|a| a.to_string());

        let is_determined = !domains.is_empty()
            && domains.iter().all(|solution_domains| {
                solution_domains.values().all(|domain| {
                    matches!(domain, FactConstraint::Enumeration(values) if values.len() == 1)
                })
            });

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

    let _ = (target, provided_facts, executable_rule);
    Err(LemmaError::Engine(
        "Inversion not yet implemented - constraint building required".to_string(),
    ))
}
