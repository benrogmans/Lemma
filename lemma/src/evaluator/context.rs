//! Evaluation context for executing Lemma rules
//!
//! Contains all state needed during evaluation of a single document.

use crate::{
    FactPath, FactType, FactValue, LemmaDoc, LemmaError, LemmaFact, LiteralValue, OperationRecord,
    OperationResult, ResourceLimits,
};
use std::collections::HashMap;
use std::time::Instant;

/// Context for evaluating a Lemma document
///
/// Contains all state needed for a single evaluation:
/// - Facts (inputs)
/// - Rule results (computed values)
/// - Operation records (execution log)
/// - Timeout tracking
pub struct EvaluationContext<'a> {
    /// Document being evaluated
    pub current_doc: &'a LemmaDoc,

    /// All loaded documents (for cross-document references)
    pub all_documents: &'a HashMap<String, LemmaDoc>,

    /// Source text for all documents (for error reporting)
    /// Maps source_id -> source text
    pub sources: &'a HashMap<String, String>,

    /// Fact values (from document + overrides)
    /// Maps fact path -> concrete value
    /// Only contains facts that have actual values (not TypeAnnotations)
    pub facts: HashMap<FactPath, LiteralValue>,

    /// Start time for timeout checking
    pub start_time: Instant,

    /// Resource limits including timeout
    pub limits: &'a ResourceLimits,

    /// Rule results computed so far (populated during execution)
    /// Maps RulePath -> operation result (either Value or Veto)
    pub rule_results: HashMap<crate::RulePath, OperationResult>,

    /// Operation records - records every operation
    pub operations: Vec<OperationRecord>,
}

impl<'a> EvaluationContext<'a> {
    /// Create a new evaluation context
    pub fn new(
        current_doc: &'a LemmaDoc,
        all_documents: &'a HashMap<String, LemmaDoc>,
        sources: &'a HashMap<String, String>,
        facts: HashMap<FactPath, LiteralValue>,
        start_time: Instant,
        limits: &'a ResourceLimits,
    ) -> Self {
        Self {
            current_doc,
            all_documents,
            sources,
            facts,
            rule_results: HashMap::new(),
            operations: Vec::new(),
            start_time,
            limits,
        }
    }

    /// Check if evaluation has exceeded timeout
    pub fn check_timeout(&self) -> Result<(), crate::LemmaError> {
        let elapsed_ms = self.start_time.elapsed().as_millis() as u64;
        if elapsed_ms > self.limits.max_evaluation_time_ms {
            return Err(crate::LemmaError::ResourceLimitExceeded {
                limit_name: "max_evaluation_time_ms".to_string(),
                limit_value: self.limits.max_evaluation_time_ms.to_string(),
                actual_value: elapsed_ms.to_string(),
                suggestion: format!(
                    "Evaluation took {}ms, exceeding the limit of {}ms. Simplify the document or increase the timeout.",
                    elapsed_ms, self.limits.max_evaluation_time_ms
                ),
            });
        }
        Ok(())
    }
}

/// Build a fact map from document facts and overrides
///
/// Includes facts with concrete values (FactValue::Literal) and expands
/// DocumentReference facts by importing all facts from the referenced document.
/// Facts with TypeAnnotation are missing and will cause evaluation errors.
///
/// Validates that fact overrides match the expected types declared in the document.
pub fn build_fact_map(
    doc: &LemmaDoc,
    doc_facts: &[LemmaFact],
    overrides: &[LemmaFact],
    all_documents: &HashMap<String, LemmaDoc>,
) -> Result<HashMap<FactPath, LiteralValue>, LemmaError> {
    let mut facts = HashMap::new();

    // Add document facts
    for fact in doc_facts {
        match &fact.value {
            FactValue::Literal(lit) => {
                let path = get_fact_path(fact);
                facts.insert(path, lit.clone());
            }
            FactValue::DocumentReference(doc_name) => {
                // Resolve document reference by recursively importing all facts from referenced doc
                if let Some(referenced_doc) = all_documents.get(doc_name) {
                    let fact_prefix = get_fact_path(fact);
                    // Recursively build fact map for the referenced document
                    let referenced_facts =
                        build_fact_map(referenced_doc, &referenced_doc.facts, &[], all_documents)?;
                    for (ref_fact_path, lit) in referenced_facts {
                        // Prepend the prefix to create the qualified path
                        let qualified_path = ref_fact_path.with_prefix(fact_prefix.segments());
                        facts.insert(qualified_path, lit);
                    }
                }
            }
            FactValue::TypeAnnotation(_) => {
                // Skip type annotations
            }
        }
    }

    // Apply overrides with type validation
    for fact in overrides {
        if let FactValue::Literal(lit) = &fact.value {
            let path = get_fact_path(fact);

            // Check if this fact exists in the document and validate type
            // Note: get_fact_type expects a string for now, we'll keep using Display for validation
            let name = path.to_string();
            if let Some(expected_type) = doc.get_fact_type(&name) {
                let actual_type = lit.to_type();
                if expected_type != actual_type {
                    return Err(LemmaError::Engine(format!(
                        "Type mismatch for fact '{}': expected {}, got {}",
                        name, expected_type, actual_type
                    )));
                }
            }

            facts.insert(path, lit.clone());
        }
    }

    Ok(facts)
}

/// Get the fact path for a fact (handles local and foreign facts)
fn get_fact_path(fact: &LemmaFact) -> FactPath {
    match &fact.fact_type {
        FactType::Local(name) => FactPath::new(vec![name.clone()]),
        FactType::Foreign(foreign_ref) => FactPath::new(foreign_ref.reference.clone()),
    }
}
