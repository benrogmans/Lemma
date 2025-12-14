//! World structure for inversion
//!
//! A World represents one "universe" where specific constraints hold.
//! Each world has a value expression and constraints that define when it's valid.

use crate::algebra::constraints::FactConstraint;
use crate::semantic::{Expression, FactPath};
use std::collections::HashMap;

/// A World represents one "universe" where specific constraints hold
#[derive(Clone, Debug)]
pub struct World {
    /// Constraints defining this universe (e.g., income in [11000, 44000])
    pub constraints: HashMap<FactPath, FactConstraint>,

    /// The value expression valid in this universe
    /// Can be literal, linear, or non-linear (sqrt, pow, etc.)
    pub value: Expression,
}

impl World {
    /// Merge two worlds (used in cross-product)
    /// Returns None if constraints contradict
    pub fn merge(
        &self,
        other: &World,
        combine_values: impl FnOnce(&Expression, &Expression) -> Expression,
    ) -> Option<World> {
        let mut new_constraints = self.constraints.clone();

        // Constraint intersection - THE KEY PRUNING MECHANISM
        for (fact, constraint_b) in &other.constraints {
            match new_constraints.get(fact) {
                Some(constraint_a) => {
                    // If they contradict, return None (world is impossible)
                    let intersection = constraint_a.intersect(constraint_b);
                    if !intersection.is_satisfiable() {
                        return None;
                    }
                    new_constraints.insert(fact.clone(), intersection);
                }
                None => {
                    new_constraints.insert(fact.clone(), constraint_b.clone());
                }
            }
        }

        // Combine values using provided function
        let new_value = combine_values(&self.value, &other.value);

        Some(World {
            constraints: new_constraints,
            value: new_value,
        })
    }
}
