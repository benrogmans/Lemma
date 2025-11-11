//! Shape representation for inversion results

use crate::{Expression, FactReference, LiteralValue};
use serde::ser::{Serialize, SerializeMap, SerializeStruct, Serializer};
use std::fmt;

/// A shape representing the solution space for an inversion query
///
/// Contains one or more branches, each representing a solution.
/// Each branch specifies conditions and the corresponding outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// Solution branches - each branch is a valid solution
    pub branches: Vec<ShapeBranch>,

    /// Variables that are not fully constrained (free to vary)
    pub free_variables: Vec<FactReference>,
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

/// Domain specification for valid values
#[derive(Debug, Clone, PartialEq)]
pub enum Domain {
    /// A single continuous range
    Range { min: Bound, max: Bound },

    /// Multiple disjoint ranges
    Union(Vec<Domain>),

    /// Specific enumerated values only
    Enumeration(Vec<LiteralValue>),

    /// Everything except these constraints
    Complement(Box<Domain>),

    /// Any value (no constraints)
    Unconstrained,
}

/// Bound specification for ranges
#[derive(Debug, Clone, PartialEq)]
pub enum Bound {
    /// Inclusive bound [value
    Inclusive(LiteralValue),

    /// Exclusive bound (value
    Exclusive(LiteralValue),

    /// Unbounded (-∞ or +∞)
    Unbounded,
}

impl Shape {
    /// Create a new shape
    pub fn new(branches: Vec<ShapeBranch>, free_variables: Vec<FactReference>) -> Self {
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

// ---------------------------
// Display implementations
// ---------------------------

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

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Domain::Unconstrained => write!(f, "any"),
            Domain::Enumeration(vals) => {
                write!(f, "{{")?;
                for (i, v) in vals.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "}}")
            }
            Domain::Range { min, max } => {
                // Represent ranges in mathematical interval notation: (a, b], [a, +∞), etc.
                let (l_bracket, r_bracket) = match (min, max) {
                    (Bound::Inclusive(_), Bound::Inclusive(_)) => ('[', ']'),
                    (Bound::Inclusive(_), Bound::Exclusive(_)) => ('[', ')'),
                    (Bound::Exclusive(_), Bound::Inclusive(_)) => ('(', ']'),
                    (Bound::Exclusive(_), Bound::Exclusive(_)) => ('(', ')'),
                    (Bound::Unbounded, Bound::Inclusive(_)) => ('(', ']'),
                    (Bound::Unbounded, Bound::Exclusive(_)) => ('(', ')'),
                    (Bound::Inclusive(_), Bound::Unbounded) => ('[', ')'),
                    (Bound::Exclusive(_), Bound::Unbounded) => ('(', ')'),
                    (Bound::Unbounded, Bound::Unbounded) => ('(', ')'),
                };

                let min_str = match min {
                    Bound::Unbounded => "-∞".to_string(),
                    Bound::Inclusive(v) | Bound::Exclusive(v) => v.to_string(),
                };
                let max_str = match max {
                    Bound::Unbounded => "+∞".to_string(),
                    Bound::Inclusive(v) | Bound::Exclusive(v) => v.to_string(),
                };
                write!(f, "{}{}, {}{}", l_bracket, min_str, max_str, r_bracket)
            }
            Domain::Union(parts) => {
                for (i, p) in parts.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ∪ ")?;
                    }
                    write!(f, "{}", p)?;
                }
                Ok(())
            }
            Domain::Complement(inner) => write!(f, "not ({})", inner),
        }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bound::Unbounded => write!(f, "∞"),
            Bound::Inclusive(v) => write!(f, "[{}", v),
            Bound::Exclusive(v) => write!(f, "({}", v),
        }
    }
}

// ---------------------------
// Serialize implementations
// ---------------------------

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

impl Serialize for FactReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.reference.join("."))
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

impl Serialize for Domain {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Domain::Unconstrained => {
                let mut st = serializer.serialize_struct("domain", 1)?;
                st.serialize_field("type", "unconstrained")?;
                st.end()
            }
            Domain::Enumeration(vals) => {
                let mut st = serializer.serialize_struct("domain", 2)?;
                st.serialize_field("type", "enumeration")?;
                st.serialize_field("values", vals)?;
                st.end()
            }
            Domain::Range { min, max } => {
                let mut st = serializer.serialize_struct("domain", 3)?;
                st.serialize_field("type", "range")?;
                st.serialize_field("min", min)?;
                st.serialize_field("max", max)?;
                st.end()
            }
            Domain::Union(parts) => {
                let mut st = serializer.serialize_struct("domain", 2)?;
                st.serialize_field("type", "union")?;
                st.serialize_field("parts", parts)?;
                st.end()
            }
            Domain::Complement(inner) => {
                let mut st = serializer.serialize_struct("domain", 2)?;
                st.serialize_field("type", "complement")?;
                st.serialize_field("inner", inner)?;
                st.end()
            }
        }
    }
}

impl Serialize for Bound {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Bound::Unbounded => {
                let mut st = serializer.serialize_struct("bound", 1)?;
                st.serialize_field("type", "unbounded")?;
                st.end()
            }
            Bound::Inclusive(v) => {
                let mut st = serializer.serialize_struct("bound", 2)?;
                st.serialize_field("type", "inclusive")?;
                st.serialize_field("value", v)?;
                st.end()
            }
            Bound::Exclusive(v) => {
                let mut st = serializer.serialize_struct("bound", 2)?;
                st.serialize_field("type", "exclusive")?;
                st.serialize_field("value", v)?;
                st.end()
            }
        }
    }
}
