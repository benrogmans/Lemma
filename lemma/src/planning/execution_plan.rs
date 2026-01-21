//! Execution plan for evaluated documents
//!
//! Provides a complete self-contained execution plan ready for the evaluator.
//! The plan contains all facts, rules flattened into executable branches,
//! and execution order - no document structure needed during evaluation.

use crate::parsing::ast::Span;
use crate::planning::graph::Graph;
use crate::semantic::{
    Expression, FactPath, FactReference, FactValue, LemmaType, LiteralValue, RulePath,
};
use crate::LemmaError;
use crate::ResourceLimits;
use crate::Source;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// A complete execution plan ready for the evaluator
///
/// Contains the topologically sorted list of rules to execute, along with all facts.
/// Self-contained structure - no document lookups required during evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Main document name
    pub doc_name: String,

    /// Resolved schema types for value-holding facts.
    ///
    /// This is the authoritative schema contract for adapters and validation.
    #[serde(serialize_with = "crate::serialization::serialize_fact_type_map")]
    #[serde(deserialize_with = "crate::serialization::deserialize_fact_type_map")]
    pub fact_schema: HashMap<FactPath, LemmaType>,

    /// Concrete literal values for facts (document-defined literals + user-provided values).
    #[serde(serialize_with = "crate::serialization::serialize_fact_value_map")]
    #[serde(deserialize_with = "crate::serialization::deserialize_fact_value_map")]
    pub fact_values: HashMap<FactPath, LiteralValue>,

    /// Document reference facts (path -> referenced document name).
    #[serde(serialize_with = "crate::serialization::serialize_fact_doc_ref_map")]
    #[serde(deserialize_with = "crate::serialization::deserialize_fact_doc_ref_map")]
    pub doc_refs: HashMap<FactPath, String>,

    /// Fact-level source information for better errors in adapters/validation.
    #[serde(serialize_with = "crate::serialization::serialize_fact_source_map")]
    #[serde(deserialize_with = "crate::serialization::deserialize_fact_source_map")]
    pub fact_sources: HashMap<FactPath, Source>,

    /// Rules to execute in topological order (sorted by dependencies)
    pub rules: Vec<ExecutableRule>,

    /// Source code for error messages
    pub sources: HashMap<String, String>,
}

/// An executable rule with flattened branches
///
/// Contains all information needed to evaluate a rule without document lookups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableRule {
    /// Unique identifier for this rule
    pub path: RulePath,

    /// Rule name
    pub name: String,

    /// Branches evaluated in order (last matching wins)
    /// First branch has condition=None (default expression)
    /// Subsequent branches have condition=Some(...) (unless clauses)
    /// The evaluation is done in reverse order with the earliest matching branch returning (winning) the result.
    pub branches: Vec<Branch>,

    /// All facts this rule needs (direct + inherited from rule dependencies)
    #[serde(serialize_with = "crate::serialization::serialize_fact_path_set")]
    #[serde(deserialize_with = "crate::serialization::deserialize_fact_path_set")]
    pub needs_facts: HashSet<FactPath>,

    /// Source location for error messages
    pub source: Option<Source>,

    /// Computed type of this rule's result
    /// Every rule MUST have a type (Lemma is strictly typed)
    pub rule_type: LemmaType,
}

/// A branch in an executable rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    /// Condition expression (None for default branch)
    pub condition: Option<Expression>,

    /// Result expression
    pub result: Expression,

    /// Source location for error messages
    pub source: Option<Source>,
}

