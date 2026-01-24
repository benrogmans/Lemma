//! Unit conversion system
//!
//! Handles conversions between duration units and scale units.

use crate::evaluation::OperationResult;
use crate::semantic::{DurationUnit, LiteralValue, TypeSpecification, Unit, Value};
use crate::ConversionTarget;
use rust_decimal::Decimal;

/// Convert a value to a target unit (for `in` operator).
///
pub fn convert_unit(value: &LiteralValue, target: &ConversionTarget) -> OperationResult {
    match &value.value {
        Value::Duration(v, from) => match target {
            ConversionTarget::Duration(to) => {
                let val = convert_duration(*v, from, to);
                OperationResult::Value(LiteralValue::duration_with_type(
                    val,
                    to.clone(),
                    value.lemma_type.clone(),
                ))
            }
            ConversionTarget::Percentage | ConversionTarget::ScaleUnit(_) => unreachable!(
                "BUG: invalid conversion target {:?} for duration; this should be rejected during planning",
                target
            ),
        },

        Value::Number(n) => match target {
            ConversionTarget::Duration(u) => {
                OperationResult::Value(LiteralValue::duration(*n, u.clone()))
            }
            ConversionTarget::Percentage => {
                // Convert number to ratio with percent unit (e.g., 0.5 -> 50%)
                use crate::semantic::standard_ratio;
                OperationResult::Value(LiteralValue::ratio_with_type(
                    *n,
                    Some("percent".to_string()),
                    standard_ratio().clone(),
                ))
            }
            ConversionTarget::ScaleUnit(_) => unreachable!(
                "BUG: converting number to scale unit should be rejected during planning"
            ),
        },

        Value::Ratio(r, unit_opt) => match target {
            ConversionTarget::Percentage => OperationResult::Value(LiteralValue::ratio_with_type(
                *r,
                unit_opt.clone().or(Some("percent".to_string())),
                value.lemma_type.clone(),
            )),
            ConversionTarget::Duration(_) | ConversionTarget::ScaleUnit(_) => unreachable!(
                "BUG: invalid conversion target {:?} for ratio; this should be rejected during planning",
                target
            ),
        },

        Value::Scale(v, from_unit) => match target {
            ConversionTarget::ScaleUnit(to_unit) => {
                let from_unit = match from_unit {
                    Some(u) => u,
                    None => {
                        unreachable!(
                            "BUG: cannot convert scale value without a unit; unit must be provided by parsing/input validation"
                        );
                    }
                };

                let from_factor = scale_unit_factor(&value.lemma_type, from_unit);
                let to_factor = scale_unit_factor(&value.lemma_type, to_unit);

                let converted = (*v) * (to_factor / from_factor);

                OperationResult::Value(LiteralValue::scale_with_type(
                    converted,
                    Some(to_unit.clone()),
                    value.lemma_type.clone(),
                ))
            }
            ConversionTarget::Duration(_) | ConversionTarget::Percentage => unreachable!(
                "BUG: invalid conversion target {:?} for scale; this should be rejected during planning",
                target
            ),
        },

        _ => unreachable!(
            "BUG: unsupported unit conversion during evaluation: {} -> {}",
            value,
            target
        ),
    }
}

fn scale_unit_factor(lemma_type: &crate::semantic::LemmaType, unit_name: &str) -> Decimal {
    let units = match &lemma_type.specifications {
        TypeSpecification::Scale { units, .. } => units,
        _ => unreachable!(
            "BUG: scale_unit_factor called with non-scale type {}",
            lemma_type.name()
        ),
    };

    match units
        .iter()
        .find(|u| u.name.eq_ignore_ascii_case(unit_name))
    {
        Some(Unit { value, .. }) => *value,
        None => {
            let valid: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            unreachable!(
                "BUG: unknown unit '{}' for scale type {}. Valid units: {}",
                unit_name,
                lemma_type.name(),
                valid.join(", ")
            );
        }
    }
}

/// Convert a duration value between units
fn convert_duration(value: Decimal, from: &DurationUnit, to: &DurationUnit) -> Decimal {
    if from == to {
        return value;
    }

    let seconds = duration_to_seconds(value, from);
    seconds_to_duration(seconds, to)
}

/// Convert a duration value to seconds (base unit)
pub fn duration_to_seconds(value: Decimal, unit: &DurationUnit) -> Decimal {
    match unit {
        DurationUnit::Microsecond => value / Decimal::from(1_000_000),
        DurationUnit::Millisecond => value / Decimal::from(1_000),
        DurationUnit::Second => value,
        DurationUnit::Minute => value * Decimal::from(60),
        DurationUnit::Hour => value * Decimal::from(3_600),
        DurationUnit::Day => value * Decimal::from(86_400),
        DurationUnit::Week => value * Decimal::from(604_800),
        DurationUnit::Month => value * Decimal::from(2_592_000), // 30 days
        DurationUnit::Year => value * Decimal::from(31_536_000), // 365 days
    }
}

/// Convert seconds to a duration value in the target unit
pub fn seconds_to_duration(seconds: Decimal, unit: &DurationUnit) -> Decimal {
    match unit {
        DurationUnit::Microsecond => seconds * Decimal::from(1_000_000),
        DurationUnit::Millisecond => seconds * Decimal::from(1_000),
        DurationUnit::Second => seconds,
        DurationUnit::Minute => seconds / Decimal::from(60),
        DurationUnit::Hour => seconds / Decimal::from(3_600),
        DurationUnit::Day => seconds / Decimal::from(86_400),
        DurationUnit::Week => seconds / Decimal::from(604_800),
        DurationUnit::Month => seconds / Decimal::from(2_592_000), // 30 days
        DurationUnit::Year => seconds / Decimal::from(31_536_000), // 365 days
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_conversion() {
        let result = convert_duration(Decimal::from(2), &DurationUnit::Hour, &DurationUnit::Minute);
        assert_eq!(result, Decimal::from(120));
    }

    #[test]
    fn duration_seconds_roundtrip() {
        let original = Decimal::from(5);
        let seconds = duration_to_seconds(original, &DurationUnit::Day);
        let back = seconds_to_duration(seconds, &DurationUnit::Day);
        assert_eq!(original, back);
    }
}
