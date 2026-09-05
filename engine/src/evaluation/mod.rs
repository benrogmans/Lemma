//! Pure Rust evaluation engine for Lemma
//!
//! Executes pre-validated execution plans by walking each rule's
//! [`NormalForm`] equation DAG. When `explain` is true the same walk fills
//! planning-time explanation trees; when false, values only.
//!
//! Request state is one value table indexed by [`NormalFormId`]: the plan's
//! node table *is* the arena.

pub(crate) mod branch_semantics;
pub(crate) mod conversion_trace;
pub mod explanations;
pub mod expression;
pub mod response;
pub mod run_data;
pub(crate) mod tree;

pub use crate::computation::OperationResult;
use crate::computation::VetoType;
use crate::evaluation::response::EvaluatedRule;
use crate::planning::execution_plan::{
    reachable_data_paths, validate_value_against_type, ExecutionPlan,
};
use crate::planning::normalize::NormalFormId;
use crate::planning::semantics::{
    DataDefinition, DataPath, LemmaType, LiteralValue, ReferenceTarget, RulePath, ValueKind,
};
use indexmap::IndexMap;
pub use response::{Response, RuleResult};
pub use run_data::{RunData, RunDataValue};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn closest_ignored_key(needed: &str, ignored: &[String]) -> Option<String> {
    crate::string_distance::closest_name(needed, ignored)
}

/// Request-local mutable state for one plan run.
///
/// The value table is indexed by [`NormalFormId`] and doubles as memo and data
/// store. Rule-embed values live in [`Self::rule_values`], filled in plan
/// topological order before a consumer body walks. Control decisions for
/// missing-data walks are read from filled condition / scrutinee slots — there
/// is no separate log.
pub(crate) struct EvaluationContext {
    /// One slot per `plan.normal_forms` cell. Filled by data resolve and by
    /// `eval`; never cleared mid-request (values are a function of data).
    pub(crate) values: Vec<Option<OperationResult>>,
    /// One slot per `plan.rules` entry (same index as `IndexMap`). Filled by
    /// [`tree::evaluate_rule`] before consumers read embeds.
    pub(crate) rule_values: Vec<Option<OperationResult>>,
    now: LiteralValue,
    /// Ignored input keys from run data (typo hints for MissingData).
    ignored_unknown: Vec<String>,
    /// Whether this run data left any of the plan's promptable data paths unbound.
    any_promptable_data_unbound: bool,
    /// Successful overlays stamped with the caller-supplied unit (display/veto).
    overlay_types: HashMap<DataPath, Arc<LemmaType>>,
    /// Explain mode only: Rule explanation nodes filled on demand for embeds.
    pub(crate) rule_explanations: HashMap<RulePath, crate::planning::explanation::ExplanationNode>,
}

