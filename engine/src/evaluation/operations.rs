//! Operation types and result handling for evaluation

use std::fmt;

use crate::{
    planning::semantics::{
        ArithmeticComputation, ComparisonComputation, DataPath, LiteralValue, LogicalComputation,
        MathematicalComputation, RulePath, SemanticDateTime, SemanticTime,
    },
    LemmaType, SemanticDurationUnit, TypeSpecification,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Why an operation yielded no value (domain veto).
///
/// JSON serialization is a single string (see [`fmt::Display`]). There is intentionally no
/// `Deserialize` implementation: veto payloads are engine output only.
#[derive(Debug, Clone, PartialEq)]
pub enum VetoType {
    /// Evaluation needed a data that was not provided
    MissingData { data: DataPath },
    /// Explicit `veto "reason"` in Lemma source
    UserDefined { message: Option<String> },
    /// Runtime domain failure (division by zero, date overflow, etc.)
    Computation { message: String },
}

impl fmt::Display for VetoType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VetoType::MissingData { data } => write!(f, "Missing data: {}", data),
            VetoType::UserDefined { message: Some(msg) } => write!(f, "{msg}"),
            VetoType::UserDefined { message: None } => write!(f, "Vetoed"),
            VetoType::Computation { message } => write!(f, "{message}"),
        }
    }
}

impl VetoType {
    #[must_use]
    pub fn computation(message: impl Into<String>) -> Self {
        VetoType::Computation {
            message: message.into(),
        }
    }
}

impl Serialize for VetoType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Result of an operation (evaluating a rule or expression)
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationResult {
    /// Operation produced a value (boxed to keep enum small)
    Value(Box<LiteralValue>),
    /// Operation was vetoed (valid result, no value)
    Veto(VetoType),
}

impl OperationResult {
    pub fn vetoed(&self) -> bool {
        matches!(self, OperationResult::Veto(_))
    }

    #[must_use]
    pub fn value(&self) -> Option<&LiteralValue> {
        match self {
            OperationResult::Value(v) => Some(v.as_ref()),
            OperationResult::Veto(_) => None,
        }
    }

    pub fn number(number: impl Into<Decimal>) -> Self {
        Self::Value(Box::new(LiteralValue::number(number.into())))
    }

    pub fn scale(
        scale: impl Into<Decimal>,
        unit: impl Into<String>,
        lemma_type: Option<LemmaType>,
    ) -> Self {
        let lemma_type =
            lemma_type.unwrap_or_else(|| LemmaType::primitive(TypeSpecification::scale()));
        Self::Value(Box::new(LiteralValue::scale_with_type(
            scale.into(),
            unit.into(),
            lemma_type,
        )))
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::Value(Box::new(LiteralValue::text(text.into())))
    }

    pub fn date(date: impl Into<SemanticDateTime>) -> Self {
        Self::Value(Box::new(LiteralValue::date(date.into())))
    }

    pub fn time(time: impl Into<SemanticTime>) -> Self {
        Self::Value(Box::new(LiteralValue::time(time.into())))
    }

    pub fn boolean(boolean: bool) -> Self {
        Self::Value(Box::new(LiteralValue::from_bool(boolean)))
    }

    pub fn duration(duration: impl Into<Decimal>, unit: impl Into<SemanticDurationUnit>) -> Self {
        Self::Value(Box::new(LiteralValue::duration(
            duration.into(),
            unit.into(),
        )))
    }

    pub fn ratio(ratio: impl Into<Decimal>) -> Self {
        Self::Value(Box::new(LiteralValue::ratio(ratio.into(), None)))
    }

    pub fn veto(veto: impl Into<String>) -> Self {
        Self::Veto(VetoType::UserDefined {
            message: Some(veto.into()),
        })
    }
}

/// The kind of computation performed
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "computation", rename_all = "snake_case")]
pub enum ComputationKind {
    Arithmetic(ArithmeticComputation),
    Comparison(ComparisonComputation),
    Logical(LogicalComputation),
    Mathematical(MathematicalComputation),
}

/// A record of a single operation during evaluation
#[derive(Debug, Clone, Serialize)]
pub struct OperationRecord {
    #[serde(flatten)]
    pub kind: OperationKind,
}

/// The kind of operation performed
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationKind {
    DataUsed {
        data_ref: DataPath,
        value: LiteralValue,
    },
    RuleUsed {
        rule_path: RulePath,
        result: OperationResult,
    },
    Computation {
        kind: ComputationKind,
        inputs: Vec<LiteralValue>,
        result: LiteralValue,
    },
    RuleBranchEvaluated {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
        matched: bool,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_value: Option<OperationResult>,
    },
}

#[cfg(test)]
mod computation_kind_serde_tests {
    use super::ComputationKind;
    use crate::parsing::ast::{
        ArithmeticComputation, ComparisonComputation, MathematicalComputation,
    };
    use crate::planning::semantics::LogicalComputation;

    #[test]
    fn computation_kind_arithmetic_round_trip() {
        let k = ComputationKind::Arithmetic(ArithmeticComputation::Add);
        let json = serde_json::to_string(&k).expect("serialize");
        assert!(json.contains("\"type\"") && json.contains("\"computation\""));
        let back: ComputationKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, k);
    }

    #[test]
    fn computation_kind_comparison_round_trip() {
        let k = ComputationKind::Comparison(ComparisonComputation::GreaterThan);
        let json = serde_json::to_string(&k).expect("serialize");
        let back: ComputationKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, k);
    }

    #[test]
    fn computation_kind_logical_round_trip() {
        let k = ComputationKind::Logical(LogicalComputation::And);
        let json = serde_json::to_string(&k).expect("serialize");
        let back: ComputationKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, k);
    }

    #[test]
    fn computation_kind_mathematical_round_trip() {
        let k = ComputationKind::Mathematical(MathematicalComputation::Sqrt);
        let json = serde_json::to_string(&k).expect("serialize");
        let back: ComputationKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, k);
    }

    #[test]
    fn veto_type_serializes_as_display_string() {
        use super::VetoType;
        use crate::planning::semantics::DataPath;
        let v = VetoType::MissingData {
            data: DataPath::new(vec![], "product".to_string()),
        };
        let json = serde_json::to_string(&v).expect("serialize");
        assert_eq!(json, "\"Missing data: product\"");
    }
}
