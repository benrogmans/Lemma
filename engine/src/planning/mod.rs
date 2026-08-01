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
            let versions: Vec<execution_plan::ShowVersion> = lemma_spec_set
                .iter_with_ranges()
                .map(|(_, from, to)| execution_plan::ShowVersion {
                    effective_from: from,
                    effective_to: to,
                })
                .collect();

            for spec in lemma_spec_set.iter_specs() {
                let spec_name = &spec.name;
                let mut slice_errors = Vec::new();
                let mut slice_plans = Vec::new();

                let breakpoints =
                    match discovery::plan_breakpoints(context, lemma_spec_set, spec, limits) {
                        Ok(dates) => dates,
                        Err(errors) => {
                            slice_errors.extend(errors);
                            // Skip the slice loop: existing error handling below treats empty
                            // slice_plans + non-empty slice_errors as a failed spec.
                            Vec::new()
                        }
                    };

                for effective in breakpoints {
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
                                Ok(mut execution_plan) => {
                                    execution_plan::attach_show_cache(
                                        &mut execution_plan,
                                        lemma_spec_set,
                                        spec,
                                        &versions,
                                    );
                                    slice_plans.push(execution_plan);
                                }
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

    use crate::literals::DateGranularity;
    use crate::parsing::ast::DateTimeValue;

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
            granularity: DateGranularity::Full,
        }
    }

    /// Count the temporal slices that were planned for `spec_name` across all versions.
    fn slice_count(engine: &crate::Engine, spec_name: &str) -> usize {
        engine
            .plans
            .get_plans(None, spec_name)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Run `spec_name` at `at` with no data and return the display string for `rule_name`.
    fn run_display(
        engine: &crate::Engine,
        spec_name: &str,
        at: &DateTimeValue,
        rule_name: &str,
    ) -> String {
        engine
            .run(
                None,
                spec_name,
                Some(at),
                std::collections::HashMap::new(),
                None,
                false,
            )
            .expect("evaluation must succeed")
            .results
            .get(rule_name)
            .expect("rule must be in results")
            .display()
            .expect("rule must have a display value")
            .to_string()
    }

    /// Sum of workspace plan-set sizes for `scale_test_0` .. `scale_test_{count-1}`.
    fn slice_count_for_independent_specs(engine: &crate::Engine, count: usize) -> usize {
        (0..count)
            .map(|i| slice_count(engine, &format!("scale_test_{i}")))
            .sum()
    }

    /// Axis 5: each temporal version is one slice → count exactly.
    #[test]
    fn planning_slice_count_temporal_versions_of_one_spec_is_exact() {
        use crate::tests::scaling_generators::temporal_versions_of_one_spec;

        for count in [1_usize, 4, 16] {
            let engine = load(&temporal_versions_of_one_spec(count));
            let slices = slice_count(&engine, "scale_test");
            assert_eq!(
                slices, count,
                "count={count}: temporal versions of one spec must yield exactly count slices, got {slices}"
            );
        }
    }

    /// Axis 6 — dated independent specs: each spec must get exactly one slice
    /// (its own effective date is the only breakpoint in its closure).
    #[test]
    fn planning_slice_count_dated_independent_specs_each_get_one_slice() {
        use crate::tests::scaling_generators::specs_at_distinct_effective_dates;

        for count in [4_usize, 8] {
            let engine = load(&specs_at_distinct_effective_dates(count));
            let slices = slice_count_for_independent_specs(&engine, count);
            assert_eq!(
                slices, count,
                "count={count}: each independent dated spec must get exactly one slice, got {slices}"
            );
            for i in 0..count {
                assert_eq!(
                    slice_count(&engine, &format!("scale_test_{i}")),
                    1,
                    "scale_test_{i} must have exactly one slice"
                );
            }
        }
    }

    /// Axis 6 — undated control: all specs undated → each gets exactly one slice.
    #[test]
    fn planning_slice_count_undated_independent_specs_each_get_one_slice() {
        use crate::tests::scaling_generators::specs_all_undated;

        for count in [4_usize, 8] {
            let engine = load(&specs_all_undated(count));
            let slices = slice_count_for_independent_specs(&engine, count);
            assert_eq!(
                slices, count,
                "count={count}: undated independent specs must each get exactly one slice, got {slices}"
            );
        }
    }

    fn nf_size(source: &str, spec_name: &str) -> usize {
        let mut engine = crate::Engine::new();
        engine
            .load([(SourceType::Volatile, source.to_string())])
            .expect("spec must load");
        engine
            .plans
            .get_plans(None, spec_name)
            .expect("plans for spec")
            .values()
            .next()
            .expect("at least one slice")
            .normal_forms
            .len()
    }

    /// Axes 1–4: NF size grows with ratio below 8 for fourfold input increase.
    ///
    /// Correct before and after all parts (NF build is already linear).
    #[test]
    fn planning_nf_size_options_scales_near_linear() {
        use crate::tests::scaling_generators::options_per_data_declaration;

        let small = nf_size(&options_per_data_declaration(8), "scale_test");
        let large = nf_size(&options_per_data_declaration(32), "scale_test");
        let ratio = large as f64 / small as f64;
        assert!(
            ratio < 8.0,
            "axis 1 (options): NF size ratio must be < 8 for 4x input; \
             small={small} large={large} ratio={ratio:.2}"
        );
    }

    #[test]
    fn planning_nf_size_shared_path_arms_scales_near_linear() {
        use crate::tests::scaling_generators::unless_arms_on_shared_path;

        let small = nf_size(&unless_arms_on_shared_path(8), "scale_test");
        let large = nf_size(&unless_arms_on_shared_path(32), "scale_test");
        let ratio = large as f64 / small as f64;
        assert!(
            ratio < 8.0,
            "axis 2 (shared-path arms): NF size ratio must be < 8 for 4x input; \
             small={small} large={large} ratio={ratio:.2}"
        );
    }

    #[test]
    fn planning_nf_size_distinct_path_arms_scales_near_linear() {
        use crate::tests::scaling_generators::unless_arms_on_distinct_paths;

        let small = nf_size(&unless_arms_on_distinct_paths(8), "scale_test");
        let large = nf_size(&unless_arms_on_distinct_paths(32), "scale_test");
        let ratio = large as f64 / small as f64;
        assert!(
            ratio < 8.0,
            "axis 3 (distinct-path arms): NF size ratio must be < 8 for 4x input; \
             small={small} large={large} ratio={ratio:.2}"
        );
    }

    #[test]
    fn planning_nf_size_conjunction_chain_scales_near_linear() {
        use crate::tests::scaling_generators::conjunction_chain;

        let small = nf_size(&conjunction_chain(8), "scale_test");
        let large = nf_size(&conjunction_chain(32), "scale_test");
        let ratio = large as f64 / small as f64;
        assert!(
            ratio < 8.0,
            "axis 4 (conjunction chain): NF size ratio must be < 8 for 4x input; \
             small={small} large={large} ratio={ratio:.2}"
        );
    }

    fn load(source: &str) -> crate::Engine {
        let mut engine = crate::Engine::new();
        engine
            .load([(SourceType::Volatile, source.to_string())])
            .expect("spec must load");
        engine
    }

    // --- Part 2 corpus: breakpoint correctness ---

    /// Unpinned dependency with two versions inside the consumer's range → consumer gets
    /// exactly two slices.  Evaluating at dates within each slice yields the corresponding
    /// dependency version's value.
    #[test]
    fn breakpoints_unpinned_dep_two_versions_consumer_gets_two_slices() {
        let engine = load(
            r#"
spec dep
data v: 1
rule r: v

spec dep 2025-06-01
data v: 2
rule r: v

spec consumer 2025-01-01
uses d: dep
rule out: d.r
"#,
        );
        assert_eq!(
            slice_count(&engine, "consumer"),
            2,
            "consumer must have exactly 2 slices"
        );
        assert_eq!(
            run_display(&engine, "consumer", &date(2025, 3, 1), "out"),
            "1",
            "before dep v2 boundary: must use dep v1"
        );
        assert_eq!(
            run_display(&engine, "consumer", &date(2025, 9, 1), "out"),
            "2",
            "after dep v2 boundary: must use dep v2"
        );
    }

    /// Pinned dependency with two versions → consumer gets exactly one slice regardless of
    /// the number of dep versions in the context.
    #[test]
    fn breakpoints_pinned_dep_two_versions_consumer_gets_one_slice() {
        let engine = load(
            r#"
spec dep
data v: 1
rule r: v

spec dep 2025-06-01
data v: 2
rule r: v

spec consumer 2025-01-01
uses d: dep 2025-01-01
rule out: d.r
"#,
        );
        assert_eq!(
            slice_count(&engine, "consumer"),
            1,
            "pinned dep: consumer must have exactly 1 slice"
        );
        assert_eq!(
            run_display(&engine, "consumer", &date(2025, 3, 1), "out"),
            "1",
            "pinned dep: must always use dep v1"
        );
        assert_eq!(
            run_display(&engine, "consumer", &date(2025, 9, 1), "out"),
            "1",
            "pinned dep: must still use dep v1 after dep v2 boundary"
        );
    }

    /// Pinned dependency whose own dependency has versions → consumer still gets one slice.
    /// The pin prunes the entire subtree: nothing beneath the pinned dep can shift.
    #[test]
    fn breakpoints_pinned_dep_subtree_pruned_consumer_gets_one_slice() {
        let engine = load(
            r#"
spec base
data v: 10
rule r: v

spec base 2025-06-01
data v: 20
rule r: v

spec dep 2025-01-01
uses b: base
rule r: b.r

spec consumer 2025-01-01
uses d: dep 2025-01-01
rule out: d.r
"#,
        );
        assert_eq!(
            slice_count(&engine, "consumer"),
            1,
            "pinned dep with versioned transitive dep: consumer must still get 1 slice"
        );
    }

    /// Origin consumer with a dependency that gains a second version at a dated boundary →
    /// consumer gets two slices (at Origin using dep v1, and at dep's v2 date using dep v2).
    #[test]
    fn breakpoints_origin_consumer_dated_dep_gets_two_slices() {
        let engine = load(
            r#"
spec dep
data v: 5
rule r: v

spec dep 2025-01-01
data v: 99
rule r: v

spec consumer
uses d: dep
rule out: d.r
"#,
        );
        assert_eq!(
            slice_count(&engine, "consumer"),
            2,
            "origin consumer with dated dep: must have slices at Origin and dep's second-version date"
        );
        assert_eq!(
            run_display(&engine, "consumer", &date(2024, 6, 1), "out"),
            "5",
            "before dep v2: must use dep v1"
        );
        assert_eq!(
            run_display(&engine, "consumer", &date(2025, 3, 1), "out"),
            "99",
            "after dep v2 boundary: must use dep v2"
        );
    }

    /// Diamond closure: two paths lead to the same dependency. Dependency version dates
    /// must be counted once, not duplicated.
    #[test]
    fn breakpoints_diamond_closure_dates_counted_once() {
        let engine = load(
            r#"
spec dep
data v: 1
rule r: v

spec dep 2025-06-01
data v: 2
rule r: v

spec mid_a
uses d: dep
rule r: d.r

spec mid_b
uses d: dep
rule r: d.r

spec consumer
uses a: mid_a
uses b: mid_b
rule out: a.r
"#,
        );
        // Two paths to dep but only 2 breakpoints: Origin and 2025-06-01.
        assert_eq!(
            slice_count(&engine, "consumer"),
            2,
            "diamond closure: dep dates must be counted once, not doubled"
        );
        assert_eq!(
            run_display(&engine, "consumer", &date(2025, 3, 1), "out"),
            "1",
            "diamond: before dep v2 boundary"
        );
        assert_eq!(
            run_display(&engine, "consumer", &date(2025, 9, 1), "out"),
            "2",
            "diamond: after dep v2 boundary"
        );
    }

    /// Unrelated noise boundaries inside the consumer's range must not add slices.
    /// A consumer with no dependencies should have exactly one slice regardless of how
    /// many other dated specs are in the context.
    #[test]
    fn breakpoints_unrelated_noise_boundaries_do_not_add_slices() {
        let engine = load(
            r#"
spec consumer
data v: 1
rule out: v

spec noise_a 2025-01-01
data x: 1

spec noise_b 2025-06-01
data x: 1
"#,
        );
        assert_eq!(
            slice_count(&engine, "consumer"),
            1,
            "independent consumer must have exactly 1 slice even with unrelated dated specs"
        );
    }

    /// A dependency closure that breaches max_dag_specs must return an error, and the spec
    /// must surface no slices.
    #[test]
    fn breakpoints_closure_exceeding_max_dag_specs_errors() {
        // Build a spec with many dependencies to exceed the tiny limit.
        let mut source = String::new();
        for i in 0..6 {
            source.push_str(&format!("spec dep_{i}\ndata v: {i}\nrule r: v\n\n"));
        }
        source.push_str("spec consumer\n");
        for i in 0..6 {
            source.push_str(&format!("uses d_{i}: dep_{i}\n"));
        }
        source.push_str("rule out: d_0.r\n");

        let limits = crate::ResourceLimits {
            max_dag_specs: 3,
            ..crate::ResourceLimits::default()
        };
        let mut engine = crate::Engine::with_limits(limits);
        engine
            .load([(SourceType::Volatile, source)])
            .expect_err("must fail with too many closure members");

        // No slice must have been produced for consumer.
        assert_eq!(
            slice_count(&engine, "consumer"),
            0,
            "consumer must have no slices when closure exceeds max_dag_specs"
        );
    }
}
