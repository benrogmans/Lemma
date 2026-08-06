use std::sync::Arc;
use tower_lsp::{LspService, Server};

use crate::workspace_files::WorkspaceFiles;

/// Run the Lemma language server over stdio until the client disconnects.
///
/// `workspace_files` is host-injected disk access from the CLI. Pass `None` only
/// for buffer-only mode (no workspace root load/watch).
pub fn run_stdio(workspace_files: Option<Arc<dyn WorkspaceFiles>>) -> std::io::Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let registry = Box::new(lemma::LemmaBase::new());
        let (service, socket) = LspService::new(move |client| {
            crate::server::LemmaLanguageServer::new(client, registry, workspace_files)
        });
        Server::new(stdin, stdout, socket).serve(service).await;
    });
    Ok(())
}
