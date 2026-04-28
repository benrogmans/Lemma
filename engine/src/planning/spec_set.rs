//! Source-level grouping: all specs sharing a name, keyed by effective_from.

use crate::engine::Context;
use crate::parsing::ast::{DateTimeValue, EffectiveDate, LemmaSpec};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

// ─── Temporal bound for Option<DateTimeValue> comparisons ────────────

/// Explicit representation of a temporal bound, eliminating the ambiguity
/// of `Option<DateTimeValue>` where `None` means `-∞` for start bounds
/// and `+∞` for end bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TemporalBound {
    NegInf,
    At(DateTimeValue),
    PosInf,
}

impl PartialOrd for TemporalBound {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TemporalBound {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (TemporalBound::NegInf, TemporalBound::NegInf) => Ordering::Equal,
            (TemporalBound::NegInf, _) => Ordering::Less,
            (_, TemporalBound::NegInf) => Ordering::Greater,
            (TemporalBound::PosInf, TemporalBound::PosInf) => Ordering::Equal,
            (TemporalBound::PosInf, _) => Ordering::Greater,
            (_, TemporalBound::PosInf) => Ordering::Less,
            (TemporalBound::At(a), TemporalBound::At(b)) => a.cmp(b),
        }
    }
}

impl TemporalBound {
    /// Convert an `Option<&DateTimeValue>` used as a start bound (None = -∞).
    pub(crate) fn from_start(opt: Option<&DateTimeValue>) -> Self {
        match opt {
            None => TemporalBound::NegInf,
            Some(d) => TemporalBound::At(d.clone()),
        }
    }

    /// Convert an `Option<&DateTimeValue>` used as an end bound (None = +∞).
    pub(crate) fn from_end(opt: Option<&DateTimeValue>) -> Self {
        match opt {
            None => TemporalBound::PosInf,
            Some(d) => TemporalBound::At(d.clone()),
        }
    }

    /// Convert back to `Option<DateTimeValue>` for a start bound (NegInf → None).
    pub(crate) fn to_start(&self) -> Option<DateTimeValue> {
        match self {
            TemporalBound::NegInf => None,
            TemporalBound::At(d) => Some(d.clone()),
            TemporalBound::PosInf => {
                unreachable!("BUG: PosInf cannot represent a start bound")
            }
        }
    }

    /// Convert back to `Option<DateTimeValue>` for an end bound (PosInf → None).
    pub(crate) fn to_end(&self) -> Option<DateTimeValue> {
        match self {
            TemporalBound::NegInf => {
                unreachable!("BUG: NegInf cannot represent an end bound")
            }
            TemporalBound::At(d) => Some(d.clone()),
            TemporalBound::PosInf => None,
        }
    }
}

/// All specs sharing a name, keyed by effective_from.
#[derive(Debug, Clone)]
pub struct LemmaSpecSet {
    pub name: String,
    specs: BTreeMap<EffectiveDate, Arc<LemmaSpec>>,
}

