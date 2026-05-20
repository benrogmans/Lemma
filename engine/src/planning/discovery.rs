use crate::engine::Context;
use crate::parsing::ast::{
    DataValue, DateTimeValue, EffectiveDate, LemmaRepository, LemmaSpec, ParentType,
    RepositoryQualifier, SpecRef,
};
use crate::parsing::source::Source;
use crate::Error;
use indexmap::IndexMap;
use std::collections::VecDeque;
use std::sync::Arc;

/// A spec together with its owning repository, as produced by DAG discovery.
pub(crate) type DagSpec = (Arc<LemmaRepository>, Arc<LemmaSpec>);

// ---------------------------------------------------------------------------
// Two-stage spec reference resolution
// ---------------------------------------------------------------------------

/// Planning-internal resolved spec reference. [`ResolvedSpecRef::repository`] is `None`
/// for same-repository references (resolved against the consumer spec's owning repository).
/// When `Some`, the arc is the [`LemmaRepository`] for an explicit repository
/// qualifier target, resolved against the context's interned repositories.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedSpecRef {
    pub repository: Option<Arc<LemmaRepository>>,
    pub name: String,
    pub effective: Option<DateTimeValue>,
}

/// Expands same-spec [`DataValue::Import`] (`uses`) aliases for bare names, then resolves
/// a parsed [`SpecRef`] against the active [`Context`].
///
/// Same-repository references (no explicit repository qualifier) leave
/// [`ResolvedSpecRef::repository`] as `None`. Explicit qualifiers resolve to interned
/// repositories as `Some(...)`.
///
/// When a bare name matches a top-level [`DataValue::Import`] (`uses` alias), the returned
/// [`SpecRef`] (second tuple element) is that import's target [`SpecRef`].
pub(crate) fn resolve_spec_ref_after_expanding_uses_aliases(
    context: &Context,
    spec_ref: &SpecRef,
    ref_source: Option<&Source>,
    consumer_name: &str,
    spec_context: Option<Arc<LemmaSpec>>,
) -> Result<(ResolvedSpecRef, SpecRef), Error> {
    let effective_ref = if spec_ref.repository.is_some() {
        spec_ref.clone()
    } else if let Some(consumer) = &spec_context {
        let mut expanded: Option<SpecRef> = None;
        for d in &consumer.data {
            if !d.reference.segments.is_empty() {
                continue;
            }
            if d.reference.name != spec_ref.name {
                continue;
            }
            let DataValue::Import(inner) = &d.value else {
                continue;
            };
            if spec_ref.effective.is_some()
                && (inner.name != spec_ref.name
                    || inner.repository != spec_ref.repository
                    || inner.effective != spec_ref.effective)
            {
                return Err(Error::validation_with_context(
                    format!(
                        "This reference pins an effective date on `{}`, which is only a `uses` import alias in this spec. Put the effective date on the matching `uses` line (for example `uses {}: <target_spec> <datetime>`).",
                        spec_ref.name, spec_ref.name
                    ),
                    ref_source.cloned(),
                    None::<String>,
                    spec_context.clone(),
                    None,
                ));
            }
            expanded = Some(inner.clone());
            break;
        }
        expanded.unwrap_or_else(|| spec_ref.clone())
    } else {
        spec_ref.clone()
    };

    let resolved_repository = match &effective_ref.repository {
        None => None,
        Some(qualifier) => {
            let Some(arc) = context.find_repository(&qualifier.name) else {
                return Err(unknown_repository_qualifier_error(
                    qualifier,
                    &effective_ref,
                    ref_source,
                    consumer_name,
                    spec_context,
                ));
            };
            Some(arc)
        }
    };
    Ok((
        ResolvedSpecRef {
            repository: resolved_repository,
            name: effective_ref.name.clone(),
            effective: effective_ref.effective.clone(),
        },
        effective_ref,
    ))
}

fn unknown_repository_qualifier_error(
    qualifier: &RepositoryQualifier,
    spec_ref: &SpecRef,
    ref_source: Option<&Source>,
    consumer_name: &str,
    spec_context: Option<Arc<LemmaSpec>>,
) -> Error {
    let message = if qualifier.is_registry() {
        format!(
            "'{}' references '{}' from '{}', but repository '{}' is not loaded",
            consumer_name, spec_ref.name, qualifier.name, qualifier.name
        )
    } else {
        format!(
            "'{}' references '{}' from repository '{}', but that repository is not loaded",
            consumer_name, spec_ref.name, qualifier.name
        )
    };

    let suggestion = if qualifier.is_registry() {
        format!(
            "Run `lemma fetch {}` to download the repository.",
            qualifier.name
        )
    } else {
        format!(
            "Ensure the repository '{}' is included in the workspace. If '{}' is a local spec, remove the repository qualifier.",
            qualifier.name, spec_ref.name
        )
    };
    Error::missing_repository(
        message,
        ref_source.cloned(),
        qualifier.name.clone(),
        Some(suggestion),
        spec_context,
    )
}

