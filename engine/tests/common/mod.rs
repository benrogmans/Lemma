//! Shared test helpers for engine integration tests.
//! Do not add production code here.

use lemma::Engine;

pub fn add_lemma_code_blocking(
    engine: &mut Engine,
    code: &str,
    source: &str,
) -> Result<(), Vec<lemma::Error>> {
    engine.load(code, lemma::LoadSource::Labeled(source))
}
