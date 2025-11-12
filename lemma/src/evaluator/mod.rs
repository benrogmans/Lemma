//! Pure Rust evaluation engine for Lemma
//!
//! Rule evaluation
//!
//! Evaluates Lemma docs by:
//! 1. Building a fact map (inputs)
//! 2. Topologically sorting rules (execution plan)
//! 3. Executing rules in dependency order
//! 4. Building response with operation records

pub mod context;
pub mod datetime;
pub mod expression;
pub mod operations;
pub mod rules;
pub mod timeout;
pub mod units;

use crate::{LemmaDoc, LemmaError, LemmaFact, LemmaResult, ResourceLimits, Response, RuleResult};
use context::{build_fact_map, EvaluationContext};
use std::collections::HashMap;
use timeout::TimeoutTracker;

/// Evaluates Lemma rules within their document context
#[derive(Default)]
pub struct Evaluator;

impl Evaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate a Lemma doc
    ///
    /// Executes all rules in the doc in topological order,
    /// applying fact overrides if provided.
    pub fn evaluate_document(
        &self,
        doc_name: &str,
        documents: &HashMap<String, LemmaDoc>,
        sources: &HashMap<String, String>,
        fact_overrides: Vec<LemmaFact>,
        requested_rules: Option<Vec<String>>,
        limits: &ResourceLimits,
    ) -> LemmaResult<Response> {
        let timeout_tracker = TimeoutTracker::new();

        let doc = documents
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        // Phase 1: Build dependency graph and execution plan
        let graph = crate::analysis::build_dependency_graph(doc, documents)?;
        let execution_order = topological_sort(&graph)?;

        // Phase 2: Build fact map (resolving document references and validating types)
        let facts = build_fact_map(doc, &doc.facts, &fact_overrides, documents)?;

        // Phase 3: Build evaluation context
        let mut context =
            EvaluationContext::new(doc, documents, sources, facts, &timeout_tracker, limits);

        // Phase 4: Execute rules in dependency order
        let mut response = Response::new(doc_name.to_string());
        let mut failed_rules: std::collections::HashSet<crate::RulePath> =
            std::collections::HashSet::new();

        for rule_path in execution_order {
            let target_doc_name = rule_path.target_doc(doc_name);
            let rule_doc = documents.get(target_doc_name).ok_or_else(|| {
                LemmaError::Engine(format!("Document {} not found", target_doc_name))
            })?;

            let rule = rule_doc
                .rules
                .iter()
                .find(|r| r.name == rule_path.rule)
                .ok_or_else(|| {
                    LemmaError::Engine(format!(
                        "Rule {} not found in document {}",
                        rule_path.rule, target_doc_name
                    ))
                })?;

            // Check if any dependencies have failed
            let all_rule_deps = graph.get(&rule_path).cloned().unwrap_or_default();

            let missing_deps: Vec<String> = all_rule_deps
                .iter()
                .filter(|dep| failed_rules.contains(dep))
                .map(|dep| dep.to_string())
                .collect();

            if !missing_deps.is_empty() {
                // This rule depends on failed rules - mark it as missing dependencies
                failed_rules.insert(rule_path.clone());
                if target_doc_name == doc_name {
                    response.add_result(RuleResult::missing_facts(rule.name.clone(), missing_deps));
                }
                continue;
            }

            // Clear operation records for this rule
            context.operations.clear();

            // Evaluate the rule with path prefix when the rule is from a document referenced by a fact
            let path_prefix: Vec<String> = if target_doc_name != doc_name {
                // Rule from referenced document: use the fact path as prefix
                // E.g., if evaluating `employee.salary?` where `employee = doc hr_doc`,
                // the prefix is ["employee"] so facts in the rule are looked up as ["employee", "field"]
                rule_path.segments.iter().map(|s| s.fact.clone()).collect()
            } else {
                // Local rule: empty prefix
                Vec::new()
            };
            let eval_result = rules::evaluate_rule(rule, rule_doc, &mut context, &path_prefix);

            match eval_result {
                Ok(result) => {
                    // Store result in context for subsequent rules
                    context
                        .rule_results
                        .insert(rule_path.clone(), result.clone());

                    // Add to response only for main document rules
                    if target_doc_name == doc_name {
                        match result {
                            crate::OperationResult::Value(value) => {
                                response.add_result(RuleResult::success_with_operations(
                                    rule.name.clone(),
                                    value.clone(),
                                    HashMap::new(),
                                    context.operations.clone(),
                                ));
                            }
                            crate::OperationResult::Veto(msg) => {
                                response.add_result(RuleResult::veto(rule.name.clone(), msg));
                            }
                        }
                    }
                }
                Err(LemmaError::Engine(msg)) if msg.starts_with("Missing fact:") => {
                    failed_rules.insert(rule_path.clone());
                    if target_doc_name == doc_name {
                        let missing = vec![msg.replace("Missing fact: ", "")];
                        response.add_result(RuleResult::missing_facts(rule.name.clone(), missing));
                    }
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

/// Topological sort of rules to get execution order.
///
/// Returns rules in an order such that dependencies come before dependents.
/// Graph format: node -> set of rules that node depends on.
pub(crate) fn topological_sort(
    graph: &HashMap<crate::RulePath, std::collections::HashSet<crate::RulePath>>,
) -> LemmaResult<Vec<crate::RulePath>> {
    use std::collections::{HashSet, VecDeque};

    // Build reverse graph: node -> set of rules that depend on node
    let mut reverse_graph: HashMap<crate::RulePath, HashSet<crate::RulePath>> = HashMap::new();
    let mut all_nodes: HashSet<crate::RulePath> = HashSet::new();

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
    let mut dependency_count: HashMap<crate::RulePath, usize> = HashMap::new();
    for node in &all_nodes {
        let count = graph.get(node).map_or(0, |deps| deps.len());
        dependency_count.insert(node.clone(), count);
    }

    // Start with nodes that have no dependencies
    let mut queue: VecDeque<crate::RulePath> = dependency_count
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
