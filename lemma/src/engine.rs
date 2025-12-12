use crate::evaluation::Evaluator;
use crate::planning::plan;
use crate::{parse, LemmaDoc, LemmaError, LemmaResult, ResourceLimits, Response};
use std::collections::HashMap;

/// Engine for evaluating Lemma rules
///
/// Pure Rust implementation that evaluates Lemma docs directly from the AST.
/// Uses pre-built execution plans that are self-contained and ready for evaluation.
pub struct Engine {
    execution_plans: HashMap<String, crate::planning::ExecutionPlan>,
    documents: HashMap<String, LemmaDoc>,
    sources: HashMap<String, (String, String)>,
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

    pub fn add_lemma_code(&mut self, lemma_code: &str, source_id: &str) -> LemmaResult<()> {
        let new_docs = parse(lemma_code, Some(source_id.to_owned()), &self.limits)?;

        for doc in &new_docs {
            if self.documents.contains_key(&doc.name) {
                return Err(LemmaError::Engine(format!(
                    "Document '{}' already exists. Use remove_document() first to replace it.",
                    doc.name
                )));
            }
        }

        for doc in &new_docs {
            self.sources.insert(
                doc.name.clone(),
                (source_id.to_string(), lemma_code.to_string()),
            );
            self.documents.insert(doc.name.clone(), doc.clone());
        }

        // Collect all documents (existing + new)
        let all_docs: Vec<LemmaDoc> = self.documents.values().cloned().collect();

        // Build execution plans for all new documents
        for doc in &new_docs {
            let execution_plan = plan(doc, &all_docs, self.sources.clone()).map_err(|errs| {
                if errs.is_empty() {
                    LemmaError::Engine(format!(
                        "Failed to build execution plan for document: {}",
                        doc.name
                    ))
                } else {
                    errs.into_iter().next().unwrap_or_else(|| {
                        LemmaError::Engine(format!(
                            "Failed to build execution plan for document: {}",
                            doc.name
                        ))
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
        self.sources.remove(doc_name);
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
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

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
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        let plan = base_plan.clone().with_typed_values(values, &self.limits)?;

        self.evaluate_plan(plan, rule_names)
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
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?;

        let values = crate::serialization::from_json(json, base_plan)?;

        self.evaluate_strict(doc_name, rule_names, values)
    }

    /// Invert a rule to find input domains that produce a desired outcome.
    ///
    /// Returns an InversionResponse containing:
    /// - `solutions`: Solutions with conditions, outcomes, and proofs
    /// # Arguments
    ///
    /// * `operator` - Comparison operator: "=", "!=", "<", "<=", ">", ">="
    /// * `outcome` - Desired result, or None for any_value (returns all possible outcomes)
    ///
    /// Values are provided as name -> value string pairs (e.g., "quantity" -> "5").
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn invert(
        &self,
        doc_name: &str,
        rule_name: &str,
        operator: &str,
        outcome: Option<crate::computation::OperationResult>,
        values: HashMap<String, String>,
    ) -> LemmaResult<crate::InversionResponse> {
        let plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?
            .clone()
            .with_values(values, &self.limits)?;

        crate::inversion::invert(&plan, rule_name, operator, outcome)
    }

    /// Invert a rule to find input domains that produce a desired outcome.
    ///
    /// Returns an InversionResponse containing:
    /// - `solutions`: Solutions with conditions, outcomes, and proofs
    ///
    /// # Arguments
    ///
    /// * `operator` - Comparison operator: "=", "!=", "<", "<=", ">", ">="
    /// * `outcome` - Desired result, or None for any_value (returns all possible outcomes)
    ///
    /// Values are provided as name -> LiteralValue pairs (e.g., "quantity" -> Number(5)).
    pub fn invert_strict(
        &self,
        doc_name: &str,
        rule_name: &str,
        operator: &str,
        outcome: Option<crate::computation::OperationResult>,
        values: HashMap<String, crate::LiteralValue>,
    ) -> LemmaResult<crate::InversionResponse> {
        let plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?
            .clone()
            .with_typed_values(values, &self.limits)?;

        crate::inversion::invert(&plan, rule_name, operator, outcome)
    }

    /// Invert a rule to find input domains that produce a desired outcome with JSON values.
    ///
    /// Returns an InversionResponse containing:
    /// - `solutions`: Solutions with conditions, outcomes, and proofs
    ///
    /// # Arguments
    ///
    /// * `operator` - Comparison operator: "=", "!=", "<", "<=", ">", ">="
    /// * `outcome` - Desired result, or None for any_value (returns all possible outcomes)
    ///
    /// Values are provided as JSON bytes (e.g., `b"{\"quantity\": 5, \"is_member\": true}"`).
    /// They are automatically parsed to the expected type based on the document schema.
    pub fn invert_json(
        &self,
        doc_name: &str,
        rule_name: &str,
        operator: &str,
        outcome: Option<crate::computation::OperationResult>,
        json: &[u8],
    ) -> LemmaResult<crate::InversionResponse> {
        
        let base_plan = self
            .execution_plans
            .get(doc_name)
            .ok_or_else(|| LemmaError::Engine(format!("Document '{}' not found", doc_name)))?
            .clone();

        let values = crate::serialization::from_json(json, &base_plan)?;
        let plan = base_plan.with_typed_values(values, &self.limits)?;

        crate::inversion::invert(&plan, rule_name, operator, outcome)
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
    use std::collections::HashMap;
    use std::str::FromStr;

    #[test]
    fn test_number_type_validation_rejects_text() {
        let code = r#"
doc test
fact age = [number]
rule doubled = age * 2
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();

        let mut facts = HashMap::new();
        facts.insert("age".to_string(), "twenty".to_string());

        let result = engine.evaluate("test", vec![], facts);

        assert!(result.is_err(), "Expected error but got: {:?}", result);
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("Failed to parse fact 'age'"),
            "Error was: {}",
            error
        );
    }

    #[test]
    fn test_multiple_type_validations() {
        let code = r#"
doc test
fact price = [number]
fact quantity = [number]
fact active = [boolean]
rule total = price * quantity
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();

        let mut facts = HashMap::new();
        facts.insert("price".to_string(), "expensive".to_string());
        facts.insert("quantity".to_string(), "5".to_string());
        facts.insert("active".to_string(), "true".to_string());

        let result = engine.evaluate("test", vec![], facts);
        assert!(result.is_err(), "Expected type mismatch error");
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse fact 'price'"));

        let mut facts = HashMap::new();
        facts.insert("price".to_string(), "100".to_string());
        facts.insert("quantity".to_string(), "five".to_string());
        facts.insert("active".to_string(), "true".to_string());

        let result = engine.evaluate("test", vec![], facts);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse fact 'quantity'"));

        let mut facts = HashMap::new();
        facts.insert("price".to_string(), "100".to_string());
        facts.insert("quantity".to_string(), "5".to_string());
        facts.insert("active".to_string(), "maybe".to_string());

        let result = engine.evaluate("test", vec![], facts);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse fact 'active'"));

        let mut facts = HashMap::new();
        facts.insert("price".to_string(), "100".to_string());
        facts.insert("quantity".to_string(), "5".to_string());
        facts.insert("active".to_string(), "true".to_string());

        let result = engine.evaluate("test", vec![], facts);
        assert!(result.is_ok(), "Should succeed with all valid fact types");

        // Verify the calculation is correct
        let response = result.unwrap();
        let total_rule = response
            .results
            .get("total")
            .expect("Should have total rule");
        match &total_rule.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                // total = price * quantity = 100 * 5 = 500
                assert_eq!(*n, Decimal::from_str("500").unwrap());
            }
            other => panic!("total should be 500, got: {:?}", other),
        }
    }

    #[test]
    fn test_literal_fact_type_validation() {
        let code = r#"
doc test
fact base_price = 50
rule total = base_price * 1.2
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();

        let mut facts = HashMap::new();
        facts.insert("base_price".to_string(), "sixty".to_string());

        let result = engine.evaluate("test", vec![], facts);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse fact 'base_price'"));

        // When base_price is overridden, it uses the override value
        let mut facts = HashMap::new();
        facts.insert("base_price".to_string(), "60".to_string());

        let result = engine.evaluate("test", vec![], facts);
        assert!(
            result.is_ok(),
            "Should succeed with valid literal fact type"
        );

        // Verify the calculation is correct
        let response = result.unwrap();
        let total_rule = response
            .results
            .get("total")
            .expect("Should have total rule");
        match &total_rule.result {
            crate::OperationResult::Value(crate::LiteralValue::Number(n)) => {
                // total = base_price * 1.2 = 60 * 1.2 = 72 (override value, not literal 50)
                assert_eq!(*n, Decimal::from_str("72").unwrap());
            }
            other => panic!("total should be 72 (60 * 1.2), got: {:?}", other),
        }
    }

    #[test]
    fn test_unknown_fact_override_rejected() {
        let code = r#"
doc test
fact price = [number]
rule total = price * 1.1
"#;

        let mut engine = Engine::new();
        engine.add_lemma_code(code, "test.lemma").unwrap();

        let mut facts = HashMap::new();
        facts.insert("price".to_string(), "100".to_string());
        facts.insert("unknown_fact".to_string(), "42".to_string());

        let result = engine.evaluate("test", vec![], facts);
        assert!(result.is_err(), "Expected error for unknown fact override");
        assert!(result.unwrap_err().to_string().contains("unknown_fact"));
    }
}
