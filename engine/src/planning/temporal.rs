use crate::engine::{Context, TemporalBound};
use crate::parsing::ast::{DateTimeValue, FactValue, LemmaDoc};
use crate::parsing::source::Source;
use crate::Error;
use std::collections::BTreeSet;
use std::sync::Arc;

/// A temporal slice: an interval within a document's active range where the
/// entire transitive dependency tree resolves to the same set of versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalSlice {
    /// Inclusive start. None = -∞.
    pub from: Option<DateTimeValue>,
    /// Exclusive end. None = +∞.
    pub to: Option<DateTimeValue>,
}

/// Collect names of implicit (unpinned) doc references with their source locations.
fn implicit_doc_refs(doc: &LemmaDoc) -> Vec<(String, Source)> {
    doc.facts
        .iter()
        .filter_map(|fact| {
            if let FactValue::DocumentReference(doc_ref) = &fact.value {
                if doc_ref.hash_pin.is_none() {
                    return Some((doc_ref.name.clone(), fact.source_location.clone()));
                }
            }
            None
        })
        .collect()
}

/// Collect just the names (for callers that don't need locations).
fn implicit_doc_ref_names(doc: &LemmaDoc) -> Vec<String> {
    implicit_doc_refs(doc).into_iter().map(|(n, _)| n).collect()
}

/// Compute temporal slices for a document within its effective range.
///
/// A slice boundary occurs at every `effective_from` date of a dependency version
/// that falls strictly within the document's effective range. Transitive
/// dependencies are followed recursively (fixed-point) to discover all
/// boundaries.
///
/// Returns sorted, non-overlapping slices that partition the document's
/// effective range. For documents without implicit doc refs or without
/// any version boundaries in range, returns a single slice covering the
/// full effective range.
pub fn compute_temporal_slices(doc: &Arc<LemmaDoc>, context: &Context) -> Vec<TemporalSlice> {
    let (eff_from, eff_to) = context.effective_range(doc);
    let range_start = TemporalBound::from_start(eff_from.as_ref());
    let range_end = TemporalBound::from_end(eff_to.as_ref());

    let direct_implicit_names = implicit_doc_ref_names(doc);
    if direct_implicit_names.is_empty() {
        return vec![TemporalSlice {
            from: eff_from,
            to: eff_to,
        }];
    }

    // Fixed-point: collect all boundary points from transitive implicit deps.
    // We track which doc names we've already visited to avoid cycles.
    let mut visited_names: BTreeSet<String> = BTreeSet::new();
    let mut pending_names: Vec<String> = direct_implicit_names;
    let mut all_boundaries: BTreeSet<DateTimeValue> = BTreeSet::new();

    while let Some(dep_name) = pending_names.pop() {
        if !visited_names.insert(dep_name.clone()) {
            continue;
        }

        let dep_versions: Vec<Arc<LemmaDoc>> =
            context.iter().filter(|d| d.name == dep_name).collect();
        assert!(
            !dep_versions.is_empty(),
            "BUG: compute_temporal_slices found implicit dep '{}' with no versions in context — \
             validate_temporal_coverage should have rejected this",
            dep_name
        );

        let boundaries = context.version_boundaries(&dep_name);
        for boundary in boundaries {
            let bound = TemporalBound::At(boundary.clone());
            if bound > range_start && bound < range_end {
                all_boundaries.insert(boundary);
            }
        }
        for dep_doc in &dep_versions {
            for transitive_name in implicit_doc_ref_names(dep_doc) {
                if !visited_names.contains(&transitive_name) {
                    pending_names.push(transitive_name);
                }
            }
        }
    }

    if all_boundaries.is_empty() {
        return vec![TemporalSlice {
            from: eff_from,
            to: eff_to,
        }];
    }

    // Split the effective range at each boundary point.
    let mut slices = Vec::new();
    let mut cursor = eff_from.clone();

    for boundary in &all_boundaries {
        slices.push(TemporalSlice {
            from: cursor,
            to: Some(boundary.clone()),
        });
        cursor = Some(boundary.clone());
    }

    slices.push(TemporalSlice {
        from: cursor,
        to: eff_to,
    });

    slices
}

