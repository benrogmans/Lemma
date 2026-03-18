//! Registry for the Language Server.
//!
//! Uses the engine's LemmaBase on both native and WASM: registry/spec links (`url_for_id`)
//! and resolution (`get_specs`, `get_types`) work in the browser via fetch.

pub use lemma::registry::Registry;
pub use lemma::LemmaBase;

/// Construct the Registry used by the LSP (LemmaBase on both native and WASM).
pub fn make_registry() -> Box<dyn Registry> {
    Box::new(LemmaBase::new())
}
