//! Reference and dependency analysis utilities
//!
//! This module provides shared infrastructure for extracting references
//! from expressions and analyzing dependencies between rules.
//!
//! Used by both semantic validation and evaluation.

use crate::{
    Expression, ExpressionKind, FactReference, FactType, FactValue, LemmaDoc, LemmaFact,
    LemmaResult, LemmaRule, RulePath,
};
use std::collections::{HashMap, HashSet};

/// References extracted from an expression
#[derive(Debug, Clone, Default)]
pub struct References {
    /// Fact references (e.g., FactReference with reference ["employee", "name"])
    pub facts: HashSet<FactReference>,
    /// Rule references as raw segments (e.g., ["employee", "is_eligible"])
    /// Note: These are syntactic references, not yet resolved to RulePath which requires semantic context
    pub rules: HashSet<Vec<String>>,
}

/// Extract all fact and rule references from an expression.
///
/// Recursively walks the expression tree to find all references to facts and rules.
/// Useful for dependency analysis and validation.
///
/// # Examples
/// ```text
/// Expression: price * quantity
/// Returns: facts = ["price", "quantity"], rules = []
///
/// Expression: base_amount + adjustment?
/// Returns: facts = ["base_amount"], rules = ["adjustment"]
/// ```
pub fn extract_references(expr: &Expression) -> References {
    let mut refs = References::default();
    collect_references(expr, &mut refs.facts, &mut refs.rules);
    refs
}

/// Recursively collect all fact and rule references from an expression
fn collect_references(
    expr: &Expression,
    fact_refs: &mut HashSet<FactReference>,
    rule_refs: &mut HashSet<Vec<String>>,
) {
    match &expr.kind {
        ExpressionKind::FactReference(fact_ref) => {
            fact_refs.insert(fact_ref.clone());
        }
        ExpressionKind::RuleReference(rule_ref) => {
            rule_refs.insert(rule_ref.reference.clone());
        }
        ExpressionKind::Arithmetic(left, _op, right) => {
            collect_references(left, fact_refs, rule_refs);
            collect_references(right, fact_refs, rule_refs);
        }
        ExpressionKind::Comparison(left, _op, right) => {
            collect_references(left, fact_refs, rule_refs);
            collect_references(right, fact_refs, rule_refs);
        }
        ExpressionKind::LogicalAnd(left, right) => {
            collect_references(left, fact_refs, rule_refs);
            collect_references(right, fact_refs, rule_refs);
        }
        ExpressionKind::LogicalOr(left, right) => {
            collect_references(left, fact_refs, rule_refs);
            collect_references(right, fact_refs, rule_refs);
        }
        ExpressionKind::LogicalNegation(inner, _negation_type) => {
            collect_references(inner, fact_refs, rule_refs);
        }
        ExpressionKind::UnitConversion(value, _target) => {
            collect_references(value, fact_refs, rule_refs);
        }
        ExpressionKind::MathematicalOperator(_op, operand) => {
            collect_references(operand, fact_refs, rule_refs);
        }
        ExpressionKind::FactHasAnyValue(fact_ref) => {
            fact_refs.insert(fact_ref.clone());
        }
        ExpressionKind::Veto(_) | ExpressionKind::Literal(_) => {}
    }
}

/// Recursively find all facts required by a rule, following rule dependencies.
///
/// This traces through the entire dependency tree:
/// - If rule A depends on rule B which depends on fact X, this returns fact X for rule A
/// - Handles cycles gracefully by tracking visited rules
/// - Only returns facts with type annotations (facts without values)
///
/// Returns a set of fact names that are required (directly or transitively).
///
/// # Examples
/// ```text
/// Given:
///   fact quantity: number
///   rule subtotal = price * quantity
///   rule total = subtotal? + shipping
///
/// For rule "total":
///   Returns: {"quantity"} (price and shipping must have values)
/// ```
pub fn find_required_facts_recursive(
    rule: &LemmaRule,
    all_rules: &[LemmaRule],
    document_facts: &[LemmaFact],
) -> HashSet<String> {
    let mut required_facts = HashSet::new();
    let mut visited_rules = HashSet::new();

    collect_required_facts_recursive(
        rule,
        all_rules,
        document_facts,
        &mut required_facts,
        &mut visited_rules,
    );

    required_facts
}

/// Helper function to recursively collect required facts
fn collect_required_facts_recursive(
    rule: &LemmaRule,
    all_rules: &[LemmaRule],
    document_facts: &[LemmaFact],
    required_facts: &mut HashSet<String>,
    visited_rules: &mut HashSet<String>,
) {
    // Prevent infinite recursion from circular dependencies
    if visited_rules.contains(&rule.name) {
        return;
    }
    visited_rules.insert(rule.name.clone());

    // Extract direct fact and rule references
    let refs = extract_references(&rule.expression);
    let mut all_fact_refs = refs.facts;
    let mut all_rule_refs = refs.rules;

    // Collect from unless clauses
    for unless_clause in &rule.unless_clauses {
        let cond_refs = extract_references(&unless_clause.condition);
        all_fact_refs.extend(cond_refs.facts);
        all_rule_refs.extend(cond_refs.rules);

        let res_refs = extract_references(&unless_clause.result);
        all_fact_refs.extend(res_refs.facts);
        all_rule_refs.extend(res_refs.rules);
    }

    // Add direct fact references (only those with type annotations - requiring values)
    for fact_ref in all_fact_refs {
        let fact_name = fact_ref.reference.join(".");
        if let Some(fact) = document_facts
            .iter()
            .find(|f| fact_display_name(f) == fact_name)
        {
            if matches!(fact.value, FactValue::TypeAnnotation(_)) {
                required_facts.insert(fact_name);
            }
        }
    }

    // Recursively process rule dependencies
    for rule_ref in all_rule_refs {
        let rule_name = rule_ref.join(".");
        if let Some(dep_rule) = all_rules.iter().find(|r| r.name == rule_name) {
            collect_required_facts_recursive(
                dep_rule,
                all_rules,
                document_facts,
                required_facts,
                visited_rules,
            );
        }
    }
}

