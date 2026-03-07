//! Pure Rust evaluation engine for Lemma
//!
//! Executes pre-validated execution plans in dependency order.
//! The execution plan is self-contained with all rules flattened into branches.
//! The evaluator executes rules linearly without recursion or tree traversal.

pub mod expression;
pub mod operations;
pub mod proof;
pub mod response;

use crate::evaluation::response::EvaluatedRule;
use crate::planning::semantics::{Expression, Fact, FactPath, FactValue, LiteralValue, RulePath};
use crate::planning::ExecutionPlan;
use indexmap::IndexMap;
pub use operations::{ComputationKind, OperationKind, OperationRecord, OperationResult};
pub use response::{Facts, Response, RuleResult};
use std::collections::HashMap;

/// Evaluation context for storing intermediate results
pub struct EvaluationContext {
    fact_values: HashMap<FactPath, LiteralValue>,
    rule_results: HashMap<RulePath, OperationResult>,
    rule_proofs: HashMap<RulePath, crate::evaluation::proof::Proof>,
    operations: Vec<crate::OperationRecord>,
    sources: HashMap<String, String>,
    proof_nodes: HashMap<Expression, crate::evaluation::proof::ProofNode>,
}

impl EvaluationContext {
    fn new(plan: &ExecutionPlan) -> Self {
        let fact_values: HashMap<FactPath, LiteralValue> = plan
            .facts
            .iter()
            .filter_map(|(path, d)| d.value().map(|v| (path.clone(), v.clone())))
            .collect();
        Self {
            fact_values,
            rule_results: HashMap::new(),
            rule_proofs: HashMap::new(),
            operations: Vec::new(),
            sources: plan.sources.clone(),
            proof_nodes: HashMap::new(),
        }
    }

    fn get_fact(&self, fact_path: &FactPath) -> Option<&LiteralValue> {
        self.fact_values.get(fact_path)
    }

    fn push_operation(&mut self, kind: OperationKind) {
        self.operations.push(OperationRecord { kind });
    }

    fn set_proof_node(
        &mut self,
        expression: &Expression,
        node: crate::evaluation::proof::ProofNode,
    ) {
        self.proof_nodes.insert(expression.clone(), node);
    }

    fn get_proof_node(
        &self,
        expression: &Expression,
    ) -> Option<&crate::evaluation::proof::ProofNode> {
        self.proof_nodes.get(expression)
    }

    fn get_rule_proof(&self, rule_path: &RulePath) -> Option<&crate::evaluation::proof::Proof> {
        self.rule_proofs.get(rule_path)
    }

    fn set_rule_proof(&mut self, rule_path: RulePath, proof: crate::evaluation::proof::Proof) {
        self.rule_proofs.insert(rule_path, proof);
    }
}

/// Evaluates Lemma rules within their document context
#[derive(Default)]
pub struct Evaluator;

impl Evaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate an execution plan.
    ///
    /// Executes rules in pre-computed dependency order with all facts pre-loaded.
    /// Rules are already flattened into executable branches with fact prefixes resolved.
    ///
    /// After planning, evaluation is guaranteed to complete. This function never returns
    /// a Error — runtime issues (division by zero, missing facts, user-defined veto)
    /// produce Vetoes, which are valid evaluation outcomes.
    pub fn evaluate(&self, plan: &ExecutionPlan) -> Response {
        let mut context = EvaluationContext::new(plan);

        let mut response = Response {
            doc_name: plan.doc_name.clone(),
            facts: Vec::new(),
            results: IndexMap::new(),
        };

        let mut used_facts: HashMap<FactPath, LiteralValue> = HashMap::new();

        // Execute each rule in topological order (already sorted by ExecutionPlan)
        for exec_rule in &plan.rules {
            context.operations.clear();
            context.proof_nodes.clear();

            let (result, proof) = expression::evaluate_rule(exec_rule, &mut context);

            context
                .rule_results
                .insert(exec_rule.path.clone(), result.clone());
            context.set_rule_proof(exec_rule.path.clone(), proof.clone());

            let rule_operations = context.operations.clone();

            for op in &rule_operations {
                if let OperationKind::FactUsed { fact_ref, value } = &op.kind {
                    used_facts.entry(fact_ref.clone()).or_insert(value.clone());
                }
            }

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
                proof: Some(proof),
                rule_type: exec_rule.rule_type.clone(),
            });
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