fn consumer_identity(spec: &LemmaSpec) -> String {
    match &spec.effective_from {
        EffectiveDate::Origin => spec.name.clone(),
        EffectiveDate::DateTimeValue(datetime) => format!("{} {}", spec.name, datetime),
    }
}

/// Resolve a `SpecRef` to the owning repository and loaded `Arc<LemmaSpec>` at the
/// planning `effective`. Returns a [`crate::Error::MissingRepository`] when the
/// repository qualifier is not loaded, or another planning error when the reference
/// cannot be resolved.
pub(crate) fn resolve_spec_ref(
    context: &Context,
    spec_ref: &SpecRef,
    consumer_repository: &Arc<LemmaRepository>,
    consumer_spec: &Arc<LemmaSpec>,
    effective: &EffectiveDate,
    ref_source: Option<Source>,
) -> Result<(Arc<LemmaRepository>, Arc<LemmaSpec>), Error> {
    let consumer_name = consumer_spec.name.as_str();
    let (resolved, effective_ref) = resolve_spec_ref_after_expanding_uses_aliases(
        context,
        spec_ref,
        ref_source.as_ref(),
        consumer_name,
        Some(Arc::clone(consumer_spec)),
    )?;
    let repository_arc = match &resolved.repository {
        Some(explicit) => Arc::clone(explicit),
        None => Arc::clone(consumer_repository),
    };
    let instant = effective_ref.at(effective);
    let Some(spec_set) = context.spec_set(&repository_arc, effective_ref.name.as_str()) else {
        let (message, suggestion) = format_missing_spec_ref(
            consumer_name,
            &repository_arc,
            effective_ref.name.as_str(),
            effective_ref.repository.as_ref(),
            &effective_ref.effective,
            &instant,
            context,
        );
        return Err(Error::validation_with_context(
            message,
            ref_source,
            Some(suggestion),
            Some(Arc::clone(consumer_spec)),
            None,
        ));
    };

    let spec = if let Some(pin) = effective_ref.effective.as_ref() {
        if let Some(exact) = spec_set.get_exact(Some(pin)) {
            Some(Arc::clone(exact))
        } else {
            spec_set.spec_at(&EffectiveDate::DateTimeValue(pin.clone()))
        }
    } else {
        spec_set.spec_at(&instant)
    };

    let spec = spec.ok_or_else(|| {
        let (message, suggestion) = format_missing_spec_ref(
            consumer_name,
            &repository_arc,
            effective_ref.name.as_str(),
            effective_ref.repository.as_ref(),
            &effective_ref.effective,
            &instant,
            context,
        );
        Error::validation_with_context(
            message,
            ref_source.clone(),
            Some(suggestion),
            Some(Arc::clone(consumer_spec)),
            None,
        )
    })?;

    if Arc::ptr_eq(consumer_spec, &spec) {
        return Err(Error::validation_with_context(
            format!(
                "spec '{}' cannot reference itself via '{}'",
                consumer_identity(consumer_spec),
                spec_ref
            ),
            ref_source,
            None::<String>,
            Some(Arc::clone(consumer_spec)),
            None,
        ));
    }

    Ok((repository_arc, spec))
}

fn format_dep_qualified_name(repository: &LemmaRepository, dep_name: &str) -> String {
    match &repository.name {
        Some(name) => format!("{dep_name} from {name}"),
        None => dep_name.to_string(),
    }
}

