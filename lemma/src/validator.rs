/// Type of an expression for validation.
///
/// Used during semantic analysis to catch type errors early,
/// before code execution. Allows validation of logical operators,
/// type compatibility, and currency matching.
#[derive(Debug, Clone, PartialEq)]
enum ExpressionType {
    Boolean,
    Number,
    Percentage,
    Text,
    Money,
    Mass,
    Length,
    Volume,
    Duration,
    Temperature,
    Power,
    Force,
    Pressure,
    Energy,
    Frequency,
    Data,
    Date,
    Unknown,
    Never,
}

impl ExpressionType {
    /// Returns true if this type is boolean
    fn is_boolean(&self) -> bool {
        matches!(self, ExpressionType::Boolean)
    }

    /// Returns a human-readable name for this type
    fn name(&self) -> &'static str {
        match self {
            ExpressionType::Boolean => "boolean",
            ExpressionType::Number => "number",
            ExpressionType::Percentage => "percentage",
            ExpressionType::Text => "text",
            ExpressionType::Money => "money",
            ExpressionType::Mass => "mass",
            ExpressionType::Length => "length",
            ExpressionType::Volume => "volume",
            ExpressionType::Duration => "duration",
            ExpressionType::Temperature => "temperature",
            ExpressionType::Power => "power",
            ExpressionType::Force => "force",
            ExpressionType::Pressure => "pressure",
            ExpressionType::Energy => "energy",
            ExpressionType::Frequency => "frequency",
            ExpressionType::Data => "data",
            ExpressionType::Date => "date",
            ExpressionType::Unknown => "unknown",
            ExpressionType::Never => "never",
        }
    }

    /// Infer the type from a literal value
    fn from_literal(lit: &crate::LiteralValue) -> Self {
        match lit {
            crate::LiteralValue::Boolean(_) => ExpressionType::Boolean,
            crate::LiteralValue::Number(_) => ExpressionType::Number,
            crate::LiteralValue::Percentage(_) => ExpressionType::Percentage,
            crate::LiteralValue::Text(_) => ExpressionType::Text,
            crate::LiteralValue::Unit(unit) => match unit {
                crate::NumericUnit::Money(_, _) => ExpressionType::Money,
                crate::NumericUnit::Mass(_, _) => ExpressionType::Mass,
                crate::NumericUnit::Length(_, _) => ExpressionType::Length,
                crate::NumericUnit::Volume(_, _) => ExpressionType::Volume,
                crate::NumericUnit::Duration(_, _) => ExpressionType::Duration,
                crate::NumericUnit::Temperature(_, _) => ExpressionType::Temperature,
                crate::NumericUnit::Power(_, _) => ExpressionType::Power,
                crate::NumericUnit::Force(_, _) => ExpressionType::Force,
                crate::NumericUnit::Pressure(_, _) => ExpressionType::Pressure,
                crate::NumericUnit::Energy(_, _) => ExpressionType::Energy,
                crate::NumericUnit::Frequency(_, _) => ExpressionType::Frequency,
                crate::NumericUnit::Data(_, _) => ExpressionType::Data,
            },
            crate::LiteralValue::Date(_) => ExpressionType::Date,
            _ => ExpressionType::Unknown,
        }
    }
}

