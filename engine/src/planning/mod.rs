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

pub mod discovery;
pub mod execution_plan;
pub mod graph;
pub mod semantics;
pub mod spec_set;
use crate::engine::Context;
use crate::parsing::ast::DateTimeValue;
use crate::Error;
pub use execution_plan::ExecutionPlanSet;
pub use execution_plan::{Branch, ExecutableRule, ExecutionPlan, SpecSchema};
pub use semantics::{
    is_same_spec, negated_comparison, ArithmeticComputation, ComparisonComputation, Data,
    DataDefinition, DataPath, DataValue, Expression, ExpressionKind, LemmaType, LiteralValue,
    LogicalComputation, MathematicalComputation, NegationType, PathSegment, RulePath, Source, Span,
    TypeDefiningSpec, TypeExtends, ValueKind, VetoExpression,
};
pub use spec_set::LemmaSpecSet;
use std::collections::BTreeMap;

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
    /// Logical spec name.
    pub name: String,
    pub lemma_spec_set: LemmaSpecSet,
    pub specs: Vec<SpecPlanningResult>,
}

impl SpecSetPlanningResult {
    pub fn errors(&self) -> impl Iterator<Item = &Error> {
        self.specs.iter().flat_map(|s| s.errors.iter())
    }

    pub fn execution_plan_set(&self) -> ExecutionPlanSet {
        ExecutionPlanSet {
            spec_name: self.name.clone(),
            plans: self.specs.iter().flat_map(|s| s.plans.clone()).collect(),
        }
    }