fn format_missing_spec_ref(
    consumer_name: &str,
    dep_repository: &Arc<LemmaRepository>,
    dep_name: &str,
    explicit_repository_qualifier: Option<&RepositoryQualifier>,
    qualified_at: &Option<DateTimeValue>,
    dep_effective: &EffectiveDate,
    context: &Context,
) -> (String, String) {
    let dep_qualified = format_dep_qualified_name(dep_repository, dep_name);
    if let Some(ref dt) = qualified_at {
        let message = format!(
            "'{}' references '{}' at {}, but no '{}' is active at that instant",
            consumer_name, dep_qualified, dt, dep_qualified
        );
        let suggestion = format!(
            "Add '{}' with effective_from on or before {}, or change the reference instant.",
            dep_qualified, dt
        );
        return if explicit_repository_qualifier.is_some_and(|q| q.is_registry()) {
            (
                message,
                format!(
                    "{} Or run `lemma fetch {}` to fetch it.",
                    suggestion,
                    explicit_repository_qualifier
                        .expect("BUG: checked above")
                        .name
                ),
            )
        } else {
            (message, suggestion)
        };
    }

    let dep_ss = context.spec_set(dep_repository, dep_name);
    let dep_exists = dep_ss.is_some_and(|ss| !ss.is_empty());

    if !dep_exists {
        let message = format!(
            "'{}' depends on '{}', but '{}' does not exist",
            consumer_name, dep_qualified, dep_qualified
        );
        let suggestion = if explicit_repository_qualifier.is_some_and(|q| q.is_registry()) {
            format!(
                "Run `lemma fetch --all` or `lemma fetch {}` to fetch this dependency.",
                explicit_repository_qualifier
                    .expect("BUG: checked above")
                    .name
            )
        } else {
            format!("Create a spec named '{}'.", dep_qualified)
        };
        return (message, suggestion);
    }

    let message = format!(
        "'{}' depends on '{}', but no '{}' is active at {}",
        consumer_name, dep_qualified, dep_qualified, dep_effective
    );
    let suggestion = format!(
        "Add '{}' with effective_from covering {}, or adjust effective_from on '{}'.",
        dep_qualified, dep_effective, consumer_name
    );
    (message, suggestion)
}

// ---------------------------------------------------------------------------
// Dependency edge extraction
// ---------------------------------------------------------------------------

/// One outgoing edge from a spec: `(dep_repository, dep_name, optional explicit
/// effective on reference, source location)`. `dep_repository` is the resolved repository
/// `Arc` — same-repository references inherit the consumer's repository here.
pub(crate) struct DependencyEdge {
    pub dep_repository: Arc<LemmaRepository>,
    pub dep_name: String,
    pub explicit_repository_qualifier: Option<RepositoryQualifier>,
    pub explicit_effective: Option<DateTimeValue>,
    pub source: Source,
}

impl DependencyEdge {
    pub(crate) fn as_spec_ref(&self) -> SpecRef {
        let mut spec_ref = match &self.explicit_repository_qualifier {
            Some(qualifier) => SpecRef::cross_repository(self.dep_name.clone(), qualifier.clone()),
            None => SpecRef::same_repository(self.dep_name.clone()),
        };
        spec_ref.effective = self.explicit_effective.clone();
        spec_ref
    }
}

