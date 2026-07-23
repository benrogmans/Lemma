use crate::computation::arithmetic::SignatureIndex;
use crate::computation::rational::{rational_abs, rational_zero, RationalInteger};
use crate::evaluation::operations::{OperationResult, VetoType};
use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, LemmaType, LiteralValue, ValueKind,
};
use std::collections::HashMap;

pub fn compute_span(left: &LiteralValue, right: &LiteralValue) -> OperationResult {
    absolute_span(compute_signed_span(left, right))
}

fn compute_signed_span(left: &LiteralValue, right: &LiteralValue) -> OperationResult {
    match (&left.value, &right.value) {
        (ValueKind::Date(left_date), ValueKind::Date(right_date)) => {
            let left_chrono = match super::datetime::semantic_datetime_to_chrono(left_date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(VetoType::computation(msg)),
            };
            let right_chrono = match super::datetime::semantic_datetime_to_chrono(right_date) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(VetoType::computation(msg)),
            };
            compute_elapsed_duration_span(left_chrono, right_chrono)
        }
        (ValueKind::Time(left_time), ValueKind::Time(right_time)) => {
            let left_chrono = match super::datetime::semantic_time_to_chrono_datetime(left_time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(VetoType::computation(msg)),
            };
            let right_chrono = match super::datetime::semantic_time_to_chrono_datetime(right_time) {
                Ok(d) => d,
                Err(msg) => return OperationResult::Veto(VetoType::computation(msg)),
            };
            compute_elapsed_duration_span(left_chrono, right_chrono)
        }
        _ => {
            // Span computation only performs Subtract, which never resolves a signature_index
            // entry (the result type matches the operand family).
            let empty_unit_index: HashMap<String, std::sync::Arc<LemmaType>> = HashMap::new();
            let empty_signature_index = SignatureIndex::new();
            super::arithmetic_operation(
                right,
                &ArithmeticComputation::Subtract,
                left,
                &empty_unit_index,
                &empty_signature_index,
            )
        }
    }
}

fn absolute_span(span: OperationResult) -> OperationResult {
    let OperationResult::Value(literal) = span else {
        return span;
    };
    let magnitude = stored_magnitude(literal.as_ref());
    match magnitude.try_cmp(&rational_zero()) {
        Ok(std::cmp::Ordering::Less) => {}
        Ok(_) => return OperationResult::from_literal_arc(literal),
        Err(e) => return OperationResult::Veto(VetoType::computation(e.to_string())),
    }
    let negated = match negate_stored_magnitude(literal.as_ref()) {
        Ok(magnitude) => magnitude,
        Err(failure) => return OperationResult::Veto(VetoType::computation(failure.message())),
    };
    OperationResult::from_literal(rebuild_literal_with_magnitude(literal.as_ref(), negated))
}

fn stored_magnitude(literal: &LiteralValue) -> RationalInteger {
    match &literal.value {
        ValueKind::Number(n) => n.clone(),
        ValueKind::Measure(value, _) if literal.lemma_type.is_calendar_like() => value.clone(),
        ValueKind::Measure(magnitude, _) => magnitude.clone(),
        ValueKind::Ratio(magnitude, _) => magnitude.clone(),
        other => unreachable!(
            "BUG: range span must be number, measure, ratio, or calendar measure, got {other:?}"
        ),
    }
}

fn negate_stored_magnitude(
    literal: &LiteralValue,
) -> Result<RationalInteger, super::arithmetic::NumberArithmeticFailure> {
    let zero = rational_zero();
    let magnitude = stored_magnitude(literal);
    super::arithmetic::number_arithmetic(&zero, &ArithmeticComputation::Subtract, &magnitude)
}

fn rebuild_literal_with_magnitude(
    literal: &LiteralValue,
    magnitude: RationalInteger,
) -> LiteralValue {
    match &literal.value {
        ValueKind::Number(_) => {
            LiteralValue::number_with_type(magnitude, literal.lemma_type.clone())
        }
        ValueKind::Measure(_, sig) if literal.lemma_type.is_calendar_like() => {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_measure_signature(sig);
            LiteralValue::calendar_with_type(magnitude, unit, literal.lemma_type.clone())
        }
        ValueKind::Measure(_, signature) => LiteralValue {
            value: ValueKind::Measure(magnitude, signature.clone()),
            lemma_type: literal.lemma_type.clone(),
        },
        ValueKind::Ratio(_, unit) => {
            LiteralValue::ratio_with_type(magnitude, unit.clone(), literal.lemma_type.clone())
        }
        other => unreachable!(
            "BUG: range span must be number, measure, ratio, or calendar measure, got {other:?}"
        ),
    }
}

fn compute_elapsed_duration_span(
    left_chrono: chrono::DateTime<chrono::FixedOffset>,
    right_chrono: chrono::DateTime<chrono::FixedOffset>,
) -> OperationResult {
    let duration = right_chrono - left_chrono;
    let second = match super::datetime::chrono_duration_to_rational_seconds(duration) {
        Ok(s) => s,
        Err(msg) => return OperationResult::Veto(VetoType::computation(msg)),
    };
    let second = match rational_abs(&second) {
        Ok(s) => s,
        Err(failure) => return OperationResult::Veto(VetoType::computation(failure.to_string())),
    };
    OperationResult::from_literal(LiteralValue {
        value: ValueKind::Measure(second, vec![("second".to_string(), 1)]),
        lemma_type: std::sync::Arc::new(
            crate::planning::semantics::LemmaType::anonymous_for_decomposition(
                crate::planning::semantics::duration_decomposition(),
            ),
        ),
    })
}

