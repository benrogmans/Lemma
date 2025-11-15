//! Evaluation context for executing Lemma rules
//!
//! Contains all state needed during evaluation of a single document.

use crate::{
    FactReference, FactType, FactValue, LemmaDoc, LemmaError, LemmaFact, LiteralValue,
    OperationRecord, OperationResult, ResourceLimits,
};
use std::collections::HashMap;

use super::timeout::TimeoutTracker;

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

    /// All loaded documents (needed when facts reference other documents)
    pub all_documents: &'a HashMap<String, LemmaDoc>,

    /// Source text for all documents (for error reporting)
    /// Maps source_id -> source text
    pub sources: &'a HashMap<String, String>,

    /// Fact values (from document + overrides)
    /// Maps fact path -> concrete value
    /// Only contains facts that have actual values (not TypeAnnotations)
    pub facts: HashMap<FactReference, LiteralValue>,

    /// Timeout tracker (platform-specific)
    pub timeout_tracker: &'a TimeoutTracker,

    /// Resource limits including timeout
    pub limits: &'a ResourceLimits,

    /// Rule results computed so far (populated during execution)
    /// Maps RulePath -> operation result (either Value or Veto)
    pub rule_results: HashMap<crate::RulePath, OperationResult>,

    /// Operation records for each rule (for nested inclusion)
    /// Maps RulePath -> operations that computed that rule
    pub rule_operations: HashMap<crate::RulePath, Vec<OperationRecord>>,

    /// Operation records - records every operation for the current rule
    pub operations: Vec<OperationRecord>,

    /// Counter for generating unique operation IDs
    next_op_id: usize,

    /// Current parent operation ID (for nesting)
    pub current_parent_id: Option<crate::OperationId>,
}

impl<'a> EvaluationContext<'a> {
    /// Create a new evaluation context
    pub fn new(
        current_doc: &'a LemmaDoc,
        all_documents: &'a HashMap<String, LemmaDoc>,
        sources: &'a HashMap<String, String>,
        facts: HashMap<FactReference, LiteralValue>,
        timeout_tracker: &'a TimeoutTracker,
        limits: &'a ResourceLimits,
    ) -> Self {
        Self {
            current_doc,
            all_documents,
            sources,
            facts,
            rule_results: HashMap::new(),
            rule_operations: HashMap::new(),
            operations: Vec::new(),
            next_op_id: 0,
            current_parent_id: None,
            timeout_tracker,
            limits,
        }
    }

    /// Generate the next operation ID
    fn next_id(&mut self) -> crate::OperationId {
        let id = crate::OperationId(self.next_op_id);
        self.next_op_id += 1;
        id
    }

    /// Push an operation and return its ID
    pub fn push_operation(&mut self, kind: crate::OperationKind) -> crate::OperationId {
        let id = self.next_id();
        let depth = self.current_parent_id.map_or(0, |parent| {
            // Find parent's depth and add 1
            self.operations
                .iter()
                .find(|op| op.id == parent)
                .map_or(1, |op| op.depth + 1)
        });
        self.operations.push(OperationRecord {
            id,
            parent_id: self.current_parent_id,
            depth,
            kind,
        });
        id
    }

    /// Check if evaluation has exceeded timeout
    pub fn check_timeout(&self) -> Result<(), crate::LemmaError> {
        self.timeout_tracker.check_timeout(self.limits)
    }

    /// Extract expression text from source using span
    pub fn extract_expr_text(&self, expr: &crate::Expression, doc: &LemmaDoc) -> Option<String> {
        let span = expr.span.as_ref()?;
        let source_id = doc.source.as_ref()?;
        let source = self.sources.get(source_id)?;

        // Extract substring from source using span
        let bytes = source.as_bytes();
        if span.start < bytes.len() && span.end <= bytes.len() {
            Some(String::from_utf8_lossy(&bytes[span.start..span.end]).to_string())
        } else {
            None
        }
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
) -> Result<HashMap<FactReference, LiteralValue>, LemmaError> {
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
                        let mut qualified_reference = fact_prefix.reference.clone();
                        qualified_reference.extend_from_slice(&ref_fact_path.reference);
                        let qualified_path = FactReference {
                            reference: qualified_reference,
                        };
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
            if let Some(expected_type) = doc.get_fact_type(&path) {
                let actual_type = lit.to_type();
                if expected_type != actual_type {
                    return Err(LemmaError::Engine(format!(
                        "Type mismatch for fact '{}': expected {}, got {}",
                        path, expected_type, actual_type
                    )));
                }
            }

            facts.insert(path, lit.clone());
        }
    }

    Ok(facts)
}

/// Get the fact reference for a fact (handles local and foreign facts)
fn get_fact_path(fact: &LemmaFact) -> FactReference {
    match &fact.fact_type {
        FactType::Local(name) => FactReference {
            reference: vec![name.clone()],
        },
        FactType::Foreign(foreign_ref) => FactReference {
            reference: foreign_ref.reference.clone(),
        },
    }
}