pub(crate) fn dependency_edges(
    spec: &Arc<LemmaSpec>,
    consumer_repository: &Arc<LemmaRepository>,
    context: &Context,
) -> Result<Vec<DependencyEdge>, Vec<Error>> {
    let mut out = Vec::new();
    let mut errors: Vec<Error> = Vec::new();

    let mut push_edge =
        |spec_ref: &SpecRef, source: &Source| match resolve_spec_ref_after_expanding_uses_aliases(
            context,
            spec_ref,
            Some(source),
            spec.name.as_str(),
            Some(Arc::clone(spec)),
        ) {
            Ok((resolved, effective_ref)) => {
                let dep_repository = match &resolved.repository {
                    Some(r) => Arc::clone(r),
                    None => Arc::clone(consumer_repository),
                };
                out.push(DependencyEdge {
                    dep_repository,
                    dep_name: resolved.name,
                    explicit_repository_qualifier: effective_ref.repository.clone(),
                    explicit_effective: resolved.effective,
                    source: source.clone(),
                });
            }
            Err(e) => errors.push(e),
        };

    for data in &spec.data {
        match &data.value {
            DataValue::Import(spec_ref) => {
                push_edge(spec_ref, &data.source_location);
            }
            DataValue::Definition {
                base: Some(ParentType::Qualified { spec_alias, .. }),
                ..
            } => {
                push_edge(
                    &SpecRef::same_repository(spec_alias.clone()),
                    &data.source_location,
                );
            }
            _ => {}
        }
    }

    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// Unqualified dep interface validation
// ---------------------------------------------------------------------------

/// For each spec with unqualified deps, verify that the dep's interface
/// (schema) is type-compatible across all dep specs active within the
/// consumer's effective range. Qualified deps are pinned and skip this check.
///
/// Returns `(consumer_repository, consumer_spec_name, error)` triples so the
/// caller can do a two-level lookup into `plan()` 's nested `IndexMap`.
pub fn validate_dependency_interfaces(
    context: &Context,
    results: &IndexMap<Arc<LemmaRepository>, IndexMap<String, super::SpecSetPlanningResult>>,
) -> Vec<(Arc<LemmaRepository>, String, Error)> {
    let mut errors: Vec<(Arc<LemmaRepository>, String, Error)> = Vec::new();

    for (_repo, by_name) in results.iter() {
        for set_result in by_name.values() {
            for spec_result in &set_result.slice_results {
                if spec_result
                    .errors
                    .iter()
                    .any(|e| e.kind() == crate::ErrorKind::MissingRepository)
                {
                    continue;
                }

                let spec = &spec_result.spec;
                let consumer_ss = &set_result.lemma_spec_set;
                let consumer_repository = Arc::clone(&consumer_ss.repository);
                let (eff_from, eff_to) = consumer_ss.effective_range(spec);

                let edges = match dependency_edges(spec, &consumer_repository, context) {
                    Ok(edges) => edges,
                    Err(errs) => {
                        for e in errs {
                            errors.push((
                                Arc::clone(&consumer_repository),
                                set_result.name.clone(),
                                e,
                            ));
                        }
                        continue;
                    }
                };

                for edge in edges {
                    if edge.explicit_effective.is_some() {
                        continue;
                    }

                    let dep_qualified =
                        format_dep_qualified_name(&edge.dep_repository, &edge.dep_name);
                    let Some(_dep_ss) = context.spec_set(&edge.dep_repository, &edge.dep_name)
                    else {
                        errors.push((
                            Arc::clone(&consumer_repository),
                            set_result.name.clone(),
                            Error::validation_with_context(
                                format!(
                                    "'{}' depends on '{}', but '{}' does not exist",
                                    spec.name, dep_qualified, dep_qualified
                                ),
                                Some(edge.source.clone()),
                                None::<String>,
                                Some(Arc::clone(spec)),
                                None,
                            ),
                        ));
                        continue;
                    };
                    let dep_set_result = results
                        .get(&edge.dep_repository)
                        .and_then(|by_name| by_name.get(&edge.dep_name))
                        .expect("BUG: dependency is in context but has no planning result — plan() must insert every context spec into results");

                    if dep_set_result.schema_over(&eff_from, &eff_to).is_none() {
                        errors.push((
                            Arc::clone(&consumer_repository),
                            set_result.name.clone(),
                            Error::validation_with_context(
                                format!(
                                    "'{}' depends on '{}' without pinning an effective date, but '{}' changed its interface between temporal slices",
                                    spec.name, dep_qualified, dep_qualified
                                ),
                                Some(edge.source.clone()),
                                Some(format!(
                                    "Pin '{}' to a specific effective date, or make '{}' interface-compatible across specs.",
                                    dep_qualified, dep_qualified
                                )),
                                Some(Arc::clone(spec)),
                                None,
                            ),
                        ));
                    }
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
/// Single-root DFS dependency discovery. Returns a topo-sorted list of
/// `(owning_repository, spec)` pairs containing `root` and its transitive deps,
/// or a typed error on cycles / missing deps.
///
/// Each pair is insertion-ordered (leaves first, root last) and deduplicated
/// by `Arc::ptr_eq` on the spec.
pub(crate) fn build_dag_for_spec(
    context: &Context,
    root: &Arc<LemmaSpec>,
    effective: &EffectiveDate,
) -> Result<Vec<DagSpec>, DagError> {
    // nodes: insertion-ordered, no duplicates (membership via Arc::ptr_eq).
    let mut nodes: Vec<DagSpec> = Vec::new();
    // edges: (from_index, to_index) into nodes — "from depends on to".
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut errors: Vec<Error> = Vec::new();

    let root_repository =
        lookup_owning_repository(context, root).unwrap_or_else(|| context.workspace());
    dfs_discover(
        context,
        Arc::clone(root),
        &root_repository,
        effective,
        &mut nodes,
        &mut edges,
        &mut errors,
    );

    if errors.is_empty() {
        kahns_topo_sort(&nodes, &edges).map_err(|err| DagError::Cycle(vec![err]))
    } else {
        Err(DagError::Other(errors))
    }
}

/// Find the repository `Arc` that owns `spec` in `context`. Used by DFS to resolve
/// same-repository references against the consumer's repository without callers having to
/// pass it explicitly.
pub(crate) fn lookup_owning_repository(
    context: &Context,
    spec: &Arc<LemmaSpec>,
) -> Option<Arc<LemmaRepository>> {
    for (repository, inner) in context.repositories().iter() {
        for (name, set) in inner.iter() {
            if name != &spec.name {
                continue;
            }
            if set.iter_specs().any(|p| Arc::ptr_eq(&p, spec)) {
                return Some(Arc::clone(repository));
            }
        }
    }
    None
}

fn dfs_discover(
    context: &Context,
    spec: Arc<LemmaSpec>,
    consumer_repository: &Arc<LemmaRepository>,
    effective: &EffectiveDate,
    nodes: &mut Vec<DagSpec>,
    edges: &mut Vec<(usize, usize)>,
    errors: &mut Vec<Error>,
) {
    // Membership is pointer identity — no Eq/Ord on LemmaSpec needed.
    if nodes.iter().any(|(_, s)| Arc::ptr_eq(s, &spec)) {
        return;
    }
    let spec_index = nodes.len();
    nodes.push((Arc::clone(consumer_repository), Arc::clone(&spec)));

    let edges_for_spec = match dependency_edges(&spec, consumer_repository, context) {
        Ok(edges) => edges,
        Err(errs) => {
            errors.extend(errs);
            return;
        }
    };

    for edge in edges_for_spec {
        let dep_effective = edge
            .explicit_effective
            .clone()
            .map_or_else(|| effective.clone(), EffectiveDate::DateTimeValue);

        let dependency = match resolve_spec_ref(
            context,
            &edge.as_spec_ref(),
            consumer_repository,
            &spec,
            effective,
            Some(edge.source.clone()),
        ) {
            Ok((_, dependency)) => dependency,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };

        dfs_discover(
            context,
            Arc::clone(&dependency),
            &edge.dep_repository,
            &dep_effective,
            nodes,
            edges,
            errors,
        );
        let dep_index = nodes
            .iter()
            .position(|(_, s)| Arc::ptr_eq(s, &dependency))
            .expect("BUG: dfs_discover must have inserted dependency before returning");
        edges.push((dep_index, spec_index));
    }
}

/// Kahn's topological sort over an index-based edge list.
///
/// `nodes` is insertion-ordered; `edges` are `(from, to)` index pairs.
/// Returns `(repo, spec)` pairs in dependency-first order (leaves first, root last).
fn kahns_topo_sort(nodes: &[DagSpec], edges: &[(usize, usize)]) -> Result<Vec<DagSpec>, Error> {
    let n = nodes.len();
    let mut in_degree = vec![0usize; n];
    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); n];

    for &(from, to) in edges {
        adjacency[from].push(to);
        in_degree[to] += 1;
    }

    let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut result: Vec<DagSpec> = Vec::with_capacity(n);

    while let Some(idx) = queue.pop_front() {
        result.push((Arc::clone(&nodes[idx].0), Arc::clone(&nodes[idx].1)));
        for &neighbor in &adjacency[idx] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    if result.len() != n {
        let mut cycle_names: Vec<String> = in_degree
            .iter()
            .enumerate()
            .filter(|(_, &deg)| deg > 0)
            .map(|(i, _)| nodes[i].1.name.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        cycle_names.sort();
        let cycle_path = if cycle_names.len() > 1 {
            let mut path = cycle_names.clone();
            path.push(cycle_names[0].clone());
            path.join(" -> ")
        } else {
            cycle_names.join(" -> ")
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
    use crate::parsing::ast::Span;
    use crate::parsing::ast::{
        DataValue as AstDataValue, LemmaData, LemmaRepository, LemmaSpec, ParentType, Reference,
        RepositoryQualifier, SpecRef,
    };
    use crate::parsing::source::Source;

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
            crate::parsing::source::SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        )
    }

    fn registry_std_repository() -> Arc<LemmaRepository> {
        Arc::new(LemmaRepository {
            name: Some("@lemma/std".to_string()),
            dependency: Some("@lemma/std".to_string()),
            start_line: 1,
            source_type: Some(crate::parsing::source::SourceType::Volatile),
        })
    }

    fn finance_slice_2024() -> Arc<LemmaSpec> {
        let mut s = LemmaSpec::new("finance".to_string());
        s.effective_from = EffectiveDate::from_option(Some(date(2024, 1, 1)));
        Arc::new(s)
    }

    fn consumer_alias_from_finance() -> LemmaSpec {
        let mut s = LemmaSpec::new("with_alias".to_string());
        s.data.push(LemmaData::new(
            Reference::local("f".to_string()),
            AstDataValue::Import(SpecRef {
                name: "finance".to_string(),
                repository: Some(RepositoryQualifier::new("@lemma/std")),
                effective: Some(date(2024, 1, 1)),
                repository_span: None,
                target_span: None,
            }),
            dummy_source(),
        ));
        s.data.push(LemmaData::new(
            Reference::local("y".to_string()),
            AstDataValue::Definition {
                base: Some(ParentType::Qualified {
                    spec_alias: "f".into(),
                    inner: Box::new(ParentType::Custom { name: "z".into() }),
                }),
                constraints: None,
                value: None,
            },
            dummy_source(),
        ));
        s
    }

    fn spec_with_dep(
        name: &str,
        eff: Option<DateTimeValue>,
        dep: &str,
        qualified_at: Option<DateTimeValue>,
        dep_repository: Option<RepositoryQualifier>,
    ) -> LemmaSpec {
        let mut s = LemmaSpec::new(name.to_string());
        s.effective_from = EffectiveDate::from_option(eff);
        s.data.push(LemmaData {
            reference: Reference::local("d".to_string()),
            value: AstDataValue::Import(SpecRef {
                name: dep.to_string(),
                repository: dep_repository,
                effective: qualified_at,
                repository_span: None,
                target_span: None,
            }),
            source_location: dummy_source(),
        });
        s
    }

    #[test]
    fn dag_error_unqualified_missing_dep_includes_parent_and_resolve_instant() {
        let mut ctx = Context::new();
        let repository = ctx.workspace();
        let consumer = Arc::new(spec_with_dep(
            "consumer",
            Some(date(2025, 1, 1)),
            "dep",
            None,
            None,
        ));
        ctx.insert_spec(Arc::clone(&repository), Arc::clone(&consumer))
            .unwrap();

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
        let repository = ctx.workspace();
        let consumer = Arc::new(spec_with_dep(
            "consumer",
            Some(date(2025, 1, 1)),
            "dep",
            Some(date(2025, 8, 1)),
            None,
        ));
        ctx.insert_spec(Arc::clone(&repository), Arc::clone(&consumer))
            .unwrap();

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
    fn dag_error_registry_dep_suggests_lemma_fetch() {
        let mut ctx = Context::new();
        let repository = ctx.workspace();
        let registry_repository = Arc::new(LemmaRepository {
            name: Some("@org/pkg".to_string()),
            dependency: Some("@org/pkg".to_string()),
            start_line: 1,
            source_type: Some(crate::parsing::source::SourceType::Volatile),
        });
        // Insert a registry-bound spec so the repository qualifier resolves but
        // the *requested* spec is missing — exercising the "registry dep
        // missing" branch of the suggestion text.
        let registry_spec = Arc::new(LemmaSpec::new("other_spec".to_string()));
        ctx.insert_spec(Arc::clone(&registry_repository), registry_spec)
            .unwrap();

        let consumer = Arc::new(spec_with_dep(
            "consumer",
            Some(date(2025, 1, 1)),
            "missing",
            None,
            Some(RepositoryQualifier::new("@org/pkg")),
        ));
        ctx.insert_spec(Arc::clone(&repository), Arc::clone(&consumer))
            .unwrap();

        let effective = EffectiveDate::DateTimeValue(date(2025, 1, 1));
        let errs = dag_errors(build_dag_for_spec(&ctx, &consumer, &effective).unwrap_err());

        assert_eq!(errs.len(), 1);
        let suggestion = errs[0].suggestion().expect("should have suggestion");
        assert!(
            suggestion.contains("lemma fetch"),
            "registry dep suggestion should include 'lemma fetch': {suggestion}"
        );
    }

    #[test]
    fn dag_error_has_source_location() {
        let mut ctx = Context::new();
        let repository = ctx.workspace();
        let consumer = Arc::new(spec_with_dep(
            "consumer",
            Some(date(2025, 1, 1)),
            "dep",
            None,
            None,
        ));
        ctx.insert_spec(Arc::clone(&repository), Arc::clone(&consumer))
            .unwrap();

        let effective = EffectiveDate::DateTimeValue(date(2025, 1, 1));
        let errs = dag_errors(build_dag_for_spec(&ctx, &consumer, &effective).unwrap_err());

        let display = format!("{}", errs[0]);
        assert!(
            display.contains("volatile") || display.contains("line"),
            "error should carry source context: {display}"
        );
    }

    #[test]
    fn resolve_spec_ref_expands_bare_from_uses_alias() {
        let mut ctx = Context::new();
        let std_repo = registry_std_repository();
        let workspace = ctx.workspace();
        ctx.insert_spec(Arc::clone(&std_repo), finance_slice_2024())
            .unwrap();
        let consumer = Arc::new(consumer_alias_from_finance());
        ctx.insert_spec(Arc::clone(&workspace), Arc::clone(&consumer))
            .unwrap();

        let effective = EffectiveDate::DateTimeValue(date(2024, 6, 1));
        let resolved = resolve_spec_ref(
            &ctx,
            &SpecRef {
                name: "f".into(),
                repository: None,
                effective: None,
                repository_span: None,
                target_span: None,
            },
            &workspace,
            &consumer,
            &effective,
            None,
        )
        .expect("finance via uses alias");

        assert_eq!(resolved.1.name, "finance");
    }

    #[test]
    fn uses_implicit_alias_same_name_with_effective_resolves_on_import_target() {
        let inner = SpecRef {
            name: "finance".to_string(),
            repository: None,
            effective: Some(date(2026, 1, 1)),
            repository_span: None,
            target_span: None,
        };
        let mut consumer = LemmaSpec::new("finance".to_string());
        consumer.effective_from = EffectiveDate::from_option(Some(date(2027, 1, 1)));
        consumer.data.push(LemmaData::new(
            Reference::local("finance".to_string()),
            AstDataValue::Import(inner.clone()),
            dummy_source(),
        ));

        let mut ctx = Context::new();
        let workspace = ctx.workspace();
        let mut finance_2026 = LemmaSpec::new("finance".to_string());
        finance_2026.effective_from = EffectiveDate::from_option(Some(date(2026, 1, 1)));
        ctx.insert_spec(Arc::clone(&workspace), Arc::new(finance_2026))
            .unwrap();
        let consumer_arc = Arc::new(consumer);
        ctx.insert_spec(Arc::clone(&workspace), Arc::clone(&consumer_arc))
            .unwrap();

        let (resolved, effective_ref) = resolve_spec_ref_after_expanding_uses_aliases(
            &ctx,
            &inner,
            Some(&consumer_arc.data[0].source_location),
            "finance",
            Some(Arc::clone(&consumer_arc)),
        )
        .expect("uses finance 2026-01-01 must resolve when alias equals target name");

        assert_eq!(resolved.name, "finance");
        assert_eq!(
            resolved.effective.as_ref(),
            Some(&date(2026, 1, 1)),
            "effective pin on uses line must be preserved"
        );
        assert_eq!(effective_ref.effective.as_ref(), Some(&date(2026, 1, 1)),);
    }

    #[test]
    fn resolve_spec_ref_rejects_forward_pin_without_exact_slice() {
        let mut ctx = Context::new();
        let workspace = ctx.workspace();
        let mut origin = LemmaSpec::new("finance".to_string());
        origin.data.push(LemmaData::new(
            Reference::local("rate".to_string()),
            DataValue::Definition {
                base: Some(ParentType::Custom {
                    name: "number".into(),
                }),
                constraints: None,
                value: None,
            },
            dummy_source(),
        ));
        ctx.insert_spec(Arc::clone(&workspace), Arc::new(origin))
            .unwrap();
        let mut consumer = LemmaSpec::new("finance".to_string());
        consumer.effective_from = EffectiveDate::from_option(Some(date(2026, 5, 20)));
        consumer.data.push(LemmaData::new(
            Reference::local("fin".to_string()),
            DataValue::Import(SpecRef {
                name: "finance".into(),
                repository: None,
                effective: Some(date(2027, 1, 1)),
                repository_span: None,
                target_span: None,
            }),
            dummy_source(),
        ));
        let consumer = Arc::new(consumer);
        ctx.insert_spec(Arc::clone(&workspace), Arc::clone(&consumer))
            .unwrap();

        let err = resolve_spec_ref(
            &ctx,
            &SpecRef {
                name: "finance".into(),
                repository: None,
                effective: Some(date(2027, 1, 1)),
                repository_span: None,
                target_span: None,
            },
            &workspace,
            &consumer,
            &consumer.effective_from,
            None,
        )
        .expect_err("finance 2027 row does not exist");

        let msg = err.to_string();
        assert!(
            msg.contains("active at that instant") || msg.contains("cannot reference itself"),
            "expected missing exact slice or same-body import, got: {msg}"
        );
    }

    #[test]
    fn resolve_spec_ref_rejects_effective_on_bare_from_uses_alias() {
        let consumer = Arc::new(consumer_alias_from_finance());
        let mut ctx = Context::new();
        let std_repo = registry_std_repository();
        let workspace = ctx.workspace();
        ctx.insert_spec(Arc::clone(&std_repo), finance_slice_2024())
            .unwrap();
        ctx.insert_spec(Arc::clone(&workspace), Arc::clone(&consumer))
            .unwrap();

        let effective = EffectiveDate::DateTimeValue(date(2024, 6, 1));
        let err = resolve_spec_ref(
            &ctx,
            &SpecRef {
                name: "f".into(),
                repository: None,
                effective: Some(date(2026, 1, 1)),
                repository_span: None,
                target_span: None,
            },
            &workspace,
            &consumer,
            &effective,
            None,
        )
        .expect_err("effective pin on from + uses alias is invalid");

        let msg = err.message();
        assert!(msg.contains("uses alias") || msg.contains("uses"), "{msg}");
    }

    #[test]
    fn dependency_edges_from_uses_alias_has_registry_qualifier() {
        let mut ctx = Context::new();
        let std_repo = registry_std_repository();
        let workspace = ctx.workspace();
        ctx.insert_spec(Arc::clone(&std_repo), finance_slice_2024())
            .unwrap();
        let consumer = Arc::new(consumer_alias_from_finance());
        ctx.insert_spec(Arc::clone(&workspace), Arc::clone(&consumer))
            .unwrap();

        let edges = dependency_edges(&consumer, &workspace, &ctx).expect("dependency_edges");
        let from_y = edges
            .iter()
            .find(|e| {
                e.dep_name == "finance"
                    && e.explicit_repository_qualifier
                        .as_ref()
                        .is_some_and(|q| q.name == "@lemma/std")
                    && e.source == consumer.data[1].source_location
            })
            .expect("edge from data y binding");
        assert_eq!(from_y.dep_name, "finance");
    }

    #[test]
    fn dependency_edges_qualified_parent_resolves_uses_alias() {
        let mut ctx = Context::new();
        let workspace = ctx.workspace();
        let child = Arc::new(LemmaSpec::new("child".to_string()));
        let mut dep = LemmaSpec::new("dep".to_string());
        dep.data.push(LemmaData::new(
            Reference::local("c".to_string()),
            DataValue::Import(SpecRef {
                effective: Some(date(2025, 6, 1)),
                ..SpecRef::same_repository("child")
            }),
            dummy_source(),
        ));
        dep.data.push(LemmaData::new(
            Reference::local("money".to_string()),
            DataValue::Definition {
                base: Some(ParentType::Qualified {
                    spec_alias: "c".into(),
                    inner: Box::new(ParentType::Custom {
                        name: "money".into(),
                    }),
                }),
                constraints: None,
                value: None,
            },
            dummy_source(),
        ));
        let dep = Arc::new(dep);
        ctx.insert_spec(Arc::clone(&workspace), Arc::clone(&child))
            .unwrap();
        ctx.insert_spec(Arc::clone(&workspace), Arc::clone(&dep))
            .unwrap();

        let edges = dependency_edges(&dep, &workspace, &ctx).expect("edges");
        assert!(
            edges.iter().any(|e| e.dep_name == "child"),
            "qualified `c.money` must depend on resolved spec `child`, not alias `c`: {:?}",
            edges
                .iter()
                .map(|e| (&e.dep_name, &e.explicit_effective))
                .collect::<Vec<_>>()
        );
        assert!(
            !edges.iter().any(|e| e.dep_name == "c"),
            "must not emit unresolved alias name as dependency"
        );
    }

    #[test]
    fn build_dag_consumer_resolves_qualified_dep_parent_via_uses_alias() {
        let source = r#"
spec consumer 2025-01-01
uses d: dep 2025-01-01
rule out: d.doubled

spec dep 2025-01-01
uses c: child 2025-06-01
data money: c.money
data p: 5 usd
rule doubled: p * 2

spec child 2025-01-01
data money: quantity
 -> unit eur 1.00
 -> decimals 2

spec child 2025-06-01
data money: quantity
 -> unit eur 1.00
 -> unit usd 0.91
 -> decimals 2
"#;
        let specs = crate::parse(
            source,
            crate::parsing::source::SourceType::Volatile,
            &crate::ResourceLimits::default(),
        )
        .expect("parse")
        .into_flattened_specs();
        let mut ctx = Context::new();
        let workspace = ctx.workspace();
        for spec in specs {
            ctx.insert_spec(Arc::clone(&workspace), Arc::new(spec))
                .unwrap();
        }
        let consumer = ctx
            .spec_set(&workspace, "consumer")
            .and_then(|ss| ss.spec_at(&EffectiveDate::from_option(Some(date(2025, 3, 1)))))
            .expect("consumer");
        let dag = build_dag_for_spec(
            &ctx,
            &consumer,
            &EffectiveDate::from_option(Some(date(2025, 3, 1))),
        )
        .expect("DAG must not reference unresolved alias `c`");
        let names: Vec<_> = dag.iter().map(|(_, s)| s.name.as_str()).collect();
        assert!(names.contains(&"child"));
        assert!(names.contains(&"dep"));
        assert!(!names.contains(&"c"));
    }
}
