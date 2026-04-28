//! Pure Rust evaluation engine for Lemma
//!
//! Executes pre-validated execution plans in dependency order.
//! The execution plan is self-contained with all rules flattened into branches.
//! The evaluator executes rules linearly without recursion or tree traversal.

pub mod explanation;
pub mod expression;
pub mod operations;
pub mod response;

use crate::evaluation::explanation::{ExplanationNode, ValueSource};
use crate::evaluation::operations::VetoType;
use crate::evaluation::response::EvaluatedRule;
use crate::planning::execution_plan::validate_value_against_type;
use crate::planning::semantics::{
    Data, DataDefinition, DataPath, DataValue, Expression, LemmaType, LiteralValue,
    ReferenceTarget, RulePath, ValueKind,
};
use crate::planning::ExecutionPlan;
use indexmap::IndexMap;
pub use operations::{ComputationKind, OperationKind, OperationRecord, OperationResult};
pub use response::{DataGroup, Response, RuleResult};
use std::collections::{HashMap, HashSet};

/// Evaluation context for storing intermediate results
pub(crate) struct EvaluationContext {
    data_values: HashMap<DataPath, LiteralValue>,
    pub(crate) rule_results: HashMap<RulePath, OperationResult>,
    rule_explanations: HashMap<RulePath, crate::evaluation::explanation::Explanation>,
    operations: Option<Vec<crate::OperationRecord>>,
    explanation_nodes: HashMap<usize, crate::evaluation::explanation::ExplanationNode>,
    now: LiteralValue,
    /// Map of rule-target reference data paths to their target rule path.
    /// Used by [`Self::lazy_rule_reference_resolve`] at first read of the
    /// reference data path: if the target rule produced a `Value`, we copy
    /// it into `data_values` (memoizing further reads); if it produced a
    /// `Veto`, we propagate that exact veto reason. The target rule is
    /// guaranteed to have been evaluated already because planning injected
    /// a `depends_on_rules` edge from every consumer rule to the target.
    rule_references: HashMap<DataPath, RulePath>,
    /// Constraint-violation vetoes on reference data paths discovered at
    /// run-time. Populated for data-target references when the copied
    /// value fails validation against the merged `resolved_type`; used by
    /// the data-path read in [`expression`] so consumers see a precise
    /// `Computation` veto naming the violated constraint instead of a
    /// generic missing-data veto.
    reference_vetoes: HashMap<DataPath, VetoType>,
    /// Merged `resolved_type` per reference data path, used to validate
    /// rule-target reference values lazily in
    /// [`Self::lazy_rule_reference_resolve`].
    reference_types: HashMap<DataPath, LemmaType>,
}

