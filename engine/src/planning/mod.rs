//! Planning module for Lemma specs
//!
//! This module performs complete static analysis and builds execution plans:
//! - Builds Graph with facts and rules (validated, with types computed)
//! - Builds ExecutionPlan from Graph (topologically sorted, ready for evaluation)
//! - Validates spec structure and references

pub mod content_hash;
pub mod execution_plan;
pub mod graph;
pub mod semantics;
pub mod slice_interface;
pub mod temporal;
pub mod types;
pub mod validation;
pub use execution_plan::{Branch, ExecutableRule, ExecutionPlan, SpecSchema};
pub use semantics::{
    negated_comparison, ArithmeticComputation, ComparisonComputation, Expression, ExpressionKind,
    Fact, FactData, FactPath, FactValue, LemmaType, LiteralValue, LogicalComputation,
    MathematicalComputation, NegationType, PathSegment, RulePath, Source, Span, TypeExtends,
    ValueKind, VetoExpression,
};
pub use types::TypeResolver;

use crate::engine::Context;
use crate::parsing::ast::LemmaSpec;
use crate::Error;
use std::collections::HashMap;
use std::sync::Arc;

/// Result of planning a single spec: the spec, its execution plans (if any), and errors produced while planning it.
#[derive(Debug, Clone)]
pub struct SpecPlanningResult {
    /// The spec we were planning (the one this result is for).
    pub spec: Arc<LemmaSpec>,
    /// Execution plans for that spec (one per temporal interval; empty if planning failed).
    pub plans: Vec<ExecutionPlan>,
    /// All planning errors produced while planning this spec.
    pub errors: Vec<Error>,
    /// Content hash of this spec (hash pin, 8 lowercase hex chars).
    pub hash_pin: String,
}

/// Result of running plan() across the context: per-spec results and global errors (e.g. temporal coverage).
#[derive(Debug, Clone)]
pub struct PlanningResult {
    /// One result per spec we attempted to plan.
    pub per_spec: Vec<SpecPlanningResult>,
    /// Errors not tied to a single spec (e.g. from validate_temporal_coverage).
    pub global_errors: Vec<Error>,
}