    /// The interface this set exposes over `[from, to)`, or `None` if any two
    /// overlapping LemmaSpec slices disagree on the type of a name they both
    /// expose. The returned schema is one of the in-range slices' full-surface
    /// schemas; all of them are type-compatible when `Some` is returned.
    pub fn schema_over(
        &self,
        from: &Option<DateTimeValue>,
        to: &Option<DateTimeValue>,
    ) -> Option<SpecSchema> {
        let schemas: Vec<SpecSchema> = self
            .specs
            .iter()
            .filter(|sr| {
                let (slice_from, slice_to) = self.lemma_spec_set.effective_range(&sr.spec);
                ranges_overlap(from, to, &slice_from, &slice_to)
            })
            .filter_map(|sr| sr.plans.first().map(|p| p.interface_schema()))
            .collect();

        let first = schemas.first()?;
        for pair in schemas.windows(2) {
            if !pair[0].is_type_compatible(&pair[1]) {
                return None;
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
pub fn plan(context: &Context) -> PlanningResult {
    let mut results: BTreeMap<String, SpecSetPlanningResult> = BTreeMap::new();

    for spec in context.iter() {
        let spec_name = &spec.name;
        let lemma_spec_set = context
            .spec_sets()
            .get(spec_name)
            .expect("spec not found in context");

        let mut spec_result = SpecPlanningResult {
            spec: std::sync::Arc::clone(&spec),
            plans: Vec::new(),
            errors: Vec::new(),
        };

        for effective in lemma_spec_set.effective_dates(&spec, context) {
            let dag = match discovery::build_dag_for_spec(context, &spec, &effective) {
                Ok(dag) => dag,
                Err(discovery::DagError::Cycle(errors)) => {
                    spec_result.errors.extend(errors);
                    continue;
                }
                Err(discovery::DagError::Other(errors)) => {
                    spec_result.errors.extend(errors);
                    continue;
                }
            };

            match graph::Graph::build(context, &spec, &dag, &effective) {
                Ok((graph, slice_types)) => {
                    let execution_plan =
                        execution_plan::build_execution_plan(&graph, &slice_types, &effective);
                    let value_errors =
                        execution_plan::validate_literal_data_against_types(&execution_plan);
                    spec_result.errors.extend(value_errors);
                    spec_result.plans.push(execution_plan);
                }
                Err(build_errors) => {
                    spec_result.errors.extend(build_errors);
                }
            }
        }

        if !spec_result.plans.is_empty() || !spec_result.errors.is_empty() {
            let entry = results
                .entry(spec_name.clone())
                .or_insert_with(|| SpecSetPlanningResult {
                    name: spec_name.clone(),
                    lemma_spec_set: lemma_spec_set.clone(),
                    specs: Vec::new(),
                });
            entry.specs.push(spec_result);
        }
    }

    for (spec_name, err) in discovery::validate_dependency_interfaces(context, &results) {
        let set_result = results
            .get_mut(&spec_name)
            .expect("BUG: validate_dependency_interfaces returned error for absent spec set");
        let first_spec = set_result
            .specs
            .first_mut()
            .expect("BUG: spec set has no specs to attach error to");
        first_spec.errors.push(err);
    }

    PlanningResult {
        results: results.into_values().collect(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod internal_tests {
    use super::plan;
    use crate::engine::Context;
    use crate::parsing::ast::{DataValue, LemmaData, LemmaSpec, ParentType, Reference, Span};
    use crate::parsing::source::Source;
    use crate::planning::execution_plan::ExecutionPlan;
    use crate::planning::semantics::{DataPath, PathSegment, TypeDefiningSpec, TypeExtends};
    use crate::{parse, Error, ResourceLimits};
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Test helper: plan a single spec and return its execution plan.
    fn plan_single(
        main_spec: &LemmaSpec,
        all_specs: &[LemmaSpec],
    ) -> Result<ExecutionPlan, Vec<Error>> {
        let mut ctx = Context::new();
        for spec in all_specs {
            if let Err(e) = ctx.insert_spec(Arc::new(spec.clone()), spec.from_registry) {
                return Err(vec![e]);
            }
        }
        let main_spec_arc = ctx
            .spec_sets()
            .get(main_spec.name.as_str())
            .and_then(|ss| ss.get_exact(main_spec.effective_from()).cloned())
            .expect("main_spec must be in all_specs");
        let result = plan(&ctx);
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
                            "<test>",
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
                    "<test>",
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

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

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

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

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
            error_string.contains("Duplicate data"),
            "Error should mention duplicate data: {}",
            error_string
        );
        assert!(error_string.contains("name"));
    }

    #[test]
    fn test_duplicate_rules() {
        let input = r#"spec person
data age: 25
rule is_adult: age >= 18
rule is_adult: age >= 21"#;

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

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

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

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
with employee: person"#;

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

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
with contract: nonexistent"#;

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

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
    fn test_type_declaration_empty_base_returns_lemma_error() {
        let mut spec = LemmaSpec::new("test".to_string());
        let source = Source::new(
            "test.lemma",
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
            DataValue::TypeDeclaration {
                base: ParentType::Custom {
                    name: String::new(),
                },
                constraints: None,
                from: None,
            },
            source,
        ));

        let specs = vec![spec.clone()];
        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), "spec test\ndata x:".to_string());

        let result = plan_single(&spec, &specs);
        assert!(
            result.is_err(),
            "TypeDeclaration with empty base should fail planning"
        );
        let errors = result.unwrap_err();
        let combined = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            combined.contains("Unknown type: ''"),
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
        //   data one.x: 7
        //   rule getx: one.x
        let code = r#"
spec one
data money: number
data x: money

spec two
with one
data one.x: 7
rule getx: one.x
"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec_two = specs.iter().find(|d| d.name == "two").unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());
        let execution_plan = plan_single(spec_two, &specs).expect("planning should succeed");

        // Verify that one.x has type 'money' (resolved from spec one)
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
            "money",
            "one.x should have type 'money', got: {}",
            one_x_type.name()
        );
        assert!(one_x_type.is_number(), "money should be number-based");
    }

    #[test]
    fn test_data_type_declaration_from_spec_has_import_defining_spec() {
        let code = r#"
spec examples
data money: scale
  -> unit eur 1.00

spec checkout
data money: scale
  -> unit eur 1.00
data local_price: money
data imported_price: money from examples
"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut ctx = Context::new();
        for spec in &specs {
            ctx.insert_spec(Arc::new(spec.clone()), spec.from_registry)
                .expect("insert spec");
        }

        let examples_arc = ctx
            .spec_sets()
            .get("examples")
            .and_then(|ss| ss.get_exact(None).cloned())
            .expect("examples spec should be present");
        let checkout_arc = ctx
            .spec_sets()
            .get("checkout")
            .and_then(|ss| ss.get_exact(None).cloned())
            .expect("checkout spec should be present");

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());

        let result = plan(&ctx);

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
    fn test_plan_with_registry_style_spec_names() {
        let source = r#"spec @user/workspace/somespec
data quantity: 10

spec user/workspace/example
with inventory: @user/workspace/somespec
rule total_quantity: inventory.quantity"#;

        let specs = parse(source, "registry_bundle.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(specs.len(), 2);

        let example_spec = specs
            .iter()
            .find(|d| d.name == "user/workspace/example")
            .expect("should find user/workspace/example");

        let mut sources = HashMap::new();
        sources.insert("registry_bundle.lemma".to_string(), source.to_string());

        let result = plan_single(example_spec, &specs);
        assert!(
            result.is_ok(),
            "Planning with @... spec names should succeed: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_multiple_independent_errors_are_all_reported() {
        // A spec referencing a non-existing type import AND a non-existing
        // spec should report errors for BOTH, not just stop at the first.
        let source = r#"spec demo
data money from nonexistent_type_source
with helper: nonexistent_spec
data price: 10
rule total: helper.value + price"#;

        let specs = parse(source, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), source.to_string());

        let result = plan_single(&specs[0], &specs);
        assert!(result.is_err(), "Planning should fail with multiple errors");

        let errors = result.unwrap_err();
        let all_messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let combined = all_messages.join("\n");

        assert!(
            combined.contains("nonexistent_type_source"),
            "Should report type import error for 'nonexistent_type_source'. Got:\n{}",
            combined
        );

        // Must also report the spec reference error (not just the type error)
        assert!(
            combined.contains("nonexistent_spec"),
            "Should report spec reference error for 'nonexistent_spec'. Got:\n{}",
            combined
        );

        // Should have at least 2 distinct kinds of errors (type + spec ref)
        assert!(
            errors.len() >= 2,
            "Expected at least 2 errors, got {}: {}",
            errors.len(),
            combined
        );

        let type_import_err = errors
            .iter()
            .find(|e| e.to_string().contains("nonexistent_type_source"))
            .expect("type import error");
        let loc = type_import_err
            .location()
            .expect("type import error should carry source location");
        assert_eq!(loc.attribute, "test.lemma");
        assert_ne!(
            (loc.span.start, loc.span.end),
            (0, 0),
            "type import error span should not be empty"
        );
    }

    #[test]
    fn test_type_error_does_not_suppress_cross_spec_data_error() {
        // When a type import fails, errors about cross-spec data references
        // (e.g. ext.some_data where ext is a spec ref to a non-existing spec)
        // must still be reported.
        let source = r#"spec demo
data currency from missing_spec
with ext: also_missing
rule val: ext.some_data"#;

        let specs = parse(source, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), source.to_string());

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
            "Should report type import error about 'missing_spec'. Got:\n{}",
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
data imported_amount: money from dep
rule passthrough: imported_amount"#;
        let specs = parse(source, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut ctx = Context::new();
        for spec in &specs {
            ctx.insert_spec(Arc::new(spec.clone()), spec.from_registry)
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
        };
        let effective = crate::parsing::ast::EffectiveDate::DateTimeValue(dt);
        let consumer_arc = ctx
            .spec_sets()
            .get("consumer")
            .and_then(|ss| ss.spec_at(&effective))
            .expect("consumer spec");
        let dag = super::discovery::build_dag_for_spec(&ctx, &consumer_arc, &effective)
            .expect("DAG should succeed");
        let ordered_names: Vec<String> = dag.iter().map(|s| s.name.clone()).collect();
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
with dep_b: b

spec b 2025-01-01
data imported_value: amount from a
"#;
        let specs = parse(source, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut ctx = Context::new();
        for spec in &specs {
            ctx.insert_spec(Arc::new(spec.clone()), spec.from_registry)
                .expect("insert spec");
        }

        let result = plan(&ctx);

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
        plan.sources.keys().any(|(n, _)| n == name)
    }

    #[test]
    fn sources_contain_main_and_dep_for_cross_spec_rule_reference() {
        let code = r#"
spec dep
data x: 10
rule val: x

spec consumer
with d: dep
data d.x: 5
rule result: d.val
"#;
        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let consumer = specs.iter().find(|s| s.name == "consumer").unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());

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
        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());

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
with r: rates
data r.base_rate: 0.03
with c: config
data c.threshold: 200
rule combined: r.rate + c.limit
"#;
        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let calc = specs.iter().find(|s| s.name == "calculator").unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());

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
with d: dep
data local: 99
rule result: local
"#;
        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let consumer = specs.iter().find(|s| s.name == "consumer").unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());

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
with d: dep
rule result: d.val
"#;
        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let consumer = specs.iter().find(|s| s.name == "consumer").unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());

        let plan = plan_single(consumer, &specs).expect("planning should succeed");

        for ((name, _), source_text) in &plan.sources {
            let parsed = parse(source_text, "roundtrip.lemma", &ResourceLimits::default());
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
