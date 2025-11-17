use crate::ast::Span;

/// Unified source location information
///
/// Combines source file identifier, span, and document name
/// for consistent source location tracking across the codebase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct SourceLocation {
    /// Source file identifier (e.g., filename or "<input>")
    pub source_id: String,

    /// Span in source code (uses Lemma's existing `Span` type from `crate::ast::Span`)
    pub span: Span,

    /// Document name (the Lemma document containing this code)
    pub doc_name: String,
}

impl SourceLocation {
    /// Create a new SourceLocation
    #[must_use]
    pub fn new(source_id: impl Into<String>, span: Span, doc_name: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            span,
            doc_name: doc_name.into(),
        }
    }
}