/// Builds an execution plan from a Graph.
/// Internal implementation detail - only called by plan()
pub(crate) fn build_execution_plan(graph: &Graph, main_doc_name: &str) -> ExecutionPlan {
    let execution_order = graph.execution_order();
    let mut fact_schema: HashMap<FactPath, LemmaType> = HashMap::new();
    let mut fact_values: HashMap<FactPath, LiteralValue> = HashMap::new();
    let mut doc_refs: HashMap<FactPath, String> = HashMap::new();
    let mut fact_sources: HashMap<FactPath, Source> = HashMap::new();

    // Collect facts and compute an authoritative type (schema) for every fact path.
    for (path, fact) in graph.facts().iter() {
        if let Some(src) = fact.source_location.clone() {
            fact_sources.insert(path.clone(), src);
        }
        match &fact.value {
            FactValue::Literal(lit) => {
                fact_values.insert(path.clone(), lit.clone());

                // Check if this literal fact overrides a type-annotated fact
                // If so, we need to resolve the original type and store it in fact_schema
                // This happens when you have: fact x = [money] and then fact one.x = 7
                let fact_ref = FactReference {
                    segments: path.segments.iter().map(|s| s.fact.clone()).collect(),
                    fact: path.fact.clone(),
                };

                // Find the original fact definition in the source documents
                // Use the document from the first segment if available
                let context_doc = if let Some(first_segment) = path.segments.first() {
                    first_segment.doc.as_str()
                } else {
                    // Top-level fact - search for it
                    let fact_ref_segments: Vec<String> =
                        path.segments.iter().map(|s| s.fact.clone()).collect();

                    let mut found_doc = None;
                    for (doc_name, doc) in graph.all_docs() {
                        for orig_fact in &doc.facts {
                            if orig_fact.reference.segments == fact_ref_segments
                                && orig_fact.reference.fact == path.fact
                            {
                                found_doc = Some(doc_name.as_str());
                                break;
                            }
                        }
                        if found_doc.is_some() {
                            break;
                        }
                    }
                    found_doc.unwrap_or(main_doc_name)
                };

                // Look for the original fact in the source document
                // For nested facts like one.x, the original fact is x (top-level in doc "one")
                // So we search for a fact with empty segments and the same fact name
                if let Some(orig_doc) = graph.all_docs().get(context_doc) {
                    for orig_fact in &orig_doc.facts {
                        // The original fact should be top-level (empty segments) with the same name
                        // For one.x, we're looking for fact x in doc "one"
                        if orig_fact.reference.segments.is_empty()
                            && orig_fact.reference.fact == fact_ref.fact
                        {
                            // Found the original fact - check if it has a type declaration
                            if let FactValue::TypeDeclaration { .. } = &orig_fact.value {
                                // Resolve the type from the original fact
                                match graph.resolve_type_declaration(&orig_fact.value, context_doc)
                                {
                                    Ok(lemma_type) => {
                                        fact_schema.insert(path.clone(), lemma_type);
                                    }
                                    Err(e) => {
                                        // Type resolution failed - this should have been caught during validation
                                        // Panic to prevent silent failures
                                        panic!(
                                            "Failed to resolve type for fact {}: {}. This indicates a bug in validation - all types should be validated before execution plan building.",
                                            path, e
                                        );
                                    }
                                }
                            }
                            break;
                        }
                    }
                }

                // If this literal does not correspond to a typed fact declaration, its schema type
                // is inferred from the literal value itself (standard types).
                if !fact_schema.contains_key(path) {
                    fact_schema.insert(path.clone(), lit.get_type().clone());
                }
            }
            FactValue::TypeDeclaration { .. } => {
                // Use TypeRegistry to determine document context and resolve type
                let fact_ref = FactReference {
                    segments: path.segments.iter().map(|s| s.fact.clone()).collect(),
                    fact: path.fact.clone(),
                };

                // For inline type definitions, check if they exist in resolved_types
                // Inline type definitions are already fully resolved during type resolution, so just use them directly
                let mut found_inline_type = false;
                for (_doc_name, document_types) in graph.resolved_types().iter() {
                    if let Some(resolved_type) =
                        document_types.inline_type_definitions.get(&fact_ref)
                    {
                        // Inline type definition already resolved - use it directly
                        fact_schema.insert(path.clone(), resolved_type.clone());
                        found_inline_type = true;
                        break;
                    }
                }
                if found_inline_type {
                    continue; // Skip the rest of the loop iteration
                }

                // Find which document this fact belongs to
                // Use the document from the first segment (set during graph building)
                // This is more reliable than searching, especially for nested facts
                let context_doc = if let Some(first_segment) = path.segments.first() {
                    first_segment.doc.as_str()
                } else {
                    // Top-level fact - search for it
                    let fact_ref_segments: Vec<String> =
                        path.segments.iter().map(|s| s.fact.clone()).collect();

                    let mut found_doc = None;
                    for (doc_name, doc) in graph.all_docs() {
                        for fact in &doc.facts {
                            if fact.reference.segments == fact_ref_segments
                                && fact.reference.fact == path.fact
                            {
                                found_doc = Some(doc_name.as_str());
                                break;
                            }
                        }
                        if found_doc.is_some() {
                            break;
                        }
                    }

                    found_doc.unwrap_or_else(|| {
                        panic!(
                            "Cannot determine document context for fact '{}'. This indicates a bug in graph building.",
                            path
                        );
                    })
                };

                match graph.resolve_type_declaration(&fact.value, context_doc) {
                    Ok(lemma_type) => {
                        fact_schema.insert(path.clone(), lemma_type);
                    }
                    Err(e) => {
                        // This should have been caught during validation, but handle gracefully
                        panic!(
                            "Failed to resolve type for fact {}: {}. This indicates a bug in validation.",
                            path, e
                        );
                    }
                }
            }
            FactValue::DocumentReference(doc_name) => {
                doc_refs.insert(path.clone(), doc_name.clone());
            }
        }
    }

    // Apply default values for facts with TypeDeclaration that don't have literal values
    for (path, schema_type) in &fact_schema {
        if fact_values.contains_key(path) {
            continue; // Fact already has a value, skip
        }
        if let Some(default_value) = schema_type.create_default_value() {
            fact_values.insert(path.clone(), default_value);
        }
    }

    // Ensure literal facts are typed consistently with their declared schema type.
    // If a fact path has a schema type, the stored literal MUST become that type,
    // or we reject it as incompatible.
    //
    // Defensive check: fact_values should only contain LiteralValue entries.
    // If a type definition somehow slipped through validation, this will catch it.
    for (path, value) in fact_values.iter_mut() {
        let Some(schema_type) = fact_schema.get(path).cloned() else {
            continue;
        };

        match coerce_literal_to_schema_type(value, &schema_type) {
            Ok(coerced) => {
                *value = coerced;
            }
            Err(msg) => {
                panic!(
                    "Fact {} literal value is incompatible with declared type {}: {}. \
                     This should have been caught during validation. If you see a type definition here, \
                     it indicates a bug: type definitions cannot override typed facts.",
                    path,
                    schema_type.name(),
                    msg
                );
            }
        }
    }

    let mut executable_rules: Vec<ExecutableRule> = Vec::new();

    for rule_path in execution_order {
        let rule_node = graph.rules().get(rule_path).expect(
            "bug: rule from topological sort not in graph - validation should have caught this",
        );

        let mut executable_branches = Vec::new();
        for (condition, result) in &rule_node.branches {
            executable_branches.push(Branch {
                condition: condition.clone(),
                result: result.clone(),
                source: Some(rule_node.source.clone()),
            });
        }

        executable_rules.push(ExecutableRule {
            path: rule_path.clone(),
            name: rule_path.rule.clone(),
            branches: executable_branches,
            source: Some(rule_node.source.clone()),
            needs_facts: HashSet::new(),
            rule_type: rule_node.rule_type.clone(),
        });
    }

    populate_needs_facts(&mut executable_rules, graph);

    ExecutionPlan {
        doc_name: main_doc_name.to_string(),
        fact_schema,
        fact_values,
        doc_refs,
        fact_sources,
        rules: executable_rules,
        sources: graph.sources().clone(),
    }
}

