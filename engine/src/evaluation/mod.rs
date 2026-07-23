//! Pure Rust evaluation engine for Lemma
//!
//! Executes pre-validated execution plans by walking each rule's
//! [`NormalForm`] equation DAG. When `explain` is true the same walk fills
//! planning-time explanation trees; when false, values only.

pub(crate) mod branch_semantics;
pub(crate) mod conversion_trace;
pub mod data_input;
pub mod explanations;
pub mod expression;
pub mod operations;
pub mod response;
pub(crate) mod tree;

use crate::evaluation::operations::VetoType;
use crate::evaluation::response::EvaluatedRule;
use crate::planning::execution_plan::{
    collect_structural_data_paths, order_data_paths_by_plan, validate_value_against_type,
    ControlDataReleases, ExecutionPlan,
};
use crate::planning::normalize::NormalFormId;
use crate::planning::semantics::{
    DataDefinition, DataPath, LiteralValue, ReferenceTarget, RulePath, ValueKind,
};
pub use data_input::{DataOverlay, DataValueInput};
use indexmap::IndexMap;
pub use operations::OperationResult;
pub use response::{Response, RuleResult};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Nearest ignored input key for a MissingData typo hint, if within edit distance.
/// Comparison is case-insensitive; returned spelling is the caller's original key.
fn closest_ignored_key(needed: &str, ignored: &[String]) -> Option<String> {
    let max_distance = if needed.len() <= 3 { 1 } else { 2 };
    let needed_lower = needed.to_ascii_lowercase();
    let mut best: Option<(usize, &String)> = None;
    for candidate in ignored {
        let distance = levenshtein(&needed_lower, &candidate.to_ascii_lowercase());
        if distance == 0 || distance > max_distance {
            continue;
        }
        if best
            .map(|(best_distance, _)| distance < best_distance)
            .unwrap_or(true)
        {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, key)| key.clone())
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

/// Request-local mutable state for one plan run (overlay values, release tracking, explain caches).
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
    /// Ignored input keys from overlay (typo hints for MissingData).
    ignored_unknown: Vec<String>,
    /// Requested local rule whose `missing_data` is being built. Fixed across embeds.
    pub(crate) release_rule: Option<RulePath>,
    /// DataPaths released by control outcomes under `release_rule` this evaluation.
    pub(crate) released: HashSet<DataPath>,
    /// When false, And/Piecewise outcomes do not apply planned releases (explain narration
    /// after the value walk already recorded them).
    pub(crate) record_releases: bool,
    /// Value memo by NormalFormId for shared-DAG walks within one requested rule.
    pub(crate) value_memo: HashMap<crate::planning::normalize::NormalFormId, OperationResult>,
}