impl LemmaSpecSet {
    #[must_use]
    pub fn new(name: String) -> Self {
        Self {
            name,
            specs: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    #[must_use]
    pub fn first(&self) -> Option<&Arc<LemmaSpec>> {
        self.specs.values().next()
    }

    /// Exact identity by `effective_from` key.
    #[must_use]
    pub fn get_exact(&self, effective_from: Option<&DateTimeValue>) -> Option<&Arc<LemmaSpec>> {
        let key = EffectiveDate::from_option(effective_from.cloned());
        self.specs.get(&key)
    }

    /// Insert a spec. Returns `false` if the same `effective_from` already exists.
    pub fn insert(&mut self, spec: Arc<LemmaSpec>) -> bool {
        debug_assert_eq!(spec.name, self.name);
        let key = spec.effective_from.clone();
        if self.specs.contains_key(&key) {
            return false;
        }
        self.specs.insert(key, spec);
        true
    }

    /// Remove by `effective_from` key. Returns whether a row was removed.
    pub fn remove(&mut self, effective_from: Option<&DateTimeValue>) -> bool {
        let key = EffectiveDate::from_option(effective_from.cloned());
        self.specs.remove(&key).is_some()
    }

    pub fn iter_specs(&self) -> impl Iterator<Item = Arc<LemmaSpec>> + '_ {
        self.specs.values().cloned()
    }

    /// Every spec paired with its half-open `[effective_from, effective_to)` range.
    ///
    /// - `effective_from = None` on the first row means no earlier version exists.
    /// - `effective_to = None` on the last row means no successor (this is the
    ///   latest loaded version; its validity is unbounded forward).
    /// - Otherwise `effective_to` equals the next row's `effective_from`
    ///   (exclusive end of this row's validity).
    ///
    /// Iteration order matches [`Self::iter_specs`] (ascending by `effective_from`).
    pub fn iter_with_ranges(
        &self,
    ) -> impl Iterator<Item = (Arc<LemmaSpec>, Option<DateTimeValue>, Option<DateTimeValue>)> + '_
    {
        self.iter_specs().map(move |spec| {
            let (effective_from, effective_to) = self.effective_range(&spec);
            (spec, effective_from, effective_to)
        })
    }

    /// Borrowed iteration in key order (for planning loops without allocating a `Vec`).
    pub fn specs_iter(&self) -> impl Iterator<Item = &Arc<LemmaSpec>> + '_ {
        self.specs.values()
    }

    /// Spec active at `effective`. Each spec covers `[effective_from, next.effective_from)`.
    /// The last spec covers `[effective_from, +∞)`.
    #[must_use]
    pub fn spec_at(&self, effective: &EffectiveDate) -> Option<Arc<LemmaSpec>> {
        self.specs
            .range(..=effective.clone())
            .next_back()
            .map(|(_, spec)| Arc::clone(spec))
    }

    /// Returns the effective range `[from, to)` for a spec in this set.
    ///
    /// - `from`: `spec.effective_from()` (None = -∞)
    /// - `to`: next temporal version's `effective_from`, or None (+∞) if no successor.
    pub fn effective_range(
        &self,
        spec: &Arc<LemmaSpec>,
    ) -> (Option<DateTimeValue>, Option<DateTimeValue>) {
        let from = spec.effective_from().cloned();
        let key = spec.effective_from.clone();
        let exact = self.specs.get_key_value(&key).unwrap_or_else(|| {
            unreachable!(
                "BUG: effective_range called with spec '{}' not in spec set",
                spec.name
            )
        });
        let to = self
            .specs
            .range((
                std::ops::Bound::Excluded(exact.0),
                std::ops::Bound::Unbounded,
            ))
            .next()
            .and_then(|(_, next)| next.effective_from().cloned());
        (from, to)
    }

    /// All `effective_from` dates, sorted ascending. Specs without `effective_from` excluded (-∞).
    #[must_use]
    pub fn temporal_boundaries(&self) -> Vec<DateTimeValue> {
        self.specs
            .values()
            .filter_map(|s| s.effective_from().cloned())
            .collect()
    }

    /// Global effective dates filtered to the `[eff_from, eff_to)` validity range of `spec`.
    #[must_use]
    pub fn effective_dates(&self, spec: &Arc<LemmaSpec>, context: &Context) -> Vec<EffectiveDate> {
        let (from, to) = self.effective_range(spec);
        let from_key = EffectiveDate::from_option(from);
        let all_dates: BTreeSet<EffectiveDate> =
            context.iter().map(|s| s.effective_from.clone()).collect();
        match to {
            Some(dt) => all_dates
                .range(from_key..EffectiveDate::DateTimeValue(dt))
                .cloned()
                .collect(),
            None => all_dates.range(from_key..).cloned().collect(),
        }
    }

