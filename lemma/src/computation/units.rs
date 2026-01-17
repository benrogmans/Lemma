//! Unit conversion system
//!
//! Handles conversions between duration units.
//! Returns OperationResult with Veto for errors instead of Result.

use crate::evaluation::OperationResult;
use crate::semantic::{DurationUnit, LiteralValue, Value};
use crate::ConversionTarget;
use rust_decimal::Decimal;

/// Convert a value to a target unit (for `in` operator).
///
/// Returns OperationResult with Veto for errors.
pub fn convert_unit(value: &LiteralValue, target: &ConversionTarget) -> OperationResult {
    match &value.value {
        Value::Duration(v, from) => match target {
            ConversionTarget::Duration(to) => match convert_duration(*v, from, to) {
                Ok(val) => OperationResult::Value(LiteralValue::duration_with_type(
                    val,
                    to.clone(),
                    value.lemma_type.clone(),
                )),
                Err(msg) => OperationResult::Veto(Some(msg)),
            },
            ConversionTarget::Percentage => {
                OperationResult::Veto(Some("Cannot convert duration to percent".to_string()))
            }
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
        },

        Value::Ratio(r, unit_opt) => match target {
            ConversionTarget::Percentage => OperationResult::Value(LiteralValue::ratio_with_type(
                *r,
                unit_opt.clone().or(Some("percent".to_string())),
                value.lemma_type.clone(),
            )),
            _ => OperationResult::Veto(Some("Cannot convert ratio to unit".to_string())),
        },

        _ => OperationResult::Veto(Some(format!("Cannot convert {} to {}", value, target))),
    }
}

/// Convert a duration value between units
fn convert_duration(
    value: Decimal,
    from: &DurationUnit,
    to: &DurationUnit,
) -> Result<Decimal, String> {
    if from == to {
        return Ok(value);
    }

    let seconds = duration_to_seconds(value, from);
    Ok(seconds_to_duration(seconds, to))
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
        assert_eq!(result, Ok(Decimal::from(120)));
    }

    #[test]
    fn duration_seconds_roundtrip() {
        let original = Decimal::from(5);
        let seconds = duration_to_seconds(original, &DurationUnit::Day);
        let back = seconds_to_duration(seconds, &DurationUnit::Day);
        assert_eq!(original, back);
    }
}
