use crate::computation::rational::{rational_abs, rational_zero, RationalInteger};
use crate::evaluation::operations::{OperationResult, VetoType};
use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, LiteralValue, SemanticDateTime, ValueKind,
};

pub fn compute_quantity(
    left: &LiteralValue,
    right: &LiteralValue,
    _other: Option<&LiteralValue>,
) -> OperationResult {
    compute_span(left, right)
}

pub fn compute_span(left: &LiteralValue, right: &LiteralValue) -> OperationResult {
    absolute_span(compute_signed_span(left, right))
}

fn compute_signed_span(left: &LiteralValue, right: &LiteralValue) -> OperationResult {
    match (&left.value, &right.value) {
        (ValueKind::Date(left_date), ValueKind::Date(right_date)) => {
            compute_date_range_span(left_date, right_date)
        }
        _ => super::arithmetic_operation(right, &ArithmeticComputation::Subtract, left),
    }
}

fn absolute_span(span: OperationResult) -> OperationResult {
    let OperationResult::Value(literal) = span else {
        return span;
    };
    if span_magnitude_is_non_negative(literal.as_ref()) {
        return OperationResult::Value(literal);
    }
    let negated = match negate_stored_magnitude(literal.as_ref()) {
        Ok(magnitude) => magnitude,
        Err(failure) => return OperationResult::Veto(VetoType::computation(failure.message())),
    };
    OperationResult::Value(Box::new(rebuild_literal_with_magnitude(
        literal.as_ref(),
        negated,
    )))
}

fn span_magnitude_is_non_negative(literal: &LiteralValue) -> bool {
    stored_magnitude(literal) >= rational_zero()
}

fn stored_magnitude(literal: &LiteralValue) -> RationalInteger {
    match &literal.value {
        ValueKind::Number(n) => *n,
        ValueKind::Quantity(magnitude, _, _) => *magnitude,
        ValueKind::Ratio(magnitude, _) => *magnitude,
        ValueKind::Calendar(value, _) => *value,
        other => unreachable!(
            "BUG: range span must be number, quantity, ratio, or calendar, got {other:?}"
        ),
    }
}

fn negate_stored_magnitude(
    literal: &LiteralValue,
) -> Result<RationalInteger, super::arithmetic::NumberArithmeticFailure> {
    super::arithmetic::number_arithmetic(
        rational_zero(),
        &ArithmeticComputation::Subtract,
        stored_magnitude(literal),
    )
}

fn rebuild_literal_with_magnitude(
    literal: &LiteralValue,
    magnitude: RationalInteger,
) -> LiteralValue {
    match &literal.value {
        ValueKind::Number(_) => {
            LiteralValue::number_with_type(magnitude, literal.lemma_type.clone())
        }
        ValueKind::Quantity(_, unit, decomp) => {
            if decomp.is_empty() {
                LiteralValue::number_with_type(magnitude, literal.lemma_type.clone())
            } else if unit.is_empty() {
                LiteralValue::quantity_anonymous(magnitude, decomp.clone())
            } else {
                LiteralValue::quantity_with_type(
                    magnitude,
                    unit.clone(),
                    literal.lemma_type.clone(),
                )
            }
        }
        ValueKind::Ratio(_, unit) => {
            LiteralValue::ratio_with_type(magnitude, unit.clone(), literal.lemma_type.clone())
        }
        ValueKind::Calendar(_, unit) => {
            LiteralValue::calendar_with_type(magnitude, unit.clone(), literal.lemma_type.clone())
        }
        other => unreachable!(
            "BUG: range span must be number, quantity, ratio, or calendar, got {other:?}"
        ),
    }
}

fn compute_date_range_span(left: &SemanticDateTime, right: &SemanticDateTime) -> OperationResult {
    let left_chrono = match super::datetime::semantic_datetime_to_chrono(left) {
        Ok(d) => d,
        Err(msg) => return OperationResult::Veto(VetoType::computation(msg)),
    };
    let right_chrono = match super::datetime::semantic_datetime_to_chrono(right) {
        Ok(d) => d,
        Err(msg) => return OperationResult::Veto(VetoType::computation(msg)),
    };
    let duration = right_chrono - left_chrono;
    let seconds = match super::datetime::chrono_duration_to_rational_seconds(duration) {
        Ok(s) => s,
        Err(msg) => return OperationResult::Veto(VetoType::computation(msg)),
    };
    let seconds = rational_abs(&seconds);
    OperationResult::Value(Box::new(LiteralValue::quantity_anonymous(
        seconds,
        crate::planning::semantics::duration_decomposition(),
    )))
}

fn comparison_boolean_result(result: OperationResult, context: &str) -> bool {
    match result {
        OperationResult::Value(literal) => match literal.value {
            ValueKind::Boolean(value) => value,
            other => {
                unreachable!("BUG: {context} expected boolean comparison result, got {other:?}")
            }
        },
        OperationResult::Veto(_) => unreachable!(
            "BUG: {context} vetoed; planning guarantees compatible range containment operands"
        ),
    }
}

/// Half-open interval `[lo, hi)` where `lo` and `hi` are the ordered range endpoints.
pub fn check_containment(
    value: &LiteralValue,
    range_left: &LiteralValue,
    range_right: &LiteralValue,
) -> bool {
    let unit_context = super::UnitResolutionContext::NamedQuantityOnly;

    let (lo, hi) = if comparison_boolean_result(
        super::comparison_operation(
            range_left,
            &ComparisonComputation::LessThan,
            range_right,
            unit_context,
        ),
        "range endpoint ordering",
    ) {
        (range_left, range_right)
    } else {
        (range_right, range_left)
    };

    let lower_ok = comparison_boolean_result(
        super::comparison_operation(
            value,
            &ComparisonComputation::GreaterThanOrEqual,
            lo,
            unit_context,
        ),
        "range containment lower bound",
    );
    let upper_ok = comparison_boolean_result(
        super::comparison_operation(value, &ComparisonComputation::LessThan, hi, unit_context),
        "range containment upper bound",
    );

    lower_ok && upper_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::RationalInteger;
    use crate::planning::semantics::LiteralValue;

    #[test]
    fn compute_span_is_absolute_for_reversed_number_range() {
        let five = LiteralValue::number(RationalInteger::new(5, 1));
        let three = LiteralValue::number(RationalInteger::new(3, 1));
        let OperationResult::Value(span) = compute_span(&five, &three) else {
            panic!("expected span value");
        };
        match &span.value {
            ValueKind::Number(n) => assert_eq!(*n, RationalInteger::new(2, 1)),
            other => panic!("expected number span, got {other:?}"),
        }
    }

    #[test]
    fn check_containment_half_open_and_reversed_number_range() {
        let three = LiteralValue::number(RationalInteger::new(3, 1));
        let four = LiteralValue::number(RationalInteger::new(4, 1));
        let five = LiteralValue::number(RationalInteger::new(5, 1));
        let two = LiteralValue::number(RationalInteger::new(2, 1));

        assert!(check_containment(&three, &three, &five));
        assert!(!check_containment(&five, &three, &five));
        assert!(!check_containment(&two, &three, &five));
        assert!(!check_containment(&five, &five, &five));
        assert!(check_containment(&four, &five, &three));
    }
}
