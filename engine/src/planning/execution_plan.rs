//! Execution plan for evaluated documents
//!
//! Provides a complete self-contained execution plan ready for the evaluator.
//! The plan contains all facts, rules flattened into executable branches,
//! and execution order - no document structure needed during evaluation.

use crate::planning::graph::Graph;
use crate::planning::semantics;
use crate::planning::semantics::{
    Expression, FactData, FactPath, LemmaType, LiteralValue, RulePath, TypeSpecification, ValueKind,
};
use crate::Error;
use crate::ResourceLimits;
use crate::Source;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A complete execution plan ready for the evaluator
///
/// Contains the topologically sorted list of rules to execute, along with all facts.
/// Self-contained structure - no document lookups required during evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Main document name
    pub doc_name: String,

    /// Per-fact data: value, type-only, or document reference (aligned with FactData).
    #[serde(serialize_with = "crate::serialization::serialize_resolved_fact_value_map")]
    #[serde(deserialize_with = "crate::serialization::deserialize_resolved_fact_value_map")]
    pub facts: HashMap<FactPath, FactData>,

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

    /// Source location for error messages (always present for rules from parsed documents)
    pub source: Source,

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

    /// Source location for error messages (always present for branches from parsed documents)
    pub source: Source,
}

/// Builds an execution plan from a Graph.
/// Internal implementation detail - only called by plan()
pub(crate) fn build_execution_plan(graph: &Graph, main_doc_name: &str) -> ExecutionPlan {
    let facts = graph.build_facts();
    let execution_order = graph.execution_order();

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
                source: rule_node.source.clone(),
            });
        }

        executable_rules.push(ExecutableRule {
            path: rule_path.clone(),
            name: rule_path.rule.clone(),
            branches: executable_branches,
            source: rule_node.source.clone(),
            needs_facts: HashSet::new(),
            rule_type: rule_node.rule_type.clone(),
        });
    }

    populate_needs_facts(&mut executable_rules, graph);

    ExecutionPlan {
        doc_name: main_doc_name.to_string(),
        facts,
        rules: executable_rules,
        sources: graph.sources().clone(),
    }
}

fn populate_needs_facts(rules: &mut [ExecutableRule], graph: &Graph) {
    // Compute direct fact references per rule.
    let mut direct: HashMap<RulePath, HashSet<FactPath>> = HashMap::new();
    for rule in rules.iter() {
        let mut facts = HashSet::new();
        for branch in &rule.branches {
            if let Some(cond) = &branch.condition {
                cond.collect_fact_paths(&mut facts);
            }
            branch.result.collect_fact_paths(&mut facts);
        }
        direct.insert(rule.path.clone(), facts);
    }

    // Compute transitive closure over rule dependencies (order-independent).
    fn compute_all_facts(
        rule_path: &RulePath,
        graph: &Graph,
        direct: &HashMap<RulePath, HashSet<FactPath>>,
        memo: &mut HashMap<RulePath, HashSet<FactPath>>,
        visiting: &mut HashSet<RulePath>,
    ) -> HashSet<FactPath> {
        if let Some(cached) = memo.get(rule_path) {
            return cached.clone();
        }

        // Defensive: graph is expected to be acyclic after validation.
        if !visiting.insert(rule_path.clone()) {
            return direct.get(rule_path).cloned().unwrap_or_default();
        }

        let mut out = direct.get(rule_path).cloned().unwrap_or_default();
        if let Some(node) = graph.rules().get(rule_path) {
            for dep in &node.depends_on_rules {
                // Only include dependencies that exist in the executable set.
                if direct.contains_key(dep) {
                    out.extend(compute_all_facts(dep, graph, direct, memo, visiting));
                }
            }
        }

        visiting.remove(rule_path);
        memo.insert(rule_path.clone(), out.clone());
        out
    }

    let mut memo: HashMap<RulePath, HashSet<FactPath>> = HashMap::new();
    let mut visiting: HashSet<RulePath> = HashSet::new();

    for rule in rules.iter_mut() {
        rule.needs_facts = compute_all_facts(&rule.path, graph, &direct, &mut memo, &mut visiting);
    }
}

