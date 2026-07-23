//! Planning module for Lemma specs
//!
//! This module performs complete static analysis and builds execution plans:
//! - Builds Graph with data and rules (validated, with types computed)
//! - Builds ExecutionPlan from Graph (topo order for cycles, typing, and dep-before-consumer)
//! - Validates spec structure and references
//!
//! Contract model:
//! - Interface contract: data (inputs) + rules (outputs), including full type constraints.
//!   Cross-spec bindings must satisfy this contract at planning time.

pub mod discovery;
pub mod execution_plan;
pub mod explanation;
pub mod graph;
pub mod normalize;
pub mod semantics;
pub mod spec_set;
use crate::engine::Context;
use crate::parsing::ast::{DateTimeValue, EffectiveDate, LemmaRepository};
use crate::Error;
pub use execution_plan::ExecutionPlan;
use indexmap::IndexMap;
pub use spec_set::LemmaSpecSet;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::unreachable;

/// Compiled plans keyed by repository name (`None` = workspace) then spec name.
///
/// Temporal slices per spec are a [`BTreeMap`] keyed by [`ExecutionPlan::effective`].
#[derive(Debug, Default)]
pub(crate) struct PlanStore {
    plans: IndexMap<Option<String>, IndexMap<String, BTreeMap<EffectiveDate, ExecutionPlan>>>,
}

impl PlanStore {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            plans: IndexMap::new(),
        }
    }

    fn repo_key(repository: Option<&str>) -> Option<String> {
        repository.map(|name| crate::parsing::ast::ascii_lowercase_logical_name(name.to_string()))
    }

    fn spec_key(spec: &str) -> String {
        crate::parsing::ast::ascii_lowercase_logical_name(spec.to_string())
    }

    /// Insert one temporal slice. Panics on duplicate [`ExecutionPlan::effective`]
    /// for the same `(repository, spec)`.
    pub(crate) fn insert_plan(
        &mut self,
        repository: Option<&str>,
        spec: &str,
        plan: ExecutionPlan,
    ) {
        let effective = plan.effective.clone();
        let occupied = self
            .plans
            .entry(Self::repo_key(repository))
            .or_default()
            .entry(Self::spec_key(spec))
            .or_default()
            .insert(effective.clone(), plan)
            .is_some();
        if occupied {
            panic!("BUG: duplicate plan effective {effective:?} for spec '{spec}'");
        }
    }

    /// Temporal slices for `(repository, spec)`, ordered by `effective`.
    #[must_use]
    pub(crate) fn get_plans(
        &self,
        repository: Option<&str>,
        spec: &str,
    ) -> Option<&BTreeMap<EffectiveDate, ExecutionPlan>> {
        self.plans
            .get(&Self::repo_key(repository))
            .and_then(|by_name| by_name.get(&Self::spec_key(spec)))
    }

    /// Covering plan at `effective_at`, or `None`.
    #[must_use]
    pub(crate) fn get_plan(
        &self,
        repository: Option<&str>,
        spec: &str,
        effective_at: &EffectiveDate,
    ) -> Option<&ExecutionPlan> {
        self.get_plans(repository, spec)
            .and_then(|plans| execution_plan::plan_at(plans, effective_at))
    }

    /// Replace entire store after successful `plan()` (load/remove).
    pub(crate) fn replace(&mut self, plans: Self) {
        *self = plans;
    }
}

/// Two half-open ranges `[a_from, a_to)` and `[b_from, b_to)` overlap when
/// `a_from < b_to AND b_from < a_to` (with `None` representing +/-infinity).
pub(crate) fn ranges_overlap(
    a_from: &Option<DateTimeValue>,
    a_to: &Option<DateTimeValue>,
    b_from: &Option<DateTimeValue>,
    b_to: &Option<DateTimeValue>,
) -> bool {
    let a_before_b_end = match (a_from, b_to) {
        (_, None) => true,
        (None, Some(_)) => true,
        (Some(a), Some(b)) => a < b,
    };
    let b_before_a_end = match (b_from, a_to) {
        (_, None) => true,
        (None, Some(_)) => true,
        (Some(b), Some(a)) => b < a,
    };
    a_before_b_end && b_before_a_end
}

/// Result of a full planning pass over a [`Context`].
pub(crate) struct PlanningResult {
    pub plans: PlanStore,
    pub errors: Vec<Error>,
}

