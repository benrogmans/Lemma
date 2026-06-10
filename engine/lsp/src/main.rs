#[cfg(not(target_arch = "wasm32"))]
fn main() {
    lemma_lsp::stdio::run_stdio().expect("failed to run Lemma LSP server");
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // WASM entry is via the lib (browser::serve); this binary is not used.
}
