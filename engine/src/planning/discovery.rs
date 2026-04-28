use crate::engine::Context;
use crate::parsing::ast::{DataValue, DateTimeValue, EffectiveDate, LemmaSpec, SpecRef};
use crate::parsing::source::Source;
use crate::Error;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Shared SpecRef resolution (used by graph builder and discovery)
// ---------------------------------------------------------------------------

/// Resolve a `SpecRef` against `Context` at the given planning `effective`.
/// Returns the resolved `Arc<LemmaSpec>` or a contextual validation error.
pub(crate) fn resolve_spec_ref(
    context: &Context,
    spec_ref: &SpecRef,
    effective: &EffectiveDate,
    consumer_name: &str,
    ref_source: Option<Source>,
    spec_context: Option<Arc<LemmaSpec>>,
) -> Result<Arc<LemmaSpec>, Error> {
    let instant = spec_ref.at(effective);
    context
        .spec_sets()
        .get(spec_ref.name.as_str())
        .and_then(|ss| ss.spec_at(&instant))
        .ok_or_else(|| {
            let (message, suggestion) = format_missing_spec_ref(
                consumer_name,
                spec_ref.name.as_str(),
                &spec_ref.effective,
                &instant,
                context,
            );
            Error::validation_with_context(
                message,
                ref_source,
                Some(suggestion),
                spec_context,
                None,
            )
        })
}

fn format_missing_spec_ref(
    consumer_name: &str,
    dep_name: &str,
    qualified_at: &Option<DateTimeValue>,
    dep_effective: &EffectiveDate,
    context: &Context,
) -> (String, String) {
    if let Some(ref dt) = qualified_at {
        let message = format!(
            "'{}' references '{}' at {}, but no '{}' is active at that instant",
            consumer_name, dep_name, dt, dep_name
        );
        let suggestion = format!(
            "Add '{}' with effective_from on or before {}, or change the reference instant.",
            dep_name, dt
        );
        return if dep_name.starts_with('@') {
            (
                message,
                format!(
                    "{} Or run `lemma get {}` to fetch it.",
                    suggestion, dep_name
                ),
            )
        } else {
            (message, suggestion)
        };
    }

    let dep_ss = context.spec_sets().get(dep_name);
    let dep_exists = dep_ss.is_some_and(|ss| !ss.is_empty());

    if !dep_exists {
        let message = format!(
            "'{}' depends on '{}', but '{}' does not exist",
            consumer_name, dep_name, dep_name
        );
        let suggestion = if dep_name.starts_with('@') {
            format!(
                "Run `lemma get` or `lemma get {}` to fetch this dependency.",
                dep_name
            )
        } else {
            format!("Create a spec named '{}'.", dep_name)
        };
        return (message, suggestion);
    }

    let message = format!(
        "'{}' depends on '{}', but no '{}' is active at {}",
        consumer_name, dep_name, dep_name, dep_effective
    );
    let suggestion = format!(
        "Add '{}' with effective_from covering {}, or adjust effective_from on '{}'.",
        dep_name, dep_effective, consumer_name
    );
    (message, suggestion)
}

// ---------------------------------------------------------------------------
// Dependency edge extraction
// ---------------------------------------------------------------------------

