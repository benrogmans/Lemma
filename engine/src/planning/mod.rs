//! Planning module for Lemma specs
//!
//! This module performs complete static analysis and builds execution plans:
//! - Builds Graph with facts and rules (validated, with types computed)
//! - Builds ExecutionPlan from Graph (topologically sorted, ready for evaluation)
//! - Validates spec structure and references
//!
//! Contract model:
//! - Interface contract: facts (inputs) + rules (outputs), including full type constraints.
//!   Cross-spec bindings must satisfy this contract at planning time.
//! - Behavior lock: plan hash pins full execution semantics (fingerprint), not only IO shape.

pub mod execution_plan;
pub mod fingerprint;
pub mod graph;
pub mod semantics;
pub mod slice_interface;
pub mod temporal;
pub mod types;
pub mod validation;
use crate::engine::Context;
use crate::parsing::ast::{DateTimeValue, FactValue as ParsedFactValue, LemmaSpec, TypeDef};
use crate::Error;
pub use execution_plan::{Branch, ExecutableRule, ExecutionPlan, SpecSchema};
pub use semantics::{
    is_same_spec, negated_comparison, ArithmeticComputation, ComparisonComputation, Expression,
    ExpressionKind, Fact, FactData, FactPath, FactValue, LemmaType, LiteralValue,
    LogicalComputation, MathematicalComputation, NegationType, PathSegment, RulePath, Source, Span,
    TypeDefiningSpec, TypeExtends, ValueKind, VetoExpression,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

/// Slice-keyed registry of plan hashes built during the planning loop.
/// Filled incrementally as specs are planned in topological order.
#[derive(Debug, Default, Clone)]
pub struct PlanHashRegistry {
    by_slice: BTreeMap<(String, Option<DateTimeValue>), String>,
    by_pin: BTreeMap<(String, String), Arc<LemmaSpec>>,
}

impl PlanHashRegistry {
    pub(crate) fn insert(
        &mut self,
        spec: &Arc<LemmaSpec>,
        slice_from: Option<DateTimeValue>,
        hash: String,
    ) {
        let hash_lower = hash.trim().to_ascii_lowercase();
        self.by_slice
            .insert((spec.name.clone(), slice_from), hash_lower.clone());
        self.by_pin
            .insert((spec.name.clone(), hash_lower), Arc::clone(spec));
    }

    /// Lookup plan hash for a dependency at a given slice start.
    pub(crate) fn get_by_slice(
        &self,
        spec_name: &str,
        slice_from: &Option<DateTimeValue>,
    ) -> Option<&str> {
        self.by_slice
            .get(&(spec_name.to_string(), slice_from.clone()))
            .map(|s| s.as_str())
    }

    /// Lookup spec arc by (name, hash) for pin resolution.
    pub(crate) fn get_by_pin(&self, spec_name: &str, hash: &str) -> Option<&Arc<LemmaSpec>> {
        let key = (spec_name.to_string(), hash.trim().to_ascii_lowercase());
        self.by_pin.get(&key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SpecName(String);

impl SpecName {
    fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl std::fmt::Display for SpecName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn collect_spec_dependencies(
    spec: &LemmaSpec,
    known_names: &BTreeSet<SpecName>,
) -> BTreeSet<SpecName> {
    let mut deps = BTreeSet::new();
    let current = SpecName::new(spec.name.clone());

    for fact in &spec.facts {
        match &fact.value {
            ParsedFactValue::SpecReference(r) => {
                let dep = SpecName::new(r.name.clone());
                if dep != current && known_names.contains(&dep) {
                    deps.insert(dep);
                }
            }
            ParsedFactValue::TypeDeclaration {
                from: Some(from_ref),
                ..
            } => {
                let dep = SpecName::new(from_ref.name.clone());
                if dep != current && known_names.contains(&dep) {
                    deps.insert(dep);
                }
            }
            _ => {}
        }
    }

    for type_def in &spec.types {
        if let TypeDef::Import { from, .. } = type_def {
            let dep = SpecName::new(from.name.clone());
            if dep != current && known_names.contains(&dep) {
                deps.insert(dep);
            }
        }
    }

    deps
}

/// Order specs so dependencies (referenced specs / type-import sources) are planned first.
/// Needed for `spec dep~hash` resolution: hashes are recorded as each spec's plans are built.
pub(crate) fn order_specs_for_planning_graph(
    specs: Vec<Arc<LemmaSpec>>,
) -> Result<Vec<Arc<LemmaSpec>>, Vec<Error>> {
    let all_names: BTreeSet<SpecName> = specs
        .iter()
        .map(|s| SpecName::new(s.name.clone()))
        .collect();

    let mut deps_by_name: BTreeMap<SpecName, BTreeSet<SpecName>> = BTreeMap::new();
    for name in &all_names {
        deps_by_name.insert(name.clone(), BTreeSet::new());
    }
    for spec in &specs {
        let deps = collect_spec_dependencies(spec, &all_names);
        deps_by_name.insert(SpecName::new(spec.name.clone()), deps);
    }

    // Kahn: in_degree[name] = number of prerequisites for that name.
    let mut in_degree: BTreeMap<SpecName, usize> = BTreeMap::new();
    for (name, deps) in &deps_by_name {
        in_degree.insert(name.clone(), deps.len());
    }

    // Reverse edges: dependency -> dependents
    let mut dependents: BTreeMap<SpecName, BTreeSet<SpecName>> = BTreeMap::new();
    for name in &all_names {
        dependents.insert(name.clone(), BTreeSet::new());
    }
    for (name, deps) in &deps_by_name {
        for dep in deps {
            if let Some(children) = dependents.get_mut(dep) {
                children.insert(name.clone());
            }
        }
    }

    let mut queue: VecDeque<SpecName> = VecDeque::new();
    for (name, degree) in &in_degree {
        if *degree == 0 {
            queue.push_back(name.clone());
        }
    }

    let mut ordered_names: Vec<SpecName> = Vec::new();
    while let Some(name) = queue.pop_front() {
        ordered_names.push(name.clone());
        if let Some(children) = dependents.get(&name) {
            for child in children {
                if let Some(degree) = in_degree.get_mut(child) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(child.clone());
                    }
                }
            }
        }
    }

    if ordered_names.len() != all_names.len() {
        let mut cycle_nodes: Vec<SpecName> = in_degree
            .iter()
            .filter_map(|(name, degree)| {
                if *degree > 0 {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        cycle_nodes.sort();
        let cycle_path = if cycle_nodes.len() > 1 {
            let mut path = cycle_nodes.clone();
            path.push(cycle_nodes[0].clone());
            path.iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(" -> ")
        } else {
            cycle_nodes
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(" -> ")
        };
        return Err(vec![Error::validation(
            format!("Spec dependency cycle: {}", cycle_path),
            None,
            None::<String>,
        )]);
    }

    let mut by_name: HashMap<SpecName, Vec<Arc<LemmaSpec>>> = HashMap::new();
    for s in specs {
        by_name
            .entry(SpecName::new(s.name.clone()))
            .or_default()
            .push(s);
    }
    for v in by_name.values_mut() {
        v.sort_by(|a, b| a.effective_from().cmp(&b.effective_from()));
    }

    let mut out = Vec::new();
    for name in ordered_names {
        if let Some(mut vec) = by_name.remove(&name) {
            out.append(&mut vec);
        }
    }
    Ok(out)
}

/// Result of planning a single spec: the spec, its execution plans (if any), and errors produced while planning it.
#[derive(Debug, Clone)]
pub struct SpecPlanningResult {
    /// The spec we were planning (the one this result is for).
    pub spec: Arc<LemmaSpec>,
    /// Execution plans for that spec (one per temporal interval; empty if planning failed).
    pub plans: Vec<ExecutionPlan>,
    /// All planning errors produced while planning this spec.
    pub errors: Vec<Error>,
}

/// Result of running plan() across the context: per-spec results and global errors (e.g. temporal coverage).
#[derive(Debug, Clone)]
pub struct PlanningResult {
    /// One result per spec we attempted to plan.
    pub per_spec: Vec<SpecPlanningResult>,
    /// Errors not tied to a single spec (e.g. from validate_temporal_coverage).
    pub global_errors: Vec<Error>,
    /// Slice- and pin-keyed registry of plan hashes for request-level spec resolution.
    pub plan_hash_registry: PlanHashRegistry,
}

/// Build execution plans for one or more Lemma specs.
///
/// Context is immutable — types are resolved per (spec, slice) inside Graph::build
/// and never stored in Context. The flow:
/// 1. Per-spec, per-slice: Graph::build registers + resolves types using Context + resolve_at
/// 2. ExecutionPlan is built from the graph (types baked into facts/rules)
///
/// Returns a PlanningResult: per-spec results (spec, plans, errors) and global errors.
/// When displaying errors, iterate per_spec and for each with non-empty errors output "In spec 'X':" then each error.
pub fn plan(context: &Context, sources: HashMap<String, String>) -> PlanningResult {
    let mut global_errors: Vec<Error> = Vec::new();
    global_errors.extend(temporal::validate_temporal_coverage(context));

    let all_specs: Vec<_> = match order_specs_for_planning_graph(context.iter().collect()) {
        Ok(specs) => specs,
        Err(mut cycle_errors) => {
            global_errors.append(&mut cycle_errors);
            return PlanningResult {
                per_spec: Vec::new(),
                global_errors,
                plan_hash_registry: PlanHashRegistry::default(),
            };
        }
    };

    let mut per_spec: Vec<SpecPlanningResult> = Vec::new();
    let mut plan_hashes = PlanHashRegistry::default();

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
                slice.from.clone(),
                &plan_hashes,
            ) {
                Ok((graph, slice_types)) => {
                    let execution_plan = execution_plan::build_execution_plan(
                        &graph,
                        &slice_types,
                        slice.from.clone(),
                        slice.to.clone(),
                    );
                    let value_errors =
                        execution_plan::validate_literal_facts_against_types(&execution_plan);
                    if value_errors.is_empty() {
                        let hash = execution_plan.plan_hash();
                        plan_hashes.insert(spec_arc, slice.from.clone(), hash);
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
                spec_arc,
                &spec_plans,
                &slice_resolved_types,
            ));
        }

        per_spec.push(SpecPlanningResult {
            spec: Arc::clone(spec_arc),
            plans: spec_plans,
            errors: spec_errors,
        });
    }

    PlanningResult {
        per_spec,
        global_errors,
        plan_hash_registry: plan_hashes,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod internal_tests {
    use super::{order_specs_for_planning_graph, plan};
    use crate::engine::Context;
    use crate::parsing::ast::{FactValue, LemmaFact, LemmaSpec, ParentType, Reference, Span};
    use crate::parsing::source::Source;
    use crate::planning::execution_plan::ExecutionPlan;
    use crate::planning::semantics::{FactPath, PathSegment, TypeDefiningSpec, TypeExtends};
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
        );
        spec.facts.push(LemmaFact::new(
            Reference {
                segments: vec![],
                name: "x".to_string(),
            },
            FactValue::TypeDeclaration {
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
    fn test_fact_type_declaration_from_spec_has_import_defining_spec() {
        let code = r#"
spec examples
type money: scale
  -> unit eur 1.00

spec checkout
type money: scale
  -> unit eur 1.00
fact local_price: [money]
fact imported_price: [money from examples]
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
            .get_spec_effective_from("examples", None)
            .expect("examples spec should be present");
        let checkout_arc = ctx
            .get_spec_effective_from("checkout", None)
            .expect("checkout spec should be present");

        let mut sources = HashMap::new();
        sources.insert("test.lemma".to_string(), code.to_string());

        let result = plan(&ctx, sources);
        assert!(
            result.global_errors.is_empty(),
            "No global errors expected, got: {:?}",
            result.global_errors
        );

        let checkout_result = result
            .per_spec
            .iter()
            .find(|r| Arc::ptr_eq(&r.spec, &checkout_arc))
            .expect("checkout result should exist");
        assert!(
            checkout_result.errors.is_empty(),
            "No checkout planning errors expected, got: {:?}",
            checkout_result.errors
        );
        assert!(
            !checkout_result.plans.is_empty(),
            "checkout should produce at least one plan"
        );
        let execution_plan = &checkout_result.plans[0];

        let local_type = execution_plan
            .facts
            .get(&FactPath::new(vec![], "local_price".to_string()))
            .and_then(|d| d.schema_type())
            .expect("local_price should have schema type");
        let imported_type = execution_plan
            .facts
            .get(&FactPath::new(vec![], "imported_price".to_string()))
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
    fn test_spec_order_includes_fact_type_declaration_from_edges() {
        let source = r#"spec dep
type money: number
fact x: [money]

spec consumer
fact imported_amount: [money from dep]
rule passthrough: imported_amount"#;
        let specs = parse(source, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut ctx = Context::new();
        for spec in &specs {
            ctx.insert_spec(Arc::new(spec.clone()), spec.from_registry)
                .expect("insert spec");
        }

        let ordered = order_specs_for_planning_graph(ctx.iter().collect())
            .expect("spec order should succeed");
        let ordered_names: Vec<String> = ordered.iter().map(|s| s.name.clone()).collect();
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
    fn test_spec_dependency_cycle_returns_global_error_and_aborts() {
        let source = r#"spec a
fact dep_b: spec b

spec b
fact imported_value: [amount from a]
"#;
        let specs = parse(source, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;

        let mut ctx = Context::new();
        for spec in &specs {
            ctx.insert_spec(Arc::new(spec.clone()), spec.from_registry)
                .expect("insert spec");
        }

        let result = plan(&ctx, HashMap::new());
        assert!(
            result
                .global_errors
                .iter()
                .any(|e| e.to_string().contains("Spec dependency cycle")),
            "expected global cycle error, got {:?}",
            result
                .global_errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
        );
        assert!(
            result.per_spec.is_empty(),
            "planning should abort before per-spec planning when cycle exists"
        );
    }
}
