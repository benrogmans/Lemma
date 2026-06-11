//! Planning module for Lemma specs
//!
//! This module performs complete static analysis and builds execution plans:
//! - Builds Graph with data and rules (validated, with types computed)
//! - Builds ExecutionPlan from Graph (topologically sorted, ready for evaluation)
//! - Validates spec structure and references
//!
//! Contract model:
//! - Interface contract: data (inputs) + rules (outputs), including full type constraints.
//!   Cross-spec bindings must satisfy this contract at planning time.

pub mod data_input;
pub mod discovery;
pub mod execution_plan;
pub mod graph;
pub mod normalize;
pub mod semantics;
pub mod spec_set;
#[cfg(test)]
mod transitive_normalization;
use crate::engine::Context;
use crate::parsing::ast::{DateTimeValue, LemmaRepository, LemmaSpec};
use crate::Error;
pub use data_input::DataValueInput;
pub use execution_plan::ExecutionPlanSet;
pub use execution_plan::{DataOverlay, ExecutionPlan, SpecSchema};
use indexmap::IndexMap;
pub use spec_set::LemmaSpecSet;
use std::sync::Arc;

/// Result of planning a single `LemmaSpec`.
#[derive(Debug, Clone)]
pub struct SpecPlanningResult {
    pub spec: std::sync::Arc<crate::parsing::ast::LemmaSpec>,
    pub plans: Vec<ExecutionPlan>,
    pub errors: Vec<Error>,
}

/// Result of planning a `LemmaSpecSet` (all specs sharing a name).
#[derive(Debug, Clone)]
pub struct SpecSetPlanningResult {
    /// Owning repository for all slices in this set.
    pub repository: Arc<LemmaRepository>,
    /// Logical spec name.
    pub name: String,
    pub lemma_spec_set: LemmaSpecSet,
    pub slice_results: Vec<SpecPlanningResult>,
}

impl SpecSetPlanningResult {
    pub fn errors(&self) -> impl Iterator<Item = &Error> {
        self.slice_results.iter().flat_map(|s| s.errors.iter())
    }

    pub fn execution_plan_set(&self) -> ExecutionPlanSet {
        ExecutionPlanSet {
            spec_name: self.name.clone(),
            plans: self
                .slice_results
                .iter()
                .flat_map(|s| s.plans.clone())
                .collect(),
        }
    }

