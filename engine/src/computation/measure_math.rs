use crate::evaluation::operations::{OperationResult, VetoType};
use crate::planning::semantics::{LiteralValue, MathematicalComputation, ValueKind};

pub fn mathematical_computation_preserves_measure_magnitude(op: &MathematicalComputation) -> bool {
    matches!(
        op,
        MathematicalComputation::Abs
            | MathematicalComputation::Ceil
            | MathematicalComputation::Floor
            | MathematicalComputation::Round
    )
}

fn apply_decimal_magnitude_op(
    op: &MathematicalComputation,
    magnitude: rust_decimal::Decimal,
) -> rust_decimal::Decimal {
    match op {
        MathematicalComputation::Abs => magnitude.abs(),
        MathematicalComputation::Floor => magnitude.floor(),
        MathematicalComputation::Ceil => magnitude.ceil(),
        MathematicalComputation::Round => magnitude.round(),
        _ => unreachable!("BUG: non-magnitude-preserving op passed to apply_decimal_magnitude_op"),
    }
}

pub fn measure_magnitude_math(
    op: &MathematicalComputation,
    value: &LiteralValue,
) -> OperationResult {
    debug_assert!(mathematical_computation_preserves_measure_magnitude(op));

    let ValueKind::Measure(canonical_magnitude, signature) = &value.value else {
        unreachable!("BUG: measure_magnitude_math called with non-measure value");
    };

    if signature.len() != 1 || signature[0].1 != 1 {
        return OperationResult::Veto(VetoType::computation(format!(
            "Cannot apply '{op}' to measure with compound unit; convert with `as <unit>` first"
        )));
    }
    let unit_name = signature[0].0.clone();

    let unit_factor = value.lemma_type.measure_unit_factor(&unit_name);
    let magnitude_in_unit =
        match crate::computation::rational::checked_div(canonical_magnitude, unit_factor) {
            Ok(magnitude) => magnitude,
            Err(failure) => {
                return OperationResult::Veto(VetoType::computation(failure.to_string()));
            }
        };

    let magnitude_decimal =
        match crate::computation::rational::commit_rational_to_decimal(&magnitude_in_unit) {
            Ok(decimal) => decimal,
            Err(_) => {
                return OperationResult::Veto(VetoType::computation(
                    "Mathematical operation requires a decimal-representable input",
                ));
            }
        };

    let rounded_decimal = apply_decimal_magnitude_op(op, magnitude_decimal);

    let rounded_rational = match crate::computation::rational::decimal_to_rational(rounded_decimal)
    {
        Ok(rational) => rational,
        Err(failure) => {
            return OperationResult::Veto(VetoType::computation(failure.to_string()));
        }
    };

    let new_canonical =
        match crate::computation::rational::checked_mul(&rounded_rational, unit_factor) {
            Ok(canonical) => canonical,
            Err(failure) => {
                return OperationResult::Veto(VetoType::computation(failure.to_string()));
            }
        };

    OperationResult::from_literal(LiteralValue::measure_with_type(
        new_canonical,
        unit_name,
        value.lemma_type.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::rational_new;
    use crate::planning::semantics::anonymous_measure_type;
    use std::sync::Arc;

    #[test]
    fn compound_signature_vetoes() {
        let value = LiteralValue::measure_with_signature(
            rational_new(100, 1),
            vec![("meter".to_string(), 1), ("second".to_string(), -1)],
            Arc::new(anonymous_measure_type()),
        );
        let result = measure_magnitude_math(&MathematicalComputation::Ceil, &value);
        assert!(
            result.vetoed(),
            "compound signature must veto, got {:?}",
            result
        );
    }
}
