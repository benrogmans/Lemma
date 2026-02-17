//! Lemma LSP: library for native (stdio) and WASM (browser streams) builds.

pub mod diagnostics;
pub mod document_links;
pub mod registry;
pub mod server;
pub mod workspace;

#[cfg(target_arch = "wasm32")]
pub mod browser;