/// Build execution plans for one or more Lemma specs.
///
/// Context is immutable — types are resolved transiently and never stored in
/// Context. The flow:
/// 1. TypeResolver registers + resolves named types → HashMap
/// 2. Per-spec Graph::build augments the HashMap with inline types
/// 3. ExecutionPlan is built from the graph (types baked into facts/rules)
///
/// Returns a PlanningResult: per-spec results (spec, plans, errors) and global errors.
/// When displaying errors, iterate per_spec and for each with non-empty errors output "In spec 'X':" then each error.
pub fn plan(context: &Context, sources: HashMap<String, String>) -> PlanningResult {
    let mut global_errors: Vec<Error> = Vec::new();
    global_errors.extend(temporal::validate_temporal_coverage(context));

    let mut type_resolver = TypeResolver::new();
    let all_specs: Vec<_> = context.iter().collect();
    for spec_arc in &all_specs {
        global_errors.extend(type_resolver.register_all(spec_arc));
    }
    let (mut resolved_types, type_errors) = type_resolver.resolve(all_specs.clone());
    global_errors.extend(type_errors);

    let mut per_spec: Vec<SpecPlanningResult> = Vec::new();

    if !global_errors.is_empty() {
        return PlanningResult {
            per_spec,
            global_errors,
        };
    }

    // Compute content hashes for all specs (own content only for now).
    // TODO: bottom-up transitive hashing once dep resolution order is settled.
    let spec_hashes: graph::SpecContentHashes = all_specs
        .iter()
        .map(|s| (graph::spec_hash_key(s), content_hash::hash_spec(s, &[])))
        .collect();

    for spec_arc in &all_specs {
        let slices = temporal::compute_temporal_slices(spec_arc, context);
        let mut spec_plans: Vec<ExecutionPlan> = Vec::new();
        let mut spec_errors: Vec<Error> = Vec::new();
        let mut slice_resolved_types: Vec<HashMap<Arc<LemmaSpec>, types::ResolvedSpecTypes>> =
            Vec::new();

        for slice in &slices {
            match graph::Graph::build(
                spec_arc,
                context,
                sources.clone(),
                &type_resolver,
                &resolved_types,
                slice.from.clone(),
                &spec_hashes,
            ) {
                Ok((graph, slice_types)) => {
                    for (arc, types) in &slice_types {
                        resolved_types.insert(Arc::clone(arc), types.clone());
                    }
                    let execution_plan = execution_plan::build_execution_plan(
                        &graph,
                        slice.from.clone(),
                        slice.to.clone(),
                    );
                    let value_errors =
                        execution_plan::validate_literal_facts_against_types(&execution_plan);
                    if value_errors.is_empty() {
                        spec_plans.push(execution_plan);
                    } else {
                        spec_errors.extend(value_errors);
                    }
                    slice_resolved_types.push(slice_types);
                }
                Err(build_errors) => {
                    spec_errors.extend(build_errors);
                }
            }
        }

        if spec_errors.is_empty() && spec_plans.len() > 1 {
            spec_errors.extend(slice_interface::validate_slice_interfaces(
                &spec_arc.name,
                &spec_plans,
                &slice_resolved_types,
            ));
        }

        let hash = spec_hashes
            .get(&graph::spec_hash_key(spec_arc))
            .cloned()
            .unwrap_or_else(|| {
                unreachable!("BUG: spec '{}' missing from spec_hashes", spec_arc.name)
            });

        per_spec.push(SpecPlanningResult {
            spec: Arc::clone(spec_arc),
            plans: spec_plans,
            errors: spec_errors,
            hash_pin: hash,
        });
    }

    PlanningResult {
        per_spec,
        global_errors,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod internal_tests {
    use super::plan;
    use crate::engine::Context;
    use crate::parsing::ast::{FactValue, LemmaFact, LemmaSpec, Reference, Span};
    use crate::parsing::source::Source;
    use crate::planning::execution_plan::ExecutionPlan;
    use crate::planning::semantics::{FactPath, PathSegment};
    use crate::{parse, Error, ResourceLimits};
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Test helper: plan a single spec and return its execution plan.
    fn plan_single(
        main_spec: &LemmaSpec,
        all_specs: &[LemmaSpec],
        sources: HashMap<String, String>,
    ) -> Result<ExecutionPlan, Vec<Error>> {
        let mut ctx = Context::new();
        for spec in all_specs {
            if let Err(e) = ctx.insert_spec(Arc::new(spec.clone()), spec.from_registry) {
                return Err(vec![e]);
            }
        }
        let main_spec_arc = ctx
            .get_spec_effective_from(main_spec.name.as_str(), main_spec.effective_from())
            .expect("main_spec must be in all_specs");
        let result = plan(&ctx, sources);
        let all_errors: Vec<Error> = result
            .global_errors
            .into_iter()
            .chain(
                result
                    .per_spec
                    .iter()
                    .flat_map(|r| r.errors.clone())
                    .collect::<Vec<_>>(),
            )
            .collect();
        if !all_errors.is_empty() {
            return Err(all_errors);
        }
        match result
            .per_spec
            .into_iter()
            .find(|r| Arc::ptr_eq(&r.spec, &main_spec_arc))
        {
            Some(spec_result) if !spec_result.plans.is_empty() => {
                let mut plans = spec_result.plans;
                Ok(plans.remove(0))
            }
            _ => Err(vec![Error::validation(
                format!("No execution plan produced for spec '{}'", main_spec.name),
                Some(crate::planning::semantics::Source::new(
                    "<test>",
                    crate::planning::semantics::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    std::sync::Arc::from("spec test\nfact x: 1"),
                )),
                None::<String>,
            )]),
        }
    }

    #[test]
    fn test_basic_validation() {
        let input = r#"spec person
fact name: "John"
fact age: 25
rule is_adult: age >= 18"#;

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        for spec in &specs {
            let result = plan_single(spec, &specs, sources.clone());
            assert!(
                result.is_ok(),
                "Basic validation should pass: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn test_duplicate_facts() {
        let input = r#"spec person
fact name: "John"
fact name: "Jane""#;

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&specs[0], &specs, sources);

        assert!(
            result.is_err(),
            "Duplicate facts should cause validation error"
        );
        let errors = result.unwrap_err();
        let error_string = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            error_string.contains("Duplicate fact"),
            "Error should mention duplicate fact: {}",
            error_string
        );
        assert!(error_string.contains("name"));
    }

    #[test]
    fn test_duplicate_rules() {
        let input = r#"spec person
fact age: 25
rule is_adult: age >= 18
rule is_adult: age >= 21"#;

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&specs[0], &specs, sources);

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

        let result = plan_single(&specs[0], &specs, sources);

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
    fn test_unified_references_work() {
        let input = r#"spec test
fact age: 25
rule is_adult: age >= 18
rule test1: age
rule test2: is_adult"#;

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&specs[0], &specs, sources);

        assert!(
            result.is_ok(),
            "Unified references should work: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_multiple_specs() {
        let input = r#"spec person
fact name: "John"
fact age: 25

spec company
fact name: "Acme Corp"
fact employee: spec person"#;

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&specs[0], &specs, sources);

        assert!(
            result.is_ok(),
            "Multiple specs should validate successfully: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_invalid_spec_reference() {
        let input = r#"spec person
fact name: "John"
fact contract: spec nonexistent"#;

        let specs = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), input.to_string());

        let result = plan_single(&specs[0], &specs, sources);

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
            Arc::from("fact x: []"),
        );
        spec.facts.push(LemmaFact::new(
            Reference {
                segments: vec![],
                name: "x".to_string(),
            },
            FactValue::TypeDeclaration {
                base: String::new(),
                constraints: None,
                from: None,
            },
            source,
        ));

        let specs = vec![spec.clone()];
        let mut sources = HashMap::new();
        sources.insert(
            "test.lemma".to_string(),
            "spec test\nfact x: []".to_string(),
        );

        let result = plan_single(&spec, &specs, sources);
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
            combined.contains("TypeDeclaration base cannot be empty"),
            "Error should mention empty base; got: {}",
            combined
        );
    }

    #[test]
    fn test_fact_binding_with_custom_type_resolves_in_correct_spec_context() {
        // This is a planning-level test: ensure fact bindings resolve custom types correctly
        // when the type is defined in a different spec than the binding.
        //
        // spec one:
        //   type money: number
        //   fact x: [money]
        // spec two:
        //   fact one: spec one
        //   fact one.x: 7
        //   rule getx: one.x
        let code = r#"
spec one
type money: number
fact x: [money]

spec two
fact one: spec one
fact one.x: 7
rule getx: one.x
"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let spec_two = specs.iter().find(|d| d.name == "two").unwrap();

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());
        let execution_plan =
            plan_single(spec_two, &specs, sources).expect("planning should succeed");

        // Verify that one.x has type 'money' (resolved from spec one)
        let one_x_path = FactPath {
            segments: vec![PathSegment {
                fact: "one".to_string(),
                spec: "one".to_string(),
            }],
            fact: "x".to_string(),
        };

        let one_x_type = execution_plan
            .facts
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
    fn test_plan_with_registry_style_spec_names() {
        let source = r#"spec @user/workspace/somespec
fact quantity: 10

spec user/workspace/example
fact inventory: spec @user/workspace/somespec
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

        let result = plan_single(example_spec, &specs, sources);
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
type money from nonexistent_type_source
fact helper: spec nonexistent_spec
fact price: 10
rule total: helper.value + price"#;

        let specs = parse(source, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), source.to_string());

        let result = plan_single(&specs[0], &specs, sources);
        assert!(result.is_err(), "Planning should fail with multiple errors");

        let errors = result.unwrap_err();
        let all_messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        let combined = all_messages.join("\n");

        // Must report the type resolution error (shows up as "Unknown type: 'money'")
        assert!(
            combined.contains("Unknown type") && combined.contains("money"),
            "Should report type import error for 'money'. Got:\n{}",
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
    }

    #[test]
    fn test_type_error_does_not_suppress_cross_spec_fact_error() {
        // When a type import fails, errors about cross-spec fact references
        // (e.g. ext.some_fact where ext is a spec ref to a non-existing spec)
        // must still be reported.
        let source = r#"spec demo
type currency from missing_spec
fact ext: spec also_missing
rule val: ext.some_fact"#;

        let specs = parse(source, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), source.to_string());

        let result = plan_single(&specs[0], &specs, sources);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        let combined: String = errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        // The type error about 'currency' should be reported
        assert!(
            combined.contains("currency"),
            "Should report type error about 'currency'. Got:\n{}",
            combined
        );

        // The spec reference error about 'also_missing' should ALSO be reported
        assert!(
            combined.contains("also_missing"),
            "Should report error about 'also_missing'. Got:\n{}",
            combined
        );
    }
}
