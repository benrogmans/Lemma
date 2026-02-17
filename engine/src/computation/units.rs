//! Unit conversion system
//!
//! Handles conversions between duration units and scale units.
//! Uses only planning/semantic types; no dependency on parsing types.

use crate::evaluation::OperationResult;
use crate::planning::semantics::{
    LiteralValue, SemanticConversionTarget, SemanticDurationUnit, ValueKind,
};
use rust_decimal::Decimal;

/// Convert a value to a target unit (for `in` operator).
pub fn convert_unit(value: &LiteralValue, target: &SemanticConversionTarget) -> OperationResult {
    match &value.value {
        ValueKind::Duration(v, from) => match target {
            SemanticConversionTarget::Duration(to) => {
                let val = convert_duration(*v, from, to);
                OperationResult::Value(Box::new(LiteralValue::duration_with_type(
                    val,
                    to.clone(),
                    value.lemma_type.clone(),
                )))
            }
            _ => unreachable!(
                "BUG: invalid conversion target {:?} for duration; this should be rejected during planning",
                target
            ),
        },

        ValueKind::Number(n) => match target {
            SemanticConversionTarget::Duration(u) => {
                OperationResult::Value(Box::new(LiteralValue::duration(*n, u.clone())))
            }
            SemanticConversionTarget::ScaleUnit(unit) => {
                OperationResult::Value(Box::new(LiteralValue::number_interpreted_as_scale(*n, unit.clone())))
            }
            SemanticConversionTarget::RatioUnit(unit) => {
                OperationResult::Value(Box::new(LiteralValue::ratio(*n, Some(unit.clone()))))
            }
        },

        ValueKind::Ratio(ratio_value, _from_unit_opt) => match target {
            SemanticConversionTarget::RatioUnit(to_unit) => OperationResult::Value(Box::new(
                LiteralValue::ratio(*ratio_value, Some(to_unit.clone())),
            )),
            _ => unreachable!(
                "BUG: invalid conversion target {:?} for ratio; this should be rejected during planning",
                target
            ),
        },

        ValueKind::Scale(v, from_unit) => match target {
            SemanticConversionTarget::ScaleUnit(to_unit) => {
                let from_factor = value.lemma_type.scale_unit_factor(from_unit);
                let to_factor = value.lemma_type.scale_unit_factor(to_unit);

                let converted = (*v) * (to_factor / from_factor);

                OperationResult::Value(Box::new(LiteralValue::scale_with_type(
                    converted,
                    to_unit.clone(),
                    value.lemma_type.clone(),
                )))
            }
            SemanticConversionTarget::Duration(duration_unit) => {
                OperationResult::Value(Box::new(LiteralValue::duration(*v, duration_unit.clone())))
            }
            SemanticConversionTarget::RatioUnit(_) => unreachable!(
                "BUG: cannot convert scale to ratio unit; this should be rejected during planning"
            ),
        },

        _ => unreachable!(
            "BUG: unsupported unit conversion during evaluation: {} -> {}",
            value,
            target
        ),
    }
}

/// Convert a duration value between units
fn convert_duration(
    value: Decimal,
    from: &SemanticDurationUnit,
    to: &SemanticDurationUnit,
) -> Decimal {
    if from == to {
        return value;
    }

    let seconds = duration_to_seconds(value, from);
    seconds_to_duration(seconds, to)
}

/// Convert a duration value to seconds (base unit).
/// Note: months and years use approximate values (30 and 365 days respectively).
pub fn duration_to_seconds(value: Decimal, unit: &SemanticDurationUnit) -> Decimal {
    match unit {
        SemanticDurationUnit::Microsecond => value / Decimal::from(1_000_000),
        SemanticDurationUnit::Millisecond => value / Decimal::from(1_000),
        SemanticDurationUnit::Second => value,
        SemanticDurationUnit::Minute => value * Decimal::from(60),
        SemanticDurationUnit::Hour => value * Decimal::from(3_600),
        SemanticDurationUnit::Day => value * Decimal::from(86_400),
        SemanticDurationUnit::Week => value * Decimal::from(604_800),
        SemanticDurationUnit::Month => value * Decimal::from(2_592_000), // 30 days
        SemanticDurationUnit::Year => value * Decimal::from(31_536_000), // 365 days
    }
}

