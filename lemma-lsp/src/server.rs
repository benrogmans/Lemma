use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Notify, RwLock};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::diagnostics;
use crate::document_links;
use crate::registry::Registry;
use crate::workspace::WorkspaceModel;

/// Shared mutable state accessed by both the LSP handlers and the debounce background task.
struct SharedState {
    workspace: RwLock<WorkspaceModel>,
    debounce_notify: Notify,
    /// The workspace root URI, set during `initialize`.
    root_uri: RwLock<Option<Url>>,
}

/// The Lemma Language Server.
///
/// Implements the LSP protocol for Lemma files:
/// - Diagnostics (parse errors + planning errors) published on file open/change
/// - Document links for clickable `@external` Registry references
pub struct LemmaLanguageServer {
    client: Client,
    state: Arc<SharedState>,
    registry: Arc<dyn Registry>,
}

impl LemmaLanguageServer {
    pub fn new(client: Client, registry: Box<dyn Registry>) -> Self {
        Self {
            client,
            state: Arc::new(SharedState {
                workspace: RwLock::new(WorkspaceModel::new()),
                debounce_notify: Notify::new(),
                root_uri: RwLock::new(None),
            }),
            registry: Arc::from(registry),
        }
    }

    /// Publish parse-only diagnostics for a single file immediately (fast path).
    ///
    /// This is called right after a file is opened or changed, before the debounced
    /// full workspace validation runs. Parse errors are cheap to compute.
    async fn publish_parse_diagnostics(&self, uri: &Url) {
        let workspace = self.state.workspace.read().await;
        let parse_errors = workspace.get_parse_errors(uri);

        if parse_errors.is_empty() {
            // No parse errors — the debounced full validation will publish the real diagnostics.
            return;
        }

        let text = workspace.get_file_text(uri).unwrap_or("");

        let error_for_conversion = if parse_errors.len() == 1 {
            parse_errors.into_iter().next().expect(
                "BUG: parse_errors confirmed non-empty with length 1 but next() returned None",
            )
        } else {
            lemma::LemmaError::MultipleErrors(parse_errors)
        };

        let lsp_diagnostics = diagnostics::parse_error_to_diagnostics(&error_for_conversion, text);

        self.client
            .publish_diagnostics(uri.clone(), lsp_diagnostics, None)
            .await;
    }

    /// Signal the debounce task that a workspace re-validation is needed.
    fn request_workspace_validation(&self) {
        self.state.debounce_notify.notify_one();
    }

    /// Discover all `.lemma` files under a directory and add them to the workspace.
    async fn discover_workspace_files(&self, root_path: &Path) {
        let lemma_files = find_lemma_files(root_path);
        let mut workspace = self.state.workspace.write().await;

        for file_path in lemma_files {
            match std::fs::read_to_string(&file_path) {
                Ok(content) => {
                    if let Ok(url) = Url::from_file_path(&file_path) {
                        workspace.update_file(url, content);
                    }
                }
                Err(_) => {
                    // Skip files we cannot read (permissions, encoding, etc.).
                }
            }
        }
    }

    /// Run a full workspace validation and publish diagnostics for all tracked files.
    async fn run_full_validation_and_publish(&self) {
        let file_diagnostics = {
            let workspace = self.state.workspace.read().await;
            workspace.validate_workspace()
        };

        for file_diag in file_diagnostics {
            let lsp_diagnostics = diagnostics::errors_to_diagnostics(
                &file_diag.errors,
                &file_diag.text,
                &file_diag.attribute,
            );
            self.client
                .publish_diagnostics(file_diag.url, lsp_diagnostics, None)
                .await;
        }
    }

