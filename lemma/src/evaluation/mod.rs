//! Pure Rust evaluation engine for Lemma
//!
//! Executes pre-validated execution plans in dependency order.
//! The execution plan is self-contained with all rules flattened into branches.
//! The evaluator executes rules linearly without recursion or tree traversal.

pub mod expression;
pub mod operations;
pub mod proof;
pub mod response;

use crate::planning::ExecutionPlan;
use crate::{
    Expression, FactPath, FactReference, FactValue, LemmaFact, LemmaResult, LiteralValue, RulePath,
};
use indexmap::IndexMap;
pub use operations::{ComputationKind, OperationKind, OperationRecord, OperationResult};
pub use response::{Facts, Response, RuleResult};
use std::collections::{HashMap, HashSet};

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
        Self {
            fact_values: plan.fact_values.clone(),
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

    /// Evaluate an execution plan
    ///
    /// Executes rules in pre-computed dependency order with all facts pre-loaded.
    /// Rules are already flattened into executable branches with fact prefixes resolved.
    /// This evaluation never errors - runtime issues create Vetoes instead.
    pub fn evaluate(&self, plan: &ExecutionPlan) -> LemmaResult<Response> {
        let mut context = EvaluationContext::new(plan);

        let mut response = Response {
            doc_name: plan.doc_name.clone(),
            facts: Vec::new(),
            results: IndexMap::new(),
        };

        let mut seen_facts = HashSet::new();
        let mut fact_list = Vec::new();

        // Execute each rule in topological order (already sorted by ExecutionPlan)
        for exec_rule in &plan.rules {
            context.operations.clear();
            context.proof_nodes.clear();

            let (result, proof) = expression::evaluate_rule(exec_rule, &mut context)?;

            context
                .rule_results
                .insert(exec_rule.path.clone(), result.clone());
            context.set_rule_proof(exec_rule.path.clone(), proof.clone());

            let rule_operations = context.operations.clone();

            // Collect facts from operations as we go
            for op in &rule_operations {
                if let OperationKind::FactUsed { fact_ref, value } = &op.kind {
                    if seen_facts.insert(fact_ref.clone()) {
                        let segments: Vec<String> =
                            fact_ref.segments.iter().map(|s| s.fact.clone()).collect();
                        fact_list.push(LemmaFact {
                            reference: FactReference {
                                segments,
                                fact: fact_ref.fact.clone(),
                            },
                            value: FactValue::Literal(value.clone()),
                            source_location: None,
                        });
                    }
                }
            }

            response.add_result(RuleResult {
                rule: crate::LemmaRule {
                    name: exec_rule.name.clone(),
                    expression: exec_rule.branches[0].result.clone(),
                    unless_clauses: exec_rule.branches[1..]
                        .iter()
                        .filter_map(|b| {
                            b.condition.as_ref().map(|cond| crate::UnlessClause {
                                condition: cond.clone(),
                                result: b.result.clone(),
                                source_location: b.source.clone(),
                            })
                        })
                        .collect(),
                    source_location: exec_rule.source.clone(),
                },
                result,
                facts: vec![],
                operations: rule_operations,
                proof: Some(proof),
                rule_type: exec_rule.rule_type.clone(),
            });
        }

        if !fact_list.is_empty() {
            response.facts = vec![Facts {
                fact_path: String::new(),
                referencing_fact_name: String::new(),
                document_reference: None,
                facts: fact_list,
                referenced_docs: Vec::new(),
            }];
        }

        Ok(response)
    }
}
