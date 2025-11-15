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

use crate::{
    Fact, LemmaDoc, LemmaError, LemmaFact, LemmaResult, ResourceLimits, Response, RuleResult,
};
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

        // Phase 4: Collect all facts for response
        let mut response = Response {
            doc_name: doc_name.to_string(),
            facts: doc
                .facts
                .iter()
                .filter_map(|fact_def| {
                    if let crate::FactType::Local(name) = &fact_def.fact_type {
                        let fact_ref = crate::FactReference {
                            reference: vec![name.clone()],
                        };
                        Some(Fact {
                            name: name.clone(),
                            value: context.facts.get(&fact_ref).cloned(),
                        })
                    } else {
                        None
                    }
                })
                .collect(),
            results: vec![],
        };

        // Phase 5: Execute rules in dependency order
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

            // Check if any rule dependencies have failed
            let rule_dependencies = graph.get(&rule_path).cloned().unwrap_or_default();
            let has_failed_rule_dependencies = rule_dependencies
                .iter()
                .any(|dependency_rule| failed_rules.contains(dependency_rule));

            if has_failed_rule_dependencies {
                failed_rules.insert(rule_path.clone());
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
                    // Store operations for this rule so they can be inlined when referenced
                    context
                        .rule_operations
                        .insert(rule_path.clone(), context.operations.clone());

                    // Store result in context for subsequent rules
                    context
                        .rule_results
                        .insert(rule_path.clone(), result.clone());

                    // Add to response only for main document rules
                    if target_doc_name == doc_name {
                        match result {
                            crate::OperationResult::Value(value) => {
                                response.add_result(RuleResult {
                                    rule: rule.clone(),
                                    result: Some(value.clone()),
                                    facts: collect_facts_from_operations(
                                        &context.operations,
                                        &context.facts,
                                    ),
                                    veto_message: None,
                                    operations: context.operations.clone(),
                                });
                            }
                            crate::OperationResult::Veto(msg) => {
                                response.add_result(RuleResult {
                                    rule: rule.clone(),
                                    result: None,
                                    facts: vec![],
                                    veto_message: msg,
                                    operations: vec![],
                                });
                            }
                        }
                    }
                }
                Err(LemmaError::MissingFact(fact_ref)) => {
                    failed_rules.insert(rule_path.clone());
                    if target_doc_name == doc_name {
                        response.add_result(RuleResult {
                            rule: rule.clone(),
                            result: None,
                            facts: vec![Fact {
                                name: fact_ref.to_string(),
                                value: None,
                            }],
                            veto_message: None,
                            operations: vec![],
                        });
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

        for dependency_rule in dependencies {
            all_nodes.insert(dependency_rule.clone());
            reverse_graph
                .entry(dependency_rule.clone())
                .or_default()
                .insert(node.clone());
        }
    }

    // Count how many rule dependencies each node has
    let mut dependency_count: HashMap<crate::RulePath, usize> = HashMap::new();
    for node in &all_nodes {
        let count = graph
            .get(node)
            .map_or(0, |rule_dependencies| rule_dependencies.len());
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

/// Collect facts from OperationRecord::FactUsed records
fn collect_facts_from_operations(
    operations: &[crate::OperationRecord],
    _fact_map: &HashMap<crate::FactReference, crate::LiteralValue>,
) -> Vec<Fact> {
    use crate::OperationRecord;
    let mut facts = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for op in operations {
        if let OperationRecord {
            kind: crate::OperationKind::FactUsed { fact_ref, value },
            ..
        } = op
        {
            let name = fact_ref.to_string();
            if seen.insert(name.clone()) {
                facts.push(Fact {
                    name,
                    value: Some(value.clone()),
                });
            }
        }
    }

    facts
}
