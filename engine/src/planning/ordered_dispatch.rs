//! Ordered dispatch: the key type, comparison classes and region model behind
//! [`crate::planning::normalize::NormalFormKind::OrderedDispatch`].
//!
//! A `Piecewise` whose arms all compare one scrutinee against constants is decided
//! by a linear reverse scan. The same decision can be made by a binary search once
//! the constants are sorted, because every arm predicate is constant over each
//! region of the domain the constants cut it into.
//!
//! For breakpoints `b_0 < b_1 < ... < b_{n-1}` the domain splits into `2n + 1`
//! regions, alternating open intervals and points:
//!
//! - `regions[2i]` is the open interval `(b_{i-1}, b_i)`, with `b_-1 = -inf` and `b_n = +inf`
//! - `regions[2i+1]` is the point `b_i`
//!
//! Point regions are what distinguishes `>` from `>=` at a shared boundary.

use std::cmp::Ordering;

use chrono::NaiveDateTime;

use crate::computation::datetime::{semantic_datetime_to_chrono, semantic_time_to_chrono_datetime};
use crate::computation::rational::{NumericFailure, RationalInteger};
use crate::parsing::ast::PrimitiveKind;
use crate::planning::semantics::{ComparisonComputation, LemmaType, LiteralValue, ValueKind};

/// One breakpoint in an ordered dispatch table.
///
/// Deliberately implements neither `Ord` nor `PartialOrd`. [`RationalInteger`]'s
/// `Ord` panics when the cross-multiplication needed to order two rationals runs
/// out of memory, while evaluation must turn that failure into a veto. Ordering
/// therefore goes through [`Self::try_compare`], which surfaces the failure.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DispatchKey {
    Text(String),
    /// Stored magnitude of a number, ratio or measure. Measure magnitudes are held
    /// in canonical base-unit space, which is why one key covers all three.
    Rational(RationalInteger),
    Date(NaiveDateTime),
    Time(NaiveDateTime),
}

/// Borrowed probe used only while searching a dispatch table.
///
/// Text and Rational borrow the evaluated scrutinee so evaluation does not clone.
/// Date and Time own a [`NaiveDateTime`] because calendar conversion is a
/// computation, not a borrow, and must happen once per evaluation either way.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DispatchProbe<'a> {
    Text(&'a str),
    Rational(&'a RationalInteger),
    Date(NaiveDateTime),
    Time(NaiveDateTime),
}

impl DispatchKey {
    pub(crate) fn as_probe(&self) -> DispatchProbe<'_> {
        match self {
            DispatchKey::Text(text) => DispatchProbe::Text(text.as_str()),
            DispatchKey::Rational(magnitude) => DispatchProbe::Rational(magnitude),
            DispatchKey::Date(moment) => DispatchProbe::Date(*moment),
            DispatchKey::Time(moment) => DispatchProbe::Time(*moment),
        }
    }

    /// Order this stored key against a probe of the same class.
    ///
    /// Panics on a cross-class comparison: every key in one dispatch cell shares a
    /// class by construction, so a mixed pair means the fold built a broken table.
    pub(crate) fn try_compare(
        &self,
        other: &DispatchProbe<'_>,
    ) -> Result<Ordering, NumericFailure> {
        match (self, other) {
            (DispatchKey::Text(left), DispatchProbe::Text(right)) => Ok(left.as_str().cmp(right)),
            (DispatchKey::Rational(left), DispatchProbe::Rational(right)) => left.try_cmp(right),
            (DispatchKey::Date(left), DispatchProbe::Date(right)) => Ok(left.cmp(right)),
            (DispatchKey::Time(left), DispatchProbe::Time(right)) => Ok(left.cmp(right)),
            (left, right) => panic!(
                "BUG: OrderedDispatch compared keys of different classes: {left:?} vs {right:?}"
            ),
        }
    }
}

/// Failure building an owned plan-time dispatch key from a literal.
#[derive(Debug)]
pub(crate) enum DispatchKeyBuildError {
    /// Calendar conversion failed. Same message surface as
    /// [`crate::computation::datetime::datetime_comparison`].
    CalendarFailure(String),
    /// A value kind ordered dispatch never keys on.
    Unsupported,
}