use crate::{
    ConversionTarget, Expression, ExpressionKind, FactType, FactValue, LemmaDoc, LemmaError,
    LemmaResult, LemmaRule, Span,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Documents that have passed semantic validation
#[derive(Debug, Clone)]
pub struct ValidatedDocuments {
    pub documents: Vec<LemmaDoc>,
}

/// Comprehensive semantic validator that runs after parsing but before evaluation
#[derive(Default)]
pub struct Validator;

impl Validator {
    /// Create a new validator
    pub fn new() -> Self {
        Self
    }

    /// Validate all documents and return validated documents
    pub fn validate_all(&self, docs: Vec<LemmaDoc>) -> LemmaResult<ValidatedDocuments> {
        // Phase 1: Check for duplicate facts and rules within each document
        self.validate_duplicates(&docs)?;

        // Phase 2: Validate cross-document references
        self.validate_document_references(&docs)?;

        // Phase 3: Validate all rule references (fact vs rule reference types)
        self.validate_rule_references(&docs)?;

        // Phase 4: Check for circular dependencies
        self.check_circular_dependencies(&docs)?;

        // Phase 5: Validate expression types
        self.validate_expression_types(&docs)?;

        Ok(ValidatedDocuments { documents: docs })
    }

    /// Check for duplicate facts and rules within each document
    fn validate_duplicates(&self, docs: &[LemmaDoc]) -> LemmaResult<()> {
        for doc in docs {
            // Check for duplicate facts
            let mut fact_names: HashMap<String, Span> = HashMap::new();
            for fact in &doc.facts {
                let fact_name = crate::analysis::fact_display_name(fact);

                if let Some(first_span) = fact_names.get(&fact_name) {
                    let duplicate_span = fact.span.clone().unwrap_or(Span {
                        start: 0,
                        end: 0,
                        line: 0,
                        col: 0,
                    });
                    let first_doc_line = if first_span.line >= doc.start_line {
                        first_span.line - doc.start_line + 1
                    } else {
                        first_span.line
                    };

                    let error_message = match fact.fact_type {
                        FactType::Local(_) => format!("Duplicate fact definition: '{}'", fact_name),
                        FactType::Foreign(_) => format!("Duplicate fact override: '{}'", fact_name),
                    };

                    let suggestion = match fact.fact_type {
                        FactType::Local(_) => format!(
                            "Fact '{}' was already defined at doc line {} (file line {}). Each fact can only be defined once per document.",
                            fact_name, first_doc_line, first_span.line
                        ),
                        FactType::Foreign(_) => format!(
                            "Fact override '{}' was already defined at doc line {} (file line {}). Each fact can only be overridden once per document.",
                            fact_name, first_doc_line, first_span.line
                        ),
                    };

                    return Err(LemmaError::Semantic(Box::new(crate::error::ErrorDetails {
                        message: error_message,
                        span: duplicate_span,
                        source_id: doc.source.clone().unwrap_or_else(|| "<input>".to_string()),
                        source_text: Arc::from(""),
                        doc_name: doc.name.clone(),
                        doc_start_line: doc.start_line,
                        suggestion: Some(suggestion),
                    })));
                }

                if let Some(span) = &fact.span {
                    fact_names.insert(fact_name, span.clone());
                }
            }

            // Check for duplicate rules
            let mut rule_names: HashMap<String, Span> = HashMap::new();
            for rule in &doc.rules {
                if let Some(first_span) = rule_names.get(&rule.name) {
                    let duplicate_span = rule.span.clone().unwrap_or(Span {
                        start: 0,
                        end: 0,
                        line: 0,
                        col: 0,
                    });
                    let first_doc_line = if first_span.line >= doc.start_line {
                        first_span.line - doc.start_line + 1
                    } else {
                        first_span.line
                    };
                    return Err(LemmaError::Semantic(Box::new(crate::error::ErrorDetails {
                        message: format!("Duplicate rule definition: '{}'", rule.name),
                        span: duplicate_span,
                        source_id: doc.source.clone().unwrap_or_else(|| "<input>".to_string()),
                        source_text: Arc::from(""),
                        doc_name: doc.name.clone(),
                        doc_start_line: doc.start_line,
                        suggestion: Some(format!(
                            "Rule '{}' was already defined at doc line {} (file line {}). Each rule can only be defined once per document. Consider using 'unless' clauses for conditional logic.",
                            rule.name, first_doc_line, first_span.line
                        )),
            })));
                }

                if let Some(span) = &rule.span {
                    rule_names.insert(rule.name.clone(), span.clone());
                }
            }

            // Check for name conflicts between facts and rules
            for rule in &doc.rules {
                if let Some(fact_span) = fact_names.get(&rule.name) {
                    let rule_span = rule.span.clone().unwrap_or(Span {
                        start: 0,
                        end: 0,
                        line: 0,
                        col: 0,
                    });
                    let fact_doc_line = if fact_span.line >= doc.start_line {
                        fact_span.line - doc.start_line + 1
                    } else {
                        fact_span.line
                    };

                    return Err(LemmaError::Semantic(Box::new(crate::error::ErrorDetails {
                        message: format!("Name conflict: '{}' is defined as both a fact and a rule", rule.name),
                        span: rule_span,
                        source_id: doc.source.clone().unwrap_or_else(|| "<input>".to_string()),
                        source_text: Arc::from(""),
                        doc_name: doc.name.clone(),
                        doc_start_line: doc.start_line,
                        suggestion: Some(format!(
                            "A fact named '{}' was already defined at doc line {} (file line {}). Facts and rules cannot share the same name within a document. Choose a different name for either the fact or the rule.",
                            rule.name, fact_doc_line, fact_span.line
                        )),
            })));
                }
            }
        }
        Ok(())
    }

    /// Validate document references (facts that reference other documents)
    fn validate_document_references(&self, docs: &[LemmaDoc]) -> LemmaResult<()> {
        for doc in docs {
            for fact in &doc.facts {
                if let FactValue::DocumentReference(ref_doc_name) = &fact.value {
                    // Check if the referenced document exists
                    if !docs.iter().any(|d| d.name == *ref_doc_name) {
                        return Err(LemmaError::Semantic(Box::new(crate::error::ErrorDetails {
                            message: format!("Document reference error: '{}' does not exist", ref_doc_name),
                            span: fact.span.clone().unwrap_or(Span { start: 0, end: 0, line: 0, col: 0 }),
                            source_id: doc.source.clone().unwrap_or_else(|| "<input>".to_string()),
                            source_text: Arc::from(""),
                            doc_name: doc.name.clone(),
                            doc_start_line: doc.start_line,
                            suggestion: Some(format!(
                                "Document '{}' is referenced but not defined. Make sure the document exists in your workspace.",
                                ref_doc_name
                            )),
            })));
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate all rule references (fact vs rule reference types)
    fn validate_rule_references(&self, docs: &[LemmaDoc]) -> LemmaResult<()> {
        for doc in docs {
            for rule in &doc.rules {
                self.validate_expression_references(&rule.expression, doc, docs)?;

                for unless_clause in &rule.unless_clauses {
                    self.validate_expression_references(&unless_clause.condition, doc, docs)?;
                    self.validate_expression_references(&unless_clause.result, doc, docs)?;
                }
            }
        }
        Ok(())
    }

    /// Helper: Check if a name is a fact in a document
    fn is_fact_in_doc(&self, fact_name: &str, doc: &LemmaDoc) -> bool {
        doc.facts.iter().any(|f| match &f.fact_type {
            FactType::Local(name) => name == fact_name,
            FactType::Foreign(foreign) => foreign.reference.join(".") == fact_name,
        })
    }

    /// Helper: Check if a name is a rule in a document
    fn is_rule_in_doc(&self, rule_name: &str, doc: &LemmaDoc) -> bool {
        doc.rules.iter().any(|r| r.name == rule_name)
    }

    /// Helper: Find the document that a fact references (if it's a document reference fact)
    fn get_referenced_doc<'a>(
        &self,
        fact_name: &str,
        doc: &LemmaDoc,
        all_docs: &'a [LemmaDoc],
    ) -> Option<&'a LemmaDoc> {
        // Find the fact in the current document
        let fact = doc.facts.iter().find(|f| match &f.fact_type {
            FactType::Local(name) => name == fact_name,
            _ => false,
        })?;

        // Check if it's a document reference
        if let FactValue::DocumentReference(ref_doc_name) = &fact.value {
            // Find and return the referenced document
            all_docs.iter().find(|d| d.name == *ref_doc_name)
        } else {
            None
        }
    }

    /// Validate references within an expression
    fn validate_expression_references(
        &self,
        expr: &Expression,
        current_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
    ) -> LemmaResult<()> {
        match &expr.kind {
            ExpressionKind::FactReference(fact_ref) => {
                self.validate_fact_reference(fact_ref, expr, current_doc, all_docs)
            }
            ExpressionKind::RuleReference(rule_ref) => {
                self.validate_rule_reference(rule_ref, expr, current_doc, all_docs)
            }
            // Recursively validate nested expressions
            ExpressionKind::LogicalAnd(left, right) | ExpressionKind::LogicalOr(left, right) => {
                self.validate_expression_references(left, current_doc, all_docs)?;
                self.validate_expression_references(right, current_doc, all_docs)
            }
            ExpressionKind::Arithmetic(left, _, right)
            | ExpressionKind::Comparison(left, _, right) => {
                self.validate_expression_references(left, current_doc, all_docs)?;
                self.validate_expression_references(right, current_doc, all_docs)
            }
            ExpressionKind::LogicalNegation(inner, _)
            | ExpressionKind::MathematicalOperator(_, inner)
            | ExpressionKind::UnitConversion(inner, _) => {
                self.validate_expression_references(inner, current_doc, all_docs)
            }
            ExpressionKind::FactHasAnyValue(_fact_ref) => {
                // For "have" expressions, we don't validate the fact reference as it's a dynamic check
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Validate a fact reference (without '?')
    fn validate_fact_reference(
        &self,
        fact_ref: &crate::FactReference,
        expr: &Expression,
        current_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
    ) -> LemmaResult<()> {
        let ref_name = fact_ref.reference.join(".");

        // Single-segment reference
        if fact_ref.reference.len() == 1 {
            return self.validate_single_segment_fact_ref(&ref_name, expr, current_doc);
        }

        // Multi-segment reference
        if fact_ref.reference.len() < 2 {
            return Ok(());
        }

        let doc_ref = &fact_ref.reference[0];
        let field_name = fact_ref.reference[1..].join(".");

        self.validate_multi_segment_fact_ref(
            &ref_name,
            doc_ref,
            &field_name,
            expr,
            current_doc,
            all_docs,
        )
    }

    /// Validate a single-segment fact reference
    fn validate_single_segment_fact_ref(
        &self,
        ref_name: &str,
        expr: &Expression,
        current_doc: &LemmaDoc,
    ) -> LemmaResult<()> {
        if self.is_rule_in_doc(ref_name, current_doc) {
            return Err(self.create_reference_error(
                format!(
                    "Reference error: '{}' is a rule and must be referenced with '?' (e.g., '{}?')",
                    ref_name, ref_name
                ),
                format!("Use '{}?' to reference the rule '{}'", ref_name, ref_name),
                expr,
                current_doc,
            ));
        }
        Ok(())
    }

    /// Validate a multi-segment fact reference
    fn validate_multi_segment_fact_ref(
        &self,
        ref_name: &str,
        doc_ref: &str,
        field_name: &str,
        expr: &Expression,
        current_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
    ) -> LemmaResult<()> {
        // Check if first segment is a fact that references a document
        if let Some(referenced_doc) = self.get_referenced_doc(doc_ref, current_doc, all_docs) {
            if self.is_rule_in_doc(field_name, referenced_doc) {
                return Err(self.create_reference_error(
                    format!("Reference error: '{}' references a rule in document '{}' and must use '?' (e.g., '{}?')", ref_name, referenced_doc.name, ref_name),
                    format!("Use '{}?' to reference the rule '{}' in document '{}'", ref_name, field_name, referenced_doc.name),
                    expr,
                    current_doc,
                ));
            }
            return Ok(());
        }

        // Check if it's a rule in the current document
        if self.is_rule_in_doc(field_name, current_doc) {
            return Err(self.create_reference_error(
                format!("Reference error: '{}' appears to reference a rule and must use '?' (e.g., '{}?')", ref_name, ref_name),
                format!("Use '{}?' to reference the rule '{}'", ref_name, ref_name),
                expr,
                current_doc,
            ));
        }
        Ok(())
    }

    /// Validate a rule reference (with '?')
    fn validate_rule_reference(
        &self,
        rule_ref: &crate::RuleReference,
        expr: &Expression,
        current_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
    ) -> LemmaResult<()> {
        let ref_name = rule_ref.reference.join(".");

        // Single-segment reference
        if rule_ref.reference.len() == 1 {
            return self.validate_single_segment_rule_ref(&ref_name, expr, current_doc);
        }

        // Multi-segment reference
        if rule_ref.reference.len() < 2 {
            return Ok(());
        }

        let doc_ref = &rule_ref.reference[0];
        let field_name = rule_ref.reference[1..].join(".");

        self.validate_multi_segment_rule_ref(
            &ref_name,
            doc_ref,
            &field_name,
            expr,
            current_doc,
            all_docs,
        )
    }

    /// Validate a single-segment rule reference
    fn validate_single_segment_rule_ref(
        &self,
        ref_name: &str,
        expr: &Expression,
        current_doc: &LemmaDoc,
    ) -> LemmaResult<()> {
        if self.is_fact_in_doc(ref_name, current_doc) {
            return Err(self.create_reference_error(
                format!("Reference error: '{}' is a fact and should not use '?' (use '{}' instead of '{}?')", ref_name, ref_name, ref_name),
                format!("Use '{}' to reference the fact '{}' (remove the '?')", ref_name, ref_name),
                expr,
                current_doc,
            ));
        }
        Ok(())
    }

    /// Validate a multi-segment rule reference
    fn validate_multi_segment_rule_ref(
        &self,
        ref_name: &str,
        doc_ref: &str,
        field_name: &str,
        expr: &Expression,
        current_doc: &LemmaDoc,
        all_docs: &[LemmaDoc],
    ) -> LemmaResult<()> {
        // Check if first segment is a fact that references a document
        if let Some(referenced_doc) = self.get_referenced_doc(doc_ref, current_doc, all_docs) {
            if self.is_fact_in_doc(field_name, referenced_doc) {
                return Err(self.create_reference_error(
                    format!("Reference error: '{}' references a fact in document '{}' and should not use '?' (use '{}' instead of '{}?')", ref_name, referenced_doc.name, ref_name, ref_name),
                    format!("Use '{}' to reference the fact '{}' in document '{}' (remove the '?')", ref_name, field_name, referenced_doc.name),
                    expr,
                    current_doc,
                ));
            }
            return Ok(());
        }

        // Check if it's a fact in the current document
        if self.is_fact_in_doc(field_name, current_doc) {
            return Err(self.create_reference_error(
                format!("Reference error: '{}' appears to reference a fact and should not use '?' (use '{}' instead of '{}?')", ref_name, ref_name, ref_name),
                format!("Use '{}' to reference the fact '{}' (remove the '?')", ref_name, ref_name),
                expr,
                current_doc,
            ));
        }
        Ok(())
    }

    /// Helper to create a semantic error for reference validation
    fn create_reference_error(
        &self,
        message: String,
        suggestion: String,
        expr: &Expression,
        current_doc: &LemmaDoc,
    ) -> LemmaError {
        LemmaError::Semantic(Box::new(crate::error::ErrorDetails {
            message,
            span: expr.span.clone().unwrap_or(Span {
                start: 0,
                end: 0,
                line: 0,
                col: 0,
            }),
            source_id: current_doc
                .source
                .clone()
                .unwrap_or_else(|| "<input>".to_string()),
            source_text: Arc::from(""),
            doc_name: current_doc.name.clone(),
            doc_start_line: current_doc.start_line,
            suggestion: Some(suggestion),
        }))
    }

    /// Check for circular dependencies in rules (moved from document transpiler)
    fn check_circular_dependencies(&self, docs: &[LemmaDoc]) -> LemmaResult<()> {
        // Build dependency graph from all rules across all documents
        let mut all_rules = Vec::new();
        for doc in docs {
            all_rules.extend(doc.rules.iter().cloned());
        }

        let graph = self.build_dependency_graph(&all_rules);
        let mut visited = HashSet::new();

        for rule_name in graph.keys() {
            if !visited.contains(rule_name) {
                let mut visiting = HashSet::new();
                let mut path = Vec::new();

                if let Some(cycle) =
                    Self::detect_cycle(&graph, rule_name, &mut visiting, &mut visited, &mut path)
                {
                    let cycle_display = cycle.join(" -> ");
                    return Err(LemmaError::CircularDependency(format!(
                        "Circular dependency detected: {}. Rules cannot depend on themselves directly or indirectly.",
                        cycle_display
                    )));
                }
            }
        }

        Ok(())
    }

    /// Build a dependency graph of rules (local document only)
    fn build_dependency_graph(&self, rules: &[LemmaRule]) -> HashMap<String, HashSet<String>> {
        let mut graph = HashMap::new();

        for rule in rules {
            let mut dependencies = HashSet::new();
            let refs = crate::analysis::extract_references(&rule.expression);
            for rule_ref in refs.rules {
                dependencies.insert(rule_ref.join("."));
            }
            for uc in &rule.unless_clauses {
                let cond_refs = crate::analysis::extract_references(&uc.condition);
                let res_refs = crate::analysis::extract_references(&uc.result);
                for rule_ref in cond_refs.rules.into_iter().chain(res_refs.rules) {
                    dependencies.insert(rule_ref.join("."));
                }
            }
            graph.insert(rule.name.clone(), dependencies);
        }

        graph
    }

    /// Detect cycles in the dependency graph using DFS (moved from document transpiler)
    fn detect_cycle(
        graph: &HashMap<String, HashSet<String>>,
        node: &str,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if visiting.contains(node) {
            let cycle_start = path.iter().position(|n| n == node).unwrap_or(0);
            let mut cycle = path[cycle_start..].to_vec();
            cycle.push(node.to_string());
            return Some(cycle);
        }

        if visited.contains(node) {
            return None;
        }

        visiting.insert(node.to_string());
        path.push(node.to_string());

        if let Some(dependencies) = graph.get(node) {
            for dep in dependencies {
                if graph.contains_key(dep) {
                    if let Some(cycle) = Self::detect_cycle(graph, dep, visiting, visited, path) {
                        return Some(cycle);
                    }
                }
            }
        }

        path.pop();
        visiting.remove(node);
        visited.insert(node.to_string());

        None
    }

    /// Validate expression types - ensure logical operators only have boolean operands
    fn validate_expression_types(&self, docs: &[LemmaDoc]) -> LemmaResult<()> {
        for doc in docs {
            for rule in &doc.rules {
                self.validate_expression_type(&rule.expression, doc)?;
                for unless_clause in &rule.unless_clauses {
                    // Validate condition is boolean
                    let condition_type = self
                        .infer_expression_type_with_context(&unless_clause.condition, Some(doc))?;
                    if condition_type != ExpressionType::Unknown && !condition_type.is_boolean() {
                        return Err(LemmaError::Semantic(Box::new(crate::error::ErrorDetails {
                            message: format!(
                                "Type error: Unless condition must be boolean, but got {}",
                                condition_type.name()
                            ),
                            span: unless_clause.condition.span.clone().unwrap_or(Span {
                                start: 0,
                                end: 0,
                                line: 0,
                                col: 0,
                            }),
                            source_id: doc.source.clone().unwrap_or_else(|| "<input>".to_string()),
                            source_text: Arc::from(""),
                            doc_name: doc.name.clone(),
                            doc_start_line: doc.start_line,
                            suggestion: Some(
                                "Use a comparison or boolean expression for unless conditions"
                                    .to_string(),
                            ),
                        })));
                    }

                    self.validate_expression_type(&unless_clause.condition, doc)?;
                    self.validate_expression_type(&unless_clause.result, doc)?;
                }
                self.validate_rule_type_consistency(rule, doc)?;
            }
        }
        Ok(())
    }

    /// Validate a single expression for type correctness
    fn validate_expression_type(&self, expr: &Expression, doc: &LemmaDoc) -> LemmaResult<()> {
        match &expr.kind {
            ExpressionKind::LogicalAnd(left, right) => {
                self.validate_logical_operand(left, doc, "and")?;
                self.validate_logical_operand(right, doc, "and")?;
                self.validate_expression_type(left, doc)?;
                self.validate_expression_type(right, doc)?;
            }
            ExpressionKind::LogicalOr(left, right) => {
                self.validate_logical_operand(left, doc, "or")?;
                self.validate_logical_operand(right, doc, "or")?;
                self.validate_expression_type(left, doc)?;
                self.validate_expression_type(right, doc)?;
            }
            ExpressionKind::Arithmetic(left, _op, right) => {
                self.validate_expression_type(left, doc)?;
                self.validate_expression_type(right, doc)?;
                self.validate_money_arithmetic(left, right, doc)?;
            }
            ExpressionKind::Comparison(left, _op, right) => {
                self.validate_expression_type(left, doc)?;
                self.validate_expression_type(right, doc)?;
                self.validate_money_comparison(left, right, doc)?;
            }
            ExpressionKind::LogicalNegation(inner, _negation_type) => {
                self.validate_expression_type(inner, doc)?;
            }
            ExpressionKind::MathematicalOperator(_op, operand) => {
                self.validate_expression_type(operand, doc)?;
            }
            ExpressionKind::UnitConversion(value, _target) => {
                self.validate_expression_type(value, doc)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Helper to validate that an operand is boolean for logical operators
    fn validate_logical_operand(
        &self,
        operand: &Expression,
        doc: &LemmaDoc,
        operator: &str,
    ) -> LemmaResult<()> {
        let operand_type = self.infer_expression_type(operand)?;

        // Only validate if we know the type (not Unknown)
        if operand_type == ExpressionType::Unknown || operand_type.is_boolean() {
            return Ok(());
        }

        Err(LemmaError::Semantic(Box::new(crate::error::ErrorDetails {
            message: format!(
                "Type error: Logical operator '{}' requires boolean operands, but operand has type {}",
                operator,
                operand_type.name()
            ),
            span: operand.span.clone().unwrap_or(Span { start: 0, end: 0, line: 0, col: 0 }),
            source_id: doc.source.clone().unwrap_or_else(|| "<input>".to_string()),
            source_text: Arc::from(""),
            doc_name: doc.name.clone(),
            doc_start_line: doc.start_line,
            suggestion: Some("Use a boolean expression or comparison for logical operations".to_string()),
        })))
    }

    /// Validate that all branches of a rule return compatible types
    fn validate_rule_type_consistency(&self, rule: &LemmaRule, doc: &LemmaDoc) -> LemmaResult<()> {
        if rule.unless_clauses.is_empty() {
            return Ok(());
        }

        let default_type = self.infer_expression_type_with_context(&rule.expression, Some(doc))?;

        let mut non_veto_types = Vec::new();
        if default_type != ExpressionType::Never {
            non_veto_types.push(("default expression", default_type.clone()));
        }

        for (idx, unless_clause) in rule.unless_clauses.iter().enumerate() {
            let result_type =
                self.infer_expression_type_with_context(&unless_clause.result, Some(doc))?;
            if result_type != ExpressionType::Never {
                non_veto_types.push((
                    if idx == 0 {
                        "first unless clause"
                    } else {
                        "unless clause"
                    },
                    result_type,
                ));
            }
        }

        if non_veto_types.is_empty() {
            return Ok(());
        }

        let (first_label, first_type) = &non_veto_types[0];
        for (label, branch_type) in &non_veto_types[1..] {
            if !self.are_types_compatible(first_type, branch_type) {
                return Err(LemmaError::Engine(format!(
                    "Rule '{}' has incompatible return types: {} returns {} but {} returns {}",
                    rule.name,
                    first_label,
                    first_type.name(),
                    label,
                    branch_type.name()
                )));
            }
        }

        Ok(())
    }

    /// Check if two types are compatible
    fn are_types_compatible(&self, type1: &ExpressionType, type2: &ExpressionType) -> bool {
        if type1 == type2 {
            return true;
        }

        if type1 == &ExpressionType::Unknown || type2 == &ExpressionType::Unknown {
            return true;
        }

        false
    }

    /// Validate that money arithmetic uses the same currency
    fn validate_money_arithmetic(
        &self,
        left: &Expression,
        right: &Expression,
        doc: &LemmaDoc,
    ) -> LemmaResult<()> {
        let left_currency = self.extract_currency(left, doc);
        let right_currency = self.extract_currency(right, doc);

        if let (Some(left_curr), Some(right_curr)) = (left_currency, right_currency) {
            if left_curr != right_curr {
                return Err(LemmaError::Engine(format!(
                    "Cannot perform arithmetic with different currencies: {} and {}",
                    left_curr, right_curr
                )));
            }
        }

        Ok(())
    }

    /// Validate that money comparisons use the same currency
    fn validate_money_comparison(
        &self,
        left: &Expression,
        right: &Expression,
        doc: &LemmaDoc,
    ) -> LemmaResult<()> {
        let left_currency = self.extract_currency(left, doc);
        let right_currency = self.extract_currency(right, doc);

        if let (Some(left_curr), Some(right_curr)) = (left_currency, right_currency) {
            if left_curr != right_curr {
                return Err(LemmaError::Engine(format!(
                    "Cannot compare different currencies: {} and {}",
                    left_curr, right_curr
                )));
            }
        }

        Ok(())
    }

    /// Extract currency from an expression if it's a Money type
    fn extract_currency(&self, expr: &Expression, doc: &LemmaDoc) -> Option<crate::MoneyUnit> {
        match &expr.kind {
            ExpressionKind::Literal(crate::LiteralValue::Unit(crate::NumericUnit::Money(
                _,
                currency,
            ))) => Some(currency.clone()),
            ExpressionKind::FactReference(fact_ref) => {
                let fact_name = &fact_ref.reference[0];
                for fact in &doc.facts {
                    if let crate::FactType::Local(name) = &fact.fact_type {
                        if name == fact_name {
                            if let crate::FactValue::Literal(crate::LiteralValue::Unit(
                                crate::NumericUnit::Money(_, currency),
                            )) = &fact.value
                            {
                                return Some(currency.clone());
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Infer the type of an expression
    fn infer_expression_type(&self, expr: &Expression) -> LemmaResult<ExpressionType> {
        self.infer_expression_type_with_context(expr, None)
    }

    #[allow(clippy::only_used_in_recursion)]
    fn infer_expression_type_with_context(
        &self,
        expr: &Expression,
        doc: Option<&LemmaDoc>,
    ) -> LemmaResult<ExpressionType> {
        match &expr.kind {
            ExpressionKind::Literal(lit) => Ok(ExpressionType::from_literal(lit)),
            ExpressionKind::Comparison(_, _, _) => Ok(ExpressionType::Boolean),
            ExpressionKind::LogicalAnd(_, _) => Ok(ExpressionType::Boolean),
            ExpressionKind::LogicalOr(_, _) => Ok(ExpressionType::Boolean),
            ExpressionKind::LogicalNegation(_, _) => Ok(ExpressionType::Boolean),
            ExpressionKind::FactHasAnyValue(_) => Ok(ExpressionType::Boolean),
            ExpressionKind::Veto(_) => Ok(ExpressionType::Never),
            ExpressionKind::FactReference(fact_ref) => {
                // Try to resolve fact type from document
                let Some(d) = doc else {
                    return Ok(ExpressionType::Unknown);
                };

                let ref_name = fact_ref.reference.join(".");
                for fact in &d.facts {
                    let fact_name = crate::analysis::fact_display_name(fact);
                    if fact_name != ref_name {
                        continue;
                    }
                    if let FactValue::Literal(lit) = &fact.value {
                        return Ok(ExpressionType::from_literal(lit));
                    }
                }
                Ok(ExpressionType::Unknown)
            }
            ExpressionKind::RuleReference(_) => {
                // Rules can't be resolved without full dependency analysis
                Ok(ExpressionType::Unknown)
            }
            ExpressionKind::Arithmetic(left, _, right) => {
                let left_type = self.infer_expression_type_with_context(left, doc)?;
                let right_type = self.infer_expression_type_with_context(right, doc)?;
                if left_type == ExpressionType::Unknown || right_type == ExpressionType::Unknown {
                    return Ok(ExpressionType::Unknown);
                }
                // Division of numbers (or other compatible types) produces a number
                Ok(ExpressionType::Number)
            }
            ExpressionKind::MathematicalOperator(_, _) => Ok(ExpressionType::Number),
            ExpressionKind::UnitConversion(value_expr, target) => {
                let value_type = self.infer_expression_type_with_context(value_expr, doc)?;
                Ok(self.infer_conversion_result_type(&value_type, target))
            }
        }
    }

    /// Helper to infer the result type of a unit conversion
    fn infer_conversion_result_type(
        &self,
        value_type: &ExpressionType,
        target: &ConversionTarget,
    ) -> ExpressionType {
        match (value_type, target) {
            // Number to Unit conversions
            (ExpressionType::Number, ConversionTarget::Mass(_)) => ExpressionType::Mass,
            (ExpressionType::Number, ConversionTarget::Length(_)) => ExpressionType::Length,
            (ExpressionType::Number, ConversionTarget::Volume(_)) => ExpressionType::Volume,
            (ExpressionType::Number, ConversionTarget::Duration(_)) => ExpressionType::Duration,
            (ExpressionType::Number, ConversionTarget::Temperature(_)) => {
                ExpressionType::Temperature
            }
            (ExpressionType::Number, ConversionTarget::Power(_)) => ExpressionType::Power,
            (ExpressionType::Number, ConversionTarget::Force(_)) => ExpressionType::Force,
            (ExpressionType::Number, ConversionTarget::Pressure(_)) => ExpressionType::Pressure,
            (ExpressionType::Number, ConversionTarget::Energy(_)) => ExpressionType::Energy,
            (ExpressionType::Number, ConversionTarget::Frequency(_)) => ExpressionType::Frequency,
            (ExpressionType::Number, ConversionTarget::Data(_)) => ExpressionType::Data,
            (ExpressionType::Number, ConversionTarget::Money(_)) => ExpressionType::Money,
            (ExpressionType::Number, ConversionTarget::Percentage) => ExpressionType::Percentage,

            // Unit to Number conversions (all physical units) and Percentage conversions
            (_, ConversionTarget::Percentage) => ExpressionType::Percentage,
            _ => ExpressionType::Number,
        }
    }
}