impl EvaluationContext {
    fn new(plan: &ExecutionPlan, overlay: &DataOverlay, now: LiteralValue) -> Self {
        let mut data_values: HashMap<DataPath, Arc<LiteralValue>> = HashMap::new();
        let mut vetoes: HashMap<DataPath, VetoType> = HashMap::new();

        for (path, binding) in &overlay.bindings {
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

        Self {
            data_values,
            rule_results: HashMap::new(),
            rule_explanations: HashMap::new(),
            now: Arc::new(now),
            vetoes,
            ignored_unknown: overlay.ignored_unknown.clone(),
            release_rule: None,
            released: HashSet::new(),
            record_releases: true,
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

    /// Begin evaluating one requested local rule: release tracking + per-rule caches.
    pub(crate) fn begin_requested_rule(&mut self, rule_path: RulePath) {
        self.release_rule = Some(rule_path);
        self.released.clear();
        self.value_memo.clear();
        self.rule_results.clear();
        self.rule_explanations.clear();
    }

    pub(crate) fn end_requested_rule(&mut self) {
        self.release_rule = None;
        self.released.clear();
    }

    /// Union planned release paths for a control outcome under the current release rule.
    pub(crate) fn apply_releases(
        &mut self,
        plan: &ExecutionPlan,
        control_id: NormalFormId,
        pick: impl FnOnce(&ControlDataReleases) -> &[DataPath],
    ) {
        if !self.record_releases {
            return;
        }
        let rule_path = self.release_rule.as_ref().unwrap_or_else(|| {
            panic!("BUG: apply_releases without release_rule set for control {control_id:?}")
        });
        let rule_releases = plan.data_releases.get(rule_path).unwrap_or_else(|| {
            panic!(
                "BUG: no data_releases for rule '{}' (control {control_id:?})",
                rule_path.rule
            )
        });
        let control_releases = rule_releases.get(&control_id).unwrap_or_else(|| {
            panic!(
                "BUG: no ControlDataReleases for control {control_id:?} under rule '{}'",
                rule_path.rule
            )
        });
        for path in pick(control_releases) {
            self.released.insert(path.clone());
        }
    }

    fn bound_paths(&self) -> HashSet<DataPath> {
        let mut bound = HashSet::new();
        bound.extend(self.data_values.keys().cloned());
        bound.extend(self.vetoes.keys().cloned());
        bound
    }

    /// `structural_needed − bound − released`, ordered by plan.data.
    ///
    /// Data→data references expand to their ultimate non-reference target (the
    /// promptable input). Rule-target references are omitted (embeds cover them).
    pub(crate) fn missing_data_for_rule(
        &self,
        plan: &ExecutionPlan,
        rule_root: NormalFormId,
    ) -> Vec<String> {
        let mut structural = HashSet::new();
        collect_structural_data_paths(plan, rule_root, &mut structural);
        let structural = expand_data_reference_targets(plan, structural);
        let bound = self.bound_paths();
        let still: HashSet<DataPath> = structural
            .into_iter()
            .filter(|path| !bound.contains(path) && !self.released.contains(path))
            .collect();
        order_data_paths_by_plan(plan, still)
            .into_iter()
            .map(|path| path.input_key())
            .collect()
    }
}

/// Replace data-reference paths with their ultimate value-bearing targets.
fn expand_data_reference_targets(
    plan: &ExecutionPlan,
    paths: HashSet<DataPath>,
) -> HashSet<DataPath> {
    let mut out = HashSet::new();
    for path in paths {
        let mut cursor = path;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(cursor.clone()) {
                panic!(
                    "BUG: cyclic data reference while expanding missing_data at '{}'",
                    cursor
                );
            }
            match plan.data.get(&cursor) {
                Some(DataDefinition::Reference {
                    target: ReferenceTarget::Data(next),
                    ..
                }) => {
                    cursor = next.clone();
                }
                Some(DataDefinition::Reference {
                    target: ReferenceTarget::Rule(_),
                    ..
                })
                | Some(DataDefinition::Import { .. }) => {
                    break;
                }
                Some(_) | None => {
                    out.insert(cursor);
                    break;
                }
            }
        }
    }
    out
}

/// Evaluates Lemma rules within their spec context
#[derive(Default)]
pub(crate) struct Evaluator;

impl Evaluator {
    /// Evaluate an execution plan: one tree walk per requested local rule.
    ///
    /// Values come from walking Kind under those roots. Unbound live inputs are
    /// reported per rule as `missing_data` (structural needed − bound − released).
    /// When `explain` is true, Rule embeds evaluate dependency rules on demand.
    pub(crate) fn evaluate(
        &self,
        plan: &ExecutionPlan,
        overlay: &DataOverlay,
        now: LiteralValue,
        response_rules: &std::collections::HashSet<String>,
        explain: bool,
    ) -> (Response, EvaluationContext) {
        let effective = match &now.value {
            ValueKind::Date(date) => date.to_string(),
            other => panic!("BUG: evaluation now must be a date, got {other:?}"),
        };
        let mut context = EvaluationContext::new(plan, overlay, now);

        let mut response = Response {
            spec_name: plan.spec_name.clone(),
            effective,
            spec_hash: None,
            spec_effective_from: None,
            spec_effective_to: None,
            results: IndexMap::new(),
        };

        for exec_rule in plan.rules.values() {
            if !(exec_rule.path.segments.is_empty() && response_rules.contains(exec_rule.name())) {
                continue;
            }

            context.begin_requested_rule(exec_rule.path.clone());

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

            let missing_data = context.missing_data_for_rule(plan, exec_rule.normal_form);
            context.end_requested_rule();

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

        (response, context)
    }
}

#[cfg(test)]
mod runtime_invariant_tests {
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

        let overlay = DataOverlay::default();

        let now = DateTimeValue::now();
        let now_lit = LiteralValue {
            value: crate::planning::semantics::ValueKind::Date(
                crate::planning::semantics::date_time_to_semantic(&now),
            ),
            lemma_type: crate::planning::semantics::primitive_date_arc().clone(),
        };
        let context = EvaluationContext::new(plan_basis, &overlay, now_lit);

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
