//! Evaluation context for executing Lemma rules
//!
//! Contains all state needed during evaluation of a single document.

use crate::{FactType, FactValue, LemmaDoc, LemmaFact, LiteralValue, OperationRecord, OperationResult};
use std::collections::HashMap;

/// Context for evaluating a Lemma document
///
/// Contains all state needed for a single evaluation:
/// - Facts (inputs)
/// - Rule results (computed values)
/// - Trace (execution log)
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
    ) -> Self {
        Self {
            current_doc,
            all_documents,
            sources,
            facts,
            rule_results: HashMap::new(),
            operations: Vec::new(),
        }
    }
}

/// Build a fact map from document facts and overrides
///
/// Includes facts with concrete values (FactValue::Literal) and expands
/// DocumentReference facts by importing all facts from the referenced document.
/// Facts with TypeAnnotation are missing and will cause evaluation errors.
pub fn build_fact_map(
    doc_facts: &[LemmaFact],
    overrides: &[LemmaFact],
    all_documents: &HashMap<String, LemmaDoc>,
) -> HashMap<String, LiteralValue> {
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

    // Apply overrides
    for fact in overrides {
        if let FactValue::Literal(lit) = &fact.value {
            let name = get_fact_name(fact);
            facts.insert(name, lit.clone());
        }
    }

    facts
}

/// Get the display name for a fact (handles local and foreign facts)
fn get_fact_name(fact: &LemmaFact) -> String {
    match &fact.fact_type {
        FactType::Local(name) => name.clone(),
        FactType::Foreign(foreign_ref) => foreign_ref.reference.join("."),
    }
}
