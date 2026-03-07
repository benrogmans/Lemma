//! Shared test helpers for engine integration tests.
//! Do not add production code here.

use lemma::Engine;
use std::collections::HashMap;

/// Run `engine.add_lemma_files` with a single file in a temporary tokio runtime.
/// Use this in sync test code instead of calling `add_lemma_files` (which is async).
/// Returns the raw error list; no collapsing to a single Error.
pub fn add_lemma_code_blocking(
    engine: &mut Engine,
    code: &str,
    source: &str,
) -> Result<(), Vec<lemma::Error>> {
    let files: HashMap<String, String> =
        std::iter::once((source.to_string(), code.to_string())).collect();
    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(engine.add_lemma_files(files))
}
