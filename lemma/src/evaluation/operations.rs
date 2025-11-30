//! Type-aware arithmetic and comparison operations
//!
//! Handles operations on different types: Number, Money, Percentage, Duration, etc.
//! Returns OperationResult with Veto for runtime errors instead of Result.

use crate::{
    ArithmeticComputation, ComparisonComputation, ExpressionId, FactPath, LiteralValue,
    LogicalComputation, MathematicalComputation, RulePath,
};
use rust_decimal::Decimal;
use serde::Serialize;

/// Result of an operation (evaluating a rule or expression)
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum OperationResult {
    /// Operation produced a value
    Value(LiteralValue),
    /// Operation was vetoed (valid result, no value)
    Veto(Option<String>),
}

impl OperationResult {
    pub fn is_veto(&self) -> bool {
        matches!(self, OperationResult::Veto(_))
    }

    #[must_use]
    pub fn value(&self) -> Option<&LiteralValue> {
        match self {
            OperationResult::Value(v) => Some(v),
            OperationResult::Veto(_) => None,
        }
    }
}

/// The kind of computation performed
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComputationKind {
    Arithmetic(ArithmeticComputation),
    Comparison(ComparisonComputation),
    Logical(LogicalComputation),
    Mathematical(MathematicalComputation),
}

/// A record of a single operation during evaluation
#[derive(Debug, Clone, Serialize)]
pub struct OperationRecord {
    pub expression_id: ExpressionId,
    #[serde(flatten)]
    pub kind: OperationKind,
}

/// The kind of operation performed
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationKind {
    FactUsed {
        fact_ref: FactPath,
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
        #[serde(skip_serializing_if = "Option::is_none", default)]
        expr: Option<String>,
    },
    RuleBranchEvaluated {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
        matched: bool,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        condition_expr: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_expr: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        result_value: Option<OperationResult>,
    },
}

const PERCENT_DENOMINATOR: i32 = 100;

