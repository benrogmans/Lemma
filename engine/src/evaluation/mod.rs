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
use crate::evaluation::response::EvaluatedRule;
use crate::planning::semantics::{Expression, Fact, FactPath, FactValue, LiteralValue, RulePath};
use crate::planning::ExecutionPlan;
use indexmap::IndexMap;
pub use operations::{ComputationKind, OperationKind, OperationRecord, OperationResult};
pub use response::{Facts, Response, RuleResult};
use std::collections::HashMap;

/// Evaluation context for storing intermediate results
pub(crate) struct EvaluationContext {
    fact_values: HashMap<FactPath, LiteralValue>,
    pub(crate) rule_results: HashMap<RulePath, OperationResult>,
    rule_explanations: HashMap<RulePath, crate::evaluation::explanation::Explanation>,
    operations: Option<Vec<crate::OperationRecord>>,
    pub(crate) sources: HashMap<String, String>,
    explanation_nodes: HashMap<usize, crate::evaluation::explanation::ExplanationNode>,
    now: LiteralValue,
}

impl EvaluationContext {
    fn new(plan: &ExecutionPlan, now: LiteralValue, record_operations: bool) -> Self {
        let fact_values: HashMap<FactPath, LiteralValue> = plan
            .facts
            .iter()
            .filter_map(|(path, d)| d.value().map(|v| (path.clone(), v.clone())))
            .collect();
        Self {
            fact_values,
            rule_results: HashMap::new(),
            rule_explanations: HashMap::new(),
            operations: if record_operations {
                Some(Vec::new())
            } else {
                None
            },
            sources: plan.sources.clone(),
            explanation_nodes: HashMap::new(),
            now,
        }
    }

    pub(crate) fn now(&self) -> &LiteralValue {
        &self.now
    }

    fn get_fact(&self, fact_path: &FactPath) -> Option<&LiteralValue> {
        self.fact_values.get(fact_path)
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

fn collect_used_facts_from_explanation(
    node: &ExplanationNode,
    out: &mut HashMap<FactPath, LiteralValue>,
) {
    match node {
        ExplanationNode::Value {
            value,
            source: ValueSource::Fact { fact_ref },
            ..
        } => {
            out.entry(fact_ref.clone()).or_insert_with(|| value.clone());
        }
        ExplanationNode::Value { .. } => {}
        ExplanationNode::RuleReference { expansion, .. } => {
            collect_used_facts_from_explanation(expansion.as_ref(), out);
        }
        ExplanationNode::Computation { operands, .. } => {
            for op in operands {
                collect_used_facts_from_explanation(op, out);
            }
        }
        ExplanationNode::Branches {
            matched,
            non_matched,
            ..
        } => {
            if let Some(ref cond) = matched.condition {
                collect_used_facts_from_explanation(cond, out);
            }
            collect_used_facts_from_explanation(&matched.result, out);
            for nm in non_matched {
                collect_used_facts_from_explanation(&nm.condition, out);
                if let Some(ref res) = nm.result {
                    collect_used_facts_from_explanation(res, out);
                }
            }
        }
        ExplanationNode::Condition { operands, .. } => {
            for op in operands {
                collect_used_facts_from_explanation(op, out);
            }
        }
        ExplanationNode::Veto { .. } => {}
    }
}

/// Evaluates Lemma rules within their spec context
#[derive(Default)]
pub(crate) struct Evaluator;

impl Evaluator {
    /// Evaluate an execution plan.
    ///
    /// Executes rules in pre-computed dependency order with all facts pre-loaded.
    /// Rules are already flattened into executable branches with fact prefixes resolved.
    ///
    /// After planning, evaluation is guaranteed to complete. This function never returns
    /// a Error — runtime issues (division by zero, missing facts, user-defined veto)
    /// produce Vetoes, which are valid evaluation outcomes.
    ///
    /// When `record_operations` is true, each rule's evaluation records a trace of
    /// operations (facts used, rules used, computations, branch evaluations) into
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
            facts: Vec::new(),
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
                facts: vec![],
                operations: rule_operations,
                explanation: Some(explanation),
                rule_type: exec_rule.rule_type.clone(),
            });
        }

        let mut used_facts: HashMap<FactPath, LiteralValue> = HashMap::new();
        for rule_result in response.results.values() {
            if let Some(ref explanation) = rule_result.explanation {
                collect_used_facts_from_explanation(explanation.tree.as_ref(), &mut used_facts);
            }
        }

        // Build fact list in definition order (plan.facts is an IndexMap)
        let fact_list: Vec<Fact> = plan
            .facts
            .keys()
            .filter_map(|path| {
                used_facts.remove(path).map(|value| Fact {
                    path: path.clone(),
                    value: FactValue::Literal(value),
                    source: None,
                })
            })
            .collect();

        if !fact_list.is_empty() {
            response.facts = vec![Facts {
                fact_path: String::new(),
                referencing_fact_name: String::new(),
                facts: fact_list,
            }];
        }

        response
    }
}
