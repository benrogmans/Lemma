use crate::evaluation::Evaluator;
use crate::parsing::ast::Span;
use crate::planning::plan;
use crate::{parse, LemmaDoc, LemmaError, LemmaResult, ResourceLimits, Response};
use std::collections::HashMap;
use std::sync::Arc;

/// Engine for evaluating Lemma rules
///
/// Pure Rust implementation that evaluates Lemma docs directly from the AST.
/// Uses pre-built execution plans that are self-contained and ready for evaluation.
pub struct Engine {
    execution_plans: HashMap<String, crate::planning::ExecutionPlan>,
    documents: HashMap<String, LemmaDoc>,
    sources: HashMap<String, String>,
    evaluator: Evaluator,
    limits: ResourceLimits,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            execution_plans: HashMap::new(),
            documents: HashMap::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits: ResourceLimits::default(),
        }
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an engine with custom resource limits
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            execution_plans: HashMap::new(),
            documents: HashMap::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits,
        }
    }

    pub fn add_lemma_code(&mut self, lemma_code: &str, source: &str) -> LemmaResult<()> {
        let new_docs = parse(lemma_code, source, &self.limits)?;

        for doc in &new_docs {
            let attribute = doc.attribute.clone().unwrap_or_else(|| doc.name.clone());
            self.sources.insert(attribute, lemma_code.to_owned());
            self.documents.insert(doc.name.clone(), doc.clone());
        }

        // Collect all documents (existing + new)
        let all_docs: Vec<LemmaDoc> = self.documents.values().cloned().collect();

        // Build execution plans for all new documents
        for doc in &new_docs {
            let execution_plan = plan(doc, &all_docs, self.sources.clone()).map_err(|errs| {
                if errs.is_empty() {
                    use crate::parsing::ast::Span;
                    let attribute = doc.attribute.as_deref().unwrap_or(&doc.name);
                    let source_text = self
                        .sources
                        .get(attribute)
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    LemmaError::engine(
                        format!("Failed to build execution plan for document: {}", doc.name),
                        Span {
                            start: 0,
                            end: 0,
                            line: doc.start_line,
                            col: 0,
                        },
                        attribute,
                        std::sync::Arc::from(source_text),
                        doc.name.clone(),
                        doc.start_line,
                        None::<String>,
                    )
                } else {
                    errs.into_iter().next().unwrap_or_else(|| {
                        use crate::parsing::ast::Span;
                        let attribute = doc.attribute.as_deref().unwrap_or(&doc.name);
                        let source_text = self
                            .sources
                            .get(attribute)
                            .map(|s| s.as_str())
                            .unwrap_or("");
                        LemmaError::engine(
                            format!("Failed to build execution plan for document: {}", doc.name),
                            Span {
                                start: 0,
                                end: 0,
                                line: doc.start_line,
                                col: 0,
                            },
                            attribute,
                            std::sync::Arc::from(source_text),
                            doc.name.clone(),
                            doc.start_line,
                            None::<String>,
                        )
                    })
                }
            })?;

            self.execution_plans
                .insert(doc.name.clone(), execution_plan);
        }

        Ok(())
    }

    pub fn remove_document(&mut self, doc_name: &str) {
        self.execution_plans.remove(doc_name);
        self.documents.remove(doc_name);
    }

    pub fn list_documents(&self) -> Vec<String> {
        self.documents.keys().cloned().collect()
    }

    pub fn get_document(&self, doc_name: &str) -> Option<&LemmaDoc> {
        self.documents.get(doc_name)
    }

    pub fn get_document_facts(&self, doc_name: &str) -> Vec<&crate::LemmaFact> {
        if let Some(doc) = self.documents.get(doc_name) {
            doc.facts.iter().collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_document_rules(&self, doc_name: &str) -> Vec<&crate::LemmaRule> {
        if let Some(doc) = self.documents.get(doc_name) {
            doc.rules.iter().collect()
        } else {
            Vec::new()
        }
    }

    /// Evaluate rules in a document with JSON values for facts.
    ///
    /// This is a convenience method that accepts JSON directly and converts it
    /// to typed values using the document's fact type declarations.
    ///
    /// If `rule_names` is empty, evaluates all rules.
    /// Otherwise, only returns results for the specified rules (dependencies still computed).
    ///
    /// Values are provided as JSON bytes (e.g., `b"{\"quantity\": 5, \"is_member\": true}"`).
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn evaluate_json(
        &self,
        doc_name: &str,
        rule_names: Vec<String>,
        json: &[u8],
    ) -> LemmaResult<Response> {
        let base_plan = self.execution_plans.get(doc_name).ok_or_else(|| {
            LemmaError::engine(
                format!("Document '{}' not found", doc_name),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(""),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;

        let values = crate::serialization::from_json(json, base_plan)?;

        self.evaluate_strict(doc_name, rule_names, values)
    }

    /// Evaluate rules in a document with string values for facts.
    ///
    /// This is the user-friendly API that accepts raw string values and parses them
    /// to the appropriate types based on the document's fact type declarations.
    /// Use this for CLI, HTTP APIs, and other user-facing interfaces.
    ///
    /// If `rule_names` is empty, evaluates all rules.
    /// Otherwise, only returns results for the specified rules (dependencies still computed).
    ///
    /// Values are provided as name -> value string pairs (e.g., "type" -> "latte").
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn evaluate(
        &self,
        doc_name: &str,
        rule_names: Vec<String>,
        values: HashMap<String, String>,
    ) -> LemmaResult<Response> {
        let base_plan = self.execution_plans.get(doc_name).ok_or_else(|| {
            LemmaError::engine(
                format!("Document '{}' not found", doc_name),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(""),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;

        let plan = base_plan.clone().with_values(values, &self.limits)?;

        self.evaluate_plan(plan, rule_names)
    }

    /// Evaluate rules in a document with typed values for facts.
    ///
    /// This is the strict API that accepts pre-typed LiteralValue values.
    /// Use this for programmatic APIs, protobuf, msgpack, FFI, and other
    /// strongly-typed interfaces where values are already parsed.
    ///
    /// If `rule_names` is empty, evaluates all rules.
    /// Otherwise, only returns results for the specified rules (dependencies still computed).
    ///
    /// Values are provided as name -> LiteralValue pairs (e.g., "age" -> Number(25)).
    pub fn evaluate_strict(
        &self,
        doc_name: &str,
        rule_names: Vec<String>,
        values: HashMap<String, crate::LiteralValue>,
    ) -> LemmaResult<Response> {
        let base_plan = self.execution_plans.get(doc_name).ok_or_else(|| {
            LemmaError::engine(
                format!("Document '{}' not found", doc_name),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(""),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;

        let plan = base_plan.clone().with_typed_values(values, &self.limits)?;

        self.evaluate_plan(plan, rule_names)
    }

    /// Invert a rule to find input domains that produce a desired outcome with JSON values.
    ///
    /// This is a convenience method that accepts JSON directly and converts it
    /// to typed values using the document's fact type declarations.
    ///
    /// Returns an InversionResponse containing:
    /// - `solutions`: Concrete domain constraints for each free variable
    /// - `undetermined_facts`: Facts that are not fully determined
    /// - `is_determined`: Whether all facts have concrete values
    ///
    /// Values are provided as JSON bytes (e.g., `b"{\"quantity\": 5, \"is_member\": true}"`).
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn invert_json(
        &self,
        doc_name: &str,
        rule_name: &str,
        target: crate::inversion::Target,
        json: &[u8],
    ) -> LemmaResult<crate::InversionResponse> {
        let base_plan = self.execution_plans.get(doc_name).ok_or_else(|| {
            LemmaError::engine(
                format!("Document '{}' not found", doc_name),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(""),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;

        let values = crate::serialization::from_json(json, base_plan)?;

        self.invert_strict(doc_name, rule_name, target, values)
    }

    /// Invert a rule to find input domains that produce a desired outcome.
    ///
    /// This is the user-friendly API that accepts raw string values and parses them
    /// to the appropriate types based on the document's fact type declarations.
    ///
    /// Returns an InversionResponse containing:
    /// - `solutions`: Concrete domain constraints for each free variable
    /// - `undetermined_facts`: Facts that are not fully determined
    /// - `is_determined`: Whether all facts have concrete values
    ///
    /// Values are provided as name -> value string pairs (e.g., "quantity" -> "5").
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn invert(
        &self,
        doc_name: &str,
        rule_name: &str,
        target: crate::inversion::Target,
        values: HashMap<String, String>,
    ) -> LemmaResult<crate::InversionResponse> {
        let base_plan = self.execution_plans.get(doc_name).ok_or_else(|| {
            LemmaError::engine(
                format!("Document '{}' not found", doc_name),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(""),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;

        let plan = base_plan.clone().with_values(values, &self.limits)?;

        // Collect provided fact paths
        let provided_facts = plan
            .facts
            .iter()
            .filter_map(|(path, fact)| {
                if matches!(fact.value, crate::FactValue::Literal(_)) {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect();

        crate::inversion::invert(rule_name, target, &plan, &provided_facts)
    }

    /// Invert a rule to find input domains that produce a desired outcome.
    ///
    /// This is the strict API that accepts pre-typed LiteralValue values.
    /// Use this for programmatic APIs, protobuf, msgpack, FFI, and other
    /// strongly-typed interfaces where values are already parsed.
    ///
    /// Returns an InversionResponse containing:
    /// - `solutions`: Concrete domain constraints for each free variable
    /// - `undetermined_facts`: Facts that are not fully determined
    /// - `is_determined`: Whether all facts have concrete values
    ///
    /// Values are provided as name -> LiteralValue pairs (e.g., "quantity" -> Number(5)).
    pub fn invert_strict(
        &self,
        doc_name: &str,
        rule_name: &str,
        target: crate::inversion::Target,
        values: HashMap<String, crate::LiteralValue>,
    ) -> LemmaResult<crate::InversionResponse> {
        let base_plan = self.execution_plans.get(doc_name).ok_or_else(|| {
            LemmaError::engine(
                format!("Document '{}' not found", doc_name),
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    col: 0,
                },
                "<unknown>",
                Arc::from(""),
                "<unknown>",
                1,
                None::<String>,
            )
        })?;

        let plan = base_plan.clone().with_typed_values(values, &self.limits)?;

        // Collect provided fact paths
        let provided_facts = plan
            .facts
            .iter()
            .filter_map(|(path, fact)| {
                if matches!(fact.value, crate::FactValue::Literal(_)) {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect();

        crate::inversion::invert(rule_name, target, &plan, &provided_facts)
    }

    fn evaluate_plan(
        &self,
        plan: crate::planning::ExecutionPlan,
        rule_names: Vec<String>,
    ) -> LemmaResult<Response> {
        let mut response = self.evaluator.evaluate(&plan)?;

        if !rule_names.is_empty() {
            response.filter_rules(&rule_names);
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn test_evaluate_document_all_rules() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
        doc test
        fact x = 10
        fact y = 5
        rule sum = x + y
        rule product = x * y
    "#,
                "test.lemma",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(response.results.len(), 2);

        let sum_result = response
            .results
            .values()
            .find(|r| r.rule.name == "sum")
            .unwrap();
        assert_eq!(
            sum_result.result,
            crate::OperationResult::Value(crate::LiteralValue::number(
                Decimal::from_str("15").unwrap()
            ))
        );

        let product_result = response
            .results
            .values()
            .find(|r| r.rule.name == "product")
            .unwrap();
        assert_eq!(
            product_result.result,
            crate::OperationResult::Value(crate::LiteralValue::number(
                Decimal::from_str("50").unwrap()
            ))
        );
    }

    #[test]
    fn test_evaluate_empty_facts() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
        doc test
        fact price = 100
        rule total = price * 2
    "#,
                "test.lemma",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(crate::LiteralValue::number(
                Decimal::from_str("200").unwrap()
            ))
        );
    }

    #[test]
    fn test_evaluate_boolean_rule() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
        doc test
        fact age = 25
        rule is_adult = age >= 18
    "#,
                "test.lemma",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(crate::LiteralValue::boolean(crate::BooleanValue::True))
        );
    }

    #[test]
    fn test_evaluate_with_unless_clause() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
        doc test
        fact quantity = 15
        rule discount = 0
          unless quantity >= 10 then 10
    "#,
                "test.lemma",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(crate::LiteralValue::number(
                Decimal::from_str("10").unwrap()
            ))
        );
    }

    #[test]
    fn test_document_not_found() {
        let engine = Engine::new();
        let result = engine.evaluate("nonexistent", vec![], HashMap::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_multiple_documents() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
        doc doc1
        fact x = 10
        rule result = x * 2
    "#,
                "doc1.lemma",
            )
            .unwrap();

        engine
            .add_lemma_code(
                r#"
        doc doc2
        fact y = 5
        rule result = y * 3
    "#,
                "doc2.lemma",
            )
            .unwrap();

        let response1 = engine.evaluate("doc1", vec![], HashMap::new()).unwrap();
        assert_eq!(
            response1.results[0].result,
            crate::OperationResult::Value(crate::LiteralValue::number(
                Decimal::from_str("20").unwrap()
            ))
        );

        let response2 = engine.evaluate("doc2", vec![], HashMap::new()).unwrap();
        assert_eq!(
            response2.results[0].result,
            crate::OperationResult::Value(crate::LiteralValue::number(
                Decimal::from_str("15").unwrap()
            ))
        );
    }

    #[test]
    fn test_runtime_error_mapping() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
        doc test
        fact numerator = 10
        fact denominator = 0
        rule division = numerator / denominator
    "#,
                "test.lemma",
            )
            .unwrap();

        let result = engine.evaluate("test", vec![], HashMap::new());
        // Division by zero returns a Veto (not an error) in the new evaluation design
        assert!(result.is_ok(), "Evaluation should succeed");
        let response = result.unwrap();
        let division_result = response
            .results
            .values()
            .find(|r| r.rule.name == "division");
        assert!(
            division_result.is_some(),
            "Should have division rule result"
        );
        match &division_result.unwrap().result {
            crate::OperationResult::Veto(message) => {
                assert!(
                    message
                        .as_ref()
                        .map(|m| m.contains("Division by zero"))
                        .unwrap_or(false),
                    "Veto message should mention division by zero: {:?}",
                    message
                );
            }
            other => panic!("Expected Veto for division by zero, got {:?}", other),
        }
    }

    #[test]
    fn test_rules_sorted_by_source_order() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
        doc test
        fact a = 1
        fact b = 2
        rule z = a + b
        rule y = a * b
        rule x = a - b
    "#,
                "test.lemma",
            )
            .unwrap();

        let response = engine.evaluate("test", vec![], HashMap::new()).unwrap();
        assert_eq!(response.results.len(), 3);

        // Check they all have span information for ordering
        for result in response.results.values() {
            assert!(
                result.rule.source_location.is_some(),
                "Rule {} missing source_location",
                result.rule.name
            );
        }

        // Verify source positions increase (z < y < x)
        let z_pos = response
            .results
            .values()
            .find(|r| r.rule.name == "z")
            .unwrap()
            .rule
            .source_location
            .as_ref()
            .unwrap()
            .span
            .start;
        let y_pos = response
            .results
            .values()
            .find(|r| r.rule.name == "y")
            .unwrap()
            .rule
            .source_location
            .as_ref()
            .unwrap()
            .span
            .start;
        let x_pos = response
            .results
            .values()
            .find(|r| r.rule.name == "x")
            .unwrap()
            .rule
            .source_location
            .as_ref()
            .unwrap()
            .span
            .start;

        assert!(z_pos < y_pos);
        assert!(y_pos < x_pos);
    }

    #[test]
    fn test_rule_filtering_evaluates_dependencies() {
        let mut engine = Engine::new();
        engine
            .add_lemma_code(
                r#"
        doc test
        fact base = 100
        rule subtotal = base * 2
        rule tax = subtotal? * 10%
        rule total = subtotal? + tax?
    "#,
                "test.lemma",
            )
            .unwrap();

        // Request only 'total', but it depends on 'subtotal' and 'tax'
        let response = engine
            .evaluate("test", vec!["total".to_string()], HashMap::new())
            .unwrap();

        // Only 'total' should be in results
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results.keys().next().unwrap(), "total");

        // But the value should be correct (dependencies were computed)
        let total = response.results.values().next().unwrap();
        assert_eq!(
            total.result,
            crate::OperationResult::Value(crate::LiteralValue::number(
                Decimal::from_str("220").unwrap()
            ))
        );
    }
}
