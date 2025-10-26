//! Reference and dependency analysis utilities
//!
//! This module provides shared infrastructure for extracting references
//! from expressions and analyzing dependencies between rules.
//!
//! Used by both semantic validation and evaluation.

use crate::{
    Expression, ExpressionKind, FactType, FactValue, LemmaDoc, LemmaFact, LemmaResult, LemmaRule,
    RulePath,
};
use std::collections::{HashMap, HashSet};

/// References extracted from an expression
#[derive(Debug, Clone, Default)]
pub struct References {
    /// Fact references (e.g., ["employee", "name"])
    pub facts: HashSet<Vec<String>>,
    /// Rule references (e.g., ["employee", "is_eligible"])
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
    fact_refs: &mut HashSet<Vec<String>>,
    rule_refs: &mut HashSet<Vec<String>>,
) {
    match &expr.kind {
        ExpressionKind::FactReference(fact_ref) => {
            fact_refs.insert(fact_ref.reference.clone());
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
            fact_refs.insert(fact_ref.reference.clone());
        }
        ExpressionKind::Veto(_) | ExpressionKind::Literal(_) => {}
    }
}

/// Build a dependency graph showing which rules depend on which other rules.
///
/// Returns a map: RulePath -> set of RulePaths it depends on.
/// This graph is used for topological sorting to determine execution order
/// and for detecting circular dependencies.
pub fn build_dependency_graph(
    rules: &[(RulePath, &LemmaRule)],
    main_doc_name: &str,
    all_documents: &HashMap<String, LemmaDoc>,
) -> LemmaResult<HashMap<RulePath, HashSet<RulePath>>> {
    let mut graph = HashMap::new();

    for (rule_path, rule) in rules {
        let mut dependencies = HashSet::new();

        let doc_name = rule_path.target_doc(main_doc_name);
        let rule_doc = all_documents
            .get(doc_name)
            .ok_or_else(|| crate::LemmaError::Engine(format!("Document {} not found", doc_name)))?;

        extract_rule_paths(&rule.expression, rule_doc, all_documents, &mut dependencies)?;
        for uc in &rule.unless_clauses {
            extract_rule_paths(&uc.condition, rule_doc, all_documents, &mut dependencies)?;
            extract_rule_paths(&uc.result, rule_doc, all_documents, &mut dependencies)?;
        }

        graph.insert(rule_path.clone(), dependencies);
    }

    Ok(graph)
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
        let fact_name = fact_ref.join(".");
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

/// Extract rule paths from an expression for cross-document dependency analysis.
///
/// Resolves rule references to `RulePath` instances that include the full
/// document traversal path. Used by both rule discovery and dependency graph building.
pub fn extract_rule_paths(
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
