//! Lemma LSP: library for native (stdio) and WASM (browser streams) builds.

pub mod diagnostics;
pub mod registry;
pub mod semantic_tokens;
pub mod server;
pub mod spec_links;
pub mod workspace;

#[cfg(target_arch = "wasm32")]
pub mod browser;
