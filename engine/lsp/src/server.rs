#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
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
    /// No-op on WASM (no filesystem); the single document is provided via didOpen.
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
    /// (no further changes) before running a full workspace validation. When a registry
    /// is configured, runs registry resolution first so that Registry errors (e.g.
    /// "Failed to reach LemmaBase") are published as diagnostics.
    ///
    /// Not available on WASM — `tokio::spawn` requires `Send` futures, but on WASM
    /// the registry trait uses `?Send` futures.
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_debounce_task(&self) {
        let state = Arc::clone(&self.state);
        let client = self.client.clone();
        let registry = Arc::clone(&self.registry);

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

                let (local_docs, mut sources, limits, attr_map) = {
                    let workspace = state.workspace.read().await;
                    (
                        workspace.all_parsed_docs(),
                        workspace.sources_map(),
                        workspace.limits().clone(),
                        workspace.attribute_to_url_and_text(),
                    )
                };

                let file_diagnostics = match lemma::resolve_registry_references(
                    local_docs,
                    &mut sources,
                    registry.as_ref(),
                    &limits,
                )
                .await
                {
                    Err(registry_error) => attribute_errors_to_files(&registry_error, &attr_map),
                    Ok(resolved_docs) => {
                        let workspace = state.workspace.read().await;
                        workspace.validate_workspace_with_resolved_docs(&resolved_docs, &sources)
                    }
                };

                for file_diag in &file_diagnostics {
                    let lsp_diagnostics = diagnostics::errors_to_diagnostics(
                        &file_diag.errors,
                        &file_diag.text,
                        &file_diag.attribute,
                    );
                    client
                        .publish_diagnostics(file_diag.url.clone(), lsp_diagnostics, None)
                        .await;
                }
            }
        });
    }
}

/// Map a registry error (possibly MultipleErrors) to FileDiagnostics by error location attribute.
#[cfg(any(not(target_arch = "wasm32"), test))]
fn attribute_errors_to_files(
    error: &lemma::LemmaError,
    attr_map: &std::collections::HashMap<String, (Url, String)>,
) -> Vec<crate::workspace::FileDiagnostics> {
    use crate::workspace::FileDiagnostics;
    use lemma::LemmaError;

    let errors: Vec<LemmaError> = match error {
        LemmaError::MultipleErrors(inner) => inner
            .iter()
            .flat_map(attribute_errors_to_files_inner)
            .collect(),
        other => vec![other.clone()],
    };

    let mut by_url: std::collections::HashMap<Url, (String, String, Vec<LemmaError>)> =
        std::collections::HashMap::new();
    for err in errors {
        let attribute = err
            .location()
            .map(|s| s.attribute.clone())
            .unwrap_or_default();
        if let Some((url, text)) = attr_map.get(&attribute) {
            by_url
                .entry(url.clone())
                .or_insert_with(|| (attribute.clone(), text.clone(), Vec::new()))
                .2
                .push(err);
        }
    }
    by_url
        .into_iter()
        .map(|(url, (attribute, text, errors))| FileDiagnostics {
            url,
            text,
            attribute,
            errors,
        })
        .collect()
}

