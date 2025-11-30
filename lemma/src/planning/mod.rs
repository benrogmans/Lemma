//! Planning module for Lemma documents
//!
//! This module performs complete static analysis and builds execution plans:
//! - Builds Graph with facts and rules (validated, with types computed)
//! - Builds ExecutionPlan from Graph (topologically sorted, ready for evaluation)

pub mod execution_plan;
pub mod graph;

pub use execution_plan::{Branch, ExecutableRule, ExecutionPlan};

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
    let graph = graph::Graph::build(main_doc, all_docs, sources)?;
    let execution_plan = execution_plan::build_execution_plan(&graph, &main_doc.name);
    Ok(execution_plan)
}