/// Owned key for plan storage, cloned from a literal's value.
pub(crate) fn dispatch_key_of_literal(
    value: &ValueKind,
) -> Result<DispatchKey, DispatchKeyBuildError> {
    match value {
        ValueKind::Text(text) => Ok(DispatchKey::Text(text.clone())),
        ValueKind::Number(magnitude)
        | ValueKind::Measure(magnitude, _)
        | ValueKind::Ratio(magnitude, _) => Ok(DispatchKey::Rational(magnitude.clone())),
        ValueKind::Date(date) => match semantic_datetime_to_chrono(date) {
            Ok(moment) => Ok(DispatchKey::Date(moment.naive_utc())),
            Err(message) => Err(DispatchKeyBuildError::CalendarFailure(message)),
        },
        ValueKind::Time(time) => match semantic_time_to_chrono_datetime(time) {
            Ok(moment) => Ok(DispatchKey::Time(moment.naive_utc())),
            Err(message) => Err(DispatchKeyBuildError::CalendarFailure(message)),
        },
        ValueKind::Boolean(_) | ValueKind::Range(_, _) => Err(DispatchKeyBuildError::Unsupported),
    }
}

/// Result of turning a value into a dispatch probe.
#[derive(Debug)]
pub(crate) enum DispatchProbeOutcome<'a> {
    Probe(DispatchProbe<'a>),
    /// Calendar conversion failed. Evaluation vetoes with this message, matching
    /// [`crate::computation::datetime::datetime_comparison`].
    CalendarFailure(String),
    /// A value kind ordered dispatch never keys on.
    Unsupported,
}

pub(crate) fn dispatch_probe_of(value: &ValueKind) -> DispatchProbeOutcome<'_> {
    match value {
        ValueKind::Text(text) => DispatchProbeOutcome::Probe(DispatchProbe::Text(text.as_str())),
        ValueKind::Number(magnitude)
        | ValueKind::Measure(magnitude, _)
        | ValueKind::Ratio(magnitude, _) => {
            DispatchProbeOutcome::Probe(DispatchProbe::Rational(magnitude))
        }
        ValueKind::Date(date) => match semantic_datetime_to_chrono(date) {
            Ok(moment) => DispatchProbeOutcome::Probe(DispatchProbe::Date(moment.naive_utc())),
            Err(message) => DispatchProbeOutcome::CalendarFailure(message),
        },
        ValueKind::Time(time) => match semantic_time_to_chrono_datetime(time) {
            Ok(moment) => DispatchProbeOutcome::Probe(DispatchProbe::Time(moment.naive_utc())),
            Err(message) => DispatchProbeOutcome::CalendarFailure(message),
        },
        ValueKind::Boolean(_) | ValueKind::Range(_, _) => DispatchProbeOutcome::Unsupported,
    }
}

/// The comparison class a dispatch cell keys on.
///
/// Each variant names a set of `(scrutinee, key)` pairs that
/// [`crate::computation::comparison::comparison_operation`] resolves by directly
/// ordering the two values. Pairs that compute something first (any range) cannot
/// be reproduced by a table lookup and have no class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DispatchClass {
    /// Text compares with `is` and `is not` only; every other operator is rejected
    /// inside `comparison_operation`.
    Text,
    Rational,
    Date,
    Time,
}

/// The class shared by a scrutinee type and every key, or `None` when the pattern
/// does not apply.
pub(crate) fn classify_dispatch(
    scrutinee_type: &LemmaType,
    keys: &[&LiteralValue],
) -> Option<DispatchClass> {
    if keys.is_empty() {
        return None;
    }
    if scrutinee_type.is_undetermined() || scrutinee_type.is_range() || scrutinee_type.is_boolean()
    {
        return None;
    }
    let mut class: Option<DispatchClass> = None;
    for key in keys {
        let key_class = class_for_pair(scrutinee_type, key)?;
        match class {
            None => class = Some(key_class),
            Some(established) if established == key_class => {}
            Some(_) => return None,
        }
    }
    class
}