fn coerce_literal_to_schema_type(
    lit: &LiteralValue,
    schema_type: &LemmaType,
) -> Result<LiteralValue, String> {
    use crate::semantic::TypeSpecification;
    use crate::Value;

    // Fast path: same specification => just retag to carry constraints/options/etc.
    if lit.lemma_type.specifications == schema_type.specifications {
        let mut out = lit.clone();
        out.lemma_type = schema_type.clone();
        return Ok(out);
    }

    match (&schema_type.specifications, &lit.value) {
        // Same value shape; retag.
        (TypeSpecification::Number { .. }, Value::Number(_))
        | (TypeSpecification::Text { .. }, Value::Text(_))
        | (TypeSpecification::Boolean { .. }, Value::Boolean(_))
        | (TypeSpecification::Date { .. }, Value::Date(_))
        | (TypeSpecification::Time { .. }, Value::Time(_))
        | (TypeSpecification::Duration { .. }, Value::Duration(_, _))
        | (TypeSpecification::Ratio { .. }, Value::Ratio(_, _))
        | (TypeSpecification::Scale { .. }, Value::Scale(_, _)) => {
            let mut out = lit.clone();
            out.lemma_type = schema_type.clone();
            Ok(out)
        }

        // Allow a bare numeric literal to satisfy a Scale type (interpreted as base unit).
        (TypeSpecification::Scale { .. }, Value::Number(n)) => {
            Ok(LiteralValue::scale_with_type(*n, None, schema_type.clone()))
        }

        // Allow a bare numeric literal to satisfy a Ratio type (unitless ratio).
        (TypeSpecification::Ratio { .. }, Value::Number(n)) => {
            Ok(LiteralValue::ratio_with_type(*n, None, schema_type.clone()))
        }

        _ => Err(format!(
            "value {} cannot be used as type {}",
            lit,
            schema_type.name()
        )),
    }
}

fn populate_needs_facts(rules: &mut [ExecutableRule], graph: &Graph) {
    let mut rule_facts: HashMap<RulePath, HashSet<FactPath>> = HashMap::new();

    for rule in rules.iter_mut() {
        let mut facts = HashSet::new();

        for branch in &rule.branches {
            if let Some(cond) = &branch.condition {
                cond.collect_fact_paths(&mut facts);
            }
            branch.result.collect_fact_paths(&mut facts);
        }

        if let Some(rule_node) = graph.rules().get(&rule.path) {
            for dep_rule in &rule_node.depends_on_rules {
                if let Some(dep_facts) = rule_facts.get(dep_rule) {
                    facts.extend(dep_facts.iter().cloned());
                }
            }
        }

        rule.needs_facts = facts.clone();
        rule_facts.insert(rule.path.clone(), facts);
    }
}

