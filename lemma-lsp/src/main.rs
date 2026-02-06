mod diagnostics;
mod document_links;
mod registry;
mod server;
mod workspace;

use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let registry = registry::StubRegistry::new();
    let (service, socket) =
        LspService::new(|client| server::LemmaLanguageServer::new(client, Box::new(registry)));

    Server::new(stdin, stdout, socket).serve(service).await;
}