/// Perform type-aware arithmetic operation, returning OperationResult (Veto on error)
pub fn arithmetic_operation(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (left, right) {
        (LiteralValue::Number(l), LiteralValue::Number(r)) => match number_arithmetic(*l, op, *r) {
            Ok(result) => OperationResult::Value(LiteralValue::Number(result)),
            Err(msg) => OperationResult::Veto(Some(msg)),
        },

        (LiteralValue::Percentage(l), LiteralValue::Number(r)) => match op {
            ArithmeticComputation::Multiply => OperationResult::Value(LiteralValue::Number(
                l * r / Decimal::from(PERCENT_DENOMINATOR),
            )),
            ArithmeticComputation::Divide => {
                if *r == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
                }
                OperationResult::Value(LiteralValue::Percentage(l / r))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for percentage and number",
                op
            ))),
        },

        (LiteralValue::Number(n), LiteralValue::Percentage(p)) => match op {
            ArithmeticComputation::Multiply => OperationResult::Value(LiteralValue::Number(
                n * p / Decimal::from(PERCENT_DENOMINATOR),
            )),
            ArithmeticComputation::Add => OperationResult::Value(LiteralValue::Number(
                n + (n * p / Decimal::from(PERCENT_DENOMINATOR)),
            )),
            ArithmeticComputation::Subtract => OperationResult::Value(LiteralValue::Number(
                n - (n * p / Decimal::from(PERCENT_DENOMINATOR)),
            )),
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for number and percentage",
                op
            ))),
        },

        (LiteralValue::Percentage(l), LiteralValue::Percentage(r)) => match op {
            ArithmeticComputation::Add => OperationResult::Value(LiteralValue::Percentage(l + r)),
            ArithmeticComputation::Subtract => {
                OperationResult::Value(LiteralValue::Percentage(l - r))
            }
            ArithmeticComputation::Multiply => OperationResult::Value(LiteralValue::Percentage(
                l * r / Decimal::from(PERCENT_DENOMINATOR),
            )),
            ArithmeticComputation::Divide => {
                if *r == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
                }
                OperationResult::Value(LiteralValue::Number(l / r))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for percentage and percentage",
                op
            ))),
        },

        (LiteralValue::Date(_), _) | (_, LiteralValue::Date(_)) => {
            super::datetime::datetime_arithmetic(left, op, right)
        }

        (LiteralValue::Time(_), _) | (_, LiteralValue::Time(_)) => {
            super::datetime::time_arithmetic(left, op, right)
        }

        // Same category unit operations (e.g., Length + Length)
        // Convert to base units for correct arithmetic, then back to left unit type
        (LiteralValue::Unit(l), LiteralValue::Unit(r)) if l.same_category(r) => {
            let left_base = super::units::to_base_unit_value(l);
            let right_base = super::units::to_base_unit_value(r);

            match op {
                ArithmeticComputation::Add => {
                    // Add in base units, then convert back to left's unit
                    let result_base = left_base + right_base;
                    let left_value = l.value();
                    let left_base_value = super::units::to_base_unit_value(l);
                    // Conversion factor: left_value / left_base_value
                    // result_in_left_unit = result_base * (left_value / left_base_value)
                    let result_value = if left_base_value == Decimal::ZERO {
                        result_base
                    } else {
                        result_base * left_value / left_base_value
                    };
                    OperationResult::Value(LiteralValue::Unit(l.with_value(result_value)))
                }
                ArithmeticComputation::Subtract => {
                    let result_base = left_base - right_base;
                    let left_value = l.value();
                    let left_base_value = super::units::to_base_unit_value(l);
                    let result_value = if left_base_value == Decimal::ZERO {
                        result_base
                    } else {
                        result_base * left_value / left_base_value
                    };
                    OperationResult::Value(LiteralValue::Unit(l.with_value(result_value)))
                }
                ArithmeticComputation::Multiply => {
                    OperationResult::Value(LiteralValue::Number(left_base * right_base))
                }
                ArithmeticComputation::Divide => {
                    if right_base == Decimal::ZERO {
                        return OperationResult::Veto(Some("Division by zero".to_string()));
                    }
                    OperationResult::Value(LiteralValue::Number(left_base / right_base))
                }
                _ => OperationResult::Veto(Some(format!(
                    "Operation {:?} not supported for same-category units",
                    op
                ))),
            }
        }

        // Different category unit operations produce dimensionless numbers
        (LiteralValue::Unit(l), LiteralValue::Unit(r)) => match op {
            ArithmeticComputation::Multiply => {
                OperationResult::Value(LiteralValue::Number(l.value() * r.value()))
            }
            ArithmeticComputation::Divide => {
                if r.value() == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
                }
                OperationResult::Value(LiteralValue::Number(l.value() / r.value()))
            }
            _ => OperationResult::Veto(Some(format!(
                "Cannot add/subtract different unit categories: {:?} and {:?}",
                type_name(left),
                type_name(right)
            ))),
        },

        // Number and Unit operations
        (LiteralValue::Number(n), LiteralValue::Unit(u)) => match op {
            ArithmeticComputation::Multiply => {
                OperationResult::Value(LiteralValue::Unit(u.with_value(*n * u.value())))
            }
            ArithmeticComputation::Divide => {
                if u.value() == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
                }
                OperationResult::Value(LiteralValue::Number(*n / u.value()))
            }
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for number and unit",
                op
            ))),
        },

        (LiteralValue::Unit(u), LiteralValue::Number(n)) => match op {
            ArithmeticComputation::Multiply => {
                OperationResult::Value(LiteralValue::Unit(u.with_value(u.value() * *n)))
            }
            ArithmeticComputation::Divide => {
                if *n == Decimal::ZERO {
                    return OperationResult::Veto(Some("Division by zero".to_string()));
                }
                OperationResult::Value(LiteralValue::Unit(u.with_value(u.value() / *n)))
            }
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => OperationResult::Veto(
                Some("Cannot add/subtract number and unit directly".to_string()),
            ),
            _ => OperationResult::Veto(Some(format!(
                "Operation {:?} not supported for unit and number",
                op
            ))),
        },

        _ => OperationResult::Veto(Some(format!(
            "Arithmetic operation {:?} not supported for types {:?} and {:?}",
            op,
            type_name(left),
            type_name(right)
        ))),
    }
}

