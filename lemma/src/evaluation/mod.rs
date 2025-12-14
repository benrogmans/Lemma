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
    FactPath, LemmaFact, LemmaResult, LiteralValue, RulePath,
};
use indexmap::IndexMap;
pub use operations::{ComputationKind, OperationKind, OperationRecord, OperationResult};
pub use response::{Facts, Response, RuleResult};
use std::collections::HashMap;

/// Evaluation context for storing intermediate results
pub struct EvaluationContext {
    facts: HashMap<FactPath, LemmaFact>,
    rule_results: HashMap<RulePath, OperationResult>,
    rule_proofs: HashMap<RulePath, crate::evaluation::proof::Proof>,
    operations: Vec<crate::OperationRecord>,
    source_text: HashMap<String, (String, String)>,
    proof_nodes: HashMap<crate::Expression, crate::evaluation::proof::ProofNode>,
    symbolic_mode: bool,
}

impl EvaluationContext {
    fn new(plan: &ExecutionPlan) -> Self {
        Self {
            facts: plan.facts.clone(),
            rule_results: HashMap::new(),
            rule_proofs: HashMap::new(),
            operations: Vec::new(),
            source_text: plan.graph().sources().clone(),
            proof_nodes: HashMap::new(),
            symbolic_mode: false,
        }
    }

    fn new_symbolic(plan: &ExecutionPlan) -> Self {
        Self {
            facts: plan.facts.clone(),
            rule_results: HashMap::new(),
            rule_proofs: HashMap::new(),
            operations: Vec::new(),
            source_text: plan.graph().sources().clone(),
            proof_nodes: HashMap::new(),
            symbolic_mode: true,
        }
    }

    /// Create a minimal evaluation context for inversion constant evaluation
    /// Used by algebraic isolation to evaluate constant expressions
    pub fn new_for_inversion(plan: &ExecutionPlan) -> Self {
        Self {
            facts: plan.facts.clone(),
            rule_results: HashMap::new(),
            rule_proofs: HashMap::new(),
            operations: Vec::new(),
            source_text: plan.graph().sources().clone(),
            proof_nodes: HashMap::new(),
            symbolic_mode: false,
        }
    }

    fn get_fact(&self, fact_path: &FactPath) -> Option<&LiteralValue> {
        self.facts.get(fact_path).and_then(|f| match &f.value {
            crate::FactValue::Literal(lit) => Some(lit),
            _ => None,
        })
    }

    fn is_symbolic(&self) -> bool {
        self.symbolic_mode
    }

    fn push_operation(&mut self, kind: OperationKind) {
        self.operations.push(OperationRecord { kind });
    }

    fn set_proof_node(
        &mut self,
        expr: &crate::Expression,
        node: crate::evaluation::proof::ProofNode,
    ) {
        self.proof_nodes.insert(expr.clone(), node);
    }