/// Convert seconds to a duration value in the target unit.
/// Note: months and years use approximate values (30 and 365 days respectively).
pub fn seconds_to_duration(seconds: Decimal, unit: &SemanticDurationUnit) -> Decimal {
    match unit {
        SemanticDurationUnit::Microsecond => seconds * Decimal::from(1_000_000),
        SemanticDurationUnit::Millisecond => seconds * Decimal::from(1_000),
        SemanticDurationUnit::Second => seconds,
        SemanticDurationUnit::Minute => seconds / Decimal::from(60),
        SemanticDurationUnit::Hour => seconds / Decimal::from(3_600),
        SemanticDurationUnit::Day => seconds / Decimal::from(86_400),
        SemanticDurationUnit::Week => seconds / Decimal::from(604_800),
        SemanticDurationUnit::Month => seconds / Decimal::from(2_592_000), // 30 days
        SemanticDurationUnit::Year => seconds / Decimal::from(31_536_000), // 365 days
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::OperationResult;
    use crate::planning::semantics::{LiteralValue, ValueKind};

    #[test]
    fn duration_conversion() {
        let result = convert_duration(
            Decimal::from(2),
            &SemanticDurationUnit::Hour,
            &SemanticDurationUnit::Minute,
        );
        assert_eq!(result, Decimal::from(120));
    }

    #[test]
    fn duration_seconds_roundtrip() {
        let original = Decimal::from(5);
        let seconds = duration_to_seconds(original, &SemanticDurationUnit::Day);
        let back = seconds_to_duration(seconds, &SemanticDurationUnit::Day);
        assert_eq!(original, back);
    }

    #[test]
    fn number_to_ratio_unit_produces_ratio_value() {
        let value = LiteralValue::number(Decimal::new(25, 2));
        let target = SemanticConversionTarget::RatioUnit("percent".to_string());
        let result = convert_unit(&value, &target);
        let OperationResult::Value(lit) = result else {
            panic!("expected Value, got {:?}", result);
        };
        match &lit.value {
            ValueKind::Ratio(r, u) => {
                assert_eq!(*r, Decimal::new(25, 2));
                assert_eq!(u.as_deref(), Some("percent"));
            }
            _ => panic!("expected Ratio value, got {:?}", lit.value),
        }
    }

    #[test]
    fn ratio_to_ratio_unit_preserves_value_attaches_unit() {
        let value = LiteralValue::ratio(Decimal::new(25, 2), Some("percent".to_string()));
        let target = SemanticConversionTarget::RatioUnit("permille".to_string());
        let result = convert_unit(&value, &target);
        let OperationResult::Value(lit) = result else {
            panic!("expected Value, got {:?}", result);
        };
        match &lit.value {
            ValueKind::Ratio(r, u) => {
                assert_eq!(*r, Decimal::new(25, 2));
                assert_eq!(u.as_deref(), Some("permille"));
            }
            _ => panic!("expected Ratio value, got {:?}", lit.value),
        }
    }

    #[test]
    fn ratio_with_none_unit_to_ratio_unit_attaches_unit() {
        let value = LiteralValue::ratio(Decimal::new(1, 2), None);
        let target = SemanticConversionTarget::RatioUnit("percent".to_string());
        let result = convert_unit(&value, &target);
        let OperationResult::Value(lit) = result else {
            panic!("expected Value, got {:?}", result);
        };
        match &lit.value {
            ValueKind::Ratio(r, u) => {
                assert_eq!(*r, Decimal::new(1, 2));
                assert_eq!(u.as_deref(), Some("percent"));
            }
            _ => panic!("expected Ratio value, got {:?}", lit.value),
        }
    }
}
