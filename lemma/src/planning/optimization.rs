//! Per-branch optimization during planning
//!
//! Prepares branches for fast inversion by:
//! 1. Expanding conditions to DNF
//! 2. Simplifying (contradiction detection, constant folding)
//! 3. Storing optimized conditions
//!
//! This happens ONCE during document loading, not during every inversion query.

use crate::algebra::{expand, simplification};
use crate::semantic::Expression;
use super::execution_plan::Branch;

/// Optimize all branches for a rule during planning
///
/// Expands + simplifies each branch condition for fast inversion runtime.
/// No cross-rule substitution - just local per-branch optimization.
pub fn optimize_branches(branches: &mut [Branch]) {
    for branch in branches {
        // Expand condition to DNF
        let expanded = expand(branch.condition.clone());
        
        // Simplify (detect contradictions, fold constants, remove redundancies)
        let simplified = simplification::reduce(expanded);
        
        // Store for fast inversion runtime
        branch.optimized_condition = Some(simplified);
    }
}
