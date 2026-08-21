//! Pure Rust evaluation engine for Lemma
//!
//! Executes pre-validated execution plans by walking each rule's
//! [`NormalForm`] equation DAG. When `explain` is true the same walk fills
//! planning-time explanation trees; when false, values only.

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
    DataDefinition, DataPath, LiteralValue, ReferenceTarget, RulePath, ValueKind,
};
use indexmap::IndexMap;
pub use response::{Response, RuleResult};
pub use run_data::{RunData, RunDataValue};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Nearest ignored input key for a MissingData typo hint, if within edit distance.
/// Comparison is case-insensitive; returned spelling is the caller's original key.
fn closest_ignored_key(needed: &str, ignored: &[String]) -> Option<String> {
    let max_distance = if needed.len() <= 3 { 1 } else { 2 };
    let needed_lower = needed.to_ascii_lowercase();
    let mut best: Option<(usize, String, &String)> = None;
    for candidate in ignored {
        let candidate_lower = candidate.to_ascii_lowercase();
        let distance = levenshtein(&needed_lower, &candidate_lower);
        if distance == 0 || distance > max_distance {
            continue;
        }
        let dominated = best
            .as_ref()
            .map(|(best_distance, best_lower, _)| {
                distance < *best_distance
                    || (distance == *best_distance && candidate_lower < *best_lower)
            })
            .unwrap_or(true);
        if dominated {
            best = Some((distance, candidate_lower, candidate));
        }
    }
    best.map(|(_, _, key)| key.clone())
}

fn levenshtein(left: &str, right: &str) -> usize {
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let (left_len, right_len) = (left_chars.len(), right_chars.len());
    if left_len == 0 {
        return right_len;
    }
    if right_len == 0 {
        return left_len;
    }
    let mut previous: Vec<usize> = (0..=right_len).collect();
    let mut current = vec![0; right_len + 1];
    for (i, left_char) in left_chars.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right_chars.iter().enumerate() {
            let substitution = usize::from(left_char != right_char);
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right_len]
}

/// Request-local mutable state for one plan run (run data, control decisions, explain caches).
/// The [`ExecutionPlan`] is passed separately so the tree walk can match Kind in place.
pub(crate) struct EvaluationContext {
    pub(crate) data_values: HashMap<DataPath, Arc<LiteralValue>>,
    /// Results of rules evaluated on demand for Rule embeds (value and explain).
    pub(crate) rule_results: HashMap<RulePath, OperationResult>,
    /// Explain mode only: Rule explanation nodes filled on demand for embeds.
    pub(crate) rule_explanations: HashMap<RulePath, crate::planning::explanation::ExplanationNode>,
    now: Arc<LiteralValue>,
    /// Computation vetoes on data that cannot be read (bad override or reference constraint).
    vetoes: HashMap<DataPath, VetoType>,
    /// Ignored input keys from run data (typo hints for MissingData).
    ignored_unknown: Vec<String>,
    /// Whether this run data left any of the plan's promptable data paths unbound.
    /// False means no rule can report missing data, so control decisions need no recording.
    any_promptable_data_unbound: bool,
    /// Per And/Piecewise control node: immediate child NormalFormIds that are dead given
    /// the control decisions observed so far in this evaluation.
    pub(crate) dead_control_edges: HashMap<NormalFormId, HashSet<NormalFormId>>,
    /// When false, And/Piecewise outcomes do not record dead control edges (fully bound
    /// run data, or explain narration re-walking nodes after the value walk recorded them).
    pub(crate) record_control_decisions: bool,
    /// Value memo by NormalFormId for shared-DAG walks within one requested rule.
    pub(crate) value_memo: HashMap<crate::planning::normalize::NormalFormId, OperationResult>,
}

