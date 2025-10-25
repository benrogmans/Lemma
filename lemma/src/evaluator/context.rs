//! Evaluation context for executing Lemma rules
//!
//! Contains all state needed during evaluation of a single document.

use crate::{
    FactType, FactValue, LemmaDoc, LemmaFact, LemmaError, LiteralValue, OperationRecord, OperationResult,
    ResourceLimits,
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
    /// Maps fact name -> concrete value
    /// Only contains facts that have actual values (not TypeAnnotations)
    pub facts: HashMap<String, LiteralValue>,

    /// Start time for timeout checking
    pub start_time: Instant,

    /// Resource limits including timeout
    pub limits: &'a ResourceLimits,

    /// Rule results computed so far (populated during execution)
    /// Maps rule name -> operation result (either Value or Veto)
    pub rule_results: HashMap<String, OperationResult>,

    /// Operation records - records every operation
    pub operations: Vec<OperationRecord>,
}

impl<'a> EvaluationContext<'a> {
    /// Create a new evaluation context
    pub fn new(
        current_doc: &'a LemmaDoc,
        all_documents: &'a HashMap<String, LemmaDoc>,
        sources: &'a HashMap<String, String>,
        facts: HashMap<String, LiteralValue>,
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
) -> Result<HashMap<String, LiteralValue>, LemmaError> {
    let mut facts = HashMap::new();

    // Add document facts
    for fact in doc_facts {
        match &fact.value {
            FactValue::Literal(lit) => {
                let name = get_fact_name(fact);
                facts.insert(name, lit.clone());
            }
            FactValue::DocumentReference(doc_name) => {
                // Resolve document reference by importing all facts from referenced doc
                if let Some(referenced_doc) = all_documents.get(doc_name) {
                    let fact_prefix = get_fact_name(fact);
                    for ref_fact in &referenced_doc.facts {
                        if let FactValue::Literal(lit) = &ref_fact.value {
                            let ref_fact_name = get_fact_name(ref_fact);
                            let qualified_name = format!("{}.{}", fact_prefix, ref_fact_name);
                            facts.insert(qualified_name, lit.clone());
                        }
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
            let name = get_fact_name(fact);

            // Check if this fact exists in the document and validate type
            if let Some(expected_type) = doc.get_fact_type(&name) {
                let actual_type = lit.to_type();
                if expected_type != actual_type {
                    return Err(LemmaError::Engine(format!(
                        "Type mismatch for fact '{}': expected {}, got {}",
                        name, expected_type, actual_type
                    )));
                }
            }

            facts.insert(name, lit.clone());
        }
    }

    Ok(facts)
}

/// Get the display name for a fact (handles local and foreign facts)
fn get_fact_name(fact: &LemmaFact) -> String {
    match &fact.fact_type {
        FactType::Local(name) => name.clone(),
        FactType::Foreign(foreign_ref) => foreign_ref.reference.join("."),
    }
}
