use crate::evaluator::Evaluator;
use crate::{parse, LemmaDoc, LemmaError, LemmaResult, ResourceLimits, Response, Validator};
use std::collections::HashMap;

/// The Lemma evaluation engine.
///
/// Pure Rust implementation that evaluates Lemma documents directly from the AST.
pub struct Engine {
    documents: HashMap<String, LemmaDoc>,
    sources: HashMap<String, String>,
    validator: Validator,
    evaluator: Evaluator,
    limits: ResourceLimits,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            documents: HashMap::new(),
            sources: HashMap::new(),
            validator: Validator,
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
            documents: HashMap::new(),
            sources: HashMap::new(),
            validator: Validator,
            evaluator: Evaluator,
            limits,
        }
    }

    /// Get the current resource limits
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    pub fn add_lemma_code(&mut self, lemma_code: &str, source: &str) -> LemmaResult<()> {
        // Parse the documents
        let new_docs = parse(lemma_code, Some(source.to_string()), &self.limits)?;

        // Store source text for all new documents
        for doc in &new_docs {
            let source_id = doc.source.clone().unwrap_or_else(|| "<input>".to_string());
            self.sources.insert(source_id, lemma_code.to_string());
        }

        // Combine existing documents with new documents for semantic validation
        let mut all_docs: Vec<crate::LemmaDoc> = self.documents.values().cloned().collect();
        all_docs.extend(new_docs);

        // Run semantic validation on all documents
        let validated = self.validator.validate_all(all_docs)?;

        // Store the validated documents
        for doc in validated.documents {
            self.documents.insert(doc.name.clone(), doc);
        }

        Ok(())
    }

    pub fn remove_document(&mut self, doc_name: &str) {
        self.documents.remove(doc_name);
    }

    pub fn list_documents(&self) -> Vec<String> {
        self.documents.keys().cloned().collect()
    }

    pub fn get_document(&self, doc_name: &str) -> Option<&crate::LemmaDoc> {
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

    /// Evaluate rules in a document with optional fact overrides
    ///
    /// If `rule_names` is None, evaluates all rules.
    /// If `rule_names` is Some, only returns results for the specified rules,
    /// but still computes their dependencies.
    ///
    /// Fact overrides must be pre-parsed using `parse_facts()`.
    pub fn evaluate(
        &self,
        doc_name: &str,
        rule_names: Option<Vec<String>>,
        fact_overrides: Option<Vec<crate::LemmaFact>>,
    ) -> LemmaResult<Response> {
        let overrides = fact_overrides.unwrap_or_default();

        // Check ALL fact value sizes against limit
        for fact in &overrides {
            if let crate::FactValue::Literal(lit) = &fact.value {
                let size = lit.byte_size();

                if size > self.limits.max_fact_value_bytes {
                    return Err(LemmaError::ResourceLimitExceeded {
                        limit_name: "max_fact_value_bytes".to_string(),
                        limit_value: self.limits.max_fact_value_bytes.to_string(),
                        actual_value: size.to_string(),
                        suggestion: format!(
                            "Reduce the size of fact values to {} bytes or less",
                            self.limits.max_fact_value_bytes
                        ),
                    });
                }
            }
        }

        self.evaluator.evaluate_document(
            doc_name,
            &self.documents,
            &self.sources,
            overrides,
            rule_names,
            &self.limits,
        )
    }

    /// Get all documents (needed by serializers for schema resolution)
    pub fn get_all_documents(&self) -> &HashMap<String, crate::LemmaDoc> {
        &self.documents
    }
}
