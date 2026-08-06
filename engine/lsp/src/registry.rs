//! Registry for the Language Server.
//!
//! Uses the engine's LemmaBase on both native and WASM: registry/spec links (`url_for_id`)
//! and resolution (`get`) work in the browser via fetch.

pub use lemma::Registry;
