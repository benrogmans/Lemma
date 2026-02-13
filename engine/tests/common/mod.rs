//! Shared test helpers for engine integration tests.
//! Do not add production code here.

use lemma::{Engine, LemmaError, LemmaResult};
use std::collections::HashMap;

/// Run `engine.add_lemma_files` with a single file in a temporary tokio runtime.
/// Use this in sync test code instead of calling `add_lemma_files` (which is async).
pub fn add_lemma_code_blocking(engine: &mut Engine, code: &str, source: &str) -> LemmaResult<()> {
    let files: HashMap<String, String> =
        std::iter::once((source.to_string(), code.to_string())).collect();
    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(engine.add_lemma_files(files))
        .map_err(|errs| match errs.len() {
            0 => unreachable!("add_lemma_files returned Err with empty error list"),
            1 => errs.into_iter().next().unwrap(),
            _ => LemmaError::MultipleErrors(errs),
        })
}