    fn get_proof_node(
        &self,
        expr: &crate::Expression,
    ) -> Option<&crate::evaluation::proof::ProofNode> {
        self.proof_nodes.get(expr)
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
    /// Evaluate an execution plan
    ///
    /// Executes rules in pre-computed dependency order with all facts pre-loaded.
    /// Rules are already flattened into executable branches with fact prefixes resolved.
    /// This evaluation never errors - runtime issues create Vetoes instead.
    pub fn evaluate(&self, plan: &ExecutionPlan) -> LemmaResult<Response> {
        let mut context = EvaluationContext::new(plan);

        let mut response = Response {
            doc_name: plan.doc_name.clone(),
            facts: plan.facts.clone(),
            results: IndexMap::new(),
        };

        // Execute each rule in topological order (already sorted by ExecutionPlan)
        for exec_rule in &plan.rules {
            context.operations.clear();
            context.proof_nodes.clear();

            let (result, proof) = expression::evaluate_rule(exec_rule, &mut context)?;

            context
                .rule_results
                .insert(exec_rule.path.clone(), result.clone());
            context.set_rule_proof(exec_rule.path.clone(), proof.clone());

            response.add_result(RuleResult {
                rule: crate::LemmaRule {
                    name: exec_rule.name.clone(),
                    expression: exec_rule.branches[0].result.clone(),
                    unless_clauses: exec_rule.branches[1..]
                        .iter()
                        .map(|b| crate::UnlessClause {
                            condition: b.condition.clone(),
                            result: b.result.clone(),
                            source: b.source.clone(),
                        })
                        .collect(),
                    source: exec_rule.source.clone(),
                },
                result,
                operations: context.operations.clone(),
                proof: Some(proof),
            });
        }

        Ok(response)
    }

    /// Symbolically reduce execution plan using known fact values
    ///
    /// Partially evaluates branch conditions and results using known facts,
    /// leaving unknown facts symbolic. Prunes branches that evaluate to false.
    /// Also prunes earlier branches when a branch becomes unconditionally true
    /// (last-wins optimization).
    ///
    /// Example:
    /// - Plan has branches: `state == "CA" && income > 50000`, `state == "NY" && ...`
    /// - Plan has `state = "CA"` (known), `income` not set (unknown)
    /// - Branch 1 simplifies to: `income > 50000` (keep)
    /// - Branch 2 simplifies to: `false` (pruned)
    ///
    /// This transforms multi-dimensional search (50 states × 4 statuses = 200 paths)
    /// into 1D search (just income) when state and status are known.
    ///
    /// Known facts should be injected into the plan using `with_values`/`with_typed_values` first.
    pub fn evaluate_symbolic(&self, plan: &ExecutionPlan) -> ExecutionPlan {
        use crate::planning::{Branch, ExecutableRule};
        use crate::semantic::{BooleanValue, Expression, ExpressionKind, LiteralValue};

        // Helper: evaluate expression and convert result back to Expression
        fn evaluate_to_expression(
            expr: &Expression,
            context: &mut EvaluationContext,
        ) -> Expression {
            match expression::evaluate_expression(expr, context) {
                Ok(expression::EvaluationResult::Evaluated(OperationResult::Value(lit))) => {
                    // Fully evaluated to literal value
                    Expression::new(
                        ExpressionKind::Literal(lit),
                        expr.source.clone(),
                    )
                }
                Ok(expression::EvaluationResult::Evaluated(OperationResult::Veto(msg))) => {
                    // Evaluated to veto
                    Expression::new(
                        ExpressionKind::Veto(crate::semantic::VetoExpression { message: msg }),
                        expr.source.clone(),
                    )
                }
                Ok(expression::EvaluationResult::Symbolic(e)) => {
                    // Contains unknown facts - return as-is
                    e
                }
                Err(e) => {
                    // Real error - this is a bug
                    panic!("Bug during symbolic evaluation: {}", e)
                }
            }
        }

        let mut context = EvaluationContext::new_symbolic(plan);

        let reduced_rules: Vec<ExecutableRule> = plan
            .rules
            .iter()
            .map(|rule| {
                let mut simplified_branches: Vec<Branch> = Vec::new();

                for branch in &rule.branches {
                    context.operations.clear();
                    context.proof_nodes.clear();

                    // Evaluate condition symbolically
                    let simplified_condition = evaluate_to_expression(&branch.condition, &mut context);

                    // Prune branches that evaluate to false
                    if matches!(
                        &simplified_condition.kind,
                        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::False))
                    ) {
                        continue;
                    }

                    // Evaluate result symbolically
                    let simplified_result = evaluate_to_expression(&branch.result, &mut context);

                    simplified_branches.push(Branch {
                        condition: simplified_condition,
                        optimized_condition: None,
                        result: simplified_result,
                        source: branch.source.clone(),
                    });
                }

                // Last-wins optimization: if a branch is unconditionally true,
                // prune all earlier branches (they'll never be reached)
                let final_branches = if let Some(pos) = simplified_branches.iter().position(|b| {
                    matches!(
                        &b.condition.kind,
                        ExpressionKind::Literal(LiteralValue::Boolean(BooleanValue::True))
                    )
                }) {
                    // Keep only from this position onward
                    simplified_branches.into_iter().skip(pos).collect()
                } else {
                    simplified_branches
                };

                ExecutableRule {
                    path: rule.path.clone(),
                    name: rule.name.clone(),
                    branches: final_branches,
                    needs_facts: rule.needs_facts.clone(),
                    source: rule.source.clone(),
                }
            })
            .collect();

        ExecutionPlan::new(
            plan.doc_name.clone(),
            plan.facts.clone(),
            reduced_rules,
            plan.graph().clone(),
        )
    }
}