/// Build execution plans for every spec in `context`.
///
/// Returns newly built plans and any planning errors. Does not mutate caller state.
pub(crate) fn plan(context: &Context, limits: &crate::limits::ResourceLimits) -> PlanningResult {
    let mut plans = PlanStore::new();
    let mut errors = Vec::new();
    let mut failed_specs: HashSet<(Arc<LemmaRepository>, String, EffectiveDate)> = HashSet::new();
    let mut failed_plan_sets: HashSet<(Arc<LemmaRepository>, String)> = HashSet::new();
    let mut missing_repository_source_specs: HashSet<(
        Arc<LemmaRepository>,
        String,
        EffectiveDate,
    )> = HashSet::new();

    for (repository, inner) in context.repositories().iter() {
        for (_, lemma_spec_set) in inner.iter() {
            for spec in lemma_spec_set.iter_specs() {
                let spec_name = &spec.name;
                let mut slice_errors = Vec::new();
                let mut slice_plans = Vec::new();

                for effective in lemma_spec_set.effective_dates(spec, context) {
                    let ordered_dependencies = match discovery::discover_dependency_order(
                        context, spec, &effective, limits,
                    ) {
                        Ok(ordered_dependencies) => ordered_dependencies,
                        Err(discovery::DependencyDiscoveryError::Cycle(plan_errors)) => {
                            slice_errors.extend(plan_errors);
                            continue;
                        }
                        Err(discovery::DependencyDiscoveryError::Other(plan_errors)) => {
                            slice_errors.extend(plan_errors);
                            continue;
                        }
                    };

                    match graph::Graph::build(
                        context,
                        repository,
                        spec,
                        &ordered_dependencies,
                        &effective,
                        limits,
                    ) {
                        Ok((graph, resolved_types)) => {
                            match execution_plan::build_execution_plan(
                                &graph,
                                resolved_types,
                                &effective,
                                limits,
                            ) {
                                Ok(execution_plan) => slice_plans.push(execution_plan),
                                Err(plan_errors) => slice_errors.extend(plan_errors),
                            }
                        }
                        Err(build_errors) => {
                            slice_errors.extend(build_errors);
                        }
                    }
                }

                if slice_plans.is_empty() && slice_errors.is_empty() {
                    unreachable!(
                        "BUG: no plans or errors for spec {}, effective from {:?}",
                        spec_name, spec.effective_from
                    );
                }

                if !slice_errors.is_empty() {
                    if slice_errors
                        .iter()
                        .any(|error| error.kind() == crate::ErrorKind::MissingRepository)
                    {
                        missing_repository_source_specs.insert((
                            Arc::clone(repository),
                            spec_name.clone(),
                            spec.effective_from.clone(),
                        ));
                    }
                    errors.extend(
                        slice_errors
                            .into_iter()
                            .map(|err| err.with_spec_context(spec)),
                    );
                    if slice_plans.is_empty() {
                        failed_specs.insert((
                            Arc::clone(repository),
                            spec_name.clone(),
                            spec.effective_from.clone(),
                        ));
                    }
                }

                if !slice_plans.is_empty() {
                    for execution_plan in slice_plans {
                        plans.insert_plan(repository.name.as_deref(), spec_name, execution_plan);
                    }
                }
            }
        }
    }

    // Validate dependency interfaces across all specs in the context.
    for (consumer_repository, consumer_spec_name, consumer_spec, err) in
        discovery::validate_dependency_interfaces(
            context,
            &plans,
            &missing_repository_source_specs,
            &failed_specs,
        )
    {
        errors.push(err.with_spec_context(consumer_spec));
        failed_plan_sets.insert((consumer_repository, consumer_spec_name));
    }

    // Remove failed plan sets from the plan store.
    for (repository, name) in &failed_plan_sets {
        if let Some(by_name) = plans.plans.get_mut(&repository.name) {
            by_name.shift_remove(name);
        }
    }

    dedup_errors(&mut errors);
    PlanningResult { plans, errors }
}

/// Remove duplicate errors in-place, preserving first occurrence order.
/// Two errors are considered duplicates when they share the same kind,
/// message, source location, and owned spec-context attribution.
fn dedup_errors(errors: &mut Vec<Error>) {
    let mut seen = std::collections::HashSet::new();
    errors.retain(|error| {
        let details = error.details();
        let key = (
            error.kind(),
            error.message().to_string(),
            error.location().cloned(),
            details.spec_context_name.clone(),
            details.spec_context_effective_from.clone(),
        );
        seen.insert(key)
    });
}

#[cfg(test)]
mod tests {
    use super::dedup_errors;
    use crate::parsing::ast::{LemmaSpec, Span};
    use crate::parsing::source::{Source, SourceType};
    use crate::Error;

    /// `plan` dedups errors globally, after spec context is attached.
    /// Two errors that agree on kind, message, and location but belong to
    /// different specs are distinct diagnostics: they render as
    /// "In spec 'a': ..." and "In spec 'b': ..." and point the user at two
    /// different places to fix. Dedup must not collapse them.
    #[test]
    fn dedup_keeps_identical_errors_attributed_to_different_specs() {
        let source = Source::new(
            SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let base = Error::validation("same message", Some(source), None::<String>);
        let spec_a = LemmaSpec::new("a".to_string());
        let spec_b = LemmaSpec::new("b".to_string());
        let mut errors = vec![
            base.clone().with_spec_context(&spec_a),
            base.with_spec_context(&spec_b),
        ];

        dedup_errors(&mut errors);

        assert_eq!(
            errors.len(),
            2,
            "errors from different specs must both survive dedup, got: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
    }
}
