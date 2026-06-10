//! Type-aware arithmetic operations

use crate::computation::rational::{
    checked_add, checked_div, checked_mul, checked_sub, rational_new,
    rational_operation_with_fallback, NumericFailure, NumericOperation, RationalInteger,
};
use crate::evaluation::operations::{OperationResult, VetoType};
use crate::planning::semantics::{
    combine_signatures, primitive_number_arc, ArithmeticComputation, LemmaType, LiteralValue,
    SemanticCalendarUnit, ValueKind,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Reverse index mapping a canonical-form unit signature `Vec<(unit_name, exponent)>`
/// to the unit (by name) and its owning quantity type. Built during planning.
pub type SignatureIndex = HashMap<Vec<(String, i32)>, (String, Arc<LemmaType>)>;

struct CalendarRangeShiftIndexes<'a> {
    unit_index: &'a HashMap<String, Arc<LemmaType>>,
    signature_index: &'a SignatureIndex,
}

/// Promote an anonymous quantity result to a named type when its signature matches an
/// entry in `signature_index`.
///
/// Used for operations that produce anonymous intermediates at runtime
/// (Date-Time subtraction, range spans, cross-unit multiplication).
fn promote_anonymous_quantity_result(
    result: OperationResult,
    signature_index: &SignatureIndex,
    unit_index: &HashMap<String, Arc<LemmaType>>,
) -> OperationResult {
    let OperationResult::Value(value) = result else {
        return result;
    };
    let ValueKind::Quantity(magnitude, raw_signature) = &value.value else {
        return OperationResult::Value(value);
    };
    if !value.lemma_type.is_anonymous_quantity() {
        return OperationResult::Value(value);
    }
    let expanded = expand_signature_to_base_units(raw_signature, unit_index);
    let Some((unit_name, owning_type)) = signature_index.get(&expanded) else {
        return OperationResult::Value(value);
    };
    OperationResult::Value(LiteralValue::quantity_with_type(
        magnitude.clone(),
        unit_name.clone(),
        owning_type.clone(),
    ))
}

/// Expand a raw signature (literal unit names) to derived base units, so that signature_index
/// (which is keyed by `derived_quantity_factors`) can be searched.
///
/// For each `(unit_name, exp)`:
/// - Look up `unit_name`'s owning type in `unit_index`.
/// - Replace it with its `derived_quantity_factors`, scaled by `exp`.
/// - When multiple units from the same family appear, normalize them to the canonical
///   unit name so exponents can cancel (e.g. `hour` and `minute` both become `second`).
/// - Single remaining units keep their declared name (e.g. `minute` stays `minute`).
fn quantity_family_key(
    unit_name: &str,
    unit_index: &HashMap<String, Arc<LemmaType>>,
) -> Option<String> {
    use crate::planning::semantics::calendar_unit_factor;
    if calendar_unit_factor(unit_name).is_some() {
        return Some("__calendar__".to_string());
    }
    unit_index
        .get(unit_name)
        .and_then(|t| t.quantity_family_name().map(str::to_string))
}

fn canonical_unit_in_family(
    family_key: &str,
    unit_index: &HashMap<String, Arc<LemmaType>>,
) -> String {
    use crate::planning::semantics::TypeSpecification;
    if family_key == "__calendar__" {
        return "month".to_string();
    }
    for lemma_type in unit_index.values() {
        if lemma_type.quantity_family_name() != Some(family_key) {
            continue;
        }
        if let TypeSpecification::Quantity { units, .. } = &lemma_type.specifications {
            if let Some(canonical) = units.iter().find(|u| u.is_canonical_factor()) {
                return canonical.name.clone();
            }
        }
    }
    family_key.to_string()
}

