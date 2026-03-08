//! Shared test helpers for engine integration tests.
//! Do not add production code here.

use lemma::Engine;
use std::collections::HashMap;

pub fn add_lemma_code_blocking(
    engine: &mut Engine,
    code: &str,
    source: &str,
) -> Result<(), Vec<lemma::Error>> {
    let files: HashMap<String, String> =
        std::iter::once((source.to_string(), code.to_string())).collect();
    engine.add_lemma_files(files)
}