/// Validate temporal coverage for all documents in the context.
///
/// For each document, checks that every implicit (unpinned) dependency has
/// versions that fully cover the document's effective range. Returns errors
/// for any dependency that has gaps.
///
/// This replaces the old `validate_later_docs_respect_original` which enforced
/// that all versions of the same name had identical interfaces. The new
/// approach allows interface evolution — coverage is checked here, and
/// interface compatibility is validated per-slice during graph building.
pub fn validate_temporal_coverage(context: &Context) -> Vec<Error> {
    let mut errors = Vec::new();

    for doc_arc in context.iter() {
        let (eff_from, eff_to) = context.effective_range(&doc_arc);
        let dep_refs = implicit_doc_refs(&doc_arc);

        for (dep_name, ref_source) in &dep_refs {
            let gaps = context.dep_coverage_gaps(dep_name, eff_from.as_ref(), eff_to.as_ref());

            for (gap_start, gap_end) in &gaps {
                let (message, suggestion) =
                    format_coverage_gap(&doc_arc.name, dep_name, gap_start, gap_end, &eff_from);
                errors.push(Error::validation(
                    message,
                    Some(ref_source.clone()),
                    Some(suggestion),
                ));
            }
        }
    }

    errors
}

fn format_coverage_gap(
    doc_name: &str,
    dep_name: &str,
    gap_start: &Option<DateTimeValue>,
    gap_end: &Option<DateTimeValue>,
    doc_from: &Option<DateTimeValue>,
) -> (String, String) {
    let message = match (gap_start, gap_end) {
        (None, Some(end)) => format!(
            "'{}' depends on '{}', but no version of '{}' is active before {}",
            doc_name, dep_name, dep_name, end
        ),
        (Some(start), None) => format!(
            "'{}' depends on '{}', but no version of '{}' is active after {}",
            doc_name, dep_name, dep_name, start
        ),
        (Some(start), Some(end)) => format!(
            "'{}' depends on '{}', but no version of '{}' is active between {} and {}",
            doc_name, dep_name, dep_name, start, end
        ),
        (None, None) => format!(
            "'{}' depends on '{}', but no version of '{}' exists",
            doc_name, dep_name, dep_name
        ),
    };

    let suggestion = if gap_start.is_none() && doc_from.is_none() {
        format!(
            "Add an effective_from date to '{}' so it starts when '{}' is available, \
             or add an earlier version of '{}'.",
            doc_name, dep_name, dep_name
        )
    } else if gap_end.is_none() {
        format!(
            "Add a newer version of '{}' that covers the remaining range.",
            dep_name
        )
    } else {
        format!(
            "Add a version of '{}' that covers the gap, \
             or adjust the effective_from date on '{}'.",
            dep_name, doc_name
        )
    };

    (message, suggestion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::{DocRef, FactValue, LemmaDoc, LemmaFact, Reference};
    use crate::parsing::source::Source;
    use crate::Span;

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

    fn dummy_source() -> Source {
        Source {
            attribute: "test".to_string(),
            span: Span {
                start: 0,
                end: 0,
                line: 0,
                col: 0,
            },
            doc_name: "test".to_string(),
            source_text: "".into(),
        }
    }

    fn make_doc(name: &str) -> LemmaDoc {
        LemmaDoc::new(name.to_string())
    }

    fn make_doc_with_range(name: &str, effective_from: Option<DateTimeValue>) -> LemmaDoc {
        let mut doc = make_doc(name);
        doc.effective_from = effective_from;
        doc
    }

    fn add_doc_ref_fact(doc: &mut LemmaDoc, fact_name: &str, dep_name: &str) {
        doc.facts.push(LemmaFact {
            reference: Reference::local(fact_name.to_string()),
            value: FactValue::DocumentReference(DocRef {
                name: dep_name.to_string(),
                is_registry: false,
                hash_pin: None,
                effective: None,
            }),
            source_location: dummy_source(),
        });
    }

    #[test]
    fn no_deps_produces_single_slice() {
        let mut ctx = Context::new();
        let doc = Arc::new(make_doc_with_range("a", Some(date(2025, 1, 1))));
        ctx.insert_doc(Arc::clone(&doc)).unwrap();

        let slices = compute_temporal_slices(&doc, &ctx);
        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].from, Some(date(2025, 1, 1)));
        assert_eq!(slices[0].to, None);
    }

    #[test]
    fn single_dep_no_boundary_in_range() {
        let mut ctx = Context::new();
        let mut main_doc = make_doc_with_range("main", Some(date(2025, 1, 1)));
        add_doc_ref_fact(&mut main_doc, "dep", "config");
        let main_arc = Arc::new(main_doc);
        ctx.insert_doc(Arc::clone(&main_arc)).unwrap();

        let config = Arc::new(make_doc("config"));
        ctx.insert_doc(config).unwrap();

        let slices = compute_temporal_slices(&main_arc, &ctx);
        assert_eq!(slices.len(), 1);
    }

    #[test]
    fn single_dep_one_boundary_produces_two_slices() {
        let mut ctx = Context::new();

        let config_v1 = Arc::new(make_doc("config"));
        ctx.insert_doc(config_v1).unwrap();
        let config_v2 = Arc::new(make_doc_with_range("config", Some(date(2025, 2, 1))));
        ctx.insert_doc(config_v2).unwrap();

        // main: [Jan 1, +inf) depends on config
        let mut main_doc = make_doc_with_range("main", Some(date(2025, 1, 1)));
        add_doc_ref_fact(&mut main_doc, "cfg", "config");
        let main_arc = Arc::new(main_doc);
        ctx.insert_doc(Arc::clone(&main_arc)).unwrap();

        let slices = compute_temporal_slices(&main_arc, &ctx);
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].from, Some(date(2025, 1, 1)));
        assert_eq!(slices[0].to, Some(date(2025, 2, 1)));
        assert_eq!(slices[1].from, Some(date(2025, 2, 1)));
        assert_eq!(slices[1].to, None);
    }

    #[test]
    fn boundary_outside_range_ignored() {
        let mut ctx = Context::new();

        let config_v1 = Arc::new(make_doc("config"));
        ctx.insert_doc(config_v1).unwrap();
        let config_v2 = Arc::new(make_doc_with_range("config", Some(date(2025, 6, 1))));
        ctx.insert_doc(config_v2).unwrap();

        // main v1: [Jan 1, Mar 1) — successor main v2 defines the end
        let main_v1 = make_doc_with_range("main", Some(date(2025, 1, 1)));
        let main_v2 = make_doc_with_range("main", Some(date(2025, 3, 1)));
        let mut main_v1 = main_v1;
        add_doc_ref_fact(&mut main_v1, "cfg", "config");
        let main_arc = Arc::new(main_v1);
        ctx.insert_doc(Arc::clone(&main_arc)).unwrap();
        ctx.insert_doc(Arc::new(main_v2)).unwrap();

        let slices = compute_temporal_slices(&main_arc, &ctx);
        assert_eq!(slices.len(), 1);
    }

    #[test]
    fn transitive_dep_boundary_included() {
        let mut ctx = Context::new();

        let mut config = make_doc("config");
        add_doc_ref_fact(&mut config, "rates_ref", "rates");
        ctx.insert_doc(Arc::new(config)).unwrap();

        let rates_v1 = Arc::new(make_doc("rates"));
        ctx.insert_doc(rates_v1).unwrap();
        let rates_v2 = Arc::new(make_doc_with_range("rates", Some(date(2025, 2, 1))));
        ctx.insert_doc(rates_v2).unwrap();

        // main: [Jan 1, +inf) depends on config
        let mut main_doc = make_doc_with_range("main", Some(date(2025, 1, 1)));
        add_doc_ref_fact(&mut main_doc, "cfg", "config");
        let main_arc = Arc::new(main_doc);
        ctx.insert_doc(Arc::clone(&main_arc)).unwrap();

        let slices = compute_temporal_slices(&main_arc, &ctx);
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].to, Some(date(2025, 2, 1)));
        assert_eq!(slices[1].from, Some(date(2025, 2, 1)));
    }

    #[test]
    fn unbounded_doc_with_versioned_dep() {
        let mut ctx = Context::new();

        let dep_v1 = Arc::new(make_doc("dep"));
        ctx.insert_doc(dep_v1).unwrap();
        let dep_v2 = Arc::new(make_doc_with_range("dep", Some(date(2025, 6, 1))));
        ctx.insert_doc(dep_v2).unwrap();

        let mut main_doc = make_doc("main");
        add_doc_ref_fact(&mut main_doc, "d", "dep");
        let main_arc = Arc::new(main_doc);
        ctx.insert_doc(Arc::clone(&main_arc)).unwrap();

        let slices = compute_temporal_slices(&main_arc, &ctx);
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].from, None);
        assert_eq!(slices[0].to, Some(date(2025, 6, 1)));
        assert_eq!(slices[1].from, Some(date(2025, 6, 1)));
        assert_eq!(slices[1].to, None);
    }

    #[test]
    fn pinned_ref_does_not_create_boundary() {
        let mut ctx = Context::new();

        let dep_v1 = Arc::new(make_doc("dep"));
        ctx.insert_doc(dep_v1).unwrap();
        let dep_v2 = Arc::new(make_doc_with_range("dep", Some(date(2025, 6, 1))));
        ctx.insert_doc(dep_v2).unwrap();

        let mut main_doc = make_doc("main");
        main_doc.facts.push(LemmaFact {
            reference: Reference::local("d".to_string()),
            value: FactValue::DocumentReference(DocRef {
                name: "dep".to_string(),
                is_registry: false,
                hash_pin: Some("abcd1234".to_string()),
                effective: None,
            }),
            source_location: dummy_source(),
        });
        let main_arc = Arc::new(main_doc);
        ctx.insert_doc(Arc::clone(&main_arc)).unwrap();

        let slices = compute_temporal_slices(&main_arc, &ctx);
        assert_eq!(slices.len(), 1);
    }
}