impl ExecutionPlan {
    /// Look up a fact by its path string (e.g., "age" or "rules.base_price").
    pub fn get_fact_path_by_str(&self, name: &str) -> Option<&FactPath> {
        self.fact_schema
            .keys()
            .find(|path| path.to_string() == name)
    }

    /// Look up a local rule by its name (rule in the main document).
    pub fn get_rule(&self, name: &str) -> Option<&ExecutableRule> {
        self.rules
            .iter()
            .find(|r| r.name == name && r.path.segments.is_empty())
    }

    /// Look up a rule by its full path.
    pub fn get_rule_by_path(&self, rule_path: &RulePath) -> Option<&ExecutableRule> {
        self.rules.iter().find(|r| &r.path == rule_path)
    }

    /// Get the literal value for a fact path, if it exists and has a literal value.
    pub fn get_fact_value(&self, path: &FactPath) -> Option<&LiteralValue> {
        self.fact_values.get(path)
    }

    /// Provide string values for facts by parsing them to their expected types.
    ///
    /// This is the main entry point for providing user-supplied string values.
    /// It parses each string value to the expected type, checks resource limits,
    /// and applies the values to the plan.
    pub fn with_values(
        self,
        values: HashMap<String, String>,
        limits: &ResourceLimits,
    ) -> Result<Self, LemmaError> {
        if values.is_empty() {
            return Ok(self);
        }

        let typed = self.parse_values(values)?;
        self.with_typed_values(typed, limits)
    }

    /// Provide pre-typed values for facts with resource limit checking.
    ///
    /// Use this for programmatic APIs where values are already parsed.
    pub fn with_typed_values(
        mut self,
        values: HashMap<String, LiteralValue>,
        limits: &ResourceLimits,
    ) -> Result<Self, LemmaError> {
        for (name, value) in &values {
            let size = value.byte_size();
            if size > limits.max_fact_value_bytes {
                return Err(LemmaError::ResourceLimitExceeded {
                    limit_name: "max_fact_value_bytes".to_string(),
                    limit_value: limits.max_fact_value_bytes.to_string(),
                    actual_value: size.to_string(),
                    suggestion: format!(
                        "Reduce the size of fact values to {} bytes or less",
                        limits.max_fact_value_bytes
                    ),
                });
            }

            let fact_path = self.get_fact_path_by_str(name).ok_or_else(|| {
                LemmaError::engine(
                    format!("Unknown fact: {}", name),
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    std::sync::Arc::from(""),
                    "<unknown>",
                    1,
                    None::<String>,
                )
            })?;
            let fact_path = fact_path.clone();

            let expected_type = self.fact_schema.get(&fact_path).cloned().ok_or_else(|| {
                LemmaError::engine(
                    format!("Unknown fact: {}", name),
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    std::sync::Arc::from(""),
                    "<unknown>",
                    1,
                    None::<String>,
                )
            })?;
            // Strict type checking: the actual type must match the expected type exactly
            if value.lemma_type.specifications != expected_type.specifications {
                return Err(LemmaError::engine(
                    format!(
                        "Type mismatch for fact {}: expected {}, got {}",
                        name,
                        expected_type.name(),
                        value.lemma_type.name()
                    ),
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    std::sync::Arc::from(""),
                    "<unknown>",
                    1,
                    None::<String>,
                ));
            }

            validate_value_against_type(&expected_type, value).map_err(|msg| {
                LemmaError::engine(
                    format!(
                        "Invalid value for fact {} (expected {}): {}",
                        name,
                        expected_type.name(),
                        msg
                    ),
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    std::sync::Arc::from(""),
                    "<unknown>",
                    1,
                    None::<String>,
                )
            })?;

            self.fact_values.insert(fact_path, value.clone());
        }

        Ok(self)
    }

