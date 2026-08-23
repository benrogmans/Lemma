//! Type-aware arithmetic operations

use crate::computation::operation_result::{OperationResult, VetoType};
use crate::computation::rational::{
    checked_add, checked_div, checked_mul, checked_sub, rational_operation_with_fallback,
    NumericFailure, NumericOperation, RationalInteger,
};
use crate::planning::semantics::{
    combine_signatures, primitive_number_arc, ArithmeticComputation, LemmaType, LiteralValue,
    SemanticCalendarUnit, ValueKind,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Reverse index mapping a canonical-form unit signature `Vec<(unit_name, exponent)>`
/// to the unit (by name) and its owning measure type. Built during planning.
pub type SignatureIndex = HashMap<Vec<(String, i32)>, (String, Arc<LemmaType>)>;

struct CalendarRangeShiftIndexes<'a> {
    unit_index: &'a crate::planning::unit_index::UnitIndex,
    signature_index: &'a SignatureIndex,
}

/// Promote an anonymous measure result to a named type when its signature matches an
/// entry in `signature_index`.
///
/// Used for operations that produce anonymous intermediates at runtime
/// (Date-Time subtraction, range spans, cross-unit multiplication).
fn promote_anonymous_measure_result(
    result: OperationResult,
    signature_index: &SignatureIndex,
    unit_index: &crate::planning::unit_index::UnitIndex,
) -> OperationResult {
    let OperationResult::Value(value) = result else {
        return result;
    };
    let ValueKind::Measure(magnitude, raw_signature) = &value.value else {
        return OperationResult::from_literal_arc(value);
    };
    if !value.lemma_type.is_anonymous_measure() {
        return OperationResult::from_literal_arc(value);
    }
    let expanded = expand_signature_to_base_units(raw_signature, unit_index, &[]);
    let Some((unit_name, owning_type)) = signature_index.get(&expanded) else {
        return OperationResult::from_literal_arc(value);
    };
    OperationResult::from_literal(LiteralValue::measure_with_type(
        magnitude.clone(),
        unit_name.clone(),
        owning_type.clone(),
    ))
}

/// Expand a raw signature (literal unit names) to derived base units, so that signature_index
/// (which is keyed by `derived_measure_factors`) can be searched.
///
/// For each `(unit_name, exp)`:
/// - Look up `unit_name`'s owning type in `unit_index`.
/// - Replace it with its `derived_measure_factors`, scaled by `exp`.
/// - When multiple units from the same family appear, normalize them to the canonical
///   unit name so exponents can cancel (e.g. `hour` and `minute` both become `second`).
/// - Single remaining units keep their declared name (e.g. `minute` stays `minute`).
fn measure_family_key(
    unit_name: &str,
    unit_index: &crate::planning::unit_index::UnitIndex,
    typed_owners: &[&LemmaType],
) -> Option<String> {
    use crate::planning::semantics::calendar_unit_factor;
    if calendar_unit_factor(unit_name).is_some() {
        return Some("__calendar__".to_string());
    }
    unit_index
        .owning_type_for_signature_factor(unit_name, typed_owners)
        .and_then(|t| t.measure_family_name().map(str::to_string))
}

fn canonical_unit_in_family(
    family_key: &str,
    unit_index: &crate::planning::unit_index::UnitIndex,
) -> String {
    use crate::planning::semantics::TypeSpecification;
    if family_key == "__calendar__" {
        return "month".to_string();
    }
    for lemma_type in unit_index.values() {
        if lemma_type.measure_family_name() != Some(family_key) {
            continue;
        }
        if let TypeSpecification::Measure { units, .. } = &lemma_type.specifications {
            if let Some(canonical) = units.iter().find(|u| u.is_canonical_factor()) {
                return canonical.name.clone();
            }
        }
    }
    family_key.to_string()
}

pub(crate) fn expand_signature_to_base_units(
    raw: &[(String, i32)],
    unit_index: &crate::planning::unit_index::UnitIndex,
    typed_owners: &[&LemmaType],
) -> Vec<(String, i32)> {
    use crate::planning::semantics::canonicalize_signature;
    use crate::planning::semantics::TypeSpecification;
    use std::collections::BTreeMap;
    let mut expanded: Vec<(String, i32)> = Vec::new();
    for (unit_name, exp) in raw {
        if let Some(owning_type) =
            unit_index.owning_type_for_signature_factor(unit_name, typed_owners)
        {
            if let TypeSpecification::Measure { units, .. } = &owning_type.specifications {
                if let Ok(unit) = units.get(unit_name) {
                    if !unit.derived_measure_factors.is_empty() {
                        for (base_name, base_exp) in &unit.derived_measure_factors {
                            expanded.push((base_name.clone(), base_exp * exp));
                        }
                        continue;
                    }
                }
            }
        }
        expanded.push((unit_name.clone(), *exp));
    }
    expanded = canonicalize_signature(&expanded);

    let mut by_family: BTreeMap<String, Vec<(String, i32)>> = BTreeMap::new();
    let mut ungrouped: Vec<(String, i32)> = Vec::new();
    for (unit_name, exp) in expanded {
        if let Some(family) = measure_family_key(&unit_name, unit_index, typed_owners) {
            by_family.entry(family).or_default().push((unit_name, exp));
        } else {
            ungrouped.push((unit_name, exp));
        }
    }

    let mut normalized = ungrouped;
    for (family, entries) in by_family {
        let distinct: std::collections::BTreeSet<&str> =
            entries.iter().map(|(name, _)| name.as_str()).collect();
        if distinct.len() > 1 {
            let canonical = canonical_unit_in_family(&family, unit_index);
            let net_exp: i32 = entries.iter().map(|(_, exp)| exp).sum();
            if net_exp != 0 {
                normalized.push((canonical, net_exp));
            }
        } else {
            normalized.extend(entries);
        }
    }
    canonicalize_signature(&normalized)
}