#[cfg(any(not(target_arch = "wasm32"), test))]
fn attribute_errors_to_files_inner(error: &lemma::LemmaError) -> Vec<lemma::LemmaError> {
    match error {
        lemma::LemmaError::MultipleErrors(inner) => inner
            .iter()
            .flat_map(attribute_errors_to_files_inner)
            .collect(),
        other => vec![other.clone()],
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
                // Replace the entire document with the formatted text.
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::attribute_errors_to_files;
    use crate::workspace::WorkspaceModel;
    use lemma::{LemmaError, Source, Span};
    use std::sync::Arc;
    use tower_lsp::lsp_types::Url;

    fn url_from_path(path: &str) -> Url {
        Url::from_file_path(path).expect("valid file path for test URL")
    }

    /// Regression test: Registry errors must be attributed to the correct file when the error's
    /// source_location.attribute matches the workspace attr_map key (same as attribute_for_url).
    /// If this test fails, registry squiggles will not appear in the editor.
    #[test]
    fn registry_error_with_matching_attribute_appears_in_file_diagnostics() {
        let path = "/tmp/registry_missing.lemma";
        let url = url_from_path(path);
        let content = "doc example\nfact ext = doc @nonexistent/foo";
        let mut workspace = WorkspaceModel::new();
        workspace.update_file(url.clone(), content.to_string());

        let attr_map = workspace.attribute_to_url_and_text();
        assert_eq!(attr_map.len(), 1);
        let (attribute, _) = attr_map.iter().next().unwrap();
        let attribute = attribute.clone();

        let source = Source::new(
            attribute.clone(),
            Span {
                start: 0,
                end: 10,
                line: 1,
                col: 1,
            },
            "example",
        );
        let registry_error = LemmaError::registry(
            "Document not found: @nonexistent/foo",
            source,
            Arc::from("doc nonexistent/foo"),
            "nonexistent/foo",
            lemma::RegistryErrorKind::NotFound,
            Some("Check that the identifier exists on the registry."),
        );

        let file_diagnostics = attribute_errors_to_files(&registry_error, &attr_map);
        assert_eq!(
            file_diagnostics.len(),
            1,
            "Registry error with matching attribute should produce one FileDiagnostics"
        );
        assert_eq!(file_diagnostics[0].url, url);
        assert_eq!(file_diagnostics[0].errors.len(), 1);
        assert!(matches!(
            &file_diagnostics[0].errors[0],
            LemmaError::Registry { .. }
        ));
    }

    /// When both a doc ref and a type import fail, both errors must appear in file diagnostics
    /// and convert to two LSP diagnostics (so both lines get squiggles).
    #[test]
    fn multiple_registry_errors_same_file_produce_multiple_diagnostics() {
        use crate::diagnostics;

        let path = "/tmp/registry_demo.lemma";
        let url = url_from_path(path);
        let content = r#"doc registry_demo
fact helper = doc @org/example/helper
type money from @lemma/std/finance"#;
        let mut workspace = WorkspaceModel::new();
        workspace.update_file(url.clone(), content.to_string());

        let attr_map = workspace.attribute_to_url_and_text();
        let (attribute, (_url, text)) = attr_map.iter().next().unwrap();
        let attribute = attribute.clone();
        let text = text.clone();

        let doc_ref_source = Source::new(
            attribute.clone(),
            Span {
                start: 15,
                end: 42,
                line: 2,
                col: 1,
            },
            "registry_demo",
        );
        let type_ref_source = Source::new(
            attribute.clone(),
            Span {
                start: 43,
                end: 72,
                line: 3,
                col: 1,
            },
            "registry_demo",
        );
        let doc_error = LemmaError::registry(
            "Document not found: @org/example/helper",
            doc_ref_source,
            Arc::from("doc org/example/helper"),
            "org/example/helper",
            lemma::RegistryErrorKind::NotFound,
            None::<String>,
        );
        let type_error = LemmaError::registry(
            "Document not found: @lemma/std/finance",
            type_ref_source,
            Arc::from("type money from @lemma/std/finance"),
            "lemma/std/finance",
            lemma::RegistryErrorKind::NotFound,
            None::<String>,
        );
        let multiple = LemmaError::MultipleErrors(vec![doc_error, type_error]);

        let file_diagnostics = attribute_errors_to_files(&multiple, &attr_map);
        assert_eq!(file_diagnostics.len(), 1);
        assert_eq!(
            file_diagnostics[0].errors.len(),
            2,
            "Both doc and type registry errors must be attributed to the file"
        );

        let lsp_diagnostics = diagnostics::errors_to_diagnostics(
            &file_diagnostics[0].errors,
            &text,
            &file_diagnostics[0].attribute,
        );
        assert_eq!(
            lsp_diagnostics.len(),
            2,
            "Both errors must become LSP diagnostics so both lines show squiggles"
        );
    }
}
