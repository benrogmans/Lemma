#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::Notify;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::diagnostics;
use crate::registry::Registry;
use crate::semantic_tokens;
use crate::spec_links;
use crate::workspace::WorkspaceModel;

/// Shared mutable state accessed by both the LSP handlers and the debounce background task.
struct SharedState {
    workspace: RwLock<WorkspaceModel>,
    #[cfg(not(target_arch = "wasm32"))]
    debounce_notify: Notify,
    /// The workspace root URI, set during `initialize`.
    root_uri: RwLock<Option<Url>>,
}

/// The Lemma Language Server.
///
/// Implements the LSP protocol for Lemma files:
/// - Diagnostics (parse errors + planning errors) published on file open/change
/// - Registry links for clickable `@external` spec references (url_for_id only, no fetching)
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
                #[cfg(not(target_arch = "wasm32"))]
                debounce_notify: Notify::new(),
                root_uri: RwLock::new(None),
            }),
            registry: Arc::from(registry),
        }
    }

    /// Signal the debounce task that a workspace re-validation is needed.
    #[cfg(not(target_arch = "wasm32"))]
    fn request_workspace_validation(&self) {
        self.state.debounce_notify.notify_one();
    }

    /// Run full workspace validation inline and publish all diagnostics.
    ///
    /// On WASM there is no background debounce task (it requires `Send` futures),
    /// so we validate synchronously inside `did_open`/`did_change` instead.
    /// This is fine for the playground: a single file, no registry, fast validation.
    #[cfg(target_arch = "wasm32")]
    async fn publish_full_diagnostics(&self) {
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

    /// Discover all `.lemma` files under a directory and add them to the workspace.
    /// No-op on WASM (no filesystem); the single spec is provided via didOpen.
    #[cfg(not(target_arch = "wasm32"))]
    async fn discover_workspace_files(&self, root_path: &Path) {
        let lemma_files = find_lemma_files(root_path);
        let mut workspace = self.state.workspace.write().await;

        for file_path in lemma_files {
            if let Ok(content) = std::fs::read_to_string(&file_path) {
                if let Ok(url) = Url::from_file_path(&file_path) {
                    workspace.update_file(url, content);
                }
            }
        }
    }

    /// Spawn the background debounce task.
    ///
    /// This task waits for change notifications, then waits for a 250ms quiet period
    /// (no further changes) before running a full workspace validation. The engine
    /// does not resolve `@` references — deps must be pre-fetched (via `lemma fetch`)
    /// and present on disk. Unresolved `@` refs surface as planning errors.
    ///
    /// Not available on WASM — `tokio::spawn` requires `Send` futures, but on WASM
    /// the registry trait uses `?Send` futures.
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_debounce_task(&self) {
        let state = Arc::clone(&self.state);
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                state.debounce_notify.notified().await;

                loop {
                    let timeout_result = tokio::time::timeout(
                        Duration::from_millis(250),
                        state.debounce_notify.notified(),
                    )
                    .await;
                    match timeout_result {
                        Ok(()) => continue,
                        Err(_) => break,
                    }
                }

                let (files, attr_map) = {
                    let workspace = state.workspace.read().await;
                    (
                        workspace.sources_map(),
                        workspace.attribute_to_url_and_text(),
                    )
                };

                let mut engine = lemma::Engine::new();
                let mut errors = Vec::new();
                for (attr, code) in &files {
                    if let Err(load_err) = engine.load(code, lemma::SourceType::Labeled(attr)) {
                        errors.extend(load_err.errors);
                    }
                }

                for (attr, (url, text)) in &attr_map {
                    let lsp_diagnostics = diagnostics::errors_to_diagnostics(&errors, text, attr);
                    client
                        .publish_diagnostics(url.clone(), lsp_diagnostics, None)
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
                document_formatting_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: semantic_tokens::TOKEN_TYPES.to_vec(),
                                token_modifiers: vec![],
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..SemanticTokensOptions::default()
                        },
                    ),
                ),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        // Spawn the debounce background task (native only; requires Send futures).
        #[cfg(not(target_arch = "wasm32"))]
        self.spawn_debounce_task();

        // Discover workspace `.lemma` files from the workspace root, if available (native only).
        #[cfg(not(target_arch = "wasm32"))]
        {
            let root_uri = {
                let root = self.state.root_uri.read().await;
                root.clone()
            };
            if let Some(root_uri) = root_uri {
                if let Ok(root_path) = root_uri.to_file_path() {
                    self.discover_workspace_files(&root_path).await;
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

        #[cfg(not(target_arch = "wasm32"))]
        self.request_workspace_validation();
        #[cfg(target_arch = "wasm32")]
        self.publish_full_diagnostics().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        // With FULL sync, the last content change contains the entire spec.
        if let Some(change) = params.content_changes.into_iter().last() {
            {
                let mut workspace = self.state.workspace.write().await;
                workspace.update_file(uri.clone(), change.text);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.request_workspace_validation();
        #[cfg(target_arch = "wasm32")]
        self.publish_full_diagnostics().await;
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
        #[cfg(not(target_arch = "wasm32"))]
        self.request_workspace_validation();
        #[cfg(target_arch = "wasm32")]
        self.publish_full_diagnostics().await;
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;

        let (text, attribute) = {
            let workspace = self.state.workspace.read().await;
            match workspace.get_file_text_and_attribute(&uri) {
                Some((text, attribute)) => (text.to_string(), attribute.to_string()),
                None => return Ok(None),
            }
        };

        // Only format if the file parses successfully — don't mangle broken code.
        match lemma::format_source(&text, &attribute) {
            Ok(formatted) if formatted == text => Ok(None), // No changes needed
            Ok(formatted) => {
                let line_count = text.lines().count() as u32;
                // Replace the entire spec with the formatted text.
                let edit = TextEdit {
                    range: Range {
                        start: Position::new(0, 0),
                        end: Position::new(line_count, 0),
                    },
                    new_text: formatted,
                };
                Ok(Some(vec![edit]))
            }
            Err(_) => Ok(None), // Parse error — don't format
        }
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let uri = params.text_document.uri;

        let text = {
            let workspace = self.state.workspace.read().await;
            workspace.get_file_text(&uri).map(|text| text.to_string())
        };

        match text {
            Some(text) => {
                let links = spec_links::find_registry_links(&text, self.registry.as_ref());
                if links.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(links))
                }
            }
            None => Ok(None),
        }
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;

        let text = {
            let workspace = self.state.workspace.read().await;
            workspace.get_file_text(&uri).map(|t| t.to_string())
        };

        match text {
            Some(text) => {
                let tokens = semantic_tokens::tokenize(&text);
                Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                    result_id: None,
                    data: tokens,
                })))
            }
            None => Ok(None),
        }
    }
}

/// Recursively find all `.lemma` files under a directory. Not used on WASM.
#[cfg(not(target_arch = "wasm32"))]
fn find_lemma_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut results = Vec::new();
    find_lemma_files_recursive(root, &mut results);
    results
}

#[cfg(not(target_arch = "wasm32"))]
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
            let dir_name = path.file_name().and_then(|name| name.to_str());
            match dir_name {
                Some(name) if name.starts_with('.') => continue,
                _ => find_lemma_files_recursive(&path, results),
            }
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("lemma") {
            results.push(path);
        }
    }
}
