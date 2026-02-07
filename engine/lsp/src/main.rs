mod diagnostics;
mod document_links;
mod registry;
mod server;
mod workspace;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    lsp_native::run();
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // WASM entry is via the lib (browser::serve); this binary is not used.
}

#[cfg(not(target_arch = "wasm32"))]
mod lsp_native {
    use tower_lsp::{LspService, Server};

    pub fn run() {
        tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(async {
                let stdin = tokio::io::stdin();
                let stdout = tokio::io::stdout();
                let registry = crate::registry::make_registry();
                let (service, socket) = LspService::new(|client| {
                    super::server::LemmaLanguageServer::new(client, registry)
                });
                Server::new(stdin, stdout, socket).serve(service).await;
            });
    }
}