/// The class for one `(scrutinee type, key literal)` pair, following the arm order
/// of `comparison_operation`.
fn class_for_pair(scrutinee_type: &LemmaType, key: &LiteralValue) -> Option<DispatchClass> {
    if key.lemma_type.is_range() {
        return None;
    }
    match &key.value {
        ValueKind::Text(_) => scrutinee_type
            .matches_primitive_kind(PrimitiveKind::Text)
            .then_some(DispatchClass::Text),
        ValueKind::Date(_) => scrutinee_type
            .matches_primitive_kind(PrimitiveKind::Date)
            .then_some(DispatchClass::Date),
        ValueKind::Time(_) => scrutinee_type
            .matches_primitive_kind(PrimitiveKind::Time)
            .then_some(DispatchClass::Time),
        ValueKind::Ratio(_, _) => scrutinee_type
            .matches_primitive_kind(PrimitiveKind::Ratio)
            .then_some(DispatchClass::Rational),
        ValueKind::Number(_) => {
            if scrutinee_type.matches_primitive_kind(PrimitiveKind::Number) {
                return Some(DispatchClass::Rational);
            }
            // `(Measure, Number)` reaches a stored-magnitude compare only for the two
            // measure shapes `comparison_operation` guards on.
            (scrutinee_type.is_duration_like_measure() || scrutinee_type.is_calendar_like())
                .then_some(DispatchClass::Rational)
        }
        ValueKind::Measure(_, _) => {
            if scrutinee_type.matches_primitive_kind(PrimitiveKind::Number) {
                return (key.lemma_type.is_duration_like_measure()
                    || key.lemma_type.is_calendar_like())
                .then_some(DispatchClass::Rational);
            }
            if !scrutinee_type.is_measure() {
                return None;
            }
            if scrutinee_type.is_calendar_like() && key.lemma_type.is_calendar_like() {
                return Some(DispatchClass::Rational);
            }
            // The general measure arm panics on incompatible operands, so the fold
            // must establish the compatibility that arm assumes.
            let compatible = scrutinee_type.same_measure_family(&key.lemma_type)
                || scrutinee_type.compatible_with_anonymous_measure(&key.lemma_type)
                || match (
                    scrutinee_type.measure_type_decomposition(),
                    key.lemma_type.measure_type_decomposition(),
                ) {
                    (Some(left), Some(right)) => left == right,
                    _ => false,
                };
            compatible.then_some(DispatchClass::Rational)
        }
        ValueKind::Boolean(_) | ValueKind::Range(_, _) => None,
    }
}

/// Number of regions for `boundary_count` breakpoints.
pub(crate) fn region_count(boundary_count: usize) -> usize {
    2 * boundary_count + 1
}

/// Inclusive region index ranges in which `operator` against the breakpoint at
/// `boundary_index` holds.
///
/// Every predicate covers one contiguous run except `is not`, which is the
/// complement of a single point and therefore two.
pub(crate) fn regions_matching(
    operator: &ComparisonComputation,
    boundary_index: usize,
    boundary_count: usize,
) -> [Option<(usize, usize)>; 2] {
    let point = 2 * boundary_index + 1;
    let last = 2 * boundary_count;
    match operator {
        ComparisonComputation::Is => [Some((point, point)), None],
        ComparisonComputation::IsNot => [Some((0, point - 1)), Some((point + 1, last))],
        ComparisonComputation::GreaterThan => [Some((point + 1, last)), None],
        ComparisonComputation::GreaterThanOrEqual => [Some((point, last)), None],
        ComparisonComputation::LessThan => [Some((0, point - 1)), None],
        ComparisonComputation::LessThanOrEqual => [Some((0, point)), None],
    }
}

/// Index of the region holding `value`.
///
/// Hand-rolled rather than `partition_point` so a failed rational comparison
/// surfaces as an error the caller can turn into a veto instead of a panic.
pub(crate) fn region_for_value(
    boundaries: &[DispatchKey],
    value: &DispatchProbe<'_>,
) -> Result<usize, NumericFailure> {
    let mut low = 0usize;
    let mut high = boundaries.len();
    while low < high {
        let middle = low + (high - low) / 2;
        match boundaries[middle].try_compare(value)? {
            Ordering::Less => low = middle + 1,
            Ordering::Equal | Ordering::Greater => high = middle,
        }
    }
    if low < boundaries.len() && boundaries[low].try_compare(value)? == Ordering::Equal {
        Ok(2 * low + 1)
    } else {
        Ok(2 * low)
    }
}