impl EvaluationContext {
    fn new(plan: &ExecutionPlan, run_data: &RunData, now: LiteralValue) -> Self {
        let mut values: Vec<Option<OperationResult>> = vec![None; plan.normal_forms.len()];

        // Caller bindings into their data-leaf slots.
        for (path, binding) in &run_data.bindings {
            let leaf = *plan
                .data_leaf
                .get(path)
                .unwrap_or_else(|| panic!("BUG: run binding for '{path}' has no data_leaf entry"));
            values[leaf.index()] = Some(binding.clone());
        }

        // Plan defaults into unbound data-leaf slots.
        for (path, definition) in &plan.data {
            let leaf = *plan
                .data_leaf
                .get(path)
                .expect("BUG: every plan.data path must have a data_leaf entry");
            if values[leaf.index()].is_some() {
                continue;
            }
            if let Some(value) = definition.value() {
                values[leaf.index()] = Some(OperationResult::from_literal(value));
            }
        }

        // Reference chains: copy target into reference leaf (data_reference_order).
        for reference_path in &plan.data_reference_order {
            let leaf = *plan
                .data_leaf
                .get(reference_path)
                .expect("BUG: reference path missing from data_leaf");
            if values[leaf.index()].is_some() {
                continue;
            }
            match plan.data.get(reference_path) {
                Some(DataDefinition::Reference {
                    target: ReferenceTarget::Data(target_path),
                    resolved_type,
                    ..
                }) => {
                    let target_leaf = *plan
                        .data_leaf
                        .get(target_path)
                        .expect("BUG: reference target missing from data_leaf");
                    match values[target_leaf.index()].as_ref() {
                        Some(OperationResult::Veto(veto)) => {
                            values[leaf.index()] = Some(OperationResult::Veto(veto.clone()));
                        }
                        Some(OperationResult::Value(value)) => {
                            let copied = LiteralValue {
                                value: value.value.clone(),
                            };
                            match validate_value_against_type(
                                resolved_type.as_ref(),
                                &copied,
                                &plan.resolved_types.unit_index,
                            ) {
                                Ok(()) => {
                                    values[leaf.index()] =
                                        Some(OperationResult::from_literal(copied));
                                }
                                Err(msg) => {
                                    values[leaf.index()] = Some(OperationResult::Veto(
                                        VetoType::computation(format!(
                                            "Reference '{}' violates declared constraint: {}",
                                            reference_path, msg
                                        )),
                                    ));
                                }
                            }
                        }
                        None => {}
                    }
                }
                Some(DataDefinition::Reference {
                    target: ReferenceTarget::Rule(_),
                    ..
                }) => {}
                Some(_) => {}
                None => unreachable!(
                    "BUG: data_reference_order references missing data path '{}'",
                    reference_path
                ),
            }
        }

        let any_promptable_data_unbound = plan.promptable_data_paths().any(|path| {
            let leaf = *plan
                .data_leaf
                .get(path)
                .expect("BUG: promptable path missing from data_leaf");
            values[leaf.index()].is_none()
        });

        Self {
            values,
            rule_values: vec![None; plan.rules.len()],
            now,
            ignored_unknown: run_data.ignored_unknown.clone(),
            any_promptable_data_unbound,
            overlay_types: run_data.overlay_types.clone(),
            rule_explanations: HashMap::new(),
        }
    }

