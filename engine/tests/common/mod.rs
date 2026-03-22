//! Shared test helpers for engine integration tests.
//! Do not add production code here.

use lemma::Engine;

pub fn add_lemma_code_blocking(
    engine: &mut Engine,
    code: &str,
    source: &str,
) -> Result<(), lemma::Errors> {
    engine.load(code, lemma::SourceType::Labeled(source))
}
