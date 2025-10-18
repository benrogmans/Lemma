//! Pure Rust evaluation engine for Lemma
//!
//! Evaluates Lemma documents by:
//! 1. Building a fact map (inputs)
//! 2. Topologically sorting rules (execution plan)
//! 3. Executing rules in dependency order
//! 4. Building response with traces

pub mod context;
pub mod datetime;
pub mod expression;
pub mod operations;
pub mod rules;
pub mod units;

use crate::{LemmaDoc, LemmaError, LemmaFact, LemmaResult, Response, RuleResult};
use context::{build_fact_map, EvaluationContext};
use std::collections::HashMap;

/// Stateless evaluator for Lemma documents
pub struct Evaluator;

impl Evaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate a Lemma document
    ///
    /// Executes all rules in the document in topological order,
    /// applying fact overrides if provided.
    pub fn evaluate_document(
        &self,
        doc_name: &str,
        documents: &HashMap<String, LemmaDoc>,
        sources: &HashMap<String, String>,
        fact_overrides: Vec<LemmaFact>,
        requested_rules: Option<Vec<String>>,
    ) -> LemmaResult<Response> {
        let doc = documents
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        // Phase 1: Build fact map (resolving document references)
        let facts = build_fact_map(&doc.facts, &fact_overrides, documents);

        // Phase 2: Build execution plan (topological sort of rules)
        let execution_order = build_execution_plan(&doc.rules)?;

        // Phase 3: Build evaluation context
        let mut context = EvaluationContext::new(doc, documents, sources, facts);

        // Phase 4: Execute rules in dependency order
        let mut response = Response::new(doc_name.to_string());
        let mut failed_rules: std::collections::HashSet<String> = std::collections::HashSet::new();

        for rule_name in execution_order {
            let rule = doc
                .rules
                .iter()
                .find(|r| r.name == rule_name)
                .ok_or_else(|| LemmaError::Engine(format!("Rule {} not found", rule_name)))?;

            // Check if any dependencies have failed
            let mut all_rule_deps = std::collections::HashSet::new();

            // Extract from main expression
            let refs = crate::analysis::extract_references(&rule.expression);
            for rule_ref in refs.rules {
                all_rule_deps.insert(rule_ref.join("."));
            }

            // Extract from unless clauses
            for uc in &rule.unless_clauses {
                let cond_refs = crate::analysis::extract_references(&uc.condition);
                let res_refs = crate::analysis::extract_references(&uc.result);
                for rule_ref in cond_refs.rules.into_iter().chain(res_refs.rules) {
                    all_rule_deps.insert(rule_ref.join("."));
                }
            }

            let mut missing_deps = Vec::new();
            for dep_name in all_rule_deps {
                if failed_rules.contains(&dep_name) {
                    missing_deps.push(dep_name);
                }
            }

            if !missing_deps.is_empty() {
                // This rule depends on failed rules - mark it as missing dependencies
                failed_rules.insert(rule.name.clone());
                response.add_result(RuleResult::missing_facts(rule.name.clone(), missing_deps));
                continue;
            }

            // Clear trace for this rule
            context.trace.clear();

            // Evaluate the rule
            match rules::evaluate_rule(rule, &mut context) {
                Ok(value) => {
                    // Store result in context for subsequent rules
                    context
                        .rule_results
                        .insert(rule.name.clone(), value.clone());

                    // Add to response
                    response.add_result(RuleResult::success_with_trace(
                        rule.name.clone(),
                        value,
                        HashMap::new(),
                        context.trace.clone(),
                    ));
                }
                Err(LemmaError::Veto(msg)) => {
                    // Mark the rule as vetoed in the context
                    // This allows other rules to reference it without getting "not yet computed" errors
                    context.vetoed_rules.insert(rule.name.clone(), msg.clone());
                    response.add_result(RuleResult::veto(rule.name.clone(), msg));
                }
                Err(LemmaError::Engine(msg)) if msg.starts_with("Missing fact:") => {
                    failed_rules.insert(rule.name.clone());
                    let missing = vec![msg.replace("Missing fact: ", "")];
                    response.add_result(RuleResult::missing_facts(rule.name.clone(), missing));
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        // Filter response to only requested rules if specified
        if let Some(rule_names) = requested_rules {
            response.filter_rules(&rule_names);
        }

        Ok(response)
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Build an execution plan for rules using topological sort
///
/// Returns rules in dependency order (dependencies before dependents)
fn build_execution_plan(rules: &[crate::LemmaRule]) -> LemmaResult<Vec<String>> {
    // Build dependency graph (rule -> rules it depends on)
    let graph = crate::analysis::build_dependency_graph(rules);

    // Topological sort to get execution order
    topological_sort(&graph)
}

/// Topological sort of rules to get execution order.
///
/// Returns rules in an order such that dependencies come before dependents.
/// Graph format: node -> set of rules that node depends on.
pub(crate) fn topological_sort(
    graph: &HashMap<String, std::collections::HashSet<String>>,
) -> LemmaResult<Vec<String>> {
    use std::collections::{HashSet, VecDeque};

    // Build reverse graph: node -> set of rules that depend on node
    let mut reverse_graph: HashMap<String, HashSet<String>> = HashMap::new();
    let mut all_nodes: HashSet<String> = HashSet::new();

    for (node, dependencies) in graph {
        all_nodes.insert(node.clone());
        reverse_graph.entry(node.clone()).or_default();

        for dep in dependencies {
            all_nodes.insert(dep.clone());
            reverse_graph
                .entry(dep.clone())
                .or_default()
                .insert(node.clone());
        }
    }

    // Count how many dependencies each node has
    let mut dependency_count: HashMap<String, usize> = HashMap::new();
    for node in &all_nodes {
        let count = graph.get(node).map(|deps| deps.len()).unwrap_or(0);
        dependency_count.insert(node.clone(), count);
    }

    // Start with nodes that have no dependencies
    let mut queue: VecDeque<String> = dependency_count
        .iter()
        .filter(|(_, &count)| count == 0)
        .map(|(node, _)| node.clone())
        .collect();

    let mut result = Vec::new();

    // Process nodes in order
    while let Some(node) = queue.pop_front() {
        result.push(node.clone());

        // For each node that depends on this one
        if let Some(dependents) = reverse_graph.get(&node) {
            for dependent in dependents {
                // Decrease its dependency count
                if let Some(count) = dependency_count.get_mut(dependent) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }
    }

    // If we haven't processed all nodes, there's a cycle
    if result.len() != all_nodes.len() {
        return Err(LemmaError::Engine(
            "Circular dependency detected in rules (validator should have caught this)".to_string(),
        ));
    }

    Ok(result)
}