fn number_arithmetic(
    left: Decimal,
    op: &ArithmeticComputation,
    right: Decimal,
) -> Result<Decimal, String> {
    use rust_decimal::prelude::ToPrimitive;

    match op {
        ArithmeticComputation::Add => Ok(left + right),
        ArithmeticComputation::Subtract => Ok(left - right),
        ArithmeticComputation::Multiply => Ok(left * right),
        ArithmeticComputation::Divide => {
            if right == Decimal::ZERO {
                return Err("Division by zero".to_string());
            }
            Ok(left / right)
        }
        ArithmeticComputation::Modulo => {
            if right == Decimal::ZERO {
                return Err("Division by zero (modulo)".to_string());
            }
            Ok(left % right)
        }
        ArithmeticComputation::Power => {
            let base = left
                .to_f64()
                .ok_or_else(|| "Cannot convert base to float".to_string())?;
            let exp = right
                .to_f64()
                .ok_or_else(|| "Cannot convert exponent to float".to_string())?;
            let result = base.powf(exp);
            Decimal::from_f64_retain(result)
                .ok_or_else(|| "Power result cannot be represented".to_string())
        }
    }
}

/// Perform type-aware comparison, returning OperationResult (Veto on error)
pub fn comparison_operation(
    left: &LiteralValue,
    op: &ComparisonComputation,
    right: &LiteralValue,
) -> OperationResult {
    match (left, right) {
        (LiteralValue::Number(l), LiteralValue::Number(r)) => {
            OperationResult::Value(LiteralValue::Boolean(compare_decimals(*l, op, r).into()))
        }

        (LiteralValue::Boolean(l), LiteralValue::Boolean(r)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => {
                OperationResult::Value(LiteralValue::Boolean((l == r).into()))
            }
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => {
                OperationResult::Value(LiteralValue::Boolean((l != r).into()))
            }
            _ => OperationResult::Veto(Some("Can only use == and != with booleans".to_string())),
        },

        (LiteralValue::Text(l), LiteralValue::Text(r)) => match op {
            ComparisonComputation::Equal | ComparisonComputation::Is => {
                OperationResult::Value(LiteralValue::Boolean((l == r).into()))
            }
            ComparisonComputation::NotEqual | ComparisonComputation::IsNot => {
                OperationResult::Value(LiteralValue::Boolean((l != r).into()))
            }
            _ => OperationResult::Veto(Some("Can only use == and != with text".to_string())),
        },

        (LiteralValue::Percentage(l), LiteralValue::Percentage(r)) => {
            OperationResult::Value(LiteralValue::Boolean(compare_decimals(*l, op, r).into()))
        }

        (LiteralValue::Date(_), LiteralValue::Date(_)) => {
            super::datetime::datetime_comparison(left, op, right)
        }

        // Unit types with the same category can be compared
        // Convert both to base units first to ensure correct comparison
        (LiteralValue::Unit(l), LiteralValue::Unit(r)) if l.same_category(r) => {
            let left_base = super::units::to_base_unit_value(l);
            let right_base = super::units::to_base_unit_value(r);
            OperationResult::Value(LiteralValue::Boolean(
                compare_decimals(left_base, op, &right_base).into(),
            ))
        }

        // Comparing unit to number extracts the unit's value for comparison
        (LiteralValue::Unit(u), LiteralValue::Number(n)) => OperationResult::Value(
            LiteralValue::Boolean(compare_decimals(u.value(), op, n).into()),
        ),
        (LiteralValue::Number(n), LiteralValue::Unit(u)) => OperationResult::Value(
            LiteralValue::Boolean(compare_decimals(*n, op, &u.value()).into()),
        ),

        // Different category units: compare numeric values
        (LiteralValue::Unit(l), LiteralValue::Unit(r)) => OperationResult::Value(
            LiteralValue::Boolean(compare_decimals(l.value(), op, &r.value()).into()),
        ),

        _ => OperationResult::Veto(Some(format!(
            "Comparison {:?} not supported for types {:?} and {:?}",
            op,
            type_name(left),
            type_name(right)
        ))),
    }
}

fn compare_decimals(left: Decimal, op: &ComparisonComputation, right: &Decimal) -> bool {
    match op {
        ComparisonComputation::GreaterThan => left > *right,
        ComparisonComputation::LessThan => left < *right,
        ComparisonComputation::GreaterThanOrEqual => left >= *right,
        ComparisonComputation::LessThanOrEqual => left <= *right,
        ComparisonComputation::Equal | ComparisonComputation::Is => left == *right,
        ComparisonComputation::NotEqual | ComparisonComputation::IsNot => left != *right,
    }
}

fn type_name(value: &LiteralValue) -> String {
    value.to_type().to_string()
}