    /// The interface this set exposes over `[from, to)`, or `None` if any two
    /// LemmaSpec slices in range disagree on the type of a name they both
    /// expose. All in-range slices are folded into one unified surface
    /// (name → type): a name must have the same type in every slice that
    /// exposes it, even when intermediate slices do not expose the name —
    /// pairwise adjacent comparison would not be transitive. The returned
    /// schema is the first in-range slice's full-surface schema.
    pub fn schema_over(
        &self,
        from: &Option<DateTimeValue>,
        to: &Option<DateTimeValue>,
    ) -> Option<SpecSchema> {
        let schemas: Vec<SpecSchema> = self
            .slice_results
            .iter()
            .filter(|sr| {
                let (slice_from, slice_to) = self.lemma_spec_set.effective_range(&sr.spec);
                ranges_overlap(from, to, &slice_from, &slice_to)
            })
            .filter_map(|sr| {
                sr.plans
                    .first()
                    .map(|p| p.interface_schema(&DataOverlay::default()))
            })
            .collect();

        let first = schemas.first()?;

        let mut data_types: std::collections::HashMap<
            &str,
            &crate::planning::semantics::LemmaType,
        > = std::collections::HashMap::new();
        let mut rule_types: std::collections::HashMap<
            &str,
            &crate::planning::semantics::LemmaType,
        > = std::collections::HashMap::new();
        for schema in &schemas {
            for (name, entry) in &schema.data {
                match data_types.get(name.as_str()) {
                    Some(existing) if **existing != entry.lemma_type => return None,
                    _ => {
                        data_types.insert(name.as_str(), &entry.lemma_type);
                    }
                }
            }
            for (name, lemma_type) in &schema.rules {
                match rule_types.get(name.as_str()) {
                    Some(existing) if *existing != lemma_type => return None,
                    _ => {
                        rule_types.insert(name.as_str(), lemma_type);
                    }
                }
            }
        }

        Some(first.clone())
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

#[derive(Debug, Clone)]
pub struct PlanningResult {
    pub results: Vec<SpecSetPlanningResult>,
}

/// Build execution plans for one or more Lemma specs.
///
/// Iterates every spec, filters effective dates to its validity range,
/// builds a per-spec DAG and ExecutionPlan for each slice.
pub fn plan(context: &Context, limits: &crate::limits::ResourceLimits) -> PlanningResult {
    let mut results: IndexMap<Arc<LemmaRepository>, IndexMap<String, SpecSetPlanningResult>> =
        IndexMap::new();

    for (repository, inner) in context.repositories().iter() {
        for (_name, lemma_spec_set) in inner.iter() {
            for spec in lemma_spec_set.iter_specs() {
                plan_spec(
                    context,
                    repository,
                    lemma_spec_set,
                    &spec,
                    limits,
                    &mut results,
                );
            }
        }
    }

    for (consumer_repository, spec_name, err) in
        discovery::validate_dependency_interfaces(context, &results)
    {
        let set_result = results
            .get_mut(&consumer_repository)
            .and_then(|by_name| by_name.get_mut(&spec_name))
            .expect("BUG: validate_dependency_interfaces returned error for absent spec set");
        let first_spec = set_result
            .slice_results
            .first_mut()
            .expect("planning result must contain at least one spec");
        first_spec.errors.push(err);
    }

    for by_name in results.values_mut() {
        for set_result in by_name.values_mut() {
            for spec_result in &mut set_result.slice_results {
                dedup_errors(&mut spec_result.errors);
            }
        }
    }

    PlanningResult {
        results: results
            .into_values()
            .flat_map(|by_name| by_name.into_values())
            .collect(),
    }
}

fn plan_spec(
    context: &Context,
    repository: &Arc<LemmaRepository>,
    lemma_spec_set: &LemmaSpecSet,
    spec: &Arc<LemmaSpec>,
    limits: &crate::limits::ResourceLimits,
    results: &mut IndexMap<Arc<LemmaRepository>, IndexMap<String, SpecSetPlanningResult>>,
) {
    let spec_name = &spec.name;

    let mut spec_result = SpecPlanningResult {
        spec: Arc::clone(spec),
        plans: Vec::new(),
        errors: Vec::new(),
    };

    for effective in lemma_spec_set.effective_dates(spec, context) {
        let (dag, dependency_discovery_failed) =
            match discovery::build_dag_for_spec(context, spec, &effective) {
                Ok(dag) => (dag, false),
                Err(discovery::DagError::Cycle(errors)) => {
                    spec_result.errors.extend(errors);
                    continue;
                }
                Err(discovery::DagError::Other(errors)) => {
                    spec_result.errors.extend(errors);
                    (vec![(Arc::clone(repository), Arc::clone(spec))], true)
                }
            };

        match graph::Graph::build(
            context,
            repository,
            spec,
            &dag,
            &effective,
            dependency_discovery_failed,
        ) {
            Ok((graph, mut slice_types)) => {
                match execution_plan::build_execution_plan(
                    &graph,
                    &mut slice_types,
                    &effective,
                    limits,
                ) {
                    Ok(execution_plan) => {
                        let mut plan_errors =
                            execution_plan::validate_unit_index_references(&execution_plan)
                                .err()
                                .into_iter()
                                .collect::<Vec<_>>();
                        plan_errors.extend(execution_plan::validate_literal_data_against_types(
                            &execution_plan,
                        ));
                        if plan_errors.is_empty() {
                            spec_result.plans.push(execution_plan);
                        } else {
                            spec_result.errors.extend(plan_errors);
                        }
                    }
                    Err(plan_errors) => {
                        spec_result.errors.extend(plan_errors);
                    }
                }
            }
            Err(build_errors) => {
                spec_result.errors.extend(build_errors);
            }
        }
    }

    if !spec_result.plans.is_empty() || !spec_result.errors.is_empty() {
        let entry = results
            .entry(Arc::clone(repository))
            .or_default()
            .entry(spec_name.clone())
            .or_insert_with(|| SpecSetPlanningResult {
                repository: Arc::clone(repository),
                name: spec_name.clone(),
                lemma_spec_set: lemma_spec_set.clone(),
                slice_results: Vec::new(),
            });
        entry.slice_results.push(spec_result);
    }
}

/// Remove duplicate errors in-place, preserving first occurrence order.
/// Two errors are considered duplicates when they share the same kind,
/// message, and source location.
fn dedup_errors(errors: &mut Vec<Error>) {
    let mut seen = std::collections::HashSet::new();
    errors.retain(|error| {
        let key = (
            error.kind(),
            error.message().to_string(),
            error.location().cloned(),
        );
        seen.insert(key)
    });
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod internal_tests {
    use super::plan;
    use crate::engine::Context;
    use crate::limits::ResourceLimits;
    use crate::literals::DateGranularity;
    use crate::parsing::ast::{
        DataValue, LemmaData, LemmaRepository, LemmaSpec, ParentType, Reference, Span,
    };
    use crate::parsing::source::Source;
    use crate::planning::execution_plan::ExecutionPlan;
    use crate::planning::semantics::{DataPath, PathSegment, TypeDefiningSpec, TypeExtends};
    use crate::{parse, Error};
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Test helper: plan a single spec and return its execution plan.
    fn plan_single(
        main_spec: &LemmaSpec,
        all_specs: &[LemmaSpec],
    ) -> Result<ExecutionPlan, Vec<Error>> {
        let mut ctx = Context::new();
        let repository = ctx.workspace();
        for spec in all_specs {
            if let Err(e) = ctx.insert_spec(Arc::clone(&repository), Arc::new(spec.clone())) {
                return Err(vec![e]);
            }
        }
        let main_spec_arc = ctx
            .spec_set(&repository, main_spec.name.as_str())
            .and_then(|ss| ss.get_exact(main_spec.effective_from()).cloned())
            .expect("main_spec must be in all_specs");
        let result = plan(&ctx, &ResourceLimits::default());
        let all_errors: Vec<Error> = result
            .results
            .iter()
            .flat_map(|r| r.errors().cloned())
            .collect();
        if !all_errors.is_empty() {
            return Err(all_errors);
        }
        match result
            .results
            .into_iter()
            .find(|r| r.name == main_spec_arc.name)
        {
            Some(spec_result) => {
                let plan_set = spec_result.execution_plan_set();
                if plan_set.plans.is_empty() {
                    Err(vec![Error::validation(
                        format!("No execution plan produced for spec '{}'", main_spec.name),
                        Some(crate::planning::semantics::Source::new(
                            crate::parsing::source::SourceType::Volatile,
                            crate::planning::semantics::Span {
                                start: 0,
                                end: 0,
                                line: 1,
                                col: 0,
                            },
                        )),
                        None::<String>,
                    )])
                } else {
                    let mut plans = plan_set.plans;
                    Ok(plans.remove(0))
                }
            }
            None => Err(vec![Error::validation(
                format!("No execution plan produced for spec '{}'", main_spec.name),
                Some(crate::planning::semantics::Source::new(
                    crate::parsing::source::SourceType::Volatile,
                    crate::planning::semantics::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                )),
                None::<String>,
            )]),
        }
    }

    #[test]
    fn test_basic_validation() {
        let input = r#"spec person
data name: "John"
data age: 25
rule is_adult: age >= 18"#;

        let specs: Vec<_> = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            input.to_string(),
        );

        for spec in &specs {
            let result = plan_single(spec, &specs);
            assert!(
                result.is_ok(),
                "Basic validation should pass: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn test_duplicate_data() {
        let input = r#"spec person
data name: "John"
data name: "Jane""#;

        let specs: Vec<_> = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            input.to_string(),
        );

        let result = plan_single(&specs[0], &specs);

        assert!(
            result.is_err(),
            "Duplicate data should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("already used"),
            "Error should mention duplicate data: {}",
            error_string
        );
        assert!(error_string.contains("name"));
    }

    #[test]
    fn mixed_type_range_literal_is_planning_error_not_panic() {
        let input = r#"spec demo
data x: 1 ... yes"#;

        let specs: Vec<_> = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let result = plan_single(&specs[0], &specs);

        let errors = result.expect_err("mixed-type range literal must be a planning error");
        assert_eq!(
            errors.len(),
            1,
            "expected exactly one planning error, got: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
        let error_string = errors[0].to_string();
        assert!(
            error_string.contains(
                "range endpoints must have the same supported base type, got number and boolean"
            ),
            "unexpected error message: {}",
            error_string
        );
    }

    #[test]
    fn text_range_literal_is_planning_error_not_panic() {
        let input = r#"spec demo
data x: "a" ... "b""#;

        let specs: Vec<_> = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let result = plan_single(&specs[0], &specs);

        let errors = result.expect_err("text range literal must be a planning error");
        assert_eq!(
            errors.len(),
            1,
            "expected exactly one planning error, got: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        );
        let error_string = errors[0].to_string();
        assert!(
            error_string.contains(
                "range endpoints must have the same supported base type, got text and text"
            ),
            "unexpected error message: {}",
            error_string
        );
    }

    #[test]
    fn qualified_type_from_spec_with_type_errors_is_planning_error_not_panic() {
        let input = r#"spec b
data money: number -> minimum 10 -> maximum 5

spec a
uses b
data x: b.money"#;

        let specs: Vec<_> = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let result = plan_single(&specs[0], &specs);

        let errors = result.expect_err("failing import target must be a planning error");
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("minimum"),
            "expected the import target's own type error to be reported: {}",
            error_string
        );
        assert!(
            error_string.contains(
                "Cannot resolve type 'money' from spec 'b' (via import 'b'): spec 'b' failed type resolution"
            ),
            "expected the consumer's qualified type resolution error to be reported: {}",
            error_string
        );
    }

    #[test]
    fn test_duplicate_rules() {
        let input = r#"spec person
data age: 25
rule is_adult: age >= 18
rule is_adult: age >= 21"#;

        let specs: Vec<_> = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            input.to_string(),
        );

        let result = plan_single(&specs[0], &specs);

        assert!(
            result.is_err(),
            "Duplicate rules should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("Duplicate rule"),
            "Error should mention duplicate rule: {}",
            error_string
        );
        assert!(error_string.contains("is_adult"));
    }

    #[test]
    fn test_circular_dependency() {
        let input = r#"spec test
rule a: b
rule b: a"#;

        let specs: Vec<_> = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            input.to_string(),
        );

        let result = plan_single(&specs[0], &specs);

        assert!(
            result.is_err(),
            "Circular dependency should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(error_string.contains("Circular dependency") || error_string.contains("circular"));
    }

    #[test]
    fn test_multiple_specs() {
        let input = r#"spec person
data name: "John"
data age: 25

spec company
data name: "Acme Corp"
uses employee: person"#;

        let specs: Vec<_> = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            input.to_string(),
        );

        let result = plan_single(&specs[0], &specs);

        assert!(
            result.is_ok(),
            "Multiple specs should validate successfully: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_invalid_spec_reference() {
        let input = r#"spec person
data name: "John"
uses contract: nonexistent"#;

        let specs: Vec<_> = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            input.to_string(),
        );

        let result = plan_single(&specs[0], &specs);

        assert!(
            result.is_err(),
            "Invalid spec reference should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("not found")
                || error_string.contains("Spec")
                || (error_string.contains("nonexistent") && error_string.contains("depends")),
            "Error should mention spec reference issue: {}",
            error_string
        );
        assert!(error_string.contains("nonexistent"));
    }

    #[test]
    fn test_definition_empty_base_returns_lemma_error() {
        let mut spec = LemmaSpec::new("test".to_string());
        let source = Source::new(
            crate::parsing::source::SourceType::Volatile,
            Span {
                start: 0,
                end: 10,
                line: 1,
                col: 0,
            },
        );
        spec.data.push(LemmaData::new(
            Reference {
                segments: vec![],
                name: "x".to_string(),
            },
            DataValue::Definition {
                base: Some(ParentType::Custom {
                    name: String::new(),
                }),
                constraints: None,
                value: None,
            },
            source,
        ));

        let specs = vec![spec.clone()];
        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            "spec test\ndata x:".to_string(),
        );

        let result = plan_single(&spec, &specs);
        assert!(
            result.is_err(),
            "Definition with empty base should fail planning"
        );
        let errors = result.unwrap_err();
        let combined = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            combined.contains("Unknown parent ''"),
            "Error should mention empty/unknown type; got: {}",
            combined
        );
    }

