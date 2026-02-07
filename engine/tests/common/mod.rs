//! Shared test helpers for engine integration tests.
//! Do not add production code here.

use lemma::{Engine, LemmaResult};

/// Run `engine.add_lemma_code(code, source)` in a temporary tokio runtime.
/// Use this in sync test code instead of calling `add_lemma_code` (which is async).
pub fn add_lemma_code_blocking(engine: &mut Engine, code: &str, source: &str) -> LemmaResult<()> {
    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(engine.add_lemma_code(code, source))
}