pub(crate) fn expand_signature_to_base_units(
    raw: &[(String, i32)],
    unit_index: &HashMap<String, Arc<LemmaType>>,
) -> Vec<(String, i32)> {
    use crate::planning::semantics::canonicalize_signature;
    use crate::planning::semantics::TypeSpecification;
    use std::collections::BTreeMap;
    let mut expanded: Vec<(String, i32)> = Vec::new();
    for (unit_name, exp) in raw {
        if let Some(owning_type) = unit_index.get(unit_name) {
            if let TypeSpecification::Quantity { units, .. } = &owning_type.specifications {
                if let Ok(unit) = units.get(unit_name) {
                    if !unit.derived_quantity_factors.is_empty() {
                        for (base_name, base_exp) in &unit.derived_quantity_factors {
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
        if let Some(family) = quantity_family_key(&unit_name, unit_index) {
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
    left: RationalInteger,
    operator: &ArithmeticComputation,
    right: RationalInteger,
    lemma_type: Arc<LemmaType>,
) -> OperationResult {
    match number_arithmetic(left, operator, right) {
        Ok(rational) => {
            OperationResult::Value(LiteralValue::number_with_type(rational, lemma_type))
        }
        Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
    }
}

fn number_op_on_stored_rationals_primitive(
    left: RationalInteger,
    operator: &ArithmeticComputation,
    right: RationalInteger,
) -> OperationResult {
    number_op_on_stored_rationals(left, operator, right, primitive_number_arc().clone())
}

fn number_ratio_arithmetic(
    n: RationalInteger,
    op: &ArithmeticComputation,
    r: RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    let one = rational_new(1, 1);
    match op {
        ArithmeticComputation::Add => {
            let factor = checked_add(&one, &r).map_err(map_numeric_failure)?;
            checked_mul(&n, &factor).map_err(map_numeric_failure)
        }
        ArithmeticComputation::Subtract => {
            let factor = checked_sub(&one, &r).map_err(map_numeric_failure)?;
            checked_mul(&n, &factor).map_err(map_numeric_failure)
        }
        ArithmeticComputation::Multiply => checked_mul(&n, &r).map_err(map_numeric_failure),
        ArithmeticComputation::Divide => checked_div(&n, &r).map_err(map_numeric_failure),
        _ => unreachable!(
            "BUG: number/ratio with {:?}; planning should have rejected this",
            op
        ),
    }
}

fn calendar_from_months_arithmetic(
    left: RationalInteger,
    left_unit: &SemanticCalendarUnit,
    operator: &ArithmeticComputation,
    right: RationalInteger,
    _right_unit: &SemanticCalendarUnit,
    result_lemma_type: Arc<LemmaType>,
) -> OperationResult {
    let result_months = match operator {
        ArithmeticComputation::Add => match checked_add(&left, &right) {
            Ok(r) => r,
            Err(failure) => {
                return OperationResult::Veto(VetoType::computation(
                    map_numeric_failure(failure).message(),
                ))
            }
        },
        ArithmeticComputation::Subtract => match checked_sub(&left, &right) {
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
    OperationResult::Value(LiteralValue::calendar_with_type(
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
    unit_index: &HashMap<String, Arc<LemmaType>>,
    signature_index: &SignatureIndex,
) -> OperationResult {
    match (&left.value, &right.value) {
        (ValueKind::Range(range_left, range_right), ValueKind::Quantity(value, sig))
            if left.lemma_type.is_calendar_like_range()
                && right.lemma_type.is_calendar_like()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(sig);
            shift_calendar_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                value.clone(),
                &unit,
                matches!(op, ArithmeticComputation::Add),
                &CalendarRangeShiftIndexes {
                    unit_index,
                    signature_index,
                },
            )
        }

        (ValueKind::Quantity(value, sig), ValueKind::Range(range_left, range_right))
            if right.lemma_type.is_calendar_like_range()
                && left.lemma_type.is_calendar_like()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(sig);
            shift_calendar_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                value.clone(),
                &unit,
                matches!(op, ArithmeticComputation::Add),
                &CalendarRangeShiftIndexes {
                    unit_index,
                    signature_index,
                },
            )
        }

        (ValueKind::Range(range_left, range_right), ValueKind::Quantity(value, sig))
            if left.lemma_type.is_date_range()
                && right.lemma_type.is_calendar_like()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(sig);
            shift_date_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                value.clone(),
                &unit,
                matches!(op, ArithmeticComputation::Add),
                right.lemma_type.clone(),
            )
        }

        (ValueKind::Quantity(value, sig), ValueKind::Range(range_left, range_right))
            if right.lemma_type.is_date_range()
                && left.lemma_type.is_calendar_like()
                && matches!(
                    op,
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract
                ) =>
        {
            let unit =
                crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(sig);
            shift_date_range_right_endpoint(
                range_left.as_ref(),
                range_right.as_ref(),
                value.clone(),
                &unit,
                matches!(op, ArithmeticComputation::Add),
                left.lemma_type.clone(),
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
            let left_measure = super::range::compute_quantity(
                left_range_left.as_ref(),
                left_range_right.as_ref(),
                None,
            );
            let right_measure = super::range::compute_quantity(
                right_range_left.as_ref(),
                right_range_right.as_ref(),
                None,
            );
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
            let measure = super::range::compute_quantity(
                range_left.as_ref(),
                range_right.as_ref(),
                Some(right),
            );
            operate_with_left_result(measure, op, right, unit_index, signature_index)
        }

        (_, ValueKind::Range(range_left, range_right))
            if matches!(
                op,
                ArithmeticComputation::Add | ArithmeticComputation::Subtract
            ) =>
        {
            let measure = super::range::compute_quantity(
                range_left.as_ref(),
                range_right.as_ref(),
                Some(left),
            );
            operate_with_right_result(left, op, measure, unit_index, signature_index)
        }

        (ValueKind::Number(l), ValueKind::Number(r)) => {
            number_op_on_stored_rationals(l.clone(), op, r.clone(), left.lemma_type.clone())
        }

        (ValueKind::Date(_), _) | (_, ValueKind::Date(_)) => promote_anonymous_quantity_result(
            super::datetime::datetime_arithmetic(left, op, right),
            signature_index,
            unit_index,
        ),

        (ValueKind::Time(_), _) | (_, ValueKind::Time(_)) => promote_anonymous_quantity_result(
            super::datetime::time_arithmetic(left, op, right),
            signature_index,
            unit_index,
        ),

        // Ratio with Number → Number (ratio semantics: add/subtract apply as multiplier)
        // r + n means n * (1 + r), r - n means n * (1 - r)
        (ValueKind::Ratio(_, _), ValueKind::Number(_))
        | (ValueKind::Number(_), ValueKind::Ratio(_, _)) => {
            let (n, r) = match (&left.value, &right.value) {
                (ValueKind::Number(n), ValueKind::Ratio(r, _)) => (n.clone(), r.clone()),
                (ValueKind::Ratio(r, _), ValueKind::Number(n)) => (n.clone(), r.clone()),
                _ => unreachable!("BUG: matched number/ratio arm with other value kinds"),
            };
            match number_ratio_arithmetic(n, op, r) {
                Ok(rational) => OperationResult::Value(LiteralValue::number_with_type(
                    rational,
                    primitive_number_arc().clone(),
                )),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }

        // Ratio op Ratio → Ratio
        (ValueKind::Ratio(l, lu), ValueKind::Ratio(r, _ru)) => {
            match number_arithmetic(l.clone(), op, r.clone()) {
                Ok(rational) => OperationResult::Value(LiteralValue::ratio_with_type(
                    rational,
                    lu.clone(),
                    left.lemma_type.clone(),
                )),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }
        // Quantity operations with Quantity
        (ValueKind::Quantity(l_val, l_signature), ValueKind::Quantity(r_val, r_signature)) => {
            if left.lemma_type.is_calendar_like() != right.lemma_type.is_calendar_like() {
                let is_quantity_left =
                    left.lemma_type.is_quantity() && !left.lemma_type.is_calendar_like();
                let is_multiply = matches!(op, ArithmeticComputation::Multiply);
                let is_divide = matches!(op, ArithmeticComputation::Divide);
                if !is_multiply && !is_divide {
                    unreachable!(
                        "BUG: quantity {:?} calendar is rejected during planning",
                        op
                    );
                }
                let (l_val, l_sig, r_val, r_sig) = if is_quantity_left {
                    (
                        l_val.clone(),
                        l_signature.clone(),
                        r_val.clone(),
                        r_signature.clone(),
                    )
                } else {
                    (
                        r_val.clone(),
                        r_signature.clone(),
                        l_val.clone(),
                        l_signature.clone(),
                    )
                };
                if is_divide && crate::computation::rational::rational_is_zero(&r_val) {
                    return OperationResult::Veto(VetoType::computation("Division by zero"));
                }
                let raw_result = match rational_operation_with_fallback(
                    &l_val,
                    if is_multiply {
                        NumericOperation::Multiply
                    } else {
                        NumericOperation::Divide
                    },
                    &r_val,
                ) {
                    Ok(p) => p,
                    Err(failure) => {
                        return OperationResult::Veto(VetoType::computation(
                            map_numeric_failure(failure).message(),
                        ))
                    }
                };
                let raw_signature = combine_signatures(&l_sig, &r_sig, is_multiply);
                let q_decomp = if is_quantity_left {
                    left.lemma_type.quantity_type_decomposition()
                } else {
                    right.lemma_type.quantity_type_decomposition()
                }
                .expect("BUG: decomposition must be resolved after planning");
                let c_decomp = crate::planning::semantics::calendar_decomposition();
                let combined = crate::planning::semantics::combine_decompositions(
                    q_decomp,
                    &c_decomp,
                    is_multiply,
                );
                if combined.is_empty() {
                    return OperationResult::Value(LiteralValue::number_with_type(
                        raw_result,
                        primitive_number_arc().clone(),
                    ));
                }
                let expanded_signature = expand_signature_to_base_units(&raw_signature, unit_index);
                if let Some((unit_name, owning_type)) = signature_index.get(&expanded_signature) {
                    return OperationResult::Value(LiteralValue::quantity_with_type(
                        raw_result,
                        unit_name.clone(),
                        owning_type.clone(),
                    ));
                }
                return OperationResult::Value(LiteralValue {
                    value: ValueKind::Quantity(raw_result, raw_signature),
                    lemma_type: Arc::new(LemmaType::anonymous_for_decomposition(combined)),
                });
            }
            if left.lemma_type.is_calendar_like() && right.lemma_type.is_calendar_like() {
                let lu = crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(
                    l_signature,
                );
                let ru = crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(
                    r_signature,
                );
                return match op {
                    ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                        calendar_from_months_arithmetic(
                            l_val.clone(),
                            &lu,
                            op,
                            r_val.clone(),
                            &ru,
                            left.lemma_type.clone(),
                        )
                    }
                    ArithmeticComputation::Divide => number_op_on_stored_rationals_primitive(
                        l_val.clone(),
                        &ArithmeticComputation::Divide,
                        r_val.clone(),
                    ),
                    _ => unreachable!(
                        "BUG: calendar * calendar with op {:?}; planning should have rejected this",
                        op
                    ),
                };
            }
            if left.lemma_type.is_calendar_like() && !right.lemma_type.is_calendar_like() {
                let unit =
                    crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(
                        l_signature,
                    );
                if let ValueKind::Number(n) = &right.value {
                    return match number_arithmetic(l_val.clone(), op, n.clone()) {
                        Ok(rational) => OperationResult::Value(LiteralValue::calendar_with_type(
                            rational,
                            unit,
                            left.lemma_type.clone(),
                        )),
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
                if let ValueKind::Ratio(r, _) = &right.value {
                    return match number_ratio_arithmetic(l_val.clone(), op, r.clone()) {
                        Ok(rational) => OperationResult::Value(LiteralValue::calendar_with_type(
                            rational,
                            unit,
                            left.lemma_type.clone(),
                        )),
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
            }
            if right.lemma_type.is_calendar_like() && !left.lemma_type.is_calendar_like() {
                let unit =
                    crate::planning::semantics::semantic_calendar_unit_from_quantity_signature(
                        r_signature,
                    );
                if let (ValueKind::Number(n), ArithmeticComputation::Multiply) = (&left.value, op) {
                    return match number_arithmetic(n.clone(), op, r_val.clone()) {
                        Ok(rational) => OperationResult::Value(LiteralValue::calendar_with_type(
                            rational,
                            unit,
                            right.lemma_type.clone(),
                        )),
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
                if let ValueKind::Number(n) = &left.value {
                    return match number_arithmetic(n.clone(), op, r_val.clone()) {
                        Ok(rational) => OperationResult::Value(LiteralValue::number_with_type(
                            rational,
                            primitive_number_arc().clone(),
                        )),
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
                if let ValueKind::Ratio(r, _) = &left.value {
                    return match number_ratio_arithmetic(r_val.clone(), op, r.clone()) {
                        Ok(rational) => OperationResult::Value(LiteralValue::calendar_with_type(
                            rational,
                            unit,
                            right.lemma_type.clone(),
                        )),
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
            }
            match op {
                ArithmeticComputation::Add | ArithmeticComputation::Subtract => {
                    let same_family = left.lemma_type.same_quantity_family(&right.lemma_type);
                    let anonymous_compatible = left
                        .lemma_type
                        .compatible_with_anonymous_quantity(&right.lemma_type);
                    if !same_family && !anonymous_compatible {
                        unreachable!(
                        "BUG: quantity add/subtract with incompatible types ({} vs {}); should be rejected during planning",
                        left.lemma_type.name(),
                        right.lemma_type.name()
                    );
                    }
                    match quantity_add_subtract(l_val.clone(), op, r_val.clone()) {
                        Ok(rational) => {
                            OperationResult::Value(LiteralValue::quantity_with_signature(
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
                        .quantity_type_decomposition()
                        .expect("BUG: decomposition must be resolved after planning");
                    let r_decomp = right
                        .lemma_type
                        .quantity_type_decomposition()
                        .expect("BUG: decomposition must be resolved after planning");
                    let combined_decomposition = crate::planning::semantics::combine_decompositions(
                        l_decomp,
                        r_decomp,
                        matches!(op, ArithmeticComputation::Multiply),
                    );
                    if combined_decomposition.is_empty() {
                        OperationResult::Value(LiteralValue::number_with_type(
                            raw_result,
                            primitive_number_arc().clone(),
                        ))
                    } else {
                        let expanded_signature =
                            expand_signature_to_base_units(&raw_signature, unit_index);
                        if let Some((unit_name, owning_type)) =
                            signature_index.get(&expanded_signature)
                        {
                            OperationResult::Value(LiteralValue::quantity_with_type(
                                raw_result,
                                unit_name.clone(),
                                owning_type.clone(),
                            ))
                        } else {
                            OperationResult::Value(LiteralValue {
                                value: ValueKind::Quantity(raw_result, raw_signature),
                                lemma_type: Arc::new(LemmaType::anonymous_for_decomposition(
                                    combined_decomposition,
                                )),
                            })
                        }
                    }
                }
                _ => unreachable!(
                    "BUG: quantity {:?} quantity is rejected during planning",
                    op
                ),
            }
        }
        (ValueKind::Quantity(_q_val, _), ValueKind::Ratio(_r, _))
        | (ValueKind::Ratio(_r, _), ValueKind::Quantity(_q_val, _)) => {
            let (quantity_val, quantity_signature, quantity_type, ratio_val) = match (
                &left.value,
                &right.value,
                &left.lemma_type,
                &right.lemma_type,
            ) {
                (ValueKind::Quantity(q_val, q_sig), ValueKind::Ratio(r, _), qt, _) => {
                    (q_val.clone(), q_sig.clone(), qt, r.clone())
                }
                (ValueKind::Ratio(r, _), ValueKind::Quantity(q_val, q_sig), _, qt) => {
                    (q_val.clone(), q_sig.clone(), qt, r.clone())
                }
                _ => unreachable!("BUG: matched quantity/ratio arm with other value kinds"),
            };
            match quantity_ratio_arithmetic(quantity_val, op, ratio_val) {
                Ok(rational) => OperationResult::Value(LiteralValue::quantity_with_signature(
                    rational,
                    quantity_signature,
                    quantity_type.clone(),
                )),
                Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
            }
        }

        // Quantity op Number → Quantity (preserves unit)
        (ValueKind::Quantity(quantity_val, quantity_signature), ValueKind::Number(n)) => {
            let rational = if matches!(
                op,
                ArithmeticComputation::Modulo | ArithmeticComputation::Power
            ) {
                let (unit_name, exponent) = quantity_signature
                    .first()
                    .expect("BUG: quantity modulo number requires single-term signature");
                if *exponent != 1 {
                    unreachable!(
                        "BUG: quantity modulo number with compound signature; planning must reject"
                    );
                }
                let factor = left.lemma_type.quantity_unit_factor(unit_name);
                let in_unit = checked_div(quantity_val, factor)
                    .expect("BUG: quantity de-canonicalization for modulo must not fail");
                let modded = match number_arithmetic(in_unit, op, n.clone()) {
                    Ok(value) => value,
                    Err(failure) => {
                        return OperationResult::Veto(VetoType::computation(failure.message()))
                    }
                };
                checked_mul(&modded, factor)
                    .expect("BUG: quantity re-canonicalization after modulo must not fail")
            } else {
                match number_arithmetic(quantity_val.clone(), op, n.clone()) {
                    Ok(value) => value,
                    Err(failure) => {
                        return OperationResult::Veto(VetoType::computation(failure.message()))
                    }
                }
            };
            OperationResult::Value(LiteralValue::quantity_with_signature(
                rational,
                quantity_signature.clone(),
                left.lemma_type.clone(),
            ))
        }
        // Number op Quantity → Quantity for multiply; for divide, negate signature if anonymous Quantity.
        (ValueKind::Number(n), ValueKind::Quantity(quantity_val, quantity_signature)) => match op {
            ArithmeticComputation::Multiply => {
                match number_arithmetic(n.clone(), op, quantity_val.clone()) {
                    Ok(rational) => OperationResult::Value(LiteralValue::quantity_with_signature(
                        rational,
                        quantity_signature.clone(),
                        right.lemma_type.clone(),
                    )),
                    Err(failure) => OperationResult::Veto(VetoType::computation(failure.message())),
                }
            }
            ArithmeticComputation::Divide => {
                if right.lemma_type.is_duration_like_quantity()
                    || right.lemma_type.is_calendar_like()
                {
                    let (unit_name, exponent) = quantity_signature.first().expect(
                        "BUG: number divide duration-like quantity requires single-term signature",
                    );
                    if *exponent != 1 {
                        unreachable!(
                            "BUG: number divide duration-like quantity with compound signature; planning must reject"
                        );
                    }
                    let factor = right.lemma_type.quantity_unit_factor(unit_name);
                    let in_unit = checked_div(quantity_val, factor)
                        .expect("BUG: quantity de-canonicalization for divide must not fail");
                    return match number_arithmetic(n.clone(), op, in_unit) {
                        Ok(rational) => OperationResult::Value(LiteralValue::number_with_type(
                            rational,
                            primitive_number_arc().clone(),
                        )),
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    };
                }
                let quantity_decomp = right
                    .lemma_type
                    .quantity_type_decomposition()
                    .expect("BUG: decomposition must be resolved after planning");
                if quantity_decomp.is_empty() {
                    match number_arithmetic(n.clone(), op, quantity_val.clone()) {
                        Ok(rational) => OperationResult::Value(LiteralValue::number_with_type(
                            rational,
                            primitive_number_arc().clone(),
                        )),
                        Err(failure) => {
                            OperationResult::Veto(VetoType::computation(failure.message()))
                        }
                    }
                } else {
                    let negated_signature =
                        crate::planning::semantics::negate_signature(quantity_signature);
                    match number_arithmetic(n.clone(), op, quantity_val.clone()) {
                        Ok(rational) => {
                            if let Some((unit_name, owning_type)) =
                                signature_index.get(&negated_signature)
                            {
                                let target_factor =
                                    owning_type.quantity_unit_factor(unit_name).clone();
                                match crate::computation::rational::checked_div(
                                    &rational,
                                    &target_factor,
                                ) {
                                    Ok(magnitude) => {
                                        OperationResult::Value(LiteralValue::quantity_with_type(
                                            magnitude,
                                            unit_name.clone(),
                                            owning_type.clone(),
                                        ))
                                    }
                                    Err(failure) => OperationResult::Veto(VetoType::computation(
                                        failure.to_string(),
                                    )),
                                }
                            } else {
                                OperationResult::Value(LiteralValue::number_with_type(
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
                let (unit_name, exponent) = quantity_signature
                    .first()
                    .expect("BUG: number modulo quantity requires single-term signature");
                if *exponent != 1 {
                    unreachable!(
                        "BUG: number modulo quantity with compound signature; planning must reject"
                    );
                }
                let factor = right.lemma_type.quantity_unit_factor(unit_name);
                let in_unit = checked_div(quantity_val, factor)
                    .expect("BUG: quantity de-canonicalization for modulo must not fail");
                number_op_on_stored_rationals_primitive(n.clone(), op, in_unit)
            }
            _ => unreachable!(
                "BUG: Number {:?} Quantity should be rejected during planning",
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

fn quantity_scale_magnitude_by_rational(
    magnitude: RationalInteger,
    factor: &RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    checked_mul(&magnitude, factor).map_err(map_numeric_failure)
}

fn quantity_ratio_arithmetic(
    quantity_value: RationalInteger,
    operator: &ArithmeticComputation,
    ratio_value: RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    let one = rational_new(1, 1);
    match operator {
        ArithmeticComputation::Add => {
            let factor = checked_add(&one, &ratio_value).map_err(map_numeric_failure)?;
            quantity_scale_magnitude_by_rational(quantity_value, &factor)
        }
        ArithmeticComputation::Subtract => {
            let factor = checked_sub(&one, &ratio_value).map_err(map_numeric_failure)?;
            quantity_scale_magnitude_by_rational(quantity_value, &factor)
        }
        ArithmeticComputation::Multiply => {
            quantity_scale_magnitude_by_rational(quantity_value, &ratio_value)
        }
        ArithmeticComputation::Divide => rational_operation_with_fallback(
            &quantity_value,
            NumericOperation::Divide,
            &ratio_value,
        )
        .map_err(map_numeric_failure),
        _ => unreachable!(
            "BUG: quantity {:?} ratio is rejected during planning",
            operator
        ),
    }
}

fn quantity_add_subtract(
    left_value: RationalInteger,
    operator: &ArithmeticComputation,
    right_value: RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    rational_operation_with_fallback(
        &left_value,
        numeric_operation_from_arithmetic(operator),
        &right_value,
    )
    .map_err(map_numeric_failure)
}

pub(crate) fn number_arithmetic(
    left: RationalInteger,
    operator: &ArithmeticComputation,
    right: RationalInteger,
) -> Result<RationalInteger, NumberArithmeticFailure> {
    rational_operation_with_fallback(&left, numeric_operation_from_arithmetic(operator), &right)
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
    unit_index: &HashMap<String, Arc<LemmaType>>,
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
    arithmetic_operation(&left_value, op, &right_value, unit_index, signature_index)
}

fn operate_with_left_result(
    left_result: OperationResult,
    op: &ArithmeticComputation,
    right: &LiteralValue,
    unit_index: &HashMap<String, Arc<LemmaType>>,
    signature_index: &SignatureIndex,
) -> OperationResult {
    let left_value = match left_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    arithmetic_operation(&left_value, op, right, unit_index, signature_index)
}

fn operate_with_right_result(
    left: &LiteralValue,
    op: &ArithmeticComputation,
    right_result: OperationResult,
    unit_index: &HashMap<String, Arc<LemmaType>>,
    signature_index: &SignatureIndex,
) -> OperationResult {
    let right_value = match right_result {
        OperationResult::Value(value) => value,
        OperationResult::Veto(reason) => return OperationResult::Veto(reason),
    };
    arithmetic_operation(left, op, &right_value, unit_index, signature_index)
}

fn shift_date_range_right_endpoint(
    range_left: &LiteralValue,
    range_right: &LiteralValue,
    calendar_value: RationalInteger,
    calendar_unit: &SemanticCalendarUnit,
    add: bool,
    calendar_lemma_type: Arc<LemmaType>,
) -> OperationResult {
    let (ValueKind::Date(_), ValueKind::Date(_)) = (&range_left.value, &range_right.value) else {
        unreachable!(
            "BUG: date range calendar arithmetic received non-date endpoints; planning should have rejected this"
        );
    };

    let calendar_literal =
        LiteralValue::calendar(calendar_value, calendar_unit.clone(), calendar_lemma_type);
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

    OperationResult::Value(LiteralValue::range(
        range_left.clone(),
        shifted_right.clone(),
    ))
}

fn shift_calendar_range_right_endpoint(
    range_left: &LiteralValue,
    range_right: &LiteralValue,
    calendar_value: RationalInteger,
    calendar_unit: &SemanticCalendarUnit,
    add: bool,
    indexes: &CalendarRangeShiftIndexes<'_>,
) -> OperationResult {
    let (ValueKind::Quantity(_, _), ValueKind::Quantity(_, _)) =
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
        calendar_value,
        calendar_unit.clone(),
        range_right.lemma_type.clone(),
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

    OperationResult::Value(LiteralValue::range(
        range_left.clone(),
        shifted_right.clone(),
    ))
}

fn type_name(value: &LiteralValue) -> String {
    value.get_type().name().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::rational_new;
    use crate::evaluation::operations::{OperationResult, VetoType};
    use crate::planning::semantics::{ArithmeticComputation, LiteralValue, ValueKind};
    use rust_decimal::Decimal;

    #[test]
    fn number_arithmetic_add_on_stored_decimals() {
        let sum = number_arithmetic(
            rational_new(1, 1),
            &ArithmeticComputation::Add,
            rational_new(2, 1),
        )
        .unwrap();
        assert_eq!(sum, rational_new(3, 1));
    }

    #[test]
    fn number_arithmetic_divide_ten_by_three_returns_value_decimal() {
        let quotient = number_arithmetic(
            rational_new(10, 1),
            &ArithmeticComputation::Divide,
            rational_new(3, 1),
        )
        .unwrap();
        let decimal = crate::computation::rational::commit_rational_to_decimal(&quotient).unwrap();
        assert!(decimal > Decimal::from(3));
        assert!(decimal < Decimal::from(4));
    }

    #[test]
    fn number_arithmetic_division_by_zero_returns_failure() {
        let failure = number_arithmetic(
            rational_new(10, 1),
            &ArithmeticComputation::Divide,
            rational_new(0, 1),
        )
        .unwrap_err();
        assert_eq!(failure, NumberArithmeticFailure::DivisionByZero);
    }

    #[test]
    fn arithmetic_operation_adds_primitive_numbers() {
        use crate::computation::rational::decimal_to_rational;
        use rust_decimal::Decimal;
        let left = LiteralValue::number(decimal_to_rational(Decimal::new(11, 1)).unwrap());
        let right = LiteralValue::number(decimal_to_rational(Decimal::new(9, 1)).unwrap());
        let unit_index = HashMap::new();
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
        let unit_index = HashMap::new();
        let signature_index = SignatureIndex::new();
        let result = operate_on_operation_results(
            left,
            &ArithmeticComputation::Add,
            OperationResult::Value(right),
            &unit_index,
            &signature_index,
        );
        assert!(matches!(result, OperationResult::Veto(_)));
    }

    // ---------------------------------------------------------------------------
    // Phase 0 — Q*Q signature behaviour for the rewritten arithmetic arm.
    //
    // These tests use ValueKind::Quantity with the OLD (RationalInteger, String,
    // BaseQuantityVector) shape today. After rewrite_quantity_value_kind_shape lands,
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
            .load(
                code,
                crate::parsing::source::SourceType::Path(Arc::new(PathBuf::from("t.lemma"))),
            )
            .expect("spec must load");
        let response = engine
            .run(None, spec, None, HashMap::new(), false, None)
            .expect("spec must evaluate");
        let rule_result = response
            .results
            .get(rule)
            .unwrap_or_else(|| panic!("rule '{}' missing", rule));
        if rule_result.vetoed {
            panic!("rule '{}' vetoed", rule);
        }
        rule_result.materialized_literal()
    }

    /// Phase 0 — Q*Q producing a signature hit must emit a named lemma_type.
    /// Today the arithmetic arm uses canonical pre-conversion (not direct multiply),
    /// so the magnitude may differ. After the rewrite, the magnitude must equal the
    /// direct product of operand magnitudes.
    #[test]
    fn q_times_q_signature_hit_emits_named_lemma_type() {
        let code = r#"spec t
data money: quantity
  -> unit eur 1
data rate: quantity
  -> unit eur_per_hour eur/hour
data hours: quantity
  -> unit hour 1
data r: 30 eur_per_hour
data h: 2 hour
rule pay: r * h
"#;
        let value = build_engine_and_get_value(code, "t", "pay");
        // Result must be a Quantity. The signature [(eur,1),(hour,-1),(hour,1)] -> [(eur,1)]
        // must hit signature_index and emit lemma_type = money.
        match &value.value {
            ValueKind::Quantity(_, _) => {}
            other => panic!("expected Quantity result, got {:?}", other),
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
data money: quantity
  -> unit eur 1
data rate: quantity
  -> unit eur_per_minute eur/minute
data hours: quantity
  -> unit hour 1
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
        let result = engine.load(
            code,
            crate::parsing::source::SourceType::Path(Arc::new(PathBuf::from("t.lemma"))),
        );
        assert!(
            result.is_err(),
            "rule producing anonymous Q*Q with no signature_index match must be rejected"
        );
    }

    /// Phase 0 — Q + Q with identical signatures sums magnitudes directly.
    #[test]
    fn q_plus_q_same_signature() {
        let code = r#"spec t
data money: quantity
  -> unit eur 1
data a: 100 eur
data b: 50 eur
rule total: a + b
"#;
        let value = build_engine_and_get_value(code, "t", "total");
        match &value.value {
            ValueKind::Quantity(n, _) => {
                let decimal = crate::computation::rational::commit_rational_to_decimal(n).unwrap();
                assert_eq!(decimal, Decimal::from(150));
            }
            other => panic!("expected Quantity, got {:?}", other),
        }
    }

    /// Phase 0 — Q + Q with different but dimensionally-compatible signatures converts
    /// via signature_factor before summing.
    /// 10 eur_per_second + 20 eur_per_minute = 10 + (20/60) eur_per_second
    #[test]
    fn q_plus_q_different_signature() {
        let code = r#"spec t
uses lemma units
data money: quantity
  -> unit eur 1
data rate: quantity
  -> unit eur_per_second eur/second
  -> unit eur_per_minute eur/minute
data a: 10 eur_per_second
data b: 20 eur_per_minute
rule total_rate: (a + b) as eur_per_second
"#;
        let value = build_engine_and_get_value(code, "t", "total_rate");
        match &value.value {
            ValueKind::Quantity(n, _) => {
                let decimal = crate::computation::rational::commit_rational_to_decimal(n).unwrap();
                // 10 + 1/3 ≈ 10.333...
                let expected_low = Decimal::new(10_333, 3);
                let expected_high = Decimal::new(10_334, 3);
                assert!(
                    decimal >= expected_low && decimal <= expected_high,
                    "expected ~10.333, got {}",
                    decimal
                );
            }
            other => panic!("expected Quantity, got {:?}", other),
        }
    }

    /// Phase 0 — `number / quantity` must produce a Quantity whose signature is the
    /// negation of the rhs signature, and lemma_type from signature_index lookup or
    /// anonymous marker.
    #[test]
    fn number_divided_by_q_reciprocates_signature() {
        let code = r#"spec t
data duration_t: quantity
  -> unit second 1
data freq: quantity
  -> unit per_second second^-1
data d: 2 second
rule f: 1 / d
"#;
        // 1 / second = 1 per_second; signature_index hit yields named 'freq' type.
        let value = build_engine_and_get_value(code, "t", "f");
        match &value.value {
            ValueKind::Quantity(_, _) => {}
            other => panic!("expected Quantity, got {:?}", other),
        }
        assert_eq!(
            value.lemma_type.name(),
            "freq",
            "1/second must promote to 'freq' (signature [(second,-1)] -> per_second)"
        );
    }
}
