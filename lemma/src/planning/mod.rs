//! Planning module for Lemma documents
//!
//! This module performs complete static analysis and builds execution plans:
//! - Builds Graph with facts and rules (validated, with types computed)
//! - Builds ExecutionPlan from Graph (topologically sorted, ready for evaluation)
//! - Validates document structure and references

pub mod execution_plan;
pub mod graph;
pub mod types;
pub mod validation;

pub use execution_plan::{Branch, ExecutableRule, ExecutionPlan};
pub use types::TypeRegistry;

use crate::semantic::LemmaDoc;
use crate::LemmaError;
use std::collections::HashMap;

/// Builds an execution plan from Lemma documents.
///
/// The `sources` parameter maps source IDs (filenames) to their source code,
/// needed for extracting original expression text in proofs.
pub fn plan(
    main_doc: &LemmaDoc,
    all_docs: &[LemmaDoc],
    sources: HashMap<String, String>,
) -> Result<ExecutionPlan, Vec<LemmaError>> {
    validate_all_documents(all_docs)?;

    let graph = graph::Graph::build(main_doc, all_docs, sources)?;
    let execution_plan = execution_plan::build_execution_plan(&graph, &main_doc.name);
    Ok(execution_plan)
}

/// Validate all documents
fn validate_all_documents(all_docs: &[LemmaDoc]) -> Result<(), Vec<LemmaError>> {
    let mut errors = Vec::new();

    // Pass all_docs to validate_types so cross-document type imports can resolve
    for doc in all_docs {
        if let Err(doc_errors) = validation::validate_types(doc, Some(all_docs)) {
            errors.extend(doc_errors);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
