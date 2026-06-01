//! Operation types and result handling for evaluation

use std::fmt;

use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, DataPath, LemmaType, LiteralValue,
    LogicalComputation, MathematicalComputation, SemanticConversionTarget, SemanticDateTime,
    SemanticTime, TypeSpecification,
};
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

    pub fn number(number: rust_decimal::Decimal) -> Self {
        Self::Value(Box::new(LiteralValue::number_from_decimal(number)))
    }

    pub fn quantity(
        value: rust_decimal::Decimal,
        unit: impl Into<String>,
        lemma_type: Option<LemmaType>,
    ) -> Self {
        use crate::computation::rational::checked_mul;
        let lemma_type = std::sync::Arc::new(
            lemma_type.unwrap_or_else(|| LemmaType::primitive(TypeSpecification::quantity())),
        );
        let unit_name = unit.into();
        let rational = crate::literals::rational_from_parsed_decimal(value)
            .expect("BUG: operation result quantity must lift at boundary");
        let factor = if let TypeSpecification::Quantity { units, .. } = &lemma_type.specifications {
            units.get(&unit_name).map(|u| u.factor).unwrap_or_else(|_| {
                panic!(
                    "BUG: OperationResult::quantity unit '{}' not declared on type",
                    unit_name
                )
            })
        } else {
            crate::computation::rational::rational_one()
        };
        let canonical = checked_mul(&rational, &factor)
            .expect("BUG: quantity canonicalization overflow in OperationResult::quantity");
        Self::Value(Box::new(LiteralValue::quantity_with_type(
            canonical, unit_name, lemma_type,
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

    pub fn ratio(rational: rust_decimal::Decimal) -> Self {
        Self::Value(Box::new(LiteralValue::ratio_from_decimal(rational, None)))
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
    Mathematical(MathematicalComputation),
    Logical(LogicalComputation),
    UnitConversion {
        target: SemanticConversionTarget,
    },
    /// Operand result was tested for veto (`is veto` syntax).
    ResultIsVeto,
}

#[cfg(test)]
mod computation_kind_serde_tests {
    use super::ComputationKind;
    use crate::parsing::ast::{
        ArithmeticComputation, ComparisonComputation, MathematicalComputation,
    };

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
    fn computation_kind_mathematical_round_trip() {
        let k = ComputationKind::Mathematical(MathematicalComputation::Sqrt);
        let json = serde_json::to_string(&k).expect("serialize");
        let back: ComputationKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, k);
    }

    #[test]
    fn computation_kind_unit_conversion_round_trip() {
        use crate::parsing::ast::PrimitiveKind;
        use crate::planning::semantics::SemanticConversionTarget;
        let k = ComputationKind::UnitConversion {
            target: SemanticConversionTarget::Type(PrimitiveKind::Number),
        };
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