impl EvaluationContext {
    fn new(plan: &ExecutionPlan, now: LiteralValue, record_operations: bool) -> Self {
        let mut data_values: HashMap<DataPath, LiteralValue> = plan
            .data
            .iter()
            .filter_map(|(path, d)| d.value().map(|v| (path.clone(), v.clone())))
            .collect();

        let rule_references: HashMap<DataPath, RulePath> =
            build_transitive_rule_references(&plan.data);

        let reference_types: HashMap<DataPath, LemmaType> = plan
            .data
            .iter()
            .filter_map(|(path, def)| match def {
                DataDefinition::Reference { resolved_type, .. } => {
                    Some((path.clone(), resolved_type.clone()))
                }
                _ => None,
            })
            .collect();
        let mut reference_vetoes: HashMap<DataPath, VetoType> = HashMap::new();

        // Resolve data-target references: copy the target's value into the
        // reference path. Must happen in dependency order so reference-of-
        // reference chains see their target already populated. A reference
        // whose target value is missing simply stays missing here; any
        // rule/expression that later reads the reference will produce a
        // `MissingData` veto, matching the existing missing-data semantics
        // for type-declaration data with no value.
        //
        // A caller-supplied value (via `with_data_values`) replaces the
        // `DataDefinition::Reference` entry with `DataDefinition::Value`
        // before evaluation, so any path that is no longer a `Reference` is
        // skipped here — the user-provided value has already been placed in
        // `data_values` and wins over the reference copy.
        //
        // Rule-target references are intentionally absent from
        // `reference_evaluation_order` (filtered in planning); they are
        // resolved lazily at the consumer's read site once the target rule
        // has been evaluated.
        for reference_path in &plan.reference_evaluation_order {
            match plan.data.get(reference_path) {
                Some(DataDefinition::Reference {
                    target: ReferenceTarget::Data(target_path),
                    resolved_type,
                    local_default,
                    ..
                }) => {
                    let copied_kind: Option<ValueKind> = data_values
                        .get(target_path)
                        .map(|v| v.value.clone())
                        .or_else(|| local_default.clone());
                    if let Some(value_kind) = copied_kind {
                        let value = LiteralValue {
                            value: value_kind,
                            lemma_type: resolved_type.clone(),
                        };
                        match validate_value_against_type(resolved_type, &value) {
                            Ok(()) => {
                                data_values.insert(reference_path.clone(), value);
                            }
                            Err(msg) => {
                                reference_vetoes.insert(
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
                }) => {
                    // Rule-target references are resolved lazily on first
                    // read. They should never appear in
                    // `reference_evaluation_order` (planning filters them
                    // out), but skip defensively.
                }
                Some(_) => {
                    // User-provided value has replaced the reference;
                    // nothing to copy.
                }
                None => unreachable!(
                    "BUG: reference_evaluation_order references missing data path '{}'",
                    reference_path
                ),
            }
        }

        Self {
            data_values,
            rule_results: HashMap::new(),
            rule_explanations: HashMap::new(),
            operations: if record_operations {
                Some(Vec::new())
            } else {
                None
            },
            explanation_nodes: HashMap::new(),
            now,
            rule_references,
            reference_vetoes,
            reference_types,
        }
    }

    /// Resolve a rule-target reference data path lazily from the already-
    /// evaluated target rule's result. Returns:
    /// - `Some(Ok(value))` — target rule produced a value; memoized into `data_values`.
    /// - `Some(Err(veto))` — target rule produced a veto, or the rule's
    ///   value violates the reference's merged `resolved_type` constraints.
    /// - `None` — the path is not a rule-target reference.
    pub(crate) fn lazy_rule_reference_resolve(
        &mut self,
        data_path: &DataPath,
    ) -> Option<Result<LiteralValue, crate::evaluation::operations::VetoType>> {
        let rule_path = self.rule_references.get(data_path)?.clone();
        let result = self
            .rule_results
            .get(&rule_path)
            .cloned()
            .unwrap_or_else(|| {
                unreachable!(
                    "BUG: rule-target reference '{}' read before target rule '{}' evaluated; \
                 planning must have injected the dependency edge",
                    data_path, rule_path
                );
            });
        match result {
            OperationResult::Value(v) => {
                let v = *v;
                let v = match self.reference_types.get(data_path) {
                    Some(ref_type) => {
                        let retyped = LiteralValue {
                            value: v.value,
                            lemma_type: ref_type.clone(),
                        };
                        if let Err(msg) = validate_value_against_type(ref_type, &retyped) {
                            return Some(Err(VetoType::computation(format!(
                                "Reference '{}' violates declared constraint: {}",
                                data_path, msg
                            ))));
                        }
                        retyped
                    }
                    None => v,
                };
                self.data_values.insert(data_path.clone(), v.clone());
                Some(Ok(v))
            }
            OperationResult::Veto(veto) => Some(Err(veto)),
        }
    }

    /// Returns a recorded constraint-violation veto for a reference data
    /// path, if any. Populated in [`Self::new`] for data-target references
    /// whose copied value failed `validate_value_against_type`.
    pub(crate) fn get_reference_veto(&self, data_path: &DataPath) -> Option<&VetoType> {
        self.reference_vetoes.get(data_path)
    }

    pub(crate) fn now(&self) -> &LiteralValue {
        &self.now
    }

    fn get_data(&self, data_path: &DataPath) -> Option<&LiteralValue> {
        self.data_values.get(data_path)
    }

    fn push_operation(&mut self, kind: OperationKind) {
        if let Some(ref mut ops) = self.operations {
            ops.push(OperationRecord { kind });
        }
    }

    fn set_explanation_node(
        &mut self,
        expression: &Expression,
        node: crate::evaluation::explanation::ExplanationNode,
    ) {
        self.explanation_nodes
            .insert(expression as *const Expression as usize, node);
    }

    fn get_explanation_node(
        &self,
        expression: &Expression,
    ) -> Option<&crate::evaluation::explanation::ExplanationNode> {
        self.explanation_nodes
            .get(&(expression as *const Expression as usize))
    }

    fn get_rule_explanation(
        &self,
        rule_path: &RulePath,
    ) -> Option<&crate::evaluation::explanation::Explanation> {
        self.rule_explanations.get(rule_path)
    }

    fn set_rule_explanation(
        &mut self,
        rule_path: RulePath,
        explanation: crate::evaluation::explanation::Explanation,
    ) {
        self.rule_explanations.insert(rule_path, explanation);
    }
}

/// For every reference data path in `data`, follow the data-target reference
/// chain and record the eventual rule-target (if any). Includes direct
/// rule-target references. A visited set guards against pathological cycles
/// that planning should already have rejected.
fn build_transitive_rule_references(
    data: &IndexMap<DataPath, DataDefinition>,
) -> HashMap<DataPath, RulePath> {
    let mut out: HashMap<DataPath, RulePath> = HashMap::new();
    for (path, def) in data {
        if !matches!(def, DataDefinition::Reference { .. }) {
            continue;
        }
        let mut visited: HashSet<DataPath> = HashSet::new();
        let mut cursor: DataPath = path.clone();
        loop {
            if !visited.insert(cursor.clone()) {
                break;
            }
            let Some(DataDefinition::Reference { target, .. }) = data.get(&cursor) else {
                break;
            };
            match target {
                ReferenceTarget::Data(next) => cursor = next.clone(),
                ReferenceTarget::Rule(rule_path) => {
                    out.insert(path.clone(), rule_path.clone());
                    break;
                }
            }
        }
    }
    out
}

fn collect_used_data_from_explanation(
    node: &ExplanationNode,
    out: &mut HashMap<DataPath, LiteralValue>,
) {
    match node {
        ExplanationNode::Value {
            value,
            source: ValueSource::Data { data_ref },
            ..
        } => {
            out.entry(data_ref.clone()).or_insert_with(|| value.clone());
        }
        ExplanationNode::Value { .. } => {}
        ExplanationNode::RuleReference { expansion, .. } => {
            collect_used_data_from_explanation(expansion.as_ref(), out);
        }
        ExplanationNode::Computation { operands, .. } => {
            for op in operands {
                collect_used_data_from_explanation(op, out);
            }
        }
        ExplanationNode::Branches {
            matched,
            non_matched,
            ..
        } => {
            if let Some(ref cond) = matched.condition {
                collect_used_data_from_explanation(cond, out);
            }
            collect_used_data_from_explanation(&matched.result, out);
            for nm in non_matched {
                collect_used_data_from_explanation(&nm.condition, out);
                if let Some(ref res) = nm.result {
                    collect_used_data_from_explanation(res, out);
                }
            }
        }
        ExplanationNode::Condition { operands, .. } => {
            for op in operands {
                collect_used_data_from_explanation(op, out);
            }
        }
        ExplanationNode::Veto { .. } => {}
    }
}

#[cfg(test)]
mod runtime_invariant_tests {
    use super::*;
    use crate::parsing::ast::DateTimeValue;
    use crate::Engine;

    /// At evaluation time the LiteralValue stored under a reference data path
    /// must carry the reference's own `resolved_type`, not the (possibly looser)
    /// type embedded in the target's LiteralValue. The merge pass folds in the
    /// LHS-declared constraints (e.g. a binding's parent-spec type chain) so
    /// the reference's `resolved_type` is the contract the evaluator promises;
    /// any consumer that reads `data_values[ref].lemma_type` directly must see
    /// that contract, not the target's loose shape.
    #[test]
    fn reference_runtime_value_carries_resolved_type_not_target_type() {
        let code = r#"
spec inner
data slot: number -> minimum 0 -> maximum 100

spec source_spec
data v: number -> default 5

spec outer
with i: inner
with src: source_spec
data i.slot: src.v
rule r: i.slot
"#;
        let mut engine = Engine::new();
        engine
            .load(code, crate::SourceType::Labeled("ref_invariant.lemma"))
            .expect("must load");

        let now = DateTimeValue::now();
        let plan = engine
            .get_plan("outer", Some(&now))
            .expect("must plan")
            .clone();

        let now_lit = LiteralValue {
            value: crate::planning::semantics::ValueKind::Date(
                crate::planning::semantics::date_time_to_semantic(&now),
            ),
            lemma_type: crate::planning::semantics::primitive_date().clone(),
        };
        let context = EvaluationContext::new(&plan, now_lit, false);

        let reference_path = plan
            .data
            .iter()
            .find_map(|(path, def)| match def {
                DataDefinition::Reference { .. } => Some(path.clone()),
                _ => None,
            })
            .expect("plan must contain the reference for `i.slot`");

        let resolved_type = match plan.data.get(&reference_path).expect("entry exists") {
            DataDefinition::Reference { resolved_type, .. } => resolved_type.clone(),
            _ => unreachable!("filter above kept only Reference entries"),
        };

        let stored = context
            .data_values
            .get(&reference_path)
            .expect("EvaluationContext must populate reference path with the copied value");

        assert_eq!(
            stored.lemma_type, resolved_type,
            "stored LiteralValue must carry the reference's resolved_type \
             (LHS-merged), not the target's loose type. \
             stored = {:?}, resolved = {:?}",
            stored.lemma_type, resolved_type
        );
    }
}

/// Evaluates Lemma rules within their spec context
#[derive(Default)]
pub(crate) struct Evaluator;

impl Evaluator {
    /// Evaluate an execution plan.
    ///
    /// Executes rules in pre-computed dependency order with all data pre-loaded.
    /// Rules are already flattened into executable branches with data prefixes resolved.
    ///
    /// After planning, evaluation is guaranteed to complete. This function never returns
    /// a Error — runtime issues (division by zero, missing data, user-defined veto)
    /// produce Vetoes, which are valid evaluation outcomes.
    ///
    /// When `record_operations` is true, each rule's evaluation records a trace of
    /// operations (data used, rules used, computations, branch evaluations) into
    /// `RuleResult::operations`. When false, no trace is recorded.
    pub(crate) fn evaluate(
        &self,
        plan: &ExecutionPlan,
        now: LiteralValue,
        record_operations: bool,
    ) -> Response {
        let mut context = EvaluationContext::new(plan, now, record_operations);

        let mut response = Response {
            spec_name: plan.spec_name.clone(),
            spec_hash: None,
            spec_effective_from: None,
            spec_effective_to: None,
            data: Vec::new(),
            results: IndexMap::new(),
        };

        // Execute each rule in topological order (already sorted by ExecutionPlan)
        for exec_rule in &plan.rules {
            if let Some(ref mut ops) = context.operations {
                ops.clear();
            }
            context.explanation_nodes.clear();

            let (result, explanation) = expression::evaluate_rule(exec_rule, &mut context);

            context
                .rule_results
                .insert(exec_rule.path.clone(), result.clone());
            context.set_rule_explanation(exec_rule.path.clone(), explanation.clone());

            let rule_operations = context.operations.clone().unwrap_or_default();

            if !exec_rule.path.segments.is_empty() {
                continue;
            }

            let unless_branches: Vec<(Option<Expression>, Expression)> = exec_rule.branches[1..]
                .iter()
                .map(|b| (b.condition.clone(), b.result.clone()))
                .collect();

            response.add_result(RuleResult {
                rule: EvaluatedRule {
                    name: exec_rule.name.clone(),
                    path: exec_rule.path.clone(),
                    default_expression: exec_rule.branches[0].result.clone(),
                    unless_branches,
                    source_location: exec_rule.source.clone(),
                    rule_type: exec_rule.rule_type.clone(),
                },
                result,
                data: vec![],
                operations: rule_operations,
                explanation: Some(explanation),
                rule_type: exec_rule.rule_type.clone(),
            });
        }

        let mut used_data: HashMap<DataPath, LiteralValue> = HashMap::new();
        for rule_result in response.results.values() {
            if let Some(ref explanation) = rule_result.explanation {
                collect_used_data_from_explanation(explanation.tree.as_ref(), &mut used_data);
            }
        }

        // Build data list in definition order (plan.data is an IndexMap)
        let data_list: Vec<Data> = plan
            .data
            .keys()
            .filter_map(|path| {
                used_data.remove(path).map(|value| Data {
                    path: path.clone(),
                    value: DataValue::Literal(value),
                    source: None,
                })
            })
            .collect();

        if !data_list.is_empty() {
            response.data = vec![DataGroup {
                data_path: String::new(),
                referencing_data_name: String::new(),
                data: data_list,
            }];
        }

        response
    }
}
