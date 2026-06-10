use tower_lsp::{LspService, Server};

/// Run the Lemma language server over stdio until the client disconnects.
pub fn run_stdio() -> std::io::Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let registry = crate::registry::make_registry();
        let (service, socket) =
            LspService::new(|client| crate::server::LemmaLanguageServer::new(client, registry));
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    Ok(())
}
