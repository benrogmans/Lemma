//! Shape representation for inversion results

use crate::{Expression, FactPath};
use serde::ser::{Serialize, SerializeMap, SerializeStruct, Serializer};
use std::collections::HashMap;
use std::fmt;

use super::Domain;

/// A shape representing the solution space for an inversion query
///
/// Contains one or more branches, each representing a solution.
/// Each branch specifies conditions and the corresponding outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// Solution branches - each branch is a valid solution
    pub branches: Vec<ShapeBranch>,

    /// Variables that are not fully constrained (free to vary)
    pub free_variables: Vec<FactPath>,
}

/// A single branch in a shape - represents one solution
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeBranch {
    /// Condition when this branch applies
    pub condition: Expression,

    /// Outcome when condition is met (value expression or veto)
    pub outcome: BranchOutcome,
}

/// Outcome of a piecewise branch
#[derive(Debug, Clone, PartialEq)]
pub enum BranchOutcome {
    /// Produces a value defined by an expression
    Value(Expression),
    /// Produces a veto with an optional message
    Veto(Option<String>),
}

impl Shape {
    /// Create a new shape
    pub fn new(branches: Vec<ShapeBranch>, free_variables: Vec<FactPath>) -> Self {
        Shape {
            branches,
            free_variables,
        }
    }

    /// Check if this shape has any free variables
    pub fn is_fully_constrained(&self) -> bool {
        self.free_variables.is_empty()
    }
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.branches.len() == 1 {
            write!(f, "{}", self.branches[0])
        } else {
            writeln!(f, "shape with {} branches:", self.branches.len())?;
            for (i, br) in self.branches.iter().enumerate() {
                writeln!(f, "  {}. {}", i + 1, br)?;
            }
            Ok(())
        }
    }
}

impl fmt::Display for ShapeBranch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "if {} then {}", self.condition, self.outcome)
    }
}

impl fmt::Display for BranchOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BranchOutcome::Value(expr) => write!(f, "{}", expr),
            BranchOutcome::Veto(Some(msg)) => write!(f, "veto \"{}\"", msg),
            BranchOutcome::Veto(None) => write!(f, "veto"),
        }
    }
}

impl Serialize for Shape {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("shape", 2)?;
        st.serialize_field("branches", &self.branches)?;
        st.serialize_field("free_variables", &self.free_variables)?;
        st.end()
    }
}

impl Serialize for ShapeBranch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("shape_branch", 2)?;
        st.serialize_field("condition", &self.condition.to_string())?;
        st.serialize_field("outcome", &self.outcome)?;
        st.end()
    }
}

impl Serialize for BranchOutcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            BranchOutcome::Value(expr) => {
                let mut st = serializer.serialize_map(Some(2))?;
                st.serialize_entry("type", "value")?;
                st.serialize_entry("expression", &expr.to_string())?;
                st.end()
            }
            BranchOutcome::Veto(msg) => {
                let mut st = serializer.serialize_map(Some(2))?;
                st.serialize_entry("type", "veto")?;
                if let Some(m) = msg {
                    st.serialize_entry("message", m)?;
                }
                st.end()
            }
        }
    }
}

/// A solution from inversion - maps each free variable to its valid domain
pub type Solution = HashMap<FactPath, Domain>;

/// Response from an inversion query
///
/// Contains:
/// - `solutions`: Concrete domain constraints for each free variable (one per branch)
/// - `shape`: The symbolic representation of the solution space
/// - `free_variables`: Facts that are not fully determined by the given values
/// - `is_fully_constrained`: Whether all facts have concrete values (no free variables)
#[derive(Debug, Clone, PartialEq)]
pub struct InversionResponse {
    /// Concrete solutions - each maps facts to their valid domains
    pub solutions: Vec<Solution>,

    /// The symbolic shape (piecewise function) of the solution
    pub shape: Shape,

    /// Facts that remain undetermined (free to vary within constraints)
    pub free_variables: Vec<FactPath>,

    /// True if there are no free variables (all facts have concrete values)
    pub is_fully_constrained: bool,
}

impl InversionResponse {
    /// Create a new InversionResponse from a shape and solutions
    pub fn new(shape: Shape, solutions: Vec<Solution>) -> Self {
        let free_variables = shape.free_variables.clone();
        let is_fully_constrained = free_variables.is_empty();
        Self {
            solutions,
            shape,
            free_variables,
            is_fully_constrained,
        }
    }

    /// Get number of solutions
    pub fn len(&self) -> usize {
        self.solutions.len()
    }

    /// Check if solutions list is empty
    pub fn is_empty(&self) -> bool {
        self.solutions.is_empty()
    }

    /// Iterate over solutions
    pub fn iter(&self) -> impl Iterator<Item = &Solution> {
        self.solutions.iter()
    }
}

impl fmt::Display for InversionResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Inversion Result:")?;
        writeln!(f, "  Solutions: {}", self.solutions.len())?;
        writeln!(f, "  Free variables: {:?}", self.free_variables)?;
        writeln!(f, "  Fully constrained: {}", self.is_fully_constrained)?;
        if !self.solutions.is_empty() {
            writeln!(f, "  Domains:")?;
            for (i, solution) in self.solutions.iter().enumerate() {
                writeln!(f, "    Solution {}:", i + 1)?;
                for (fact, domain) in solution {
                    writeln!(f, "      {}: {}", fact, domain)?;
                }
            }
        }
        Ok(())
    }
}

impl Serialize for InversionResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("inversion_response", 4)?;
        st.serialize_field("solutions", &self.solutions)?;
        st.serialize_field("shape", &self.shape)?;
        st.serialize_field("free_variables", &self.free_variables)?;
        st.serialize_field("is_fully_constrained", &self.is_fully_constrained)?;
        st.end()
    }
}