fn comparison_boolean_result(result: OperationResult, context: &str) -> Result<bool, VetoType> {
    match result {
        OperationResult::Value(literal) => match &literal.value {
            ValueKind::Boolean(value) => Ok(*value),
            other => {
                unreachable!("BUG: {context} expected boolean comparison result, got {other:?}")
            }
        },
        OperationResult::Veto(v) => Err(v),
    }
}

/// Half-open interval `[lo, hi)` where `lo` and `hi` are the ordered range endpoints.
/// Returns `OperationResult::from_literal(Boolean)` or propagates a Veto from inner comparisons.
pub fn check_containment(
    value: &LiteralValue,
    range_left: &LiteralValue,
    range_right: &LiteralValue,
) -> OperationResult {
    let unit_context = super::UnitResolutionContext::NamedMeasureOnly;

    let (lo, hi) = match comparison_boolean_result(
        super::comparison_operation(
            range_left,
            &ComparisonComputation::LessThan,
            range_right,
            unit_context,
        ),
        "range endpoint ordering",
    ) {
        Ok(true) => (range_left, range_right),
        Ok(false) => (range_right, range_left),
        Err(v) => return OperationResult::Veto(v),
    };

    let lower_ok = match comparison_boolean_result(
        super::comparison_operation(
            value,
            &ComparisonComputation::GreaterThanOrEqual,
            lo,
            unit_context,
        ),
        "range containment lower bound",
    ) {
        Ok(b) => b,
        Err(v) => return OperationResult::Veto(v),
    };
    let upper_ok = match comparison_boolean_result(
        super::comparison_operation(value, &ComparisonComputation::LessThan, hi, unit_context),
        "range containment upper bound",
    ) {
        Ok(b) => b,
        Err(v) => return OperationResult::Veto(v),
    };

    OperationResult::from_literal(LiteralValue::from_bool(lower_ok && upper_ok))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::rational_new;
    use crate::planning::semantics::LiteralValue;

    #[test]
    fn compute_span_is_absolute_for_reversed_number_range() {
        let five = LiteralValue::number(rational_new(5, 1));
        let three = LiteralValue::number(rational_new(3, 1));
        let OperationResult::Value(span) = compute_span(&five, &three) else {
            panic!("expected span value");
        };
        match &span.value {
            ValueKind::Number(n) => assert_eq!(n, &rational_new(2, 1)),
            other => panic!("expected number span, got {other:?}"),
        }
    }

    fn assert_contained(value: &LiteralValue, left: &LiteralValue, right: &LiteralValue) -> bool {
        match check_containment(value, left, right) {
            OperationResult::Value(lit) => match &lit.value {
                ValueKind::Boolean(b) => *b,
                other => panic!("expected Boolean, got {other:?}"),
            },
            OperationResult::Veto(v) => panic!("unexpected veto: {v:?}"),
        }
    }

    #[test]
    fn check_containment_half_open_and_reversed_number_range() {
        let three = LiteralValue::number(rational_new(3, 1));
        let four = LiteralValue::number(rational_new(4, 1));
        let five = LiteralValue::number(rational_new(5, 1));
        let two = LiteralValue::number(rational_new(2, 1));

        assert!(assert_contained(&three, &three, &five));
        assert!(!assert_contained(&five, &three, &five));
        assert!(!assert_contained(&two, &three, &five));
        assert!(!assert_contained(&five, &five, &five));
        assert!(assert_contained(&four, &five, &three));
    }

    /// Phase 0 — pin that `rebuild_literal_with_magnitude` for a `Measure` value reads
    /// only the signature from the source value (not decomposition or the empty-unit
    /// workaround). After the rewrite, the function trivially constructs
    /// `Measure(new_magnitude, original.signature)` with `original.lemma_type`.
    ///
    /// Today the function branches on `decomp.is_empty()` and `unit.is_empty()`; those
    /// branches must collapse.
    #[test]
    fn rebuild_literal_with_magnitude_uses_signature_only() {
        // Build a Measure value that today has an empty-string unit and a non-empty decomposition
        // (i.e. an anonymous intermediate). After the rewrite this is replaced by Measure(_, signature).
        use crate::planning::semantics::{BaseMeasureVector, LemmaType, ValueKind};
        let mut decomp = BaseMeasureVector::new();
        decomp.insert("money".to_string(), 1);
        let signature: Vec<(String, i32)> = vec![("eur".to_string(), 1)];
        let original = LiteralValue {
            value: ValueKind::Measure(rational_new(10, 1), signature.clone()),
            lemma_type: std::sync::Arc::new(LemmaType::anonymous_for_decomposition(decomp)),
        };
        let rebuilt = rebuild_literal_with_magnitude(&original, rational_new(99, 1));
        match &rebuilt.value {
            ValueKind::Measure(n, _) => {
                assert_eq!(n, &rational_new(99, 1));
            }
            other => panic!("expected Measure, got {:?}", other),
        }
        // After the rewrite: the rebuilt value's signature must equal the original's.
        // Today we check that lemma_type is preserved.
        assert_eq!(rebuilt.lemma_type, original.lemma_type);
    }
}