/// A document's public interface: its facts (inputs) and rules (outputs) with
/// full structured type information.
///
/// Built from an [`ExecutionPlan`] via [`ExecutionPlan::schema`] (all facts and
/// rules) or [`ExecutionPlan::schema_for_rules`] (scoped to specific rules and
/// only the facts they need).
///
/// Shared by the HTTP server, the CLI, the MCP server, WASM, and any other
/// consumer. Carries the real [`LemmaType`] and [`LiteralValue`] so consumers
/// can work at whatever fidelity they need — structured types for input forms,
/// or `Display` for plain text.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentSchema {
    /// Document name
    pub doc: String,
    /// Facts (inputs) keyed by name: (type, optional default value)
    pub facts: indexmap::IndexMap<String, (LemmaType, Option<LiteralValue>)>,
    /// Rules (outputs) keyed by name, with their computed result types
    pub rules: indexmap::IndexMap<String, LemmaType>,
}

impl std::fmt::Display for DocumentSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Document: {}", self.doc)?;

        if !self.facts.is_empty() {
            write!(f, "\n\nFacts:")?;
            for (name, (lemma_type, default)) in &self.facts {
                write!(f, "\n  {} ({}", name, lemma_type.name())?;
                if let Some(constraints) = format_type_constraints(&lemma_type.specifications) {
                    write!(f, ", {}", constraints)?;
                }
                if let Some(val) = default {
                    write!(f, ", default: {}", val)?;
                }
                write!(f, ")")?;
            }
        }

        if !self.rules.is_empty() {
            write!(f, "\n\nRules:")?;
            for (name, rule_type) in &self.rules {
                write!(f, "\n  {} ({})", name, rule_type.name())?;
            }
        }

        if self.facts.is_empty() && self.rules.is_empty() {
            write!(f, "\n  (no facts or rules)")?;
        }

        Ok(())
    }
}