    #[test]
    fn test_data_binding_with_custom_type_resolves_in_correct_spec_context() {
        // This is a planning-level test: ensure data bindings resolve custom types correctly
        // when the type is defined in a different spec than the binding.
        //
        // spec one:
        //   data money: number
        //   data x: money
        // spec two:
        //   with one
        //   with one.x: 7
        //   rule getx: one.x
        let code = r#"
spec one
data money: number
data x: money

spec two
uses one
with one.x: 7
rule getx: one.x
"#;

        let specs = parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let spec_two = specs.iter().find(|d| d.name == "two").unwrap();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            code.to_string(),
        );
        let execution_plan = plan_single(spec_two, &specs).expect("planning should succeed");

        // Verify that one.x keeps its declared custom type name while resolving in spec one.
        let one_x_path = DataPath {
            segments: vec![PathSegment {
                data: "one".to_string(),
                spec: "one".to_string(),
            }],
            data: "x".to_string(),
        };

        let one_x_type = execution_plan
            .data
            .get(&one_x_path)
            .and_then(|d| d.schema_type())
            .expect("one.x should have a resolved type");

        assert_eq!(
            one_x_type.name(),
            "x",
            "one.x should have declared type 'x', got: {}",
            one_x_type.name()
        );
        assert!(one_x_type.is_number(), "money should be number-based");
    }

    #[test]
    fn test_data_definition_from_spec_has_import_defining_spec() {
        let code = r#"
spec examples
data money: quantity
  -> unit eur 1.00

spec checkout
uses examples
data money: quantity
  -> unit eur 1.00
data local_price: money
data imported_price: examples.money
"#;

        let specs = parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut ctx = Context::new();
        let repository = ctx.workspace();
        for spec in &specs {
            ctx.insert_spec(Arc::clone(&repository), Arc::new(spec.clone()))
                .expect("insert spec");
        }

        let examples_arc = ctx
            .spec_set(&repository, "examples")
            .and_then(|ss| ss.get_exact(None).cloned())
            .expect("examples spec should be present");
        let checkout_arc = ctx
            .spec_set(&repository, "checkout")
            .and_then(|ss| ss.get_exact(None).cloned())
            .expect("checkout spec should be present");

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            code.to_string(),
        );

        let result = plan(&ctx, &ResourceLimits::default());

        let checkout_result = result
            .results
            .iter()
            .find(|r| r.name == checkout_arc.name)
            .expect("checkout result should exist");
        let checkout_errors: Vec<_> = checkout_result.errors().collect();
        assert!(
            checkout_errors.is_empty(),
            "No checkout planning errors expected, got: {:?}",
            checkout_errors
        );
        let checkout_plans = checkout_result.execution_plan_set();
        assert!(
            !checkout_plans.plans.is_empty(),
            "checkout should produce at least one plan"
        );
        let execution_plan = &checkout_plans.plans[0];

        let local_type = execution_plan
            .data
            .get(&DataPath::new(vec![], "local_price".to_string()))
            .and_then(|d| d.schema_type())
            .expect("local_price should have schema type");
        let imported_type = execution_plan
            .data
            .get(&DataPath::new(vec![], "imported_price".to_string()))
            .and_then(|d| d.schema_type())
            .expect("imported_price should have schema type");

        match &local_type.extends {
            TypeExtends::Custom {
                defining_spec: TypeDefiningSpec::Local,
                ..
            } => {}
            other => panic!(
                "local_price should resolve as local defining_spec, got {:?}",
                other
            ),
        }

        match &imported_type.extends {
            TypeExtends::Custom {
                defining_spec: TypeDefiningSpec::Import { spec, .. },
                ..
            } => {
                assert!(
                    Arc::ptr_eq(spec, &examples_arc),
                    "imported_price should point to resolved 'examples' spec arc"
                );
            }
            other => panic!(
                "imported_price should resolve as import defining_spec, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_plan_with_registry_grouped_specs() {
        let source = r#"spec somespec
data quantity: 10

spec example
uses inventory: somespec
rule total_quantity: inventory.quantity"#;

        let parsed = parse(
            source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.flatten_specs().len(), 2);

        let mut ctx = Context::new();
        let repository = Arc::new(
            LemmaRepository::new(Some("@user/workspace".to_string()))
                .with_dependency("@user/workspace")
                .with_start_line(1)
                .with_source_type(crate::parsing::source::SourceType::Volatile),
        );
        for spec in parsed.flatten_specs() {
            ctx.insert_spec(Arc::clone(&repository), Arc::new(spec.clone()))
                .expect("insert spec");
        }

        let result = plan(&ctx, &ResourceLimits::default());
        let example_result = result
            .results
            .iter()
            .find(|r| r.name == "example")
            .expect("example result must exist");
        let errors: Vec<_> = example_result.errors().collect();
        assert!(
            errors.is_empty(),
            "Planning under registry-scoped specs should succeed: {:?}",
            errors
        );
        assert!(
            !example_result.execution_plan_set().plans.is_empty(),
            "expected at least one plan for registry-grouped example"
        );
    }

    #[test]
    fn test_multiple_independent_errors_are_all_reported() {
        // A spec referencing a non-existing import AND a non-existing
        // spec should report errors for BOTH, not just stop at the first.
        let source = r#"spec demo
uses type_src: nonexistent_type_source
with type_src.amount: 10
uses helper: nonexistent_spec
data price: 10
rule total: helper.value + price"#;

        let specs = parse(
            source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            source.to_string(),
        );

        let result = plan_single(&specs[0], &specs);
        assert!(result.is_err(), "Planning should fail with multiple errors");

        let errors = result.unwrap_err();
        let all_messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let combined = all_messages.join("\n");

        assert!(
            combined.contains("nonexistent_type_source"),
            "Should report import error for 'nonexistent_type_source'. Got:\n{}",
            combined
        );

        // Must also report the spec reference error (not just the import error)
        assert!(
            combined.contains("nonexistent_spec"),
            "Should report spec reference error for 'nonexistent_spec'. Got:\n{}",
            combined
        );

        // Should have at least 2 distinct kinds of errors (import + spec ref)
        assert!(
            errors.len() >= 2,
            "Expected at least 2 errors, got {}: {}",
            errors.len(),
            combined
        );

        let data_import_err = errors
            .iter()
            .find(|e| e.to_string().contains("nonexistent_type_source"))
            .expect("import error");
        let loc = data_import_err
            .location()
            .expect("import error should carry source location");
        assert_eq!(
            loc.source_type,
            crate::parsing::source::SourceType::Volatile
        );
        assert_ne!(
            (loc.span.start, loc.span.end),
            (0, 0),
            "import error span should not be empty"
        );
    }

    #[test]
    fn test_type_error_does_not_suppress_cross_spec_data_error() {
        // When a import fails, errors about cross-spec data references
        // (e.g. ext.some_data where ext is a spec ref to a non-existing spec)
        // must still be reported.
        let source = r#"spec demo
uses cur: missing_spec
with cur.currency: 10
uses ext: also_missing
rule val: ext.some_data"#;

        let specs = parse(
            source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            source.to_string(),
        );

        let result = plan_single(&specs[0], &specs);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        let combined: String = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            combined.contains("missing_spec"),
            "Should report import error about 'missing_spec'. Got:\n{}",
            combined
        );

        // The spec reference error about 'also_missing' should ALSO be reported
        assert!(
            combined.contains("also_missing"),
            "Should report error about 'also_missing'. Got:\n{}",
            combined
        );
    }

    #[test]
    fn test_spec_dag_orders_dep_before_consumer() {
        let source = r#"spec dep 2025-01-01
data money: number
data x: money

spec consumer 2025-01-01
uses dep
data imported_amount: dep.money
rule passthrough: imported_amount"#;
        let specs = parse(
            source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut ctx = Context::new();
        let repository = ctx.workspace();
        for spec in &specs {
            ctx.insert_spec(Arc::clone(&repository), Arc::new(spec.clone()))
                .expect("insert spec");
        }

        let dt = crate::DateTimeValue {
            year: 2025,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
            granularity: DateGranularity::Full,
        };
        let effective = crate::parsing::ast::EffectiveDate::DateTimeValue(dt);
        let consumer_arc = ctx
            .spec_set(&repository, "consumer")
            .and_then(|ss| ss.spec_at(&effective))
            .expect("consumer spec");
        let dag = super::discovery::build_dag_for_spec(&ctx, &consumer_arc, &effective)
            .expect("DAG should succeed");
        let ordered_names: Vec<String> = dag.iter().map(|s| s.1.name.clone()).collect();
        let dep_idx = ordered_names
            .iter()
            .position(|n| n == "dep")
            .expect("dep must exist");
        let consumer_idx = ordered_names
            .iter()
            .position(|n| n == "consumer")
            .expect("consumer must exist");
        assert!(
            dep_idx < consumer_idx,
            "dependency must be planned before dependent. order={:?}",
            ordered_names
        );
    }

    #[test]
    fn test_spec_dependency_cycle_surfaces_as_spec_error_and_populates_results() {
        let source = r#"spec a 2025-01-01
uses dep_b: b
data amount: number

spec b 2025-01-01
uses src_a: a
data imported_value: src_a.amount
"#;
        let specs = parse(
            source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut ctx = Context::new();
        let repository = ctx.workspace();
        for spec in &specs {
            ctx.insert_spec(Arc::clone(&repository), Arc::new(spec.clone()))
                .expect("insert spec");
        }

        let result = plan(&ctx, &ResourceLimits::default());

        let spec_errors: Vec<String> = result
            .results
            .iter()
            .flat_map(|r| r.errors())
            .map(|e| e.to_string())
            .collect();
        assert!(
            spec_errors
                .iter()
                .any(|e| e.contains("Spec dependency cycle")),
            "expected cycle error on spec, got: {spec_errors:?}",
        );

        assert!(
            result.results.iter().any(|r| r.name == "b"),
            "cyclic spec 'b' must still have an entry in results so downstream invariants hold"
        );
    }

    // ========================================================================
    // Source transparency
    // ========================================================================

    fn has_source_for(plan: &super::execution_plan::ExecutionPlan, name: &str) -> bool {
        plan.sources.iter().any(|e| e.name == name)
    }

    #[test]
    fn sources_contain_main_and_dep_for_cross_spec_rule_reference() {
        let code = r#"
spec dep
data x: 10
rule val: x

spec consumer
uses d: dep
with d.x: 5
rule result: d.val
"#;
        let specs = parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let consumer = specs.iter().find(|s| s.name == "consumer").unwrap();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            code.to_string(),
        );

        let plan = plan_single(consumer, &specs).expect("planning should succeed");

        assert_eq!(plan.sources.len(), 2, "main + dep, got: {:?}", plan.sources);
        assert!(
            has_source_for(&plan, "consumer"),
            "sources must include main spec"
        );
        assert!(
            has_source_for(&plan, "dep"),
            "sources must include dep spec"
        );
    }

    #[test]
    fn sources_contain_only_main_for_standalone_spec() {
        let code = r#"
spec standalone
data age: 25
rule is_adult: age >= 18
"#;
        let specs = parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            code.to_string(),
        );

        let plan = plan_single(&specs[0], &specs).expect("planning should succeed");

        assert_eq!(
            plan.sources.len(),
            1,
            "standalone should have only main spec"
        );
        assert!(has_source_for(&plan, "standalone"));
    }

    #[test]
    fn sources_contain_all_cross_spec_refs() {
        let code = r#"
spec rates
data base_rate: 0.05
rule rate: base_rate

spec config
data threshold: 100
rule limit: threshold

spec calculator
uses r: rates
with r.base_rate: 0.03
uses c: config
with c.threshold: 200
rule combined: r.rate + c.limit
"#;
        let specs = parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let calc = specs.iter().find(|s| s.name == "calculator").unwrap();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            code.to_string(),
        );

        let plan = plan_single(calc, &specs).expect("planning should succeed");

        assert_eq!(
            plan.sources.len(),
            3,
            "calculator + rates + config, got: {:?}",
            plan.sources
        );
        assert!(has_source_for(&plan, "calculator"));
        assert!(has_source_for(&plan, "rates"));
        assert!(has_source_for(&plan, "config"));
    }

    #[test]
    fn sources_include_spec_ref_even_without_rules() {
        let code = r#"
spec dep
data x: 10

spec consumer
uses d: dep
data local: 99
rule result: local
"#;
        let specs = parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let consumer = specs.iter().find(|s| s.name == "consumer").unwrap();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            code.to_string(),
        );

        let plan = plan_single(consumer, &specs).expect("planning should succeed");

        assert_eq!(
            plan.sources.len(),
            2,
            "consumer + dep, got: {:?}",
            plan.sources
        );
        assert!(
            has_source_for(&plan, "dep"),
            "spec ref dep must be in sources even without rules"
        );
    }

    #[test]
    fn sources_round_trip_to_valid_specs() {
        let code = r#"
spec dep
data x: 42
rule val: x

spec consumer
uses d: dep
rule result: d.val
"#;
        let specs = parse(
            code,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let consumer = specs.iter().find(|s| s.name == "consumer").unwrap();

        let mut sources = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            code.to_string(),
        );

        let plan = plan_single(consumer, &specs).expect("planning should succeed");

        for super::execution_plan::SpecSource {
            name,
            source: source_text,
            ..
        } in &plan.sources
        {
            let parsed = parse(
                source_text,
                crate::parsing::source::SourceType::Volatile,
                &ResourceLimits::default(),
            );
            assert!(
                parsed.is_ok(),
                "source for '{}' must re-parse: {:?}\nsource:\n{}",
                name,
                parsed.err(),
                source_text
            );
        }
    }
}