/// Get a display name for a fact
///
/// Local facts use their name directly.
/// Foreign facts join their reference path with dots.
pub fn fact_display_name(fact: &LemmaFact) -> String {
    match &fact.fact_type {
        FactType::Local(name) => name.clone(),
        FactType::Foreign(foreign_ref) => foreign_ref.reference.join("."),
    }
}

/// Extract rule paths from an expression for dependency analysis across document references.
///
/// Resolves rule references to `RulePath` instances that include the full
/// fact traversal path (e.g., `employee.salary?` where `employee` is a fact
/// referencing another document). Used internally by dependency graph building.
fn extract_rule_paths(
    expr: &Expression,
    current_doc: &LemmaDoc,
    all_documents: &HashMap<String, LemmaDoc>,
    paths: &mut HashSet<RulePath>,
) -> LemmaResult<()> {
    match &expr.kind {
        ExpressionKind::RuleReference(rule_ref) => {
            let path = RulePath::from_reference(&rule_ref.reference, current_doc, all_documents)?;
            paths.insert(path);
        }
        ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right)
        | ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right) => {
            extract_rule_paths(left, current_doc, all_documents, paths)?;
            extract_rule_paths(right, current_doc, all_documents, paths)?;
        }
        ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::MathematicalOperator(_, inner) => {
            extract_rule_paths(inner, current_doc, all_documents, paths)?;
        }
        _ => {}
    }
    Ok(())
}

/// Build dependency graph for all reachable rules
///
/// Starting from the document being evaluated, discovers all rules
/// (local + rules from documents referenced by facts) and extracts dependencies in a single traversal.
///
/// Returns: RulePath -> Set of RulePaths it depends on
pub fn build_dependency_graph(
    doc: &LemmaDoc,
    documents: &HashMap<String, LemmaDoc>,
) -> LemmaResult<HashMap<RulePath, HashSet<RulePath>>> {
    use std::collections::VecDeque;

    let mut graph = HashMap::new();
    let mut queue = VecDeque::new();

    // Start with rules from document being evaluated
    for rule in &doc.rules {
        let path = RulePath {
            rule: rule.name.clone(),
            segments: vec![],
        };
        queue.push_back((path, rule, doc));
    }

    // BFS: discover rules and build dependencies simultaneously
    while let Some((path, rule, rule_doc)) = queue.pop_front() {
        // Skip if already processed
        if graph.contains_key(&path) {
            continue;
        }

        // Extract dependencies for this rule (relative to rule_doc)
        let mut relative_dependencies = HashSet::new();
        extract_rule_paths(
            &rule.expression,
            rule_doc,
            documents,
            &mut relative_dependencies,
        )?;
        for uc in &rule.unless_clauses {
            extract_rule_paths(
                &uc.condition,
                rule_doc,
                documents,
                &mut relative_dependencies,
            )?;
            extract_rule_paths(&uc.result, rule_doc, documents, &mut relative_dependencies)?;
        }

        // Transform to full paths (prepend parent segments)
        let full_dependencies: HashSet<_> = relative_dependencies
            .iter()
            .map(|dep_path| {
                if path.segments.is_empty() {
                    dep_path.clone()
                } else {
                    let mut full_segments = path.segments.clone();
                    full_segments.extend_from_slice(&dep_path.segments);
                    crate::RulePath {
                        rule: dep_path.rule.clone(),
                        segments: full_segments,
                    }
                }
            })
            .collect();

        // Store full paths in graph
        graph.insert(path.clone(), full_dependencies.clone());

        // Queue dependencies for discovery
        for full_dep_path in full_dependencies {
            if !graph.contains_key(&full_dep_path) {
                let target_doc_name = full_dep_path.target_doc(&doc.name);
                let target_doc = documents.get(target_doc_name).ok_or_else(|| {
                    crate::LemmaError::Engine(format!(
                        "Rule {} references document '{}' which does not exist",
                        path, target_doc_name
                    ))
                })?;

                let target_rule = target_doc
                    .rules
                    .iter()
                    .find(|r| r.name == full_dep_path.rule)
                    .ok_or_else(|| {
                        crate::LemmaError::Engine(format!(
                            "Rule {} references rule '{}' in document '{}' which does not exist",
                            path, full_dep_path.rule, target_doc_name
                        ))
                    })?;

                queue.push_back((full_dep_path, target_rule, target_doc));
            }
        }
    }

    Ok(graph)
}
