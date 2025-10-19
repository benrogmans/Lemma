//! Reference and dependency analysis utilities
//!
//! This module provides shared infrastructure for extracting references
//! from expressions and analyzing dependencies between rules.
//!
//! Used by both semantic validation and evaluation.

use crate::{Expression, ExpressionKind, FactType, FactValue, LemmaFact, LemmaRule, RuleResult};
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
/// Returns a map: rule_name -> set of rule names it depends on.
/// This graph is used for topological sorting to determine execution order
/// and for detecting circular dependencies.
///
/// # Examples
/// ```text
/// Given rules:
///   rule total = subtotal?
///   rule subtotal = price * quantity
///
/// Returns:
///   {"total": {"subtotal"}, "subtotal": {}}
/// ```
pub fn build_dependency_graph(rules: &[LemmaRule]) -> HashMap<String, HashSet<String>> {
    let mut graph = HashMap::new();

    for rule in rules {
        let mut dependencies = HashSet::new();

        // Extract rule references from the main expression
        extract_rule_references(&rule.expression, &mut dependencies);

        // Extract rule references from unless clauses
        for unless_clause in &rule.unless_clauses {
            extract_rule_references(&unless_clause.condition, &mut dependencies);
            extract_rule_references(&unless_clause.result, &mut dependencies);
        }

        graph.insert(rule.name.clone(), dependencies);
    }

    graph
}

/// Extract only rule references from an expression
fn extract_rule_references(expr: &Expression, references: &mut HashSet<String>) {
    match &expr.kind {
        ExpressionKind::RuleReference(rule_ref) => {
            let rule_name = if rule_ref.reference.len() > 1 {
                rule_ref.reference.join(".")
            } else {
                rule_ref.reference.last().unwrap_or(&String::new()).clone()
            };
            references.insert(rule_name);
        }
        ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right)
        | ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right) => {
            extract_rule_references(left, references);
            extract_rule_references(right, references);
        }
        ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::MathematicalOperator(_, inner) => {
            extract_rule_references(inner, references);
        }
        ExpressionKind::Veto(_)
        | ExpressionKind::FactHasAnyValue(_)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::Literal(_) => {}
    }
}

/// Find all missing facts and rules for a rule.
///
/// Returns (missing_facts, missing_rules) where:
/// - missing_facts: Facts that have type annotations (not provided)
/// - missing_rules: Rules that this rule depends on that couldn't be evaluated
///
/// Used to provide helpful error messages about what inputs are needed
/// to successfully evaluate a rule.
///
/// # Examples
/// ```text
/// Given:
///   fact price: number
///   rule total = price * 2
///
/// Returns: (["price [number]"], [])
/// ```
pub fn find_missing_dependencies(
    rule: &LemmaRule,
    document_facts: &[LemmaFact],
    evaluated_results: &[RuleResult],
) -> (Vec<String>, Vec<String>) {
    let refs = extract_references(&rule.expression);

    // Also collect from unless clauses
    let mut all_fact_refs = refs.facts;
    let mut all_rule_refs = refs.rules;

    for unless_clause in &rule.unless_clauses {
        let unless_refs = extract_references(&unless_clause.condition);
        all_fact_refs.extend(unless_refs.facts);
        all_rule_refs.extend(unless_refs.rules);

        let result_refs = extract_references(&unless_clause.result);
        all_fact_refs.extend(result_refs.facts);
        all_rule_refs.extend(result_refs.rules);
    }

    // Find missing facts (have type annotations)
    let mut missing_facts = Vec::new();
    for fact_ref in all_fact_refs {
        let fact_name = fact_ref.join(".");

        if let Some(fact) = document_facts
            .iter()
            .find(|f| fact_display_name(f) == fact_name)
        {
            if let FactValue::TypeAnnotation(type_ann) = &fact.value {
                let formatted = format!("{} [{}]", fact_name, format_type_annotation(type_ann));
                missing_facts.push(formatted);
            }
        }
    }

    // Find missing rules (couldn't be evaluated or have missing facts)
    let mut missing_rules = Vec::new();
    for rule_ref in all_rule_refs {
        let rule_name = rule_ref.join(".");

        // Check if this rule was evaluated successfully
        if let Some(result) = evaluated_results.iter().find(|r| r.rule_name == rule_name) {
            // If it has no result or has missing_facts, it couldn't be evaluated
            if result.result.is_none() {
                missing_rules.push(rule_name);
            }
        }
    }

    missing_facts.sort();
    missing_rules.sort();

    (missing_facts, missing_rules)
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

/// Format a type annotation for display
fn format_type_annotation(type_ann: &crate::TypeAnnotation) -> String {
    use crate::{LemmaType, TypeAnnotation};
    match type_ann {
        TypeAnnotation::LemmaType(lemma_type) => match lemma_type {
            LemmaType::Boolean => "boolean".to_string(),
            LemmaType::Number => "number".to_string(),
            LemmaType::Money => "money".to_string(),
            LemmaType::Text => "text".to_string(),
            LemmaType::Date => "date".to_string(),
            LemmaType::Duration => "duration".to_string(),
            LemmaType::Percentage => "percentage".to_string(),
            LemmaType::Mass => "mass".to_string(),
            LemmaType::Length => "length".to_string(),
            LemmaType::Volume => "volume".to_string(),
            LemmaType::Data => "datasize".to_string(),
            LemmaType::Energy => "energy".to_string(),
            LemmaType::Power => "power".to_string(),
            LemmaType::Pressure => "pressure".to_string(),
            LemmaType::Temperature => "temperature".to_string(),
            LemmaType::Force => "force".to_string(),
            LemmaType::Frequency => "frequency".to_string(),
            LemmaType::Regex => "regex".to_string(),
        },
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