/// `(dep_name, optional explicit effective on reference, source location)`.
pub(crate) fn dependency_edges(
    spec: &Arc<LemmaSpec>,
) -> Vec<(String, Option<DateTimeValue>, Source)> {
    let mut out = Vec::new();

    for data in &spec.data {
        match &data.value {
            DataValue::SpecReference(spec_ref) => {
                out.push((
                    spec_ref.name.clone(),
                    spec_ref.effective.clone(),
                    data.source_location.clone(),
                ));
            }
            DataValue::TypeDeclaration {
                from: Some(from_ref),
                ..
            } => {
                out.push((
                    from_ref.name.clone(),
                    from_ref.effective.clone(),
                    data.source_location.clone(),
                ));
            }
            _ => {}
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Unqualified dep interface validation
// ---------------------------------------------------------------------------

/// For each spec with unqualified deps, verify that the dep's interface
/// (schema) is type-compatible across all dep specs active within the
/// consumer's effective range. Qualified deps are pinned and skip this check.
pub fn validate_dependency_interfaces(
    context: &Context,
    results: &BTreeMap<String, super::SpecSetPlanningResult>,
) -> Vec<(String, Error)> {
    let mut errors: Vec<(String, Error)> = Vec::new();

    for set_result in results.values() {
        for spec_result in &set_result.specs {
            let spec = &spec_result.spec;
            let consumer_ss = context
                .spec_sets()
                .get(&spec.name)
                .expect("spec must be in context");
            let (eff_from, eff_to) = consumer_ss.effective_range(spec);

            for (dep_name, qualified_at, ref_source) in dependency_edges(spec) {
                if qualified_at.is_some() {
                    continue;
                }

                if context.spec_sets().get(&dep_name).is_none() {
                    errors.push((
                        set_result.name.clone(),
                        Error::validation_with_context(
                            format!(
                                "'{}' depends on '{}', but '{}' does not exist",
                                spec.name, dep_name, dep_name
                            ),
                            Some(ref_source.clone()),
                            None::<String>,
                            Some(Arc::clone(spec)),
                            None,
                        ),
                    ));
                    continue;
                }
                let dep_set_result = results.get(&dep_name).expect("BUG: dependency is in context but has no planning result — plan() must insert every context spec into results");

                if dep_set_result.schema_over(&eff_from, &eff_to).is_none() {
                    errors.push((
                        set_result.name.clone(),
                        Error::validation_with_context(
                            format!(
                                "'{}' depends on '{}' without pinning an effective date, but '{}' changed its interface between temporal slices",
                                spec.name, dep_name, dep_name
                            ),
                            Some(ref_source.clone()),
                            Some(format!(
                                "Pin '{}' to a specific effective date, or make '{}' interface-compatible across specs.",
                                dep_name, dep_name
                            )),
                            Some(Arc::clone(spec)),
                            None,
                        ),
                    ));
                }
            }
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Spec DAG: DFS discovery + Kahn's topological sort
// ---------------------------------------------------------------------------

/// Errors from DAG construction, distinguishing cycles (global) from other errors (per-spec).
#[derive(Debug)]
pub(crate) enum DagError {
    /// Dependency cycle detected -- global structural error.
    Cycle(Vec<Error>),
    /// Missing deps, resolution failures, etc. -- per-spec errors.
    Other(Vec<Error>),
}

/// Single-root DFS dependency discovery. Returns topo-sorted DAG containing
/// `root` and its transitive deps, or a typed error on cycles / missing deps.
pub(crate) fn build_dag_for_spec(
    context: &Context,
    root: &Arc<LemmaSpec>,
    effective: &EffectiveDate,
) -> Result<Vec<Arc<LemmaSpec>>, DagError> {
    let mut visited: BTreeSet<Arc<LemmaSpec>> = BTreeSet::new();
    let mut edges: Vec<(Arc<LemmaSpec>, Arc<LemmaSpec>)> = Vec::new();
    let mut nodes: BTreeMap<Arc<LemmaSpec>, Arc<LemmaSpec>> = BTreeMap::new();
    let mut errors: Vec<Error> = Vec::new();

    dfs_discover(
        context,
        Arc::clone(root),
        effective,
        &mut visited,
        &mut edges,
        &mut nodes,
        &mut errors,
    );

    if errors.is_empty() {
        kahns_topo_sort(&nodes, &edges).map_err(|err| DagError::Cycle(vec![err]))
    } else {
        Err(DagError::Other(errors))
    }
}

fn dfs_discover(
    context: &Context,
    spec: Arc<LemmaSpec>,
    effective: &EffectiveDate,
    visited: &mut BTreeSet<Arc<LemmaSpec>>,
    edges: &mut Vec<(Arc<LemmaSpec>, Arc<LemmaSpec>)>,
    nodes: &mut BTreeMap<Arc<LemmaSpec>, Arc<LemmaSpec>>,
    errors: &mut Vec<Error>,
) {
    if !visited.insert(Arc::clone(&spec)) {
        return;
    }
    nodes.insert(Arc::clone(&spec), Arc::clone(&spec));

    for (dep_name, qualified_at, ref_source) in dependency_edges(&spec) {
        let dep_effective = qualified_at
            .clone()
            .map_or_else(|| effective.clone(), EffectiveDate::DateTimeValue);

        match context
            .spec_sets()
            .get(&dep_name)
            .and_then(|ss| ss.spec_at(&dep_effective))
        {
            Some(dependency) => {
                edges.push((Arc::clone(&dependency), Arc::clone(&spec)));
                dfs_discover(
                    context,
                    dependency,
                    &dep_effective,
                    visited,
                    edges,
                    nodes,
                    errors,
                );
            }
            None => {
                let (message, suggestion) = format_missing_spec_ref(
                    &spec.name,
                    &dep_name,
                    &qualified_at,
                    &dep_effective,
                    context,
                );
                errors.push(Error::validation_with_context(
                    message,
                    Some(ref_source),
                    Some(suggestion),
                    Some(Arc::clone(&spec)),
                    None,
                ));
            }
        }
    }
}

fn kahns_topo_sort(
    nodes: &BTreeMap<Arc<LemmaSpec>, Arc<LemmaSpec>>,
    edges: &[(Arc<LemmaSpec>, Arc<LemmaSpec>)],
) -> Result<Vec<Arc<LemmaSpec>>, Error> {
    let mut in_degree: BTreeMap<Arc<LemmaSpec>, usize> = BTreeMap::new();
    let mut adjacency: BTreeMap<Arc<LemmaSpec>, Vec<Arc<LemmaSpec>>> = BTreeMap::new();

    for key in nodes.keys() {
        in_degree.entry(key.clone()).or_insert(0);
        adjacency.entry(key.clone()).or_default();
    }

    for (from, to) in edges {
        if nodes.contains_key(from) && nodes.contains_key(to) {
            adjacency.entry(from.clone()).or_default().push(to.clone());
            *in_degree.entry(to.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<Arc<LemmaSpec>> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(k, _)| Arc::clone(k))
        .collect();

    let mut result = Vec::new();
    while let Some(key) = queue.pop_front() {
        if let Some(spec) = nodes.get(&key) {
            result.push(Arc::clone(spec));
        }
        if let Some(neighbors) = adjacency.get(&key) {
            for neighbor in neighbors {
                if let Some(deg) = in_degree.get_mut(neighbor) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
    }

    if result.len() != nodes.len() {
        let mut cycle_nodes: Vec<String> = in_degree
            .iter()
            .filter(|(_, &deg)| deg > 0)
            .map(|(k, _)| Arc::clone(k).name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        cycle_nodes.sort();
        let cycle_path = if cycle_nodes.len() > 1 {
            let mut path = cycle_nodes.clone();
            path.push(cycle_nodes[0].clone());
            path.join(" -> ")
        } else {
            cycle_nodes.join(" -> ")
        };
        return Err(Error::validation(
            format!("Spec dependency cycle: {}", cycle_path),
            None,
            None::<String>,
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::{
        DataValue as AstDataValue, LemmaData, LemmaSpec, Reference, SpecRef,
    };
    use crate::parsing::source::Source;
    use crate::Span;

    fn dag_errors(e: DagError) -> Vec<Error> {
        match e {
            DagError::Cycle(e) | DagError::Other(e) => e,
        }
    }

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
        Source::new(
            "test",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        )
    }

    fn spec_with_dep(
        name: &str,
        eff: Option<DateTimeValue>,
        dep: &str,
        qualified_at: Option<DateTimeValue>,
    ) -> LemmaSpec {
        let mut s = LemmaSpec::new(name.to_string());
        s.effective_from = EffectiveDate::from_option(eff);
        s.data.push(LemmaData {
            reference: Reference::local("d".to_string()),
            value: AstDataValue::SpecReference(SpecRef {
                name: dep.to_string(),
                from_registry: dep.starts_with('@'),
                effective: qualified_at,
            }),
            source_location: dummy_source(),
        });
        s
    }

    #[test]
    fn dag_error_unqualified_missing_dep_includes_parent_and_resolve_instant() {
        let mut ctx = Context::new();
        let consumer = Arc::new(spec_with_dep(
            "consumer",
            Some(date(2025, 1, 1)),
            "dep",
            None,
        ));
        ctx.insert_spec(Arc::clone(&consumer), false).unwrap();

        let effective = EffectiveDate::DateTimeValue(date(2025, 1, 1));
        let errs = dag_errors(build_dag_for_spec(&ctx, &consumer, &effective).unwrap_err());

        assert_eq!(errs.len(), 1);
        let msg = errs[0].message();
        assert!(msg.contains("'consumer'"), "should name parent spec: {msg}");
        assert!(msg.contains("'dep'"), "should name missing dep: {msg}");
        assert!(
            msg.contains("does not exist"),
            "should say dep doesn't exist: {msg}"
        );

        let suggestion = errs[0].suggestion().expect("should have suggestion");
        assert!(
            suggestion.contains("dep"),
            "suggestion should name dep: {suggestion}"
        );
    }

    #[test]
    fn dag_error_qualified_missing_dep_mentions_qualifier_instant() {
        let mut ctx = Context::new();
        let consumer = Arc::new(spec_with_dep(
            "consumer",
            Some(date(2025, 1, 1)),
            "dep",
            Some(date(2025, 8, 1)),
        ));
        ctx.insert_spec(Arc::clone(&consumer), false).unwrap();

        let effective = EffectiveDate::DateTimeValue(date(2025, 1, 1));
        let errs = dag_errors(build_dag_for_spec(&ctx, &consumer, &effective).unwrap_err());

        assert_eq!(errs.len(), 1);
        let msg = errs[0].message();
        assert!(msg.contains("'consumer'"), "should name parent: {msg}");
        assert!(msg.contains("'dep'"), "should name dep: {msg}");
        assert!(
            msg.contains("2025"),
            "should mention qualifier instant: {msg}"
        );
        assert!(
            msg.contains("at that instant"),
            "should use qualified wording: {msg}"
        );

        let suggestion = errs[0].suggestion().expect("should have suggestion");
        assert!(
            suggestion.contains("effective_from") || suggestion.contains("reference instant"),
            "suggestion should guide fix: {suggestion}"
        );
    }

    #[test]
    fn dag_error_registry_dep_suggests_lemma_get() {
        let mut ctx = Context::new();
        let consumer = Arc::new(spec_with_dep(
            "consumer",
            Some(date(2025, 1, 1)),
            "@org/pkg",
            None,
        ));
        ctx.insert_spec(Arc::clone(&consumer), false).unwrap();

        let effective = EffectiveDate::DateTimeValue(date(2025, 1, 1));
        let errs = dag_errors(build_dag_for_spec(&ctx, &consumer, &effective).unwrap_err());

        assert_eq!(errs.len(), 1);
        let suggestion = errs[0].suggestion().expect("should have suggestion");
        assert!(
            suggestion.contains("lemma get"),
            "registry dep suggestion should include 'lemma get': {suggestion}"
        );
    }

    #[test]
    fn dag_error_has_source_location() {
        let mut ctx = Context::new();
        let consumer = Arc::new(spec_with_dep(
            "consumer",
            Some(date(2025, 1, 1)),
            "dep",
            None,
        ));
        ctx.insert_spec(Arc::clone(&consumer), false).unwrap();

        let effective = EffectiveDate::DateTimeValue(date(2025, 1, 1));
        let errs = dag_errors(build_dag_for_spec(&ctx, &consumer, &effective).unwrap_err());

        let display = format!("{}", errs[0]);
        assert!(
            display.contains("test") || display.contains("line"),
            "error should carry source context: {display}"
        );
    }
}
