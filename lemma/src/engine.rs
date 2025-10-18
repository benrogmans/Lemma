use crate::evaluator::Evaluator;
use crate::{parse, Response, LemmaResult, Validator};
use std::collections::HashMap;

/// The Lemma evaluation engine.
///
/// Pure Rust implementation that evaluates Lemma documents directly from the AST.
pub struct Engine {
    documents: HashMap<String, crate::LemmaDoc>,
    sources: HashMap<String, String>,
    validator: Validator,
    evaluator: Evaluator,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            sources: HashMap::new(),
            validator: Validator::new(),
            evaluator: Evaluator::new(),
        }
    }

    pub fn add_lemma_code(&mut self, lemma_code: &str, source: &str) -> LemmaResult<()> {
        // Parse the documents
        let new_docs = parse(lemma_code, Some(source.to_string()))?;

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

    pub fn evaluate(&self, doc_name: &str, fact_values: Vec<&str>) -> LemmaResult<Response> {
        self.evaluate_rules(doc_name, None, fact_values)
    }

    /// Evaluate specific rules in a document
    ///
    /// If `rule_names` is None, evaluates all rules (backward compatible).
    /// If `rule_names` is Some, only returns results for the specified rules,
    /// but still computes their dependencies.
    pub fn evaluate_rules(
        &self,
        doc_name: &str,
        rule_names: Option<Vec<String>>,
        fact_values: Vec<&str>,
    ) -> LemmaResult<Response> {
        // Parse fact overrides
        let fact_overrides = if !fact_values.is_empty() {
            crate::parser::parse_facts(&fact_values)?
        } else {
            vec![]
        };

        // Use the pure Rust evaluator
        self.evaluator.evaluate_document(
            doc_name,
            &self.documents,
            &self.sources,
            fact_overrides,
            rule_names,
        )
    }
}