impl EvaluationContext {
    fn new(plan: &ExecutionPlan, run_data: &RunData, now: LiteralValue) -> Self {
        let mut data_values: HashMap<DataPath, Arc<LiteralValue>> = HashMap::new();
        let mut vetoes: HashMap<DataPath, VetoType> = HashMap::new();

        for (path, binding) in &run_data.bindings {
            match binding {
                OperationResult::Value(value) => {
                    data_values.insert(path.clone(), Arc::clone(value));
                }
                OperationResult::Veto(veto) => {
                    vetoes.insert(path.clone(), veto.clone());
                }
            }
        }

        for (path, definition) in &plan.data {
            if data_values.contains_key(path) || vetoes.contains_key(path) {
                continue;
            }
            if let Some(value) = definition.value() {
                data_values.insert(path.clone(), Arc::new(value.clone()));
            }
        }

        for reference_path in &plan.data_reference_order {
            if data_values.contains_key(reference_path) || vetoes.contains_key(reference_path) {
                continue;
            }
            match plan.data.get(reference_path) {
                Some(DataDefinition::Reference {
                    target: ReferenceTarget::Data(target_path),
                    resolved_type,
                    ..
                }) => {
                    if let Some(veto) = vetoes.get(target_path) {
                        vetoes.insert(reference_path.clone(), veto.clone());
                        continue;
                    }
                    let copied_kind: Option<ValueKind> =
                        data_values.get(target_path).map(|v| v.value.clone());
                    if let Some(value_kind) = copied_kind {
                        let value = LiteralValue {
                            value: value_kind,
                            lemma_type: Arc::clone(resolved_type),
                        };
                        match validate_value_against_type(
                            resolved_type.as_ref(),
                            &value,
                            &plan.resolved_types.unit_index,
                        ) {
                            Ok(()) => {
                                data_values.insert(reference_path.clone(), Arc::new(value));
                            }
                            Err(msg) => {
                                vetoes.insert(
                                    reference_path.clone(),
                                    VetoType::computation(format!(
                                        "Reference '{}' violates declared constraint: {}",
                                        reference_path, msg
                                    )),
                                );
                            }
                        }
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

        let any_promptable_data_unbound = plan
            .promptable_data_paths()
            .any(|path| !data_values.contains_key(path) && !vetoes.contains_key(path));

        Self {
            data_values,
            rule_results: HashMap::new(),
            rule_explanations: HashMap::new(),
            now: Arc::new(now),
            vetoes,
            ignored_unknown: run_data.ignored_unknown.clone(),
            any_promptable_data_unbound,
            dead_control_edges: HashMap::new(),
            record_control_decisions: any_promptable_data_unbound,
            value_memo: HashMap::new(),
        }
    }

    pub(crate) fn get_veto(&self, data_path: &DataPath) -> Option<&VetoType> {
        self.vetoes.get(data_path)
    }

    pub(crate) fn now(&self) -> &LiteralValue {
        self.now.as_ref()
    }

    pub(crate) fn get_data_value(&self, data_path: &DataPath) -> Option<&Arc<LiteralValue>> {
        self.data_values.get(data_path)
    }

    pub(crate) fn missing_data_suggestion(&self, data_path: &DataPath) -> Option<String> {
        closest_ignored_key(&data_path.input_key(), &self.ignored_unknown)
    }

    /// Begin evaluating one requested local rule: dead-edge tracking + per-rule caches.
    pub(crate) fn begin_requested_rule(&mut self) {
        self.dead_control_edges.clear();
        self.value_memo.clear();
        self.rule_results.clear();
        self.rule_explanations.clear();
    }

    /// Record immediate child NormalFormIds of `control_id` that are dead given the
    /// current control decision. Called once per control outcome during the value walk.
    pub(crate) fn record_dead_control_edges(
        &mut self,
        control_id: NormalFormId,
        dead_children: impl IntoIterator<Item = NormalFormId>,
    ) {
        if !self.record_control_decisions {
            return;
        }
        let entry = self.dead_control_edges.entry(control_id).or_default();
        for child in dead_children {
            entry.insert(child);
        }
    }

    /// Whether this evaluation has a value or a veto for `data_path`.
    fn is_data_bound(&self, data_path: &DataPath) -> bool {
        self.data_values.contains_key(data_path) || self.vetoes.contains_key(data_path)
    }

    /// Promptable data paths this rule still needs, in plan.data declaration order.
    ///
    /// Returns immediately when the run data bound every promptable path: no rule can
    /// report missing data, and no control decisions were recorded. Otherwise walks from
    /// `rule_root` respecting the `dead_control_edges` recorded during the value walk,
    /// and keeps the promptable paths that are both reachable and unbound.
    pub(crate) fn missing_data_for_rule(
        &self,
        plan: &ExecutionPlan,
        rule_root: NormalFormId,
    ) -> Vec<String> {
        if !self.any_promptable_data_unbound {
            return Vec::new();
        }
        let reachable = reachable_data_paths(plan, rule_root, &self.dead_control_edges);
        let promptable: HashSet<&DataPath> = reachable
            .iter()
            .filter_map(|path| plan.promptable_data_path(path))
            .collect();
        plan.promptable_data_paths()
            .filter(|path| promptable.contains(path) && !self.is_data_bound(path))
            .map(|path| path.input_key())
            .collect()
    }
}

/// Evaluates Lemma rules within their spec context
#[derive(Default)]
pub(crate) struct Evaluator;

impl Evaluator {
    /// Evaluate an execution plan: one tree walk per requested local rule.
    ///
    /// Values come from walking Kind under those roots. Unbound live inputs are
    /// reported per rule as `missing_data` (reachable under recorded control
    /// decisions, intersected with unbound promptable paths). When `explain` is
    /// true, Rule embeds evaluate dependency rules on demand.
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

        for exec_rule in plan.rules.values() {
            if !(exec_rule.path.segments.is_empty() && response_rules.contains(exec_rule.name())) {
                continue;
            }

            context.begin_requested_rule();

            let (result, explanation) = if explain {
                let (result, explanation) =
                    tree::evaluate_rule_explained(exec_rule, plan, &mut context);
                context
                    .rule_results
                    .insert(exec_rule.path.clone(), result.clone());
                (result, Some(explanation))
            } else {
                (tree::evaluate_rule(exec_rule, plan, &mut context), None)
            };

            let missing_data = match &result {
                OperationResult::Veto(VetoType::MissingData { .. }) => {
                    context.missing_data_for_rule(plan, exec_rule.normal_form)
                }
                _ => Vec::new(),
            };

            response.add_result(RuleResult::from_operation_result(
                EvaluatedRule {
                    name: exec_rule.name().to_string(),
                    path: exec_rule.path.clone(),
                    source_location: exec_rule.source.clone(),
                    rule_type: (*exec_rule.rule_type).clone(),
                },
                &result,
                exec_rule.rule_type.as_ref(),
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
uses src: source_spec
with i.slot: src.v
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
            lemma_type: crate::planning::semantics::primitive_date_arc().clone(),
        };
        let context = EvaluationContext::new(plan_basis, &run_data, now_lit);

        let stored = context
            .data_values
            .get(&reference_path)
            .expect("EvaluationContext must populate reference path with the copied value");

        assert_eq!(
            stored.as_ref().lemma_type,
            resolved_type,
            "stored LiteralValue must carry the reference's resolved_type \
             (LHS-merged), not the target's loose type. \
             stored = {:?}, resolved = {:?}",
            stored.as_ref().lemma_type,
            resolved_type,
        );
    }
}