    /// Spawn the background debounce task.
    ///
    /// This task waits for change notifications, then waits for a 250ms quiet period
    /// (no further changes) before running a full workspace validation.
    fn spawn_debounce_task(&self) {
        let state = Arc::clone(&self.state);
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                // Wait for the first change notification.
                state.debounce_notify.notified().await;

                // Debounce loop: keep resetting the timer as long as changes arrive
                // within the debounce window.
                loop {
                    let timeout_result = tokio::time::timeout(
                        Duration::from_millis(250),
                        state.debounce_notify.notified(),
                    )
                    .await;
                    match timeout_result {
                        Ok(()) => {
                            // Another change arrived within the window — reset the timer.
                            continue;
                        }
                        Err(_) => {
                            // Timeout: no changes for 250ms — run validation.
                            break;
                        }
                    }
                }

                // Run full workspace validation.
                let file_diagnostics = {
                    let workspace = state.workspace.read().await;
                    workspace.validate_workspace()
                };

                for file_diag in file_diagnostics {
                    let lsp_diagnostics = diagnostics::errors_to_diagnostics(
                        &file_diag.errors,
                        &file_diag.text,
                        &file_diag.attribute,
                    );
                    client
                        .publish_diagnostics(file_diag.url, lsp_diagnostics, None)
                        .await;
                }
            }
        });
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for LemmaLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Store the workspace root for file discovery during initialized().
        if let Some(root_uri) = params.root_uri {
            let mut root = self.state.root_uri.write().await;
            *root = Some(root_uri);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: Some(false),
                    },
                }),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "lemma-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        // Spawn the debounce background task.
        self.spawn_debounce_task();

        // Discover workspace `.lemma` files from the workspace root, if available.
        let root_uri = {
            let root = self.state.root_uri.read().await;
            root.clone()
        };

        if let Some(root_uri) = root_uri {
            if let Ok(root_path) = root_uri.to_file_path() {
                self.discover_workspace_files(&root_path).await;

                // Run initial validation for discovered files.
                self.run_full_validation_and_publish().await;
            }
        }

        self.client
            .log_message(MessageType::INFO, "Lemma LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;

        {
            let mut workspace = self.state.workspace.write().await;
            workspace.update_file(uri.clone(), text);
        }

        // Fast path: publish parse errors immediately.
        self.publish_parse_diagnostics(&uri).await;

        // Signal the debounce task for full workspace validation.
        self.request_workspace_validation();
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        // With FULL sync, the last content change contains the entire document.
        if let Some(change) = params.content_changes.into_iter().last() {
            {
                let mut workspace = self.state.workspace.write().await;
                workspace.update_file(uri.clone(), change.text);
            }

            // Fast path: publish parse errors immediately.
            self.publish_parse_diagnostics(&uri).await;
        }

        // Signal the debounce task for full workspace validation.
        self.request_workspace_validation();
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;

        {
            let mut workspace = self.state.workspace.write().await;
            workspace.remove_file(&uri);
        }

        // Clear diagnostics for the closed file.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;

        // Signal re-validation since removing a file may affect other files' diagnostics.
        self.request_workspace_validation();
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let uri = params.text_document.uri;

        let text = {
            let workspace = self.state.workspace.read().await;
            workspace.get_file_text(&uri).map(|text| text.to_string())
        };

        match text {
            Some(text) => {
                let links = document_links::find_registry_links(&text, self.registry.as_ref());
                if links.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(links))
                }
            }
            None => Ok(None),
        }
    }
}

/// Recursively find all `.lemma` files under a directory.
fn find_lemma_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut results = Vec::new();
    find_lemma_files_recursive(root, &mut results);
    results
}

fn find_lemma_files_recursive(directory: &Path, results: &mut Vec<std::path::PathBuf>) {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let path = entry.path();

        if path.is_dir() {
            // Skip common non-source directories.
            let dir_name = path.file_name().and_then(|name| name.to_str());
            match dir_name {
                Some(name)
                    if name.starts_with('.')
                        || name == "node_modules"
                        || name == "target"
                        || name == "__pycache__" =>
                {
                    continue;
                }
                _ => {
                    find_lemma_files_recursive(&path, results);
                }
            }
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("lemma") {
            results.push(path);
        }
    }
}