fn number_op_on_stored_rationals(
    left: &RationalInteger,
    operator: &ArithmeticComputation,
    right: &RationalInteger,
    lemma_type: Arc<LemmaType>,
) -> OperationResult {
    match number_arithmetic(left, operator, right) {
        Ok(rational) => {
            OperationResult::from_literal(LiteralValue::number_with_type(rational, lemma_type))
        }
        Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
    }
}

fn number_op_on_stored_rationals_primitive(
    left: &RationalInteger,
    operator: &ArithmeticComputation,
    right: &RationalInteger,
) -> OperationResult {
    number_op_on_stored_rationals(left, operator, right, primitive_number_arc().clone())
}

fn number_ratio_arithmetic(
    left: &RationalInteger,
    op: &ArithmeticComputation,
    right: &RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    match op {
        ArithmeticComputation::Multiply | ArithmeticComputation::Divide => {
            rational_operation_with_fallback(left, numeric_operation_from_arithmetic(op), right)
                .map_err(map_numeric_failure)
        }
        ArithmeticComputation::Power => {
            rational_operation_with_fallback(left, NumericOperation::Power, right)
                .map_err(map_numeric_failure)
        }
        ArithmeticComputation::Modulo => {
            rational_operation_with_fallback(left, NumericOperation::Modulo, right).map_err(|f| {
                if matches!(f, NumericFailure::DivisionByZero) {
                    NumberArithmeticFailure::ModuloByZero
                } else {
                    map_numeric_failure(f)
                }
            })
        }
        ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
            unreachable!("BUG: number/ratio add-subtract rejected during planning")
        }
    }
}

fn calendar_from_months_arithmetic(
    left: &RationalInteger,
    left_unit: &SemanticCalendarUnit,
    operator: &ArithmeticComputation,
    right: &RationalInteger,
    _right_unit: &SemanticCalendarUnit,
    result_lemma_type: Arc<LemmaType>,
) -> OperationResult {
    let result_months = match operator {
        ArithmeticComputation::Add => match checked_add(left, right) {
            Ok(r) => r,
            Err(failure) => {
                return OperationResult::Veto(VetoType::computation(
                    map_numeric_failure(failure).message(),
                ))
            }
        },
        ArithmeticComputation::Subtract => match checked_sub(left, right) {
            Ok(r) => r,
            Err(failure) => {
                return OperationResult::Veto(VetoType::computation(
                    map_numeric_failure(failure).message(),
                ))
            }
        },
        _ => unreachable!(
            "BUG: calendar_from_months_arithmetic called with {:?}; planning rejects this",
            operator
        ),
    };

    let result_unit = left_unit.clone();
    OperationResult::from_literal(LiteralValue::calendar_with_type(
        result_months,
        result_unit,
        result_lemma_type,
    ))
}