    /// Gaps where this spec set's specs do not cover `[required_from, required_to)`.
    ///
    /// Start: `None` = −∞, end: `None` = +∞. Empty result means full coverage.
    /// When the set is empty, the entire required range is one gap.
    #[must_use]
    pub fn coverage_gaps(
        &self,
        required_from: Option<&DateTimeValue>,
        required_to: Option<&DateTimeValue>,
    ) -> Vec<(Option<DateTimeValue>, Option<DateTimeValue>)> {
        let all_specs: Vec<&Arc<LemmaSpec>> = self.specs.values().collect();
        if all_specs.is_empty() {
            return vec![(required_from.cloned(), required_to.cloned())];
        }

        let req_start = TemporalBound::from_start(required_from);
        let req_end = TemporalBound::from_end(required_to);

        let intervals: Vec<(TemporalBound, TemporalBound)> = all_specs
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let start = TemporalBound::from_start(v.effective_from());
                let end = match all_specs.get(i + 1).and_then(|next| next.effective_from()) {
                    Some(next_from) => TemporalBound::At(next_from.clone()),
                    None => TemporalBound::PosInf,
                };
                (start, end)
            })
            .collect();

        let mut gaps = Vec::new();
        let mut cursor = req_start.clone();

        for (v_start, v_end) in &intervals {
            if cursor >= req_end {
                break;
            }

            if *v_end <= cursor {
                continue;
            }

            if *v_start > cursor {
                let gap_end = std::cmp::min(v_start.clone(), req_end.clone());
                if cursor < gap_end {
                    gaps.push((cursor.to_start(), gap_end.to_end()));
                }
            }

            if *v_end > cursor {
                cursor = v_end.clone();
            }
        }

        if cursor < req_end {
            gaps.push((cursor.to_start(), req_end.to_end()));
        }

        gaps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::LemmaSpec;

    fn date(year: i32, month: u32, day: u32) -> DateTimeValue {
        DateTimeValue {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        }
    }

    fn make_spec(name: &str) -> LemmaSpec {
        LemmaSpec::new(name.to_string())
    }

    fn make_spec_with_range(name: &str, effective_from: Option<DateTimeValue>) -> LemmaSpec {
        let mut spec = LemmaSpec::new(name.to_string());
        spec.effective_from = EffectiveDate::from_option(effective_from);
        spec
    }

    #[test]
    fn effective_range_unbounded_single_spec() {
        let mut ss = LemmaSpecSet::new("a".to_string());
        let spec = Arc::new(make_spec("a"));
        assert!(ss.insert(Arc::clone(&spec)));

        let (from, to) = ss.effective_range(&spec);
        assert_eq!(from, None);
        assert_eq!(to, None);
    }

    #[test]
    fn effective_range_soft_end_from_next_spec() {
        let mut ss = LemmaSpecSet::new("a".to_string());
        let v1 = Arc::new(make_spec_with_range("a", Some(date(2025, 1, 1))));
        let v2 = Arc::new(make_spec_with_range("a", Some(date(2025, 6, 1))));
        assert!(ss.insert(Arc::clone(&v1)));
        assert!(ss.insert(Arc::clone(&v2)));

        let (from, to) = ss.effective_range(&v1);
        assert_eq!(from, Some(date(2025, 1, 1)));
        assert_eq!(to, Some(date(2025, 6, 1)));

        let (from, to) = ss.effective_range(&v2);
        assert_eq!(from, Some(date(2025, 6, 1)));
        assert_eq!(to, None);
    }

    /// `iter_with_ranges` yields each spec paired with its half-open
    /// `[effective_from, effective_to)` range. Earlier rows end where the
    /// next row begins; the latest row's `effective_to` is `None`.
    #[test]
    fn iter_with_ranges_yields_specs_paired_with_half_open_range() {
        let mut ss = LemmaSpecSet::new("a".to_string());
        let earlier = Arc::new(make_spec_with_range("a", Some(date(2025, 1, 1))));
        let latest = Arc::new(make_spec_with_range("a", Some(date(2025, 6, 1))));
        assert!(ss.insert(Arc::clone(&earlier)));
        assert!(ss.insert(Arc::clone(&latest)));

        let entries: Vec<_> = ss.iter_with_ranges().collect();
        assert_eq!(entries.len(), 2);

        let (spec_0, from_0, to_0) = &entries[0];
        assert!(Arc::ptr_eq(spec_0, &earlier));
        assert_eq!(from_0, &Some(date(2025, 1, 1)));
        assert_eq!(
            to_0,
            &Some(date(2025, 6, 1)),
            "earlier row ends at the next row's effective_from"
        );

        let (spec_1, from_1, to_1) = &entries[1];
        assert!(Arc::ptr_eq(spec_1, &latest));
        assert_eq!(from_1, &Some(date(2025, 6, 1)));
        assert_eq!(
            to_1, &None,
            "latest row has no successor; effective_to is None"
        );
    }

    #[test]
    fn effective_range_unbounded_start_with_successor() {
        let mut ss = LemmaSpecSet::new("a".to_string());
        let v1 = Arc::new(make_spec("a"));
        let v2 = Arc::new(make_spec_with_range("a", Some(date(2025, 3, 1))));
        assert!(ss.insert(Arc::clone(&v1)));
        assert!(ss.insert(Arc::clone(&v2)));

        let (from, to) = ss.effective_range(&v1);
        assert_eq!(from, None);
        assert_eq!(to, Some(date(2025, 3, 1)));
    }

    #[test]
    fn temporal_boundaries_single_spec() {
        let mut ss = LemmaSpecSet::new("a".to_string());
        assert!(ss.insert(Arc::new(make_spec("a"))));
        assert!(ss.temporal_boundaries().is_empty());
    }

    #[test]
    fn temporal_boundaries_multiple_specs() {
        let mut ss = LemmaSpecSet::new("a".to_string());
        assert!(ss.insert(Arc::new(make_spec("a"))));
        assert!(ss.insert(Arc::new(make_spec_with_range("a", Some(date(2025, 3, 1))))));
        assert!(ss.insert(Arc::new(make_spec_with_range("a", Some(date(2025, 6, 1))))));

        assert_eq!(
            ss.temporal_boundaries(),
            vec![date(2025, 3, 1), date(2025, 6, 1)]
        );
    }

    #[test]
    fn coverage_empty_set_is_full_gap() {
        let ss = LemmaSpecSet::new("missing".to_string());
        let gaps = ss.coverage_gaps(Some(&date(2025, 1, 1)), Some(&date(2025, 6, 1)));
        assert_eq!(gaps, vec![(Some(date(2025, 1, 1)), Some(date(2025, 6, 1)))]);
    }

    #[test]
    fn coverage_single_unbounded_spec_covers_everything() {
        let mut ss = LemmaSpecSet::new("dep".to_string());
        assert!(ss.insert(Arc::new(make_spec("dep"))));

        assert!(ss.coverage_gaps(None, None).is_empty());
        assert!(ss
            .coverage_gaps(Some(&date(2025, 1, 1)), Some(&date(2025, 12, 1)))
            .is_empty());
    }

    #[test]
    fn coverage_single_spec_with_from_leaves_leading_gap() {
        let mut ss = LemmaSpecSet::new("dep".to_string());
        assert!(ss.insert(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 3, 1))
        ))));

        assert_eq!(
            ss.coverage_gaps(None, None),
            vec![(None, Some(date(2025, 3, 1)))]
        );
    }

    #[test]
    fn coverage_continuous_specs_no_gaps() {
        let mut ss = LemmaSpecSet::new("dep".to_string());
        assert!(ss.insert(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 1, 1))
        ))));
        assert!(ss.insert(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 6, 1))
        ))));

        assert!(ss
            .coverage_gaps(Some(&date(2025, 1, 1)), Some(&date(2025, 12, 1)))
            .is_empty());
    }

    #[test]
    fn coverage_dep_starts_after_required_start() {
        let mut ss = LemmaSpecSet::new("dep".to_string());
        assert!(ss.insert(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 6, 1))
        ))));

        assert_eq!(
            ss.coverage_gaps(Some(&date(2025, 1, 1)), Some(&date(2025, 12, 1))),
            vec![(Some(date(2025, 1, 1)), Some(date(2025, 6, 1)))]
        );
    }

    #[test]
    fn coverage_unbounded_required_range() {
        let mut ss = LemmaSpecSet::new("dep".to_string());
        assert!(ss.insert(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 6, 1))
        ))));

        assert_eq!(
            ss.coverage_gaps(None, None),
            vec![(None, Some(date(2025, 6, 1)))]
        );
    }
}