/// Sorted, deduplicated breakpoints.
///
/// Keys are always reduced rationals or plain values, so structural equality is
/// faithful to ordering equality and `dedup` is exact.
pub(crate) fn sorted_unique_boundaries(
    mut keys: Vec<DispatchKey>,
) -> Result<Vec<DispatchKey>, NumericFailure> {
    let mut failure: Option<NumericFailure> = None;
    keys.sort_by(|left, right| match left.try_compare(&right.as_probe()) {
        Ok(ordering) => ordering,
        Err(numeric) => {
            failure.get_or_insert(numeric);
            Ordering::Equal
        }
    });
    match failure {
        Some(numeric) => Err(numeric),
        None => {
            keys.dedup();
            Ok(keys)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::rational_new;

    fn rational_key(numerator: i64) -> DispatchKey {
        DispatchKey::Rational(rational_new(numerator, 1))
    }

    fn text_key(text: &str) -> DispatchKey {
        DispatchKey::Text(text.to_string())
    }

    /// Truth of `scrutinee operator boundary`, computed the slow way.
    fn predicate_holds(
        scrutinee: &DispatchKey,
        operator: &ComparisonComputation,
        boundary: &DispatchKey,
    ) -> bool {
        let ordering = scrutinee
            .try_compare(&boundary.as_probe())
            .expect("test keys compare without failure");
        match operator {
            ComparisonComputation::Is => ordering == Ordering::Equal,
            ComparisonComputation::IsNot => ordering != Ordering::Equal,
            ComparisonComputation::GreaterThan => ordering == Ordering::Greater,
            ComparisonComputation::GreaterThanOrEqual => ordering != Ordering::Less,
            ComparisonComputation::LessThan => ordering == Ordering::Less,
            ComparisonComputation::LessThanOrEqual => ordering != Ordering::Greater,
        }
    }

    fn all_operators() -> Vec<ComparisonComputation> {
        vec![
            ComparisonComputation::Is,
            ComparisonComputation::IsNot,
            ComparisonComputation::GreaterThan,
            ComparisonComputation::GreaterThanOrEqual,
            ComparisonComputation::LessThan,
            ComparisonComputation::LessThanOrEqual,
        ]
    }

    #[test]
    fn region_lookup_places_values_below_at_and_above_each_boundary() {
        let boundaries = vec![rational_key(0), rational_key(5), rational_key(10)];
        let cases = [
            (-1, 0usize),
            (0, 1),
            (3, 2),
            (5, 3),
            (7, 4),
            (10, 5),
            (11, 6),
        ];
        for (value, expected) in cases {
            assert_eq!(
                region_for_value(&boundaries, &rational_key(value).as_probe()).expect("comparable"),
                expected,
                "value {value} must land in region {expected}"
            );
        }
    }

    #[test]
    fn empty_boundary_list_has_one_region() {
        assert_eq!(region_count(0), 1);
        assert_eq!(
            region_for_value(&[], &rational_key(42).as_probe()).expect("comparable"),
            0
        );
    }

    /// The region ranges an operator claims must match the predicate evaluated
    /// directly at a representative value of every region.
    #[test]
    fn claimed_regions_agree_with_the_predicate_at_every_region() {
        let boundaries = vec![rational_key(0), rational_key(5), rational_key(10)];
        // Two units apart so every open interval has an integer representative.
        let representatives = [-5, 0, 2, 5, 7, 10, 15];
        assert_eq!(representatives.len(), region_count(boundaries.len()));

        for operator in all_operators() {
            for (boundary_index, boundary) in boundaries.iter().enumerate() {
                let mut claimed = vec![false; region_count(boundaries.len())];
                for span in regions_matching(&operator, boundary_index, boundaries.len())
                    .into_iter()
                    .flatten()
                {
                    let (start, end) = span;
                    for region in claimed.iter_mut().take(end + 1).skip(start) {
                        *region = true;
                    }
                }
                for (region, value) in representatives.iter().enumerate() {
                    let scrutinee = rational_key(*value);
                    assert_eq!(
                        region_for_value(&boundaries, &scrutinee.as_probe()).expect("comparable"),
                        region,
                        "representative {value} must sit in region {region}"
                    );
                    assert_eq!(
                        claimed[region],
                        predicate_holds(&scrutinee, &operator, boundary),
                        "operator {operator} against boundary index {boundary_index} \
                         disagrees at region {region} (value {value})"
                    );
                }
            }
        }
    }

    #[test]
    fn greater_than_and_greater_or_equal_differ_only_at_the_point_region() {
        let strictly = regions_matching(&ComparisonComputation::GreaterThan, 1, 3);
        let inclusive = regions_matching(&ComparisonComputation::GreaterThanOrEqual, 1, 3);
        assert_eq!(strictly, [Some((4, 6)), None]);
        assert_eq!(inclusive, [Some((3, 6)), None]);
    }

    #[test]
    fn less_than_and_less_or_equal_differ_only_at_the_point_region() {
        let strictly = regions_matching(&ComparisonComputation::LessThan, 1, 3);
        let inclusive = regions_matching(&ComparisonComputation::LessThanOrEqual, 1, 3);
        assert_eq!(strictly, [Some((0, 2)), None]);
        assert_eq!(inclusive, [Some((0, 3)), None]);
    }

    #[test]
    fn is_not_claims_everything_except_its_point() {
        assert_eq!(
            regions_matching(&ComparisonComputation::IsNot, 1, 3),
            [Some((0, 2)), Some((4, 6))]
        );
    }

    #[test]
    fn boundaries_are_sorted_and_deduplicated() {
        let sorted = sorted_unique_boundaries(vec![
            rational_key(5),
            rational_key(-2),
            rational_key(5),
            rational_key(0),
        ])
        .expect("comparable");
        assert_eq!(
            sorted,
            vec![rational_key(-2), rational_key(0), rational_key(5)]
        );
    }

    #[test]
    fn text_boundaries_sort_lexicographically() {
        let sorted = sorted_unique_boundaries(vec![text_key("NL"), text_key("AD"), text_key("ZW")])
            .expect("comparable");
        assert_eq!(sorted, vec![text_key("AD"), text_key("NL"), text_key("ZW")]);
        assert_eq!(
            region_for_value(&sorted, &text_key("NL").as_probe()).expect("comparable"),
            3
        );
        assert_eq!(
            region_for_value(&sorted, &text_key("BE").as_probe()).expect("comparable"),
            2
        );
    }

    #[test]
    #[should_panic(expected = "BUG: OrderedDispatch compared keys of different classes")]
    fn comparing_keys_of_different_classes_is_a_bug() {
        let _ = text_key("AD").try_compare(&rational_key(1).as_probe());
    }

    #[test]
    fn probe_of_text_and_rational_borrows_without_clone() {
        let text = ValueKind::Text("NL".to_string());
        let number = ValueKind::Number(rational_new(3, 1));
        match dispatch_probe_of(&text) {
            DispatchProbeOutcome::Probe(DispatchProbe::Text(borrowed)) => {
                assert_eq!(borrowed, "NL");
            }
            other => panic!("expected text probe, got {other:?}"),
        }
        match dispatch_probe_of(&number) {
            DispatchProbeOutcome::Probe(DispatchProbe::Rational(borrowed)) => {
                assert_eq!(*borrowed, rational_new(3, 1));
            }
            other => panic!("expected rational probe, got {other:?}"),
        }
    }

    /// Every pair `classify_dispatch` accepts must be a live `comparison_operation` arm.
    #[test]
    fn classify_dispatch_pairs_are_accepted_by_comparison_operation() {
        use crate::computation::comparison::comparison_operation;
        use crate::computation::operation_result::OperationResult;
        use crate::computation::UnitResolutionContext;
        use crate::planning::semantics::LiteralValue;

        let cases: Vec<(LiteralValue, LiteralValue, ComparisonComputation)> = vec![
            (
                LiteralValue::number(rational_new(1, 1)),
                LiteralValue::number(rational_new(2, 1)),
                ComparisonComputation::LessThan,
            ),
            (
                LiteralValue::number(rational_new(3, 1)),
                LiteralValue::number(rational_new(3, 1)),
                ComparisonComputation::Is,
            ),
            (
                LiteralValue::text("NL".into()),
                LiteralValue::text("NL".into()),
                ComparisonComputation::Is,
            ),
            (
                LiteralValue::text("NL".into()),
                LiteralValue::text("BE".into()),
                ComparisonComputation::IsNot,
            ),
            (
                LiteralValue::ratio(rational_new(1, 2), Some("percent".into())),
                LiteralValue::ratio(rational_new(3, 4), Some("percent".into())),
                ComparisonComputation::LessThan,
            ),
        ];

        for (scrutinee, key, operator) in &cases {
            let class =
                classify_dispatch(scrutinee.lemma_type.as_ref(), &[key]).unwrap_or_else(|| {
                    panic!(
                        "classify_dispatch must accept {:?} vs {:?}",
                        scrutinee.value, key.value
                    )
                });
            if class == DispatchClass::Text {
                assert!(
                    matches!(
                        operator,
                        ComparisonComputation::Is | ComparisonComputation::IsNot
                    ),
                    "text class only uses is / is not"
                );
            }
            match comparison_operation(
                scrutinee,
                operator,
                key,
                UnitResolutionContext::NamedMeasureOnly,
            ) {
                OperationResult::Value(result) => match &result.value {
                    ValueKind::Boolean(_) => {}
                    other => panic!("expected boolean comparison result, got {other:?}"),
                },
                OperationResult::Veto(veto) => {
                    panic!("comparison_operation vetoed a classify-accepted pair: {veto:?}");
                }
            }
        }

        let boolean = LiteralValue::from_bool(true);
        assert!(
            classify_dispatch(boolean.lemma_type.as_ref(), &[&boolean]).is_none(),
            "boolean scrutinees are not a dispatch class"
        );
    }
}