/// Perform type-aware arithmetic operation, returning OperationResult (Veto for runtime errors).
///
/// `expression_units` and `signature_index` come from the plan's resolved types and are
/// used during expression evaluation to resolve combined unit signatures back to a named
/// unit and owning type.
pub fn arithmetic_operation(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right: &LiteralValue,
    unit_index: &crate::planning::unit_index::UnitIndex,
    signature_index: &SignatureIndex,
) -> OperationResult {
    match (&left.value, &right.value) {
        (ValueKind::Range(range_left, range_right), ValueKind::Measure(value, sig))
            if left.lemma_type.is_calendar_like_range()
                && right.lemma_type.is_calendar_like()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_measure_signature(sig);
            shift_calendar_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                value,
                &unit,
                matches!(op, ArithmeticComputation::Add),
                &CalendarRangeShiftIndexes {
                    unit_index,
                    signature_index,
                },
            )
        }

        (ValueKind::Measure(value, sig), ValueKind::Range(range_left, range_right))
            if right.lemma_type.is_calendar_like_range()
                && left.lemma_type.is_calendar_like()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_measure_signature(sig);
            shift_calendar_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                value,
                &unit,
                matches!(op, ArithmeticComputation::Add),
                &CalendarRangeShiftIndexes {
                    unit_index,
                    signature_index,
                },
            )
        }

        (ValueKind::Range(range_left, range_right), ValueKind::Measure(value, sig))
            if left.lemma_type.is_date_range()
                && right.lemma_type.is_calendar_like()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_measure_signature(sig);
            shift_date_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                value,
                &unit,
                matches!(op, ArithmeticComputation::Add),
                Arc::clone(&right.lemma_type),
            )
        }

        (ValueKind::Measure(value, sig), ValueKind::Range(range_left, range_right))
            if right.lemma_type.is_date_range()
                && left.lemma_type.is_calendar_like()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_measure_signature(sig);
            shift_date_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                value,
                &unit,
                matches!(op, ArithmeticComputation::Add),
                Arc::clone(&left.lemma_type),
            )
        }

        (
            ValueKind::Range(left_range_left, left_range_right),
            ValueKind::Range(right_range_left, right_range_right),
        ) if matches!(
            op,
            ArithmeticComputation::Add | ArithmeticComputation::Subtract
        ) =>
        {
            let left_measure =
                super::range::compute_span(left_range_left.as_ref(), left_range_right.as_ref());
            let right_measure =
                super::range::compute_span(right_range_left.as_ref(), right_range_right.as_ref());
            operate_on_operation_results(
                left_measure,
                op,
                right_measure,
                unit_index,
                signature_index,
            )
        }

        (ValueKind::Range(range_left, range_right), _)
            if matches!(
                op,
                ArithmeticComputation::Add | ArithmeticComputation::Subtract
            ) =>
        {
            let measure = super::range::compute_span(range_left.as_ref(), range_right.as_ref());
            operate_with_left_result(measure, op, right, unit_index, signature_index)
        }

        (_, ValueKind::Range(range_left, range_right))
            if matches!(
                op,
                ArithmeticComputation::Add | ArithmeticComputation::Subtract
            ) =>
        {
            let measure = super::range::compute_span(range_left.as_ref(), range_right.as_ref());
            operate_with_right_result(left, op, measure, unit_index, signature_index)
        }

        (ValueKind::Number(l), ValueKind::Number(r)) => {
            number_op_on_stored_rationals(l, op, r, Arc::clone(&left.lemma_type))
        }

        (ValueKind::Date(_), _) | (_, ValueKind::Date(_)) => promote_anonymous_measure_result(
            super::datetime::datetime_arithmetic(left, op, right),
            signature_index,
            unit_index,
        ),

        (ValueKind::Time(_), _) | (_, ValueKind::Time(_)) => promote_anonymous_measure_result(
            super::datetime::time_arithmetic(left, op, right),
            signature_index,
            unit_index,
        ),

        // Number op Ratio → Number (multiply is symmetric; divide/power/modulo preserve order)
        (ValueKind::Number(n), ValueKind::Ratio(r, _)) => match number_ratio_arithmetic(n, op, r) {
            Ok(rational) => OperationResult::from_literal(LiteralValue::number_with_type(
                rational,
                primitive_number_arc().clone(),
            )),
            Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
        },
        // Ratio op Number → result type depends on operator
        (ValueKind::Ratio(r, ru), ValueKind::Number(n)) => match op {
            ArithmeticComputation::Multiply => match number_ratio_arithmetic(r, op, n) {
                Ok(rational) => OperationResult::from_literal(LiteralValue::number_with_type(
                    rational,
                    primitive_number_arc().clone(),
                )),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            },
            ArithmeticComputation::Divide
            | ArithmeticComputation::Power
            | ArithmeticComputation::Modulo => match number_ratio_arithmetic(r, op, n) {
                Ok(rational) => OperationResult::from_literal(LiteralValue::ratio_with_type(
                    rational,
                    ru.clone(),
                    Arc::clone(&left.lemma_type),
                )),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            },
            _ => unreachable!(
                "BUG: ratio {:?} number; add/subtract rejected during planning",
                op
            ),
        },

        // Ratio op Ratio → Ratio
        (ValueKind::Ratio(l, lu), ValueKind::Ratio(r, _ru)) => match number_arithmetic(l, op, r) {
            Ok(rational) => OperationResult::from_literal(LiteralValue::ratio_with_type(
                rational,
                lu.clone(),
                Arc::clone(&left.lemma_type),
            )),
            Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
        },
        // Measure operations with Measure
        (ValueKind::Measure(l_val, l_signature), ValueKind::Measure(r_val, r_signature)) => {
            if left.lemma_type.is_calendar_like() != right.lemma_type.is_calendar_like() {
                let is_measure_left =
                    left.lemma_type.is_measure() && !left.lemma_type.is_calendar_like();
                let is_multiply = matches!(op, ArithmeticComputation::Multiply);
                let is_divide = matches!(op, ArithmeticComputation::Divide);
                if !is_multiply && !is_divide {
                    unreachable!("BUG: measure {:?} calendar is rejected during planning", op);
                }
                let (l_val_ref, l_sig_ref, r_val_ref, r_sig_ref) = if is_measure_left {
                    (l_val, l_signature, r_val, r_signature)
                } else {
                    (r_val, r_signature, l_val, l_signature)
                };
                if is_divide && crate::computation::rational::rational_is_zero(r_val_ref) {
                    return OperationResult::Veto(VetoType::computation("Division by zero"));
                }
                let raw_result = match rational_operation_with_fallback(
                    l_val_ref,
                    if is_multiply {
                        NumericOperation::Multiply
                    } else {
                        NumericOperation::Divide
                    },
                    r_val_ref,
                ) {
                    Ok(p) => p,
                    Err(failure) => {
                        return OperationResult::Veto(VetoType::computation(
                            map_numeric_failure(failure).message(),
                        ))
                    }
                };
                let raw_signature = combine_signatures(l_sig_ref, r_sig_ref, is_multiply);
                let q_decomp = if is_measure_left {
                    left.lemma_type.measure_type_decomposition()
                } else {
                    right.lemma_type.measure_type_decomposition()
                }
                .expect("BUG: decomposition must be resolved after planning");
                let c_decomp = crate::planning::semantics::calendar_decomposition();
                let combined = crate::planning::semantics::combine_decompositions(
                    q_decomp,
                    &c_decomp,
                    is_multiply,
                );
                if combined.is_empty() {
                    return OperationResult::from_literal(LiteralValue::number_with_type(
                        raw_result,
                        primitive_number_arc().clone(),
                    ));
                }
                let owners = [left.lemma_type.as_ref(), right.lemma_type.as_ref()];
                let expanded_signature =
                    expand_signature_to_base_units(&raw_signature, unit_index, &owners);
                if let Some((unit_name, owning_type)) = signature_index.get(&expanded_signature) {
                    return OperationResult::from_literal(LiteralValue::measure_with_type(
                        raw_result,
                        unit_name.clone(),
                        owning_type.clone(),
                    ));
                }
                return OperationResult::from_literal(LiteralValue {
                    value: ValueKind::Measure(raw_result, raw_signature),
                    lemma_type: Arc::new(LemmaType::anonymous_for_decomposition(combined)),
                });
            }
            if left.lemma_type.is_calendar_like() && right.lemma_type.is_calendar_like() {
                let lu = crate::planning::semantics::semantic_calendar_unit_from_measure_signature(
                    l_signature,
                );
                let ru = crate::planning::semantics::semantic_calendar_unit_from_measure_signature(
                    r_signature,
                );
                return match op {
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                        calendar_from_months_arithmetic(
                            l_val,
                            &lu,
                            op,
                            r_val,
                            &ru,
                            Arc::clone(&left.lemma_type),
                        )
                    }
                    ArithmeticComputation::Divide => number_op_on_stored_rationals_primitive(
                        l_val,
                        &ArithmeticComputation::Divide,
                        r_val,
                    ),
                    _ => unreachable!(
                        "BUG: calendar * calendar with op {:?}; planning should have rejected this",
                        op
                    ),
                };
            }
            if left.lemma_type.is_calendar_like() && !right.lemma_type.is_calendar_like() {
                let unit =
                    crate::planning::semantics::semantic_calendar_unit_from_measure_signature(
                        l_signature,
                    );
                if let ValueKind::Number(n) = &right.value {
                    return match number_arithmetic(l_val, op, n) {
                        Ok(rational) => {
                            OperationResult::from_literal(LiteralValue::calendar_with_type(
                                rational,
                                unit,
                                left.lemma_type.clone(),
                            ))
                        }
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
                if let ValueKind::Ratio(r, _) = &right.value {
                    return match number_ratio_arithmetic(l_val, op, r) {
                        Ok(rational) => {
                            OperationResult::from_literal(LiteralValue::calendar_with_type(
                                rational,
                                unit,
                                left.lemma_type.clone(),
                            ))
                        }
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
            }
            if right.lemma_type.is_calendar_like() && !left.lemma_type.is_calendar_like() {
                let unit =
                    crate::planning::semantics::semantic_calendar_unit_from_measure_signature(
                        r_signature,
                    );
                if let (ValueKind::Number(n), ArithmeticComputation::Multiply) = (&left.value, op) {
                    return match number_arithmetic(n, op, r_val) {
                        Ok(rational) => {
                            OperationResult::from_literal(LiteralValue::calendar_with_type(
                                rational,
                                unit,
                                right.lemma_type.clone(),
                            ))
                        }
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
                if let ValueKind::Number(n) = &left.value {
                    return match number_arithmetic(n, op, r_val) {
                        Ok(rational) => {
                            OperationResult::from_literal(LiteralValue::number_with_type(
                                rational,
                                primitive_number_arc().clone(),
                            ))
                        }
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
                if let ValueKind::Ratio(r, _) = &left.value {
                    return match number_ratio_arithmetic(r_val, op, r) {
                        Ok(rational) => {
                            OperationResult::from_literal(LiteralValue::calendar_with_type(
                                rational,
                                unit,
                                right.lemma_type.clone(),
                            ))
                        }
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
            }
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    let same_family = left.lemma_type.same_measure_family(&right.lemma_type);
                    let anonymous_compatible = left
                        .lemma_type
                        .compatible_with_anonymous_measure(&right.lemma_type);
                    if !same_family && !anonymous_compatible {
                        unreachable!(
                        "BUG: measure add/subtract with incompatible types ({} vs {}); should be rejected during planning",
                        left.lemma_type.name(),
                        right.lemma_type.name()
                    );
                    }
                    match measure_add_subtract(l_val, op, r_val) {
                        Ok(rational) => {
                            OperationResult::from_literal(LiteralValue::measure_with_signature(
                                rational,
                                l_signature.clone(),
                                left.lemma_type.clone(),
                            ))
                        }
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    }
                }
                ArithmeticComputation::Multiply | ArithmeticComputation::Divide => {
                    if matches!(op, ArithmeticComputation::Divide)
                        && crate::computation::rational::rational_is_zero(r_val)
                    {
                        return OperationResult::Veto(VetoType::computation("Division by zero"));
                    }
                    let numeric_op = match op {
                        ArithmeticComputation::Multiply => NumericOperation::Multiply,
                        ArithmeticComputation::Divide => NumericOperation::Divide,
                        _ => unreachable!("BUG: matched multiply/divide arm with other op"),
                    };
                    let raw_result =
                        match rational_operation_with_fallback(l_val, numeric_op, r_val) {
                            Ok(p) => p,
                            Err(failure) => {
                                return OperationResult::Veto(VetoType::computation(
                                    map_numeric_failure(failure).message(),
                                ))
                            }
                        };
                    let raw_signature = combine_signatures(
                        l_signature,
                        r_signature,
                        matches!(op, ArithmeticComputation::Multiply),
                    );
                    let l_decomp = left
                        .lemma_type
                        .measure_type_decomposition()
                        .expect("BUG: decomposition must be resolved after planning");
                    let r_decomp = right
                        .lemma_type
                        .measure_type_decomposition()
                        .expect("BUG: decomposition must be resolved after planning");
                    let combined_decomposition = crate::planning::semantics::combine_decompositions(
                        l_decomp,
                        r_decomp,
                        matches!(op, ArithmeticComputation::Multiply),
                    );
                    if combined_decomposition.is_empty() {
                        OperationResult::from_literal(LiteralValue::number_with_type(
                            raw_result,
                            primitive_number_arc().clone(),
                        ))
                    } else {
                        let owners = [left.lemma_type.as_ref(), right.lemma_type.as_ref()];
                        let expanded_signature =
                            expand_signature_to_base_units(&raw_signature, unit_index, &owners);
                        if let Some((unit_name, owning_type)) =
                            signature_index.get(&expanded_signature)
                        {
                            OperationResult::from_literal(LiteralValue::measure_with_type(
                                raw_result,
                                unit_name.clone(),
                                owning_type.clone(),
                            ))
                        } else {
                            OperationResult::from_literal(LiteralValue {
                                value: ValueKind::Measure(raw_result, raw_signature),
                                lemma_type: Arc::new(LemmaType::anonymous_for_decomposition(
                                    combined_decomposition,
                                )),
                            })
                        }
                    }
                }
                _ => unreachable!("BUG: measure {:?} measure is rejected during planning", op),
            }
        }
        // Measure op Ratio → Measure (multiply/divide; add/subtract rejected at planning)
        (ValueKind::Measure(q_val, q_sig), ValueKind::Ratio(r, _)) => {
            match measure_ratio_arithmetic(q_val.clone(), op, r.clone()) {
                Ok(rational) => {
                    OperationResult::from_literal(LiteralValue::measure_with_signature(
                        rational,
                        q_sig.clone(),
                        left.lemma_type.clone(),
                    ))
                }
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }
        // Ratio op Measure → Measure only for multiply (commutative); others rejected at planning
        (ValueKind::Ratio(r, _), ValueKind::Measure(q_val, q_sig)) => match op {
            ArithmeticComputation::Multiply => {
                match measure_ratio_arithmetic(q_val.clone(), op, r.clone()) {
                    Ok(rational) => {
                        OperationResult::from_literal(LiteralValue::measure_with_signature(
                            rational,
                            q_sig.clone(),
                            right.lemma_type.clone(),
                        ))
                    }
                    Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
                }
            }
            _ => unreachable!(
                "BUG: ratio {:?} measure; planning should have rejected this",
                op
            ),
        },

        // Measure op Number → Measure (preserves unit)
        (ValueKind::Measure(measure_val, measure_signature), ValueKind::Number(n)) => {
            let rational = if matches!(
                op,
                ArithmeticComputation::Modulo | ArithmeticComputation::Power
            ) {
                let (unit_name, exponent) = measure_signature
                    .first()
                    .expect("BUG: measure modulo number requires single-term signature");
                if *exponent != 1 {
                    unreachable!(
                        "BUG: measure modulo number with compound signature; planning must reject"
                    );
                }
                let factor = left.lemma_type.measure_unit_factor(unit_name);
                let in_unit = checked_div(measure_val, factor)
                    .expect("BUG: measure de-canonicalization for modulo must not fail");
                let modded = match number_arithmetic(&in_unit, op, n) {
                    Ok(value) => value,
                    Err(failure) => {
                        return OperationResult::Veto(VetoType::computation(failure.message()))
                    }
                };
                checked_mul(&modded, factor)
                    .expect("BUG: measure re-canonicalization after modulo must not fail")
            } else {
                match number_arithmetic(measure_val, op, n) {
                    Ok(value) => value,
                    Err(failure) => {
                        return OperationResult::Veto(VetoType::computation(failure.message()))
                    }
                }
            };
            OperationResult::from_literal(LiteralValue::measure_with_signature(
                rational,
                measure_signature.clone(),
                left.lemma_type.clone(),
            ))
        }
        // Number op Measure → Measure for multiply; for divide, negate signature if anonymous measure.
        (ValueKind::Number(n), ValueKind::Measure(measure_val, measure_signature)) => match op {
            ArithmeticComputation::Multiply => match number_arithmetic(n, op, measure_val) {
                Ok(rational) => {
                    OperationResult::from_literal(LiteralValue::measure_with_signature(
                        rational,
                        measure_signature.clone(),
                        right.lemma_type.clone(),
                    ))
                }
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            },
            ArithmeticComputation::Divide => {
                if right.lemma_type.is_duration_like_measure()
                    || right.lemma_type.is_calendar_like()
                {
                    let (unit_name, exponent) = measure_signature.first().expect(
                        "BUG: number divide duration-like measure requires single-term signature",
                    );
                    if *exponent != 1 {
                        unreachable!(
                            "BUG: number divide duration-like measure with compound signature; planning must reject"
                        );
                    }
                    let factor = right.lemma_type.measure_unit_factor(unit_name);
                    let in_unit = checked_div(measure_val, factor)
                        .expect("BUG: measure de-canonicalization for divide must not fail");
                    return match number_arithmetic(n, op, &in_unit) {
                        Ok(rational) => {
                            OperationResult::from_literal(LiteralValue::number_with_type(
                                rational,
                                primitive_number_arc().clone(),
                            ))
                        }
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
                let measure_decomp = right
                    .lemma_type
                    .measure_type_decomposition()
                    .expect("BUG: decomposition must be resolved after planning");
                if measure_decomp.is_empty() {
                    match number_arithmetic(n, op, measure_val) {
                        Ok(rational) => {
                            OperationResult::from_literal(LiteralValue::number_with_type(
                                rational,
                                primitive_number_arc().clone(),
                            ))
                        }
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    }
                } else {
                    let negated_signature =
                        crate::planning::semantics::negate_signature(measure_signature);
                    match number_arithmetic(n, op, measure_val) {
                        Ok(rational) => {
                            if let Some((unit_name, owning_type)) =
                                signature_index.get(&negated_signature)
                            {
                                let target_factor =
                                    owning_type.measure_unit_factor(unit_name).clone();
                                match crate::computation::rational::checked_div(
                                    &rational,
                                    &target_factor,
                                ) {
                                    Ok(magnitude) => OperationResult::from_literal(
                                        LiteralValue::measure_with_type(
                                            magnitude,
                                            unit_name.clone(),
                                            owning_type.clone(),
                                        ),
                                    ),
                                    Err(failure) => OperationResult::Veto(VetoType::computation(
                                        failure.to_string(),
                                    )),
                                }
                            } else {
                                OperationResult::from_literal(LiteralValue::number_with_type(
                                    rational,
                                    primitive_number_arc().clone(),
                                ))
                            }
                        }
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    }
                }
            }
            ArithmeticComputation::Modulo => {
                let (unit_name, exponent) = measure_signature
                    .first()
                    .expect("BUG: number modulo measure requires single-term signature");
                if *exponent != 1 {
                    unreachable!(
                        "BUG: number modulo measure with compound signature; planning must reject"
                    );
                }
                let factor = right.lemma_type.measure_unit_factor(unit_name);
                let in_unit = checked_div(measure_val, factor)
                    .expect("BUG: measure de-canonicalization for modulo must not fail");
                number_op_on_stored_rationals_primitive(n, op, &in_unit)
            }
            _ => unreachable!(
                "BUG: Number {:?} Measure should be rejected during planning",
                op
            ),
        },
        _ => unreachable!(
            "BUG: arithmetic {:?} for {:?} and {:?}; planning should have rejected this",
            op,
            type_name(left),
            type_name(right)
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NumberArithmeticFailure {
    DivisionByZero,
    ModuloByZero,
    Computation(String),
}

impl NumberArithmeticFailure {
    pub(crate) fn message(self) -> String {
        match self {
            Self::DivisionByZero => "Division by zero".to_string(),
            Self::ModuloByZero => "Division by zero (modulo)".to_string(),
            Self::Computation(message) => message,
        }
    }
}

fn numeric_operation_from_arithmetic(operator: &ArithmeticComputation) -> NumericOperation {
    match operator {
        ArithmeticComputation::Add => NumericOperation::Add,
        ArithmeticComputation::Subtract => NumericOperation::Subtract,
        ArithmeticComputation::Multiply => NumericOperation::Multiply,
        ArithmeticComputation::Divide => NumericOperation::Divide,
        ArithmeticComputation::Modulo => NumericOperation::Modulo,
        ArithmeticComputation::Power => NumericOperation::Power,
    }
}

fn map_numeric_failure(failure: NumericFailure) -> NumberArithmeticFailure {
    match failure {
        NumericFailure::DivisionByZero => NumberArithmeticFailure::DivisionByZero,
        other => NumberArithmeticFailure::Computation(other.to_string()),
    }
}

fn map_numeric_failure_modulo(failure: NumericFailure) -> NumberArithmeticFailure {
    match failure {
        NumericFailure::DivisionByZero => NumberArithmeticFailure::ModuloByZero,
        other => map_numeric_failure(other),
    }
}

fn measure_scale_magnitude_by_rational(
    magnitude: RationalInteger,
    factor: &RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    checked_mul(&magnitude, factor).map_err(map_numeric_failure)
}

fn measure_ratio_arithmetic(
    measure_value: RationalInteger,
    operator: &ArithmeticComputation,
    ratio_value: RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    match operator {
        ArithmeticComputation::Multiply => {
            measure_scale_magnitude_by_rational(measure_value, &ratio_value)
        }
        ArithmeticComputation::Divide => {
            rational_operation_with_fallback(&measure_value, NumericOperation::Divide, &ratio_value)
                .map_err(map_numeric_failure)
        }
        _ => unreachable!(
            "BUG: measure {:?} ratio is rejected during planning",
            operator
        ),
    }
}

fn measure_add_subtract(
    left_value: &RationalInteger,
    operator: &ArithmeticComputation,
    right_value: &RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    rational_operation_with_fallback(
        left_value,
        numeric_operation_from_arithmetic(operator),
        right_value,
    )
    .map_err(map_numeric_failure)
}

pub(crate) fn number_arithmetic(
    left: &RationalInteger,
    operator: &ArithmeticComputation,
    right: &RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    rational_operation_with_fallback(left, numeric_operation_from_arithmetic(operator), right)
        .map_err(|failure| {
            if matches!(operator, ArithmeticComputation::Modulo) {
                map_numeric_failure_modulo(failure)
            } else {
                map_numeric_failure(failure)
            }
        })
}

fn operate_on_operation_results(
    left_result: OperationResult,
    op: &ArithmeticComputation,
    right_result: OperationResult,
    unit_index: &crate::planning::unit_index::UnitIndex,
    signature_index: &SignatureIndex,
) -> OperationResult {
    let left_value = match left_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    let right_value = match right_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    arithmetic_operation(
        left_value.as_ref(),
        op,
        right_value.as_ref(),
        unit_index,
        signature_index,
    )
}

fn operate_with_left_result(
    left_result: OperationResult,
    op: &ArithmeticComputation,
    right: &LiteralValue,
    unit_index: &crate::planning::unit_index::UnitIndex,
    signature_index: &SignatureIndex,
) -> OperationResult {
    let left_value = match left_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    arithmetic_operation(left_value.as_ref(), op, right, unit_index, signature_index)
}

fn operate_with_right_result(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right_result: OperationResult,
    unit_index: &crate::planning::unit_index::UnitIndex,
    signature_index: &SignatureIndex,
) -> OperationResult {
    let right_value = match right_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    arithmetic_operation(left, op, right_value.as_ref(), unit_index, signature_index)
}

fn shift_date_range_right_endpoint(
    range_left: &LiteralValue,
    range_right: &LiteralValue,
    calendar_value: &RationalInteger,
    calendar_unit: &SemanticCalendarUnit,
    add: bool,
    calendar_lemma_type: Arc<LemmaType>,
) -> OperationResult {
    let (ValueKind::Date(_), ValueKind::Date(_)) = (&range_left.value, &range_right.value) else {
        unreachable!(
            "BUG: date range calendar arithmetic received non-date endpoints; planning should have rejected this"
        );
    };

    let calendar_literal = LiteralValue::calendar(
        calendar_value.clone(),
        calendar_unit.clone(),
        calendar_lemma_type,
    );
    let shifted_right = match super::datetime::datetime_arithmetic(
        range_right,
        if add {
            &ArithmeticComputation::Add
        } else {
            &ArithmeticComputation::Subtract
        },
        &calendar_literal,
    ) {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };

    OperationResult::from_literal(LiteralValue::range(
        range_left.clone(),
        shifted_right.as_ref().clone(),
    ))
}

fn shift_calendar_range_right_endpoint(
    range_left: &LiteralValue,
    range_right: &LiteralValue,
    calendar_value: &RationalInteger,
    calendar_unit: &SemanticCalendarUnit,
    add: bool,
    indexes: &CalendarRangeShiftIndexes<'_>,
) -> OperationResult {
    let (ValueKind::Measure(_, _), ValueKind::Measure(_, _)) =
        (&range_left.value, &range_right.value)
    else {
        unreachable!(
            "BUG: calendar range calendar arithmetic received non-calendar endpoints; planning should have rejected this"
        );
    };
    if !range_left.lemma_type.is_calendar_like() || !range_right.lemma_type.is_calendar_like() {
        unreachable!(
            "BUG: calendar range calendar arithmetic received non-calendar endpoints; planning should have rejected this"
        );
    }

    let calendar_literal = LiteralValue::calendar(
        calendar_value.clone(),
        calendar_unit.clone(),
        Arc::clone(&range_right.lemma_type),
    );
    let op = if add {
        ArithmeticComputation::Add
    } else {
        ArithmeticComputation::Subtract
    };
    let shifted_right = match arithmetic_operation(
        range_right,
        &op,
        &calendar_literal,
        indexes.unit_index,
        indexes.signature_index,
    ) {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };

    OperationResult::from_literal(LiteralValue::range(
        range_left.clone(),
        shifted_right.as_ref().clone(),
    ))
}

fn type_name(value: &LiteralValue) -> String {
    value.get_type().name().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::rational_new;
    use crate::planning::semantics::{ArithmeticComputation, LiteralValue, ValueKind};
    use rust_decimal::Decimal;

    #[test]
    fn number_arithmetic_add_on_stored_decimals() {
        let left = rational_new(1, 1);
        let right = rational_new(2, 1);
        let sum = number_arithmetic(&left, &ArithmeticComputation::Add, &right).unwrap();
        assert_eq!(sum, rational_new(3, 1));
    }

    #[test]
    fn number_arithmetic_divide_ten_by_three_returns_value_decimal() {
        let left = rational_new(10, 1);
        let right = rational_new(3, 1);
        let quotient = number_arithmetic(&left, &ArithmeticComputation::Divide, &right).unwrap();
        let decimal = quotient.try_to_decimal().unwrap();
        assert!(decimal > Decimal::from(3));
        assert!(decimal < Decimal::from(4));
    }

    #[test]
    fn number_arithmetic_division_by_zero_returns_failure() {
        let left = rational_new(10, 1);
        let right = rational_new(0, 1);
        let failure = number_arithmetic(&left, &ArithmeticComputation::Divide, &right).unwrap_err();
        assert_eq!(failure, NumberArithmeticFailure::DivisionByZero);
    }

    #[test]
    fn arithmetic_operation_adds_primitive_numbers() {
        use crate::computation::rational::decimal_to_rational;
        use rust_decimal::Decimal;
        let left = LiteralValue::number(decimal_to_rational(Decimal::new(11, 1)).unwrap());
        let right = LiteralValue::number(decimal_to_rational(Decimal::new(9, 1)).unwrap());
        let unit_index = crate::planning::unit_index::UnitIndex::new();
        let signature_index = SignatureIndex::new();
        let OperationResult::Value(lit) = arithmetic_operation(
            &left,
            &ArithmeticComputation::Add,
            &right,
            &unit_index,
            &signature_index,
        ) else {
            panic!("expected value");
        };
        match &lit.value {
            ValueKind::Number(n) => {
                assert_eq!(n, &decimal_to_rational(Decimal::new(2, 0)).unwrap());
            }
            other => panic!("expected number, got {other:?}"),
        }
    }

    #[test]
    fn arithmetic_operation_propagates_veto_from_left() {
        let left = OperationResult::Veto(VetoType::computation("left failed"));
        let right = LiteralValue::number(rational_new(1, 1));
        let unit_index = crate::planning::unit_index::UnitIndex::new();
        let signature_index = SignatureIndex::new();
        let result = operate_on_operation_results(
            left,
            &ArithmeticComputation::Add,
            OperationResult::from_literal(right),
            &unit_index,
            &signature_index,
        );
        assert!(matches!(result, OperationResult::Veto(_)));
    }

    // ---------------------------------------------------------------------------
    // Phase 0 — Q*Q signature behaviour for the rewritten arithmetic arm.
    //
    // These tests use ValueKind::Measure with the OLD (RationalInteger, String,
    // BaseMeasureVector) shape today. After rewrite_measure_value_kind_shape lands,
    // they will be updated to (RationalInteger, Vec<(String,i32)>). They will fail today
    // because the Q*Q arm uses canonical magnitudes (not direct multiply), and the result
    // emission paths differ from the rewrite spec.
    // ---------------------------------------------------------------------------

    fn build_engine_and_get_value(code: &str, spec: &str, rule: &str) -> LiteralValue {
        use crate::engine::Engine;
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::Arc;
        let mut engine = Engine::new();
        engine
            .load([(
                crate::parsing::source::SourceType::Path(Arc::new(PathBuf::from("t.lemma"))),
                code.to_string(),
            )])
            .expect("spec must load");
        let response = engine
            .run(None, spec, None, HashMap::new(), None, false)
            .expect("spec must evaluate");
        let rule_result = response
            .results
            .get(rule)
            .unwrap_or_else(|| panic!("rule '{}' missing", rule));
        if rule_result.vetoed {
            panic!("rule '{}' vetoed", rule);
        }
        rule_result.to_literal()
    }

    /// Phase 0 — Q*Q producing a signature hit must emit a named lemma_type.
    /// Today the arithmetic arm uses canonical pre-conversion (not direct multiply),
    /// so the magnitude may differ. After the rewrite, the magnitude must equal the
    /// direct product of operand magnitudes.
    #[test]
    fn q_times_q_signature_hit_emits_named_lemma_type() {
        let code = r#"spec t
data money: measure
  -> unit eur: 1
data rate: measure
  -> unit eur_per_hour: eur/hour
data hour: measure
  -> unit hour: 1
data r: 30 eur_per_hour
data h: 2 hour
rule pay: r * h
"#;
        let value = build_engine_and_get_value(code, "t", "pay");
        // Result must be a Measure. The signature [(eur,1),(hour,-1),(hour,1)] -> [(eur,1)]
        // must hit signature_index and emit lemma_type = money.
        match &value.value {
            ValueKind::Measure(_, _) => {}
            other => panic!("expected Measure result, got {:?}", other),
        }
        assert_eq!(
            value.lemma_type.name(),
            "money",
            "result must be promoted to 'money' via signature hit"
        );
    }

    /// Phase 0 — Q*Q producing a signature miss must emit anonymous lemma_type.
    /// e.g., `eur_per_minute * hour` does not cancel; signature is
    /// [(eur,1),(hour,1),(minute,-1)] which has no signature_index entry.
    #[test]
    fn q_times_q_signature_miss_emits_anonymous_lemma_type() {
        let code = r#"spec t
data money: measure
  -> unit eur: 1
data rate: measure
  -> unit eur_per_minute: eur/minute
data hour: measure
  -> unit hour: 1
data r: 40 eur_per_minute
data h: 2 hour
rule weird: r * h
"#;
        // The full evaluation will be rejected today by the rule boundary check (anonymous
        // intermediate). After the rewrite, planning either accepts (no matching named type
        // -> rejected with precise message) or accepts when the signature combines to a known
        // type. For this test, the expectation is that the rule is rejected today.
        use crate::engine::Engine;
        use std::path::PathBuf;
        use std::sync::Arc;
        let mut engine = Engine::new();
        let result = engine.load([(
            crate::parsing::source::SourceType::Path(Arc::new(PathBuf::from("t.lemma"))),
            code.to_string(),
        )]);
        assert!(
            result.is_err(),
            "rule producing anonymous Q*Q with no signature_index match must be rejected"
        );
    }

    /// Phase 0 — Q + Q with identical signatures sums magnitudes directly.
    #[test]
    fn q_plus_q_same_signature() {
        let code = r#"spec t
data money: measure
  -> unit eur: 1
data a: 100 eur
data b: 50 eur
rule total: a + b
"#;
        let value = build_engine_and_get_value(code, "t", "total");
        match &value.value {
            ValueKind::Measure(n, _) => {
                let decimal = n.try_to_decimal().unwrap();
                assert_eq!(decimal, Decimal::from(150));
            }
            other => panic!("expected Measure, got {:?}", other),
        }
    }

    /// Phase 0 — Q + Q with different but dimensionally-compatible signatures converts
    /// via signature_factor before summing.
    /// 10 eur_per_second + 20 eur_per_minute = 10 + (20/60) eur_per_second
    #[test]
    fn q_plus_q_different_signature() {
        let code = r#"spec t
uses lemma units
data money: measure
  -> unit eur: 1
data rate: measure
  -> unit eur_per_second: eur/second
  -> unit eur_per_minute: eur/minute
data a: 10 eur_per_second
data b: 20 eur_per_minute
rule total_rate: (a + b) as eur_per_second
"#;
        let value = build_engine_and_get_value(code, "t", "total_rate");
        match &value.value {
            ValueKind::Measure(n, _) => {
                let decimal = n.try_to_decimal().unwrap();
                // 10 + 1/3 ≈ 10.333...
                let expected_low = Decimal::new(10_333, 3);
                let expected_high = Decimal::new(10_334, 3);
                assert!(
                    decimal >= expected_low && decimal <= expected_high,
                    "expected ~10.333, got {}",
                    decimal
                );
            }
            other => panic!("expected Measure, got {:?}", other),
        }
    }

    /// Phase 0 — `number / measure` must produce a Measure whose signature is the
    /// negation of the rhs signature, and lemma_type from signature_index lookup or
    /// anonymous marker.
    #[test]
    fn number_divided_by_q_reciprocates_signature() {
        let code = r#"spec t
data duration_t: measure
  -> unit second: 1
data freq: measure
  -> unit per_second: second^-1
data d: 2 second
rule f: 1 / d
"#;
        // 1 / second = 1 per_second; signature_index hit yields named 'freq' type.
        let value = build_engine_and_get_value(code, "t", "f");
        match &value.value {
            ValueKind::Measure(_, _) => {}
            other => panic!("expected Measure, got {:?}", other),
        }
        assert_eq!(
            value.lemma_type.name(),
            "freq",
            "1/second must promote to 'freq' (signature [(second,-1)] -> per_second)"
        );
    }
}