    /// Stored value for a rule previously evaluated in this request.
    pub(crate) fn rule_value<'a>(
        &'a self,
        plan: &ExecutionPlan,
        path: &RulePath,
    ) -> &'a OperationResult {
        let index = plan.rules.get_index_of(path).unwrap_or_else(|| {
            panic!(
                "BUG: rule '{}' missing from execution plan while reading embed value",
                path.rule
            )
        });
        self.rule_values[index].as_ref().unwrap_or_else(|| {
            panic!(
                "BUG: rule '{}' embedded before evaluation (plan.rules topological order broken)",
                path.rule
            )
        })
    }

    pub(crate) fn now(&self) -> &LiteralValue {
        &self.now
    }

    /// Overlay type for a data path when the caller supplied an explicit unit.
    #[must_use]
    pub(crate) fn overlay_type(&self, path: &DataPath) -> Option<&Arc<LemmaType>> {
        self.overlay_types.get(path)
    }

    /// Schema or overlay type for displaying a data path value.
    #[must_use]
    pub(crate) fn data_display_type(
        &self,
        plan: &ExecutionPlan,
        path: &DataPath,
    ) -> Arc<LemmaType> {
        if let Some(overlay) = self.overlay_type(path) {
            return Arc::clone(overlay);
        }
        plan.data
            .get(path)
            .and_then(|def| def.schema_type())
            .map(|ty| Arc::new(ty.clone()))
            .expect("BUG: data path leaf missing schema type")
    }

    /// Rule result type with overlay binding when the rule body is a bound data path.
    #[must_use]
    pub(crate) fn rule_result_type(
        &self,
        plan: &ExecutionPlan,
        rule: &crate::planning::execution_plan::ExecutableRule,
    ) -> Arc<LemmaType> {
        let planned = Arc::clone(&rule.rule_type);
        match &plan.normal_form(rule.normal_form).kind {
            crate::planning::normalize::NormalFormKind::Leaf(
                crate::planning::normalize::LeafKind::DataPath(path),
            ) => {
                if let Some(overlay) = self.overlay_type(path) {
                    return Arc::new(
                        planned.as_ref().clone().with_measure_binding_unit(
                            overlay
                                .measure_binding_unit
                                .clone()
                                .expect("BUG: overlay_types entry must carry binding unit"),
                        ),
                    );
                }
                planned
            }
            _ => planned,
        }
    }

    /// Slot for a data path's leaf cell.
    pub(crate) fn data_slot(
        &self,
        plan: &ExecutionPlan,
        data_path: &DataPath,
    ) -> Option<&OperationResult> {
        let leaf = *plan
            .data_leaf
            .get(data_path)
            .unwrap_or_else(|| panic!("BUG: data path '{data_path}' has no data_leaf entry"));
        self.values[leaf.index()].as_ref()
    }

    pub(crate) fn missing_data_suggestion(&self, data_path: &DataPath) -> Option<String> {
        closest_ignored_key(&data_path.input_key(), &self.ignored_unknown)
    }

    /// Whether this evaluation has a value or a veto for `data_path`.
    fn is_data_bound(&self, plan: &ExecutionPlan, data_path: &DataPath) -> bool {
        self.data_slot(plan, data_path).is_some()
    }

    /// Promptable data paths this rule still needs, in evaluation / decision-tree order.
    ///
    /// First key is the next fact the live tree needs. Returns immediately when the run
    /// data bound every promptable path: no rule can report missing data. Otherwise walks
    /// from `rule_root` deriving liveness from filled condition/scrutinee slots, maps each
    /// leaf through [`ExecutionPlan::promptable_data_path`], and keeps unbound keys.
    pub(crate) fn missing_data_for_rule(
        &self,
        plan: &ExecutionPlan,
        rule_root: NormalFormId,
    ) -> Vec<String> {
        if !self.any_promptable_data_unbound {
            return Vec::new();
        }
        let reachable = reachable_data_paths(plan, rule_root, &self.values);
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for path in &reachable {
            let Some(promptable) = plan.promptable_data_path(path) else {
                continue;
            };
            if self.is_data_bound(plan, promptable) {
                continue;
            }
            let key = promptable.input_key();
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
        out
    }
}

/// Evaluates Lemma rules within their spec context
#[derive(Default)]
pub(crate) struct Evaluator;

