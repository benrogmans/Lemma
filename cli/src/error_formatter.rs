use lemma::format_error as lemma_format_error;
use std::collections::HashMap;

/// Re-export: format Lemma errors with required sources.
#[must_use]
pub fn format_error(error: &lemma::Error, sources: &HashMap<String, String>) -> String {
    lemma_format_error(error, sources)
}