/// Produce a human-readable summary of type constraints, or `None` when there
/// are no constraints worth showing (e.g. bare `boolean`).
fn format_type_constraints(spec: &TypeSpecification) -> Option<String> {
    let mut parts = Vec::new();

    match spec {
        TypeSpecification::Number {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Scale {
            minimum,
            maximum,
            decimals,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                parts.push(format!("units: {}", unit_names.join(", ")));
            }
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
            }
            if let Some(d) = decimals {
                parts.push(format!("decimals: {}", d));
            }
        }
        TypeSpecification::Ratio {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Text { options, .. } => {
            if !options.is_empty() {
                let quoted: Vec<String> = options.iter().map(|o| format!("\"{}\"", o)).collect();
                parts.push(format!("options: {}", quoted.join(", ")));
            }
        }
        TypeSpecification::Date {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Time {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Boolean { .. }
        | TypeSpecification::Duration { .. }
        | TypeSpecification::Veto { .. }
        | TypeSpecification::Error => {}
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

impl ExecutionPlan {
    /// Build a [`DocumentSchema`] summarising **all** of this plan's facts and
    /// rules.
    ///
    /// All facts with a typed schema (local and cross-document) are included.
    /// Document-reference facts (which have no schema type) are excluded.
    /// Only local rules (no cross-document segments) are included.
    /// Results are sorted alphabetically by name for deterministic output.
    pub fn schema(&self) -> DocumentSchema {
        let mut fact_entries: Vec<(String, (LemmaType, Option<LiteralValue>))> = self
            .facts
            .iter()
            .filter(|(_, data)| data.schema_type().is_some())
            .map(|(path, data)| {
                let lemma_type = data.schema_type().unwrap().clone();
                let default = data.value().cloned();
                (path.input_key(), (lemma_type, default))
            })
            .collect();
        fact_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut rule_entries: Vec<(String, LemmaType)> = self
            .rules
            .iter()
            .filter(|r| r.path.segments.is_empty())
            .map(|r| (r.name.clone(), r.rule_type.clone()))
            .collect();
        rule_entries.sort_by(|a, b| a.0.cmp(&b.0));

        DocumentSchema {
            doc: self.doc_name.clone(),
            facts: fact_entries.into_iter().collect(),
            rules: rule_entries.into_iter().collect(),
        }
    }

    /// Build a [`DocumentSchema`] scoped to specific rules.
    ///
    /// The returned schema contains only the facts **needed** by the given rules
    /// (transitively, via `needs_facts`) and only those rules. This is the
    /// "what do I need to evaluate these rules?" view.
    ///
    /// Returns `Err` if any rule name is not found in the plan.
    pub fn schema_for_rules(&self, rule_names: &[String]) -> Result<DocumentSchema, Error> {
        let mut needed_facts = HashSet::new();
        let mut rule_entries: Vec<(String, LemmaType)> = Vec::new();

        for rule_name in rule_names {
            let rule = self.get_rule(rule_name).ok_or_else(|| {
                Error::planning(
                    format!(
                        "Rule '{}' not found in document '{}'",
                        rule_name, self.doc_name
                    ),
                    None,
                    None::<String>,
                )
            })?;
            needed_facts.extend(rule.needs_facts.iter().cloned());
            rule_entries.push((rule.name.clone(), rule.rule_type.clone()));
        }
        rule_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut fact_entries: Vec<(String, (LemmaType, Option<LiteralValue>))> = self
            .facts
            .iter()
            .filter(|(path, _)| needed_facts.contains(path))
            .filter(|(_, data)| data.schema_type().is_some())
            .map(|(path, data)| {
                let lemma_type = data.schema_type().unwrap().clone();
                let default = data.value().cloned();
                (path.input_key(), (lemma_type, default))
            })
            .collect();
        fact_entries.sort_by(|a, b| a.0.cmp(&b.0));

        Ok(DocumentSchema {
            doc: self.doc_name.clone(),
            facts: fact_entries.into_iter().collect(),
            rules: rule_entries.into_iter().collect(),
        })
    }

    /// Look up a fact by its input key (e.g., "age" or "rules.base_price").
    pub fn get_fact_path_by_str(&self, name: &str) -> Option<&FactPath> {
        self.facts.keys().find(|path| path.input_key() == name)
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
        self.facts.get(path).and_then(|d| d.value())
    }

    /// Provide string values for facts.
    ///
    /// Parses each string to its expected type, validates constraints, and applies to the plan.
    pub fn with_fact_values(
        mut self,
        values: HashMap<String, String>,
        limits: &ResourceLimits,
    ) -> Result<Self, Error> {
        for (name, raw_value) in values {
            let fact_path = self.get_fact_path_by_str(&name).ok_or_else(|| {
                let available: Vec<String> = self.facts.keys().map(|p| p.input_key()).collect();
                Error::planning(
                    format!(
                        "Fact '{}' not found. Available facts: {}",
                        name,
                        available.join(", ")
                    ),
                    None,
                    None::<String>,
                )
            })?;
            let fact_path = fact_path.clone();

            let fact_data = self
                .facts
                .get(&fact_path)
                .expect("BUG: fact_path was just resolved from self.facts, must exist");

            let fact_source = fact_data.source().clone();
            let expected_type = fact_data.schema_type().cloned().ok_or_else(|| {
                Error::planning(
                    format!(
                        "Fact '{}' is a document reference; cannot provide a value.",
                        name
                    ),
                    None,
                    None::<String>,
                )
            })?;

            // Parse string to typed value
            let parsed_value = crate::planning::semantics::parse_value_from_string(
                &raw_value,
                &expected_type.specifications,
                &fact_source,
            )
            .map_err(|e| {
                Error::planning(
                    format!(
                        "Failed to parse fact '{}' as {}: {}",
                        name,
                        expected_type.name(),
                        e
                    ),
                    Some(fact_source.clone()),
                    None::<String>,
                )
            })?;
            let semantic_value = semantics::value_to_semantic(&parsed_value).map_err(|e| {
                Error::planning(
                    format!("Failed to convert fact '{}' value: {}", name, e),
                    Some(fact_source.clone()),
                    None::<String>,
                )
            })?;
            let literal_value = LiteralValue {
                value: semantic_value,
                lemma_type: expected_type.clone(),
            };

            // Check resource limits
            let size = literal_value.byte_size();
            if size > limits.max_fact_value_bytes {
                return Err(Error::ResourceLimitExceeded {
                    limit_name: "max_fact_value_bytes".to_string(),
                    limit_value: limits.max_fact_value_bytes.to_string(),
                    actual_value: size.to_string(),
                    suggestion: format!(
                        "Reduce the size of fact values to {} bytes or less",
                        limits.max_fact_value_bytes
                    ),
                });
            }

            // Validate constraints
            validate_value_against_type(&expected_type, &literal_value).map_err(|msg| {
                Error::planning(
                    format!(
                        "Invalid value for fact {} (expected {}): {}",
                        name,
                        expected_type.name(),
                        msg
                    ),
                    Some(fact_source.clone()),
                    None::<String>,
                )
            })?;

            self.facts.insert(
                fact_path,
                FactData::Value {
                    value: literal_value,
                    source: fact_source,
                },
            );
        }

        Ok(self)
    }
}

fn validate_value_against_type(
    expected_type: &LemmaType,
    value: &LiteralValue,
) -> Result<(), String> {
    use crate::planning::semantics::TypeSpecification;

    let effective_decimals = |n: rust_decimal::Decimal| n.scale();

    match (&expected_type.specifications, &value.value) {
        (
            TypeSpecification::Number {
                minimum,
                maximum,
                decimals,
                ..
            },
            ValueKind::Number(n),
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
            ValueKind::Scale(n, _unit),
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
        (TypeSpecification::Text { options, .. }, ValueKind::Text(s)) => {
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

pub(crate) fn validate_literal_facts_against_types(plan: &ExecutionPlan) -> Vec<Error> {
    let mut errors = Vec::new();

    for (fact_path, fact_data) in &plan.facts {
        let (expected_type, lit) = match fact_data {
            FactData::Value { value, .. } => (&value.lemma_type, value),
            FactData::TypeDeclaration { .. } | FactData::DocumentRef { .. } => continue,
        };

        if let Err(msg) = validate_value_against_type(expected_type, lit) {
            let source = fact_data.source().clone();
            errors.push(Error::planning(
                format!(
                    "Invalid value for fact {} (expected {}): {}",
                    fact_path,
                    expected_type.name(),
                    msg
                ),
                Some(source),
                None::<String>,
            ));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::semantics::{
        primitive_boolean, primitive_text, FactPath, LiteralValue, PathSegment, RulePath,
    };
    use crate::Engine;
    use serde_json;
    use std::str::FromStr;
    use std::sync::Arc;

    fn default_limits() -> ResourceLimits {
        ResourceLimits::default()
    }

    fn add_lemma_code_blocking(
        engine: &mut Engine,
        code: &str,
        source: &str,
    ) -> crate::LemmaResult<()> {
        let files: std::collections::HashMap<String, String> =
            std::iter::once((source.to_string(), code.to_string())).collect();
        tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(engine.add_lemma_files(files))
            .map_err(|errs| match errs.len() {
                0 => unreachable!("add_lemma_files returned Err with empty error list"),
                1 => errs.into_iter().next().unwrap(),
                _ => crate::Error::MultipleErrors(errs),
            })
    }

    #[test]
    fn test_with_raw_values() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
                doc test
                fact age = [number -> default 25]
                "#,
            "test.lemma",
        )
        .unwrap();

        let plan = engine.get_execution_plan("test").unwrap().clone();
        let fact_path = FactPath::new(vec![], "age".to_string());

        let mut values = HashMap::new();
        values.insert("age".to_string(), "30".to_string());

        let updated_plan = plan.with_fact_values(values, &default_limits()).unwrap();
        let updated_value = updated_plan.get_fact_value(&fact_path).unwrap();
        match &updated_value.value {
            crate::planning::semantics::ValueKind::Number(n) => {
                assert_eq!(n, &rust_decimal::Decimal::from(30))
            }
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    #[test]
    fn test_with_raw_values_type_mismatch() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
                doc test
                fact age = [number]
                "#,
            "test.lemma",
        )
        .unwrap();

        let plan = engine.get_execution_plan("test").unwrap().clone();

        let mut values = HashMap::new();
        values.insert("age".to_string(), "thirty".to_string());

        assert!(plan.with_fact_values(values, &default_limits()).is_err());
    }

    #[test]
    fn test_with_raw_values_unknown_fact() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
                doc test
                fact known = [number]
                "#,
            "test.lemma",
        )
        .unwrap();

        let plan = engine.get_execution_plan("test").unwrap().clone();

        let mut values = HashMap::new();
        values.insert("unknown".to_string(), "30".to_string());

        assert!(plan.with_fact_values(values, &default_limits()).is_err());
    }

    #[test]
    fn test_with_raw_values_nested() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
                doc private
                fact base_price = [number]

                doc test
                fact rules = doc private
                "#,
            "test.lemma",
        )
        .unwrap();

        let plan = engine.get_execution_plan("test").unwrap().clone();

        let mut values = HashMap::new();
        values.insert("rules.base_price".to_string(), "100".to_string());

        let updated_plan = plan.with_fact_values(values, &default_limits()).unwrap();
        let fact_path = FactPath {
            segments: vec![PathSegment {
                fact: "rules".to_string(),
                doc: "private".to_string(),
            }],
            fact: "base_price".to_string(),
        };
        let updated_value = updated_plan.get_fact_value(&fact_path).unwrap();
        match &updated_value.value {
            crate::planning::semantics::ValueKind::Number(n) => {
                assert_eq!(n, &rust_decimal::Decimal::from(100))
            }
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    fn test_source() -> crate::Source {
        use crate::parsing::ast::Span;
        crate::Source {
            attribute: "<test>".to_string(),
            span: Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            doc_name: "<test>".to_string(),
            source_text: Arc::from("doc test\nfact x = 1\nrule result = x"),
        }
    }

    fn create_literal_expr(value: LiteralValue) -> Expression {
        Expression::new(
            crate::planning::semantics::ExpressionKind::Literal(Box::new(value)),
            test_source(),
        )
    }

    fn create_fact_path_expr(path: FactPath) -> Expression {
        Expression::new(
            crate::planning::semantics::ExpressionKind::FactPath(path),
            test_source(),
        )
    }

    fn create_number_literal(n: rust_decimal::Decimal) -> LiteralValue {
        LiteralValue::number(n)
    }

    fn create_boolean_literal(b: bool) -> LiteralValue {
        LiteralValue::from_bool(b)
    }

    fn create_text_literal(s: String) -> LiteralValue {
        LiteralValue::text(s)
    }

    #[test]
    fn with_values_should_enforce_number_maximum_constraint() {
        // Higher-standard requirement: user input must be validated against type constraints.
        // If this test fails, Lemma accepts invalid values and gives false reassurance.
        let fact_path = FactPath::new(vec![], "x".to_string());

        let max10 = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Number {
                minimum: None,
                maximum: Some(rust_decimal::Decimal::from_str("10").unwrap()),
                decimals: None,
                precision: None,
                help: String::new(),
                default: None,
            },
        );
        let source = Source::new(
            "<test>",
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "test",
            Arc::from("doc test\nfact x = 1\nrule result = x"),
        );
        let mut facts = HashMap::new();
        facts.insert(
            fact_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: crate::planning::semantics::LiteralValue::number_with_type(
                    0.into(),
                    max10.clone(),
                ),
                source: source.clone(),
            },
        );

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
            rules: Vec::new(),
            sources: HashMap::from([("<test>".to_string(), "".to_string())]),
        };

        let mut values = HashMap::new();
        values.insert("x".to_string(), "11".to_string());

        assert!(
            plan.with_fact_values(values, &default_limits()).is_err(),
            "Providing x=11 should fail due to maximum 10"
        );
    }

    #[test]
    fn with_values_should_enforce_text_enum_options() {
        // Higher-standard requirement: enum options must be enforced for text types.
        let fact_path = FactPath::new(vec![], "tier".to_string());

        let tier = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Text {
                minimum: None,
                maximum: None,
                length: None,
                options: vec!["silver".to_string(), "gold".to_string()],
                help: String::new(),
                default: None,
            },
        );
        let source = Source::new(
            "<test>",
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "test",
            Arc::from("doc test\nfact x = 1\nrule result = x"),
        );
        let mut facts = HashMap::new();
        facts.insert(
            fact_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: crate::planning::semantics::LiteralValue::text_with_type(
                    "silver".to_string(),
                    tier.clone(),
                ),
                source,
            },
        );

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
            rules: Vec::new(),
            sources: HashMap::from([("<test>".to_string(), "".to_string())]),
        };

        let mut values = HashMap::new();
        values.insert("tier".to_string(), "platinum".to_string());

        assert!(
            plan.with_fact_values(values, &default_limits()).is_err(),
            "Invalid enum value should be rejected (tier='platinum')"
        );
    }

    #[test]
    fn with_values_should_enforce_scale_decimals() {
        // Higher-standard requirement: decimals should be enforced on scale inputs,
        // unless the language explicitly defines rounding semantics.
        let fact_path = FactPath::new(vec![], "price".to_string());

        let money = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Scale {
                minimum: None,
                maximum: None,
                decimals: Some(2),
                precision: None,
                units: crate::planning::semantics::ScaleUnits::from(vec![
                    crate::planning::semantics::ScaleUnit {
                        name: "eur".to_string(),
                        value: rust_decimal::Decimal::from_str("1.0").unwrap(),
                    },
                ]),
                help: String::new(),
                default: None,
            },
        );
        let source = Source::new(
            "<test>",
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
            "test",
            Arc::from("doc test\nfact x = 1\nrule result = x"),
        );
        let mut facts = HashMap::new();
        facts.insert(
            fact_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: crate::planning::semantics::LiteralValue::scale_with_type(
                    rust_decimal::Decimal::from_str("0").unwrap(),
                    "eur".to_string(),
                    money.clone(),
                ),
                source,
            },
        );

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
            rules: Vec::new(),
            sources: HashMap::from([("<test>".to_string(), "".to_string())]),
        };

        let mut values = HashMap::new();
        values.insert("price".to_string(), "1.234 eur".to_string());

        assert!(
            plan.with_fact_values(values, &default_limits()).is_err(),
            "Scale decimals=2 should reject 1.234 eur"
        );
    }

    #[test]
    fn test_serialize_deserialize_execution_plan() {
        let fact_path = FactPath {
            segments: vec![],
            fact: "age".to_string(),
        };
        let mut facts = HashMap::new();
        facts.insert(
            fact_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
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
        assert_eq!(deserialized.facts.len(), plan.facts.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
        assert_eq!(deserialized.sources.len(), plan.sources.len());
    }

    #[test]
    fn test_serialize_deserialize_plan_with_rules() {
        use crate::planning::semantics::ExpressionKind;

        let age_path = FactPath::new(vec![], "age".to_string());
        let mut facts = HashMap::new();
        facts.insert(
            age_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "can_drive".to_string()),
            name: "can_drive".to_string(),
            branches: vec![Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Arc::new(create_fact_path_expr(age_path.clone())),
                        crate::ComparisonComputation::GreaterThanOrEqual,
                        Arc::new(create_literal_expr(create_number_literal(18.into()))),
                    ),
                    test_source(),
                )),
                result: create_literal_expr(create_boolean_literal(true)),
                source: test_source(),
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(age_path);
                set
            },
            source: test_source(),
            rule_type: primitive_boolean().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.doc_name, plan.doc_name);
        assert_eq!(deserialized.facts.len(), plan.facts.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
        assert_eq!(deserialized.rules[0].name, "can_drive");
        assert_eq!(deserialized.rules[0].branches.len(), 1);
        assert_eq!(deserialized.rules[0].needs_facts.len(), 1);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_nested_fact_paths() {
        use crate::planning::semantics::PathSegment;
        let fact_path = FactPath {
            segments: vec![PathSegment {
                fact: "employee".to_string(),
                doc: "private".to_string(),
            }],
            fact: "salary".to_string(),
        };

        let mut facts = HashMap::new();
        facts.insert(
            fact_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.facts.len(), 1);
        let (deserialized_path, _) = deserialized.facts.iter().next().unwrap();
        assert_eq!(deserialized_path.segments.len(), 1);
        assert_eq!(deserialized_path.segments[0].fact, "employee");
        assert_eq!(deserialized_path.fact, "salary");
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_fact_types() {
        let name_path = FactPath::new(vec![], "name".to_string());
        let age_path = FactPath::new(vec![], "age".to_string());
        let active_path = FactPath::new(vec![], "active".to_string());

        let mut facts = HashMap::new();
        facts.insert(
            name_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: create_text_literal("Alice".to_string()),
                source: test_source(),
            },
        );
        facts.insert(
            age_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: create_number_literal(30.into()),
                source: test_source(),
            },
        );
        facts.insert(
            active_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: create_boolean_literal(true),
                source: test_source(),
            },
        );

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.facts.len(), 3);

        assert_eq!(
            deserialized.get_fact_value(&name_path).unwrap().value,
            crate::planning::semantics::ValueKind::Text("Alice".to_string())
        );
        assert_eq!(
            deserialized.get_fact_value(&age_path).unwrap().value,
            crate::planning::semantics::ValueKind::Number(30.into())
        );
        assert_eq!(
            deserialized.get_fact_value(&active_path).unwrap().value,
            crate::planning::semantics::ValueKind::Boolean(true)
        );
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_branches() {
        use crate::planning::semantics::ExpressionKind;

        let points_path = FactPath::new(vec![], "points".to_string());
        let mut facts = HashMap::new();
        facts.insert(
            points_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "tier".to_string()),
            name: "tier".to_string(),
            branches: vec![
                Branch {
                    condition: None,
                    result: create_literal_expr(create_text_literal("bronze".to_string())),
                    source: test_source(),
                },
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Arc::new(create_fact_path_expr(points_path.clone())),
                            crate::ComparisonComputation::GreaterThanOrEqual,
                            Arc::new(create_literal_expr(create_number_literal(100.into()))),
                        ),
                        test_source(),
                    )),
                    result: create_literal_expr(create_text_literal("silver".to_string())),
                    source: test_source(),
                },
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Arc::new(create_fact_path_expr(points_path.clone())),
                            crate::ComparisonComputation::GreaterThanOrEqual,
                            Arc::new(create_literal_expr(create_number_literal(500.into()))),
                        ),
                        test_source(),
                    )),
                    result: create_literal_expr(create_text_literal("gold".to_string())),
                    source: test_source(),
                },
            ],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(points_path);
                set
            },
            source: test_source(),
            rule_type: primitive_text().clone(),
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
            facts: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.doc_name, "empty");
        assert_eq!(deserialized.facts.len(), 0);
        assert_eq!(deserialized.rules.len(), 0);
        assert_eq!(deserialized.sources.len(), 0);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_arithmetic_expressions() {
        use crate::planning::semantics::ExpressionKind;

        let x_path = FactPath::new(vec![], "x".to_string());
        let mut facts = HashMap::new();
        facts.insert(
            x_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "doubled".to_string()),
            name: "doubled".to_string(),
            branches: vec![Branch {
                condition: None,
                result: Expression::new(
                    ExpressionKind::Arithmetic(
                        Arc::new(create_fact_path_expr(x_path.clone())),
                        crate::ArithmeticComputation::Multiply,
                        Arc::new(create_literal_expr(create_number_literal(2.into()))),
                    ),
                    test_source(),
                ),
                source: test_source(),
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(x_path);
                set
            },
            source: test_source(),
            rule_type: crate::planning::semantics::primitive_number().clone(),
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
        use crate::planning::semantics::ExpressionKind;

        let age_path = FactPath::new(vec![], "age".to_string());
        let mut facts = HashMap::new();
        facts.insert(
            age_path.clone(),
            crate::planning::semantics::FactData::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts,
            rules: Vec::new(),
            sources: {
                let mut s = HashMap::new();
                s.insert("test.lemma".to_string(), "fact age: number".to_string());
                s
            },
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "is_adult".to_string()),
            name: "is_adult".to_string(),
            branches: vec![Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Arc::new(create_fact_path_expr(age_path.clone())),
                        crate::ComparisonComputation::GreaterThanOrEqual,
                        Arc::new(create_literal_expr(create_number_literal(18.into()))),
                    ),
                    test_source(),
                )),
                result: create_literal_expr(create_boolean_literal(true)),
                source: test_source(),
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(age_path);
                set
            },
            source: test_source(),
            rule_type: primitive_boolean().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        let json2 = serde_json::to_string(&deserialized).expect("Should serialize again");
        let deserialized2: ExecutionPlan =
            serde_json::from_str(&json2).expect("Should deserialize again");

        assert_eq!(deserialized2.doc_name, plan.doc_name);
        assert_eq!(deserialized2.facts.len(), plan.facts.len());
        assert_eq!(deserialized2.rules.len(), plan.rules.len());
        assert_eq!(deserialized2.sources.len(), plan.sources.len());
        assert_eq!(deserialized2.rules[0].name, plan.rules[0].name);
        assert_eq!(
            deserialized2.rules[0].branches.len(),
            plan.rules[0].branches.len()
        );
    }
}