impl Evaluator {
    /// Evaluate an execution plan: dependency closure of requested local rules
    /// in plan topological order, then report requested results.
    ///
    /// Rule embeds are evaluation boundaries: a dependency's value is read from
    /// [`EvaluationContext::rule_values`], never by re-entering its body.
    /// Unbound live inputs are reported per rule as `missing_data` (reachable
    /// under control decisions derived from filled slots, intersected with
    /// unbound promptable paths). When `explain` is true, dependency
    /// explanations are ensured before each requested rule's explain walk.
    pub(crate) fn evaluate(
        &self,
        plan: &ExecutionPlan,
        run_data: &RunData,
        now: LiteralValue,
        response_rules: &std::collections::HashSet<String>,
        explain: bool,
    ) -> Response {
        let effective = match &now.value {
            ValueKind::Date(date) => date.to_string(),
            other => panic!("BUG: evaluation now must be a date, got {other:?}"),
        };
        let mut context = EvaluationContext::new(plan, run_data, now);

        let mut response = Response {
            spec_name: plan.spec_name.clone(),
            effective,
            // Set by `Engine::run` after evaluation from the plan's cached
            // version window (`ExecutionPlan::effective_from` / `effective_to`).
            spec_effective_from: None,
            spec_effective_to: None,
            results: IndexMap::new(),
        };

        let mut marked = vec![false; plan.rules.len()];
        let mut worklist: Vec<usize> = Vec::new();
        for (index, (path, _)) in plan.rules.iter().enumerate() {
            if path.segments.is_empty() && response_rules.contains(path.rule.as_str()) {
                marked[index] = true;
                worklist.push(index);
            }
        }
        while let Some(index) = worklist.pop() {
            let rule = plan
                .rules
                .get_index(index)
                .map(|(_, rule)| rule)
                .expect("BUG: marked rule index out of plan.rules range");
            for dep in &rule.depends_on_rules {
                let dep_index = plan.rules.get_index_of(dep).unwrap_or_else(|| {
                    panic!(
                        "BUG: depends_on_rules entry '{}' missing from plan.rules",
                        dep.rule
                    )
                });
                if !marked[dep_index] {
                    marked[dep_index] = true;
                    worklist.push(dep_index);
                }
            }
        }

        for (index, exec_rule) in plan.rules.values().enumerate() {
            if !marked[index] {
                continue;
            }

            let result = tree::evaluate_rule(exec_rule, plan, &mut context);
            let report =
                exec_rule.path.segments.is_empty() && response_rules.contains(exec_rule.name());
            if !report {
                continue;
            }

            let explanation = if explain {
                for dep in &exec_rule.depends_on_rules {
                    tree::ensure_rule_explained(dep, plan, &mut context);
                }
                Some(tree::evaluate_rule_explained(exec_rule, plan, &mut context).1)
            } else {
                None
            };

            let missing_data = match &result {
                OperationResult::Veto(VetoType::MissingData { .. }) => {
                    context.missing_data_for_rule(plan, exec_rule.normal_form)
                }
                _ => Vec::new(),
            };

            let rule_type = context.rule_result_type(plan, exec_rule);

            response.add_result(RuleResult::from_operation_result(
                EvaluatedRule {
                    name: exec_rule.name().to_string(),
                    path: exec_rule.path.clone(),
                    source_location: exec_rule.source.clone(),
                    rule_type: Arc::clone(&rule_type),
                },
                &result,
                rule_type.as_ref(),
                &plan.family_units,
                explanation,
                missing_data,
            ));
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::DateTimeValue;
    use crate::Engine;

    #[test]
    fn reference_runtime_value_carries_resolved_type_not_target_type() {
        let code = r#"
spec inner
data slot: number -> minimum 0 -> maximum 100

spec source_spec
data v: 5

spec outer
uses i: inner
  -> with slot: src.v
uses src: source_spec
rule r: i.slot
"#;
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "ref_invariant.lemma",
                ))),
                code.to_string(),
            )])
            .expect("must load");

        let plan_basis = engine
            .plans
            .get_plans(None, "outer")
            .and_then(|plans| plans.values().next())
            .expect("must plan");

        let reference_path = plan_basis
            .data
            .iter()
            .find_map(|(path, def)| match def {
                DataDefinition::Reference { .. } => Some(path.clone()),
                _ => None,
            })
            .expect("plan must contain the reference for `i.slot`");

        let resolved_type = match plan_basis.data.get(&reference_path).expect("entry exists") {
            DataDefinition::Reference { resolved_type, .. } => Arc::clone(resolved_type),
            _ => unreachable!("filter above kept only Reference entries"),
        };

        let run_data = RunData::default();

        let now = DateTimeValue::now();
        let now_lit = LiteralValue {
            value: crate::planning::semantics::ValueKind::Date(
                crate::planning::semantics::date_time_to_semantic(&now),
            ),
        };
        let context = EvaluationContext::new(plan_basis, &run_data, now_lit);

        let stored = context
            .data_slot(plan_basis, &reference_path)
            .expect("EvaluationContext must populate reference path with the copied value");

        let OperationResult::Value(value) = stored else {
            panic!("reference path must hold a value, got {stored:?}");
        };

        // Type lives on DataDefinition::Reference.resolved_type, not LiteralValue.
        assert!(
            matches!(
                resolved_type.specifications,
                crate::planning::semantics::TypeSpecification::Number {
                    minimum: Some(_),
                    maximum: Some(_),
                    ..
                }
            ),
            "reference resolved_type must keep LHS constraints, got {:?}",
            resolved_type.specifications
        );
        assert!(
            matches!(
                value.value,
                crate::planning::semantics::ValueKind::Number(_)
            ),
            "stored value must be the copied number, got {:?}",
            value.value
        );
    }
}