    fn parse_values(
        &self,
        values: HashMap<String, String>,
    ) -> Result<HashMap<String, LiteralValue>, LemmaError> {
        let mut typed = HashMap::new();

        for (fact_key, raw_value) in values {
            let fact_path = self.get_fact_path_by_str(&fact_key).ok_or_else(|| {
                let available: Vec<String> =
                    self.fact_schema.keys().map(|p| p.to_string()).collect();
                LemmaError::engine(
                    format!(
                        "Fact '{}' not found. Available facts: {}",
                        fact_key,
                        available.join(", ")
                    ),
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    std::sync::Arc::from(""),
                    "<unknown>",
                    1,
                    None::<String>,
                )
            })?;
            let expected_type = self.fact_schema.get(fact_path).cloned().ok_or_else(|| {
                LemmaError::engine(
                    format!("Fact '{}' not found", fact_key),
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    std::sync::Arc::from(""),
                    "<unknown>",
                    1,
                    None::<String>,
                )
            })?;

            let literal_value = expected_type.parse_value(&raw_value).map_err(|e| {
                LemmaError::engine(
                    format!(
                        "Failed to parse fact '{}' as {}: {}",
                        fact_key,
                        expected_type.name(),
                        e
                    ),
                    Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                    "<unknown>",
                    Arc::from(""),
                    &self.doc_name,
                    1,
                    None::<String>,
                )
            })?;

            typed.insert(fact_key, literal_value);
        }

        Ok(typed)
    }
}

fn validate_value_against_type(
    expected_type: &LemmaType,
    value: &LiteralValue,
) -> Result<(), String> {
    use crate::semantic::TypeSpecification;
    use crate::Value;

    let effective_decimals = |n: rust_decimal::Decimal| n.scale();

    match (&expected_type.specifications, &value.value) {
        (
            TypeSpecification::Number {
                minimum,
                maximum,
                decimals,
                ..
            },
            Value::Number(n),
        ) => {
            if let Some(min) = minimum {
                if n < min {
                    return Err(format!("{} is below minimum {}", n, min));
                }
            }
            if let Some(max) = maximum {
                if n > max {
                    return Err(format!("{} is above maximum {}", n, max));
                }
            }
            if let Some(d) = decimals {
                if effective_decimals(*n) > u32::from(*d) {
                    return Err(format!("{} has more than {} decimals", n, d));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Scale {
                minimum,
                maximum,
                decimals,
                ..
            },
            Value::Scale(n, _unit),
        ) => {
            if let Some(min) = minimum {
                if n < min {
                    return Err(format!("{} is below minimum {}", n, min));
                }
            }
            if let Some(max) = maximum {
                if n > max {
                    return Err(format!("{} is above maximum {}", n, max));
                }
            }
            if let Some(d) = decimals {
                if effective_decimals(*n) > u32::from(*d) {
                    return Err(format!("{} has more than {} decimals", n, d));
                }
            }
            Ok(())
        }
        (TypeSpecification::Text { options, .. }, Value::Text(s)) => {
            if !options.is_empty() && !options.iter().any(|opt| opt == s) {
                return Err(format!(
                    "'{}' is not in allowed options: {}",
                    s,
                    options.join(", ")
                ));
            }
            Ok(())
        }
        // If we get here, type mismatch should already have been rejected by the caller.
        _ => Ok(()),
    }
}

pub(crate) fn validate_literal_facts_against_types(plan: &ExecutionPlan) -> Vec<LemmaError> {
    let mut errors = Vec::new();

    for (fact_path, lit) in &plan.fact_values {
        let Some(expected_type) = plan.fact_schema.get(fact_path) else {
            continue;
        };

        if let Err(msg) = validate_value_against_type(expected_type, lit) {
            errors.push(LemmaError::engine(
                format!(
                    "Invalid value for fact {} (expected {}): {}",
                    fact_path,
                    expected_type.name(),
                    msg
                ),
                crate::parsing::ast::Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                std::sync::Arc::from(""),
                "<unknown>",
                1,
                None::<String>,
            ));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::{BooleanValue, Expression, FactPath, LiteralValue, RulePath, Value};
    use serde_json;
    use std::str::FromStr;
    use std::sync::Arc;

    fn default_limits() -> ResourceLimits {
        ResourceLimits::default()
    }

    #[test]
    fn test_with_typed_values() {
        let fact_path = FactPath {
            segments: vec![],
            fact: "age".to_string(),
        };
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: {
                let mut s = HashMap::new();
                s.insert(
                    fact_path.clone(),
                    crate::semantic::standard_number().clone(),
                );
                s
            },
            fact_values: {
                let mut v = HashMap::new();
                v.insert(fact_path.clone(), create_number_literal(25.into()));
                v
            },
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("age".to_string(), create_number_literal(30.into()));

        let updated_plan = plan.with_typed_values(values, &default_limits()).unwrap();
        let updated_value = updated_plan.fact_values.get(&fact_path).unwrap();
        match &updated_value.value {
            Value::Number(n) => assert_eq!(*n, 30.into()),
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    #[test]
    fn test_with_typed_values_type_mismatch() {
        let fact_path = FactPath {
            segments: vec![],
            fact: "age".to_string(),
        };
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: {
                let mut s = HashMap::new();
                s.insert(fact_path, crate::semantic::standard_number().clone());
                s
            },
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("age".to_string(), create_text_literal("thirty".to_string()));

        assert!(plan.with_typed_values(values, &default_limits()).is_err());
    }

    #[test]
    fn test_with_typed_values_unknown_fact() {
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: HashMap::new(),
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("unknown".to_string(), create_number_literal(30.into()));

        assert!(plan.with_typed_values(values, &default_limits()).is_err());
    }

    #[test]
    fn test_with_nested_typed_values() {
        use crate::semantic::PathSegment;
        let fact_path = FactPath {
            segments: vec![PathSegment {
                fact: "rules".to_string(),
                doc: "private".to_string(),
            }],
            fact: "base_price".to_string(),
        };
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: {
                let mut types = HashMap::new();
                types.insert(
                    fact_path.clone(),
                    crate::semantic::standard_number().clone(),
                );
                types
            },
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert(
            "rules.base_price".to_string(),
            create_number_literal(100.into()),
        );

        let updated_plan = plan.with_typed_values(values, &default_limits()).unwrap();
        let updated_value = updated_plan.fact_values.get(&fact_path).unwrap();
        match &updated_value.value {
            Value::Number(n) => assert_eq!(*n, 100.into()),
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    fn create_literal_expr(value: LiteralValue) -> Expression {
        use crate::semantic::ExpressionKind;
        Expression::new(ExpressionKind::Literal(value), None)
    }

    fn create_number_literal(n: rust_decimal::Decimal) -> LiteralValue {
        LiteralValue::number(n)
    }

    fn create_boolean_literal(b: BooleanValue) -> LiteralValue {
        LiteralValue::boolean(b)
    }

    fn create_text_literal(s: String) -> LiteralValue {
        LiteralValue::text(s)
    }

    #[test]
    fn with_values_should_enforce_number_maximum_constraint() {
        // Higher-standard requirement: user input must be validated against type constraints.
        // If this test fails, Lemma accepts invalid values and gives false reassurance.
        let fact_path = FactPath::local("x".to_string());

        let mut fact_schema = HashMap::new();
        let max10 = crate::LemmaType::without_name(crate::TypeSpecification::Number {
            minimum: None,
            maximum: Some(rust_decimal::Decimal::from_str("10").unwrap()),
            decimals: None,
            precision: None,
            help: None,
            default: None,
        });
        fact_schema.insert(fact_path.clone(), max10.clone());

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema,
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("x".to_string(), "11".to_string());

        assert!(
            plan.with_values(values, &default_limits()).is_err(),
            "Providing x=11 should fail due to maximum 10"
        );
    }

    #[test]
    fn with_values_should_enforce_text_enum_options() {
        // Higher-standard requirement: enum options must be enforced for text types.
        let fact_path = FactPath::local("tier".to_string());

        let mut fact_schema = HashMap::new();
        let tier = crate::LemmaType::without_name(crate::TypeSpecification::Text {
            minimum: None,
            maximum: None,
            length: None,
            options: vec!["silver".to_string(), "gold".to_string()],
            help: None,
            default: None,
        });
        fact_schema.insert(fact_path.clone(), tier.clone());

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema,
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("tier".to_string(), "platinum".to_string());

        assert!(
            plan.with_values(values, &default_limits()).is_err(),
            "Invalid enum value should be rejected (tier='platinum')"
        );
    }

    #[test]
    fn with_values_should_enforce_scale_decimals() {
        // Higher-standard requirement: decimals should be enforced on scale inputs,
        // unless the language explicitly defines rounding semantics.
        let fact_path = FactPath::local("price".to_string());

        let mut fact_schema = HashMap::new();
        let money = crate::LemmaType::without_name(crate::TypeSpecification::Scale {
            minimum: None,
            maximum: None,
            decimals: Some(2),
            precision: None,
            units: vec![crate::semantic::Unit {
                name: "eur".to_string(),
                value: rust_decimal::Decimal::from_str("1.0").unwrap(),
            }],
            help: None,
            default: None,
        });
        fact_schema.insert(fact_path.clone(), money.clone());

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema,
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("price".to_string(), "1.234 eur".to_string());

        assert!(
            plan.with_values(values, &default_limits()).is_err(),
            "Scale decimals=2 should reject 1.234 eur"
        );
    }

    #[test]
    fn test_serialize_deserialize_execution_plan() {
        let fact_path = FactPath {
            segments: vec![],
            fact: "age".to_string(),
        };
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: {
                let mut s = HashMap::new();
                s.insert(
                    fact_path.clone(),
                    crate::semantic::standard_number().clone(),
                );
                s
            },
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: {
                let mut s = HashMap::new();
                s.insert("test.lemma".to_string(), "fact age: number".to_string());
                s
            },
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.doc_name, plan.doc_name);
        assert_eq!(deserialized.fact_schema.len(), plan.fact_schema.len());
        assert_eq!(deserialized.fact_values.len(), plan.fact_values.len());
        assert_eq!(deserialized.doc_refs.len(), plan.doc_refs.len());
        assert_eq!(deserialized.fact_sources.len(), plan.fact_sources.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
        assert_eq!(deserialized.sources.len(), plan.sources.len());
    }

    #[test]
    fn test_serialize_deserialize_plan_with_rules() {
        use crate::semantic::ExpressionKind;

        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: HashMap::new(),
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let age_path = FactPath::local("age".to_string());
        plan.fact_schema
            .insert(age_path.clone(), crate::semantic::standard_number().clone());

        let rule = ExecutableRule {
            path: RulePath::local("can_drive".to_string()),
            name: "can_drive".to_string(),
            branches: vec![Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Arc::new(Expression::new(
                            ExpressionKind::FactPath(age_path.clone()),
                            None,
                        )),
                        crate::ComparisonComputation::GreaterThanOrEqual,
                        Arc::new(create_literal_expr(create_number_literal(18.into()))),
                    ),
                    None,
                )),
                result: create_literal_expr(create_boolean_literal(crate::BooleanValue::True)),
                source: None,
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(age_path);
                set
            },
            source: None,
            rule_type: crate::semantic::standard_boolean().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.doc_name, plan.doc_name);
        assert_eq!(deserialized.fact_schema.len(), plan.fact_schema.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
        assert_eq!(deserialized.rules[0].name, "can_drive");
        assert_eq!(deserialized.rules[0].branches.len(), 1);
        assert_eq!(deserialized.rules[0].needs_facts.len(), 1);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_nested_fact_paths() {
        use crate::semantic::PathSegment;
        let fact_path = FactPath {
            segments: vec![PathSegment {
                fact: "employee".to_string(),
                doc: "private".to_string(),
            }],
            fact: "salary".to_string(),
        };

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: {
                let mut s = HashMap::new();
                s.insert(
                    fact_path.clone(),
                    crate::semantic::standard_number().clone(),
                );
                s
            },
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.fact_schema.len(), 1);
        let (deserialized_path, _) = deserialized.fact_schema.iter().next().unwrap();
        assert_eq!(deserialized_path.segments.len(), 1);
        assert_eq!(deserialized_path.segments[0].fact, "employee");
        assert_eq!(deserialized_path.fact, "salary");
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_fact_types() {
        let name_path = FactPath::local("name".to_string());
        let age_path = FactPath::local("age".to_string());
        let active_path = FactPath::local("active".to_string());

        let mut fact_schema = HashMap::new();
        fact_schema.insert(name_path.clone(), crate::semantic::standard_text().clone());
        fact_schema.insert(age_path.clone(), crate::semantic::standard_number().clone());
        fact_schema.insert(
            active_path.clone(),
            crate::semantic::standard_boolean().clone(),
        );

        let mut fact_values = HashMap::new();
        fact_values.insert(name_path.clone(), create_text_literal("Alice".to_string()));
        fact_values.insert(age_path.clone(), create_number_literal(30.into()));
        fact_values.insert(
            active_path.clone(),
            create_boolean_literal(crate::BooleanValue::True),
        );

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema,
            fact_values,
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.fact_values.len(), 3);

        assert_eq!(
            deserialized.fact_values.get(&name_path).unwrap().value,
            Value::Text("Alice".to_string())
        );
        assert_eq!(
            deserialized.fact_values.get(&age_path).unwrap().value,
            Value::Number(30.into())
        );
        assert_eq!(
            deserialized.fact_values.get(&active_path).unwrap().value,
            Value::Boolean(crate::BooleanValue::True)
        );
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_branches() {
        use crate::semantic::ExpressionKind;

        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: HashMap::new(),
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let points_path = FactPath::local("points".to_string());
        plan.fact_schema.insert(
            points_path.clone(),
            crate::semantic::standard_number().clone(),
        );

        let rule = ExecutableRule {
            path: RulePath::local("tier".to_string()),
            name: "tier".to_string(),
            branches: vec![
                Branch {
                    condition: None,
                    result: create_literal_expr(create_text_literal("bronze".to_string())),
                    source: None,
                },
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Arc::new(Expression::new(
                                ExpressionKind::FactPath(points_path.clone()),
                                None,
                            )),
                            crate::ComparisonComputation::GreaterThanOrEqual,
                            Arc::new(create_literal_expr(create_number_literal(100.into()))),
                        ),
                        None,
                    )),
                    result: create_literal_expr(create_text_literal("silver".to_string())),
                    source: None,
                },
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Arc::new(Expression::new(
                                ExpressionKind::FactPath(points_path.clone()),
                                None,
                            )),
                            crate::ComparisonComputation::GreaterThanOrEqual,
                            Arc::new(create_literal_expr(create_number_literal(500.into()))),
                        ),
                        None,
                    )),
                    result: create_literal_expr(create_text_literal("gold".to_string())),
                    source: None,
                },
            ],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(points_path);
                set
            },
            source: None,
            rule_type: crate::semantic::standard_text().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.rules.len(), 1);
        assert_eq!(deserialized.rules[0].branches.len(), 3);
        assert!(deserialized.rules[0].branches[0].condition.is_none());
        assert!(deserialized.rules[0].branches[1].condition.is_some());
        assert!(deserialized.rules[0].branches[2].condition.is_some());
    }

    #[test]
    fn test_serialize_deserialize_empty_plan() {
        let plan = ExecutionPlan {
            doc_name: "empty".to_string(),
            fact_schema: HashMap::new(),
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.doc_name, "empty");
        assert_eq!(deserialized.fact_schema.len(), 0);
        assert_eq!(deserialized.fact_values.len(), 0);
        assert_eq!(deserialized.rules.len(), 0);
        assert_eq!(deserialized.sources.len(), 0);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_arithmetic_expressions() {
        use crate::semantic::ExpressionKind;

        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: HashMap::new(),
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let x_path = FactPath::local("x".to_string());
        plan.fact_schema
            .insert(x_path.clone(), crate::semantic::standard_number().clone());

        let rule = ExecutableRule {
            path: RulePath::local("doubled".to_string()),
            name: "doubled".to_string(),
            branches: vec![Branch {
                condition: None,
                result: Expression::new(
                    ExpressionKind::Arithmetic(
                        Arc::new(Expression::new(
                            ExpressionKind::FactPath(x_path.clone()),
                            None,
                        )),
                        crate::ArithmeticComputation::Multiply,
                        Arc::new(create_literal_expr(create_number_literal(2.into()))),
                    ),
                    None,
                ),
                source: None,
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(x_path);
                set
            },
            source: None,
            rule_type: crate::semantic::standard_number().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.rules.len(), 1);
        match &deserialized.rules[0].branches[0].result.kind {
            ExpressionKind::Arithmetic(left, op, right) => {
                assert_eq!(*op, crate::ArithmeticComputation::Multiply);
                match &left.kind {
                    ExpressionKind::FactPath(_) => {}
                    _ => panic!("Expected FactPath in left operand"),
                }
                match &right.kind {
                    ExpressionKind::Literal(_) => {}
                    _ => panic!("Expected Literal in right operand"),
                }
            }
            _ => panic!("Expected Arithmetic expression"),
        }
    }

    #[test]
    fn test_serialize_deserialize_round_trip_equality() {
        use crate::semantic::ExpressionKind;

        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            fact_schema: HashMap::new(),
            fact_values: HashMap::new(),
            doc_refs: HashMap::new(),
            fact_sources: HashMap::new(),
            rules: Vec::new(),
            sources: {
                let mut s = HashMap::new();
                s.insert("test.lemma".to_string(), "fact age: number".to_string());
                s
            },
        };

        let age_path = FactPath::local("age".to_string());
        plan.fact_schema
            .insert(age_path.clone(), crate::semantic::standard_number().clone());

        let rule = ExecutableRule {
            path: RulePath::local("is_adult".to_string()),
            name: "is_adult".to_string(),
            branches: vec![Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Arc::new(Expression::new(
                            ExpressionKind::FactPath(age_path.clone()),
                            None,
                        )),
                        crate::ComparisonComputation::GreaterThanOrEqual,
                        Arc::new(create_literal_expr(create_number_literal(18.into()))),
                    ),
                    None,
                )),
                result: create_literal_expr(create_boolean_literal(crate::BooleanValue::True)),
                source: None,
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(age_path);
                set
            },
            source: None,
            rule_type: crate::semantic::standard_boolean().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        let json2 = serde_json::to_string(&deserialized).expect("Should serialize again");
        let deserialized2: ExecutionPlan =
            serde_json::from_str(&json2).expect("Should deserialize again");

        assert_eq!(deserialized2.doc_name, plan.doc_name);
        assert_eq!(deserialized2.fact_schema.len(), plan.fact_schema.len());
        assert_eq!(deserialized2.rules.len(), plan.rules.len());
        assert_eq!(deserialized2.sources.len(), plan.sources.len());
        assert_eq!(deserialized2.rules[0].name, plan.rules[0].name);
        assert_eq!(
            deserialized2.rules[0].branches.len(),
            plan.rules[0].branches.len()
        );
    }
}
