pub mod http {
    use axum::{
        extract::{Path, Query, State},
        http::StatusCode,
        response::{Html, IntoResponse, Json},
        routing::get,
        Router,
    };
    use lemma::Engine;
    use serde::Serialize;

    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower_http::cors::CorsLayer;
    use tracing::{error, info, warn};

    type SharedEngine = Arc<RwLock<Engine>>;

    #[derive(Debug, Serialize)]
    struct RuleResultJson {
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        veto_reason: Option<String>,
    }

    #[derive(Debug, Serialize)]
    struct ErrorResponse {
        error: String,
    }

    /// Start the Lemma HTTP server.
    ///
    /// The server auto-generates typed REST endpoints for each loaded document:
    /// - `GET /{doc}/{rules?}` — evaluate rules (all if rules omitted), facts as query params
    /// - `POST /{doc}/{rules?}` — evaluate rules (all if rules omitted), facts as JSON body
    ///
    /// Meta routes:
    /// - `GET /` — list all documents
    /// - `GET /health` — health check
    /// - `GET /openapi.json` — OpenAPI 3.1 specification
    /// - `GET /docs` — Scalar interactive documentation
    pub async fn start_server(
        engine: Engine,
        host: &str,
        port: u16,
        watch: bool,
        workdir: PathBuf,
    ) -> anyhow::Result<()> {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "lemma=info,tower_http=info".into()),
            )
            .init();

        let shared_engine: SharedEngine = Arc::new(RwLock::new(engine));

        if watch {
            start_file_watcher(shared_engine.clone(), workdir)?;
        }

        let app = Router::new()
            .route("/", get(list_documents))
            .route("/health", get(health_check))
            .route("/openapi.json", get(openapi_spec))
            .route("/docs", get(scalar_docs))
            .route("/scalar.js", get(scalar_js))
            .route("/{doc_name}", get(evaluate_get).post(evaluate_post))
            .route(
                "/{doc_name}/{rules}",
                get(evaluate_get_with_rules).post(evaluate_post_with_rules),
            )
            .fallback(fallback_404)
            .layer(CorsLayer::permissive())
            .with_state(shared_engine);

        let addr: SocketAddr = format!("{host}:{port}").parse()?;
        info!("Lemma server listening on http://{}", addr);
        info!("Interactive docs at http://{}/docs", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Meta routes
    // -----------------------------------------------------------------------

    async fn list_documents(State(engine): State<SharedEngine>) -> impl IntoResponse {
        let engine = engine.read().await;
        let mut document_names = engine.list_documents();
        document_names.sort();

        let documents: Vec<lemma::DocumentSchema> = document_names
            .iter()
            .filter_map(|name| engine.get_execution_plan(name))
            .map(|plan| plan.document_schema())
            .collect();

        Json(documents)
    }

    async fn health_check() -> impl IntoResponse {
        Json(serde_json::json!({
            "status": "ok",
            "service": "lemma",
            "version": env!("CARGO_PKG_VERSION")
        }))
    }

    /// Fallback when no route matches — return 404 with JSON body (never empty).
    async fn fallback_404() -> (StatusCode, Json<ErrorResponse>) {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Not found. Use GET / for document list, GET /docs for API docs."
                    .to_string(),
            }),
        )
    }

    async fn openapi_spec(State(engine): State<SharedEngine>) -> impl IntoResponse {
        let engine = engine.read().await;
        let spec = lemma_openapi::generate_openapi(&engine);
        Json(spec)
    }

    async fn scalar_docs() -> impl IntoResponse {
        Html(
            r#"<!doctype html>
<html>
<head>
  <title>Lemma API</title>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
</head>
<body>
  <div id="app"></div>
  <script src="/scalar.js"></script>
  <script>
    Scalar.createApiReference('#app', {
      url: '/openapi.json',
      layout: 'modern',
      theme: 'solarized',
      agent: { disabled: true },
      hideClientButton: true,
      hideTestRequestButton: false,
      showSidebar: true,
      showDeveloperTools: 'localhost',
      operationTitleSource: 'summary',
      persistAuth: false,
      telemetry: false,
      hideModels: true,
      documentDownloadType: 'both',
      hideSearch: false,
      showOperationId: false,
      hideDarkModeToggle: false,
      withDefaultFonts: false,
      defaultOpenAllTags: false,
      expandAllModelSections: true,
      expandAllResponses: true,
      orderSchemaPropertiesBy: 'alpha',
      orderRequiredPropertiesFirst: true,
      customCss: `
        a[href="https://www.scalar.com"] {
          font-size: 0 !important;
        }
        a[href="https://www.scalar.com"]::after {
          content: 'Powered by Lemma';
          font-size: var(--scalar-mini, 10px);
        }
      `,
    })
  </script>
</body>
</html>"#
                .to_string(),
        )
    }

    /// Serve the vendored Scalar API reference JavaScript bundle.
    /// Embedded at compile time so the server has zero external dependencies.
    async fn scalar_js() -> impl IntoResponse {
        static SCALAR_JS: &str = include_str!("../vendor/scalar-api-reference.js");

        (
            [(
                axum::http::header::CONTENT_TYPE,
                "application/javascript; charset=utf-8",
            )],
            SCALAR_JS,
        )
    }

    // -----------------------------------------------------------------------
    // Document evaluation routes
    // -----------------------------------------------------------------------

    /// `GET /{doc_name}` — evaluate all rules, facts as query parameters
    async fn evaluate_get(
        State(engine): State<SharedEngine>,
        Path(doc_name): Path<String>,
        Query(params): Query<HashMap<String, String>>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        evaluate_with_query_params(&engine, &doc_name, &[], params).await
    }

    /// `GET /{doc_name}/{rules}` — evaluate selected rules, facts as query parameters.
    /// If the rules segment is empty after trimming, evaluates all rules.
    async fn evaluate_get_with_rules(
        State(engine): State<SharedEngine>,
        Path((doc_name, rules)): Path<(String, String)>,
        Query(params): Query<HashMap<String, String>>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let rule_names = parse_rule_names(&rules);
        if rule_names.is_empty() {
            return evaluate_with_query_params(&engine, &doc_name, &[], params).await;
        }
        validate_rule_names(&engine, &doc_name, &rule_names).await?;
        evaluate_with_query_params(&engine, &doc_name, &rule_names, params).await
    }

    /// `POST /{doc_name}` — evaluate all rules, facts as JSON body
    async fn evaluate_post(
        State(engine): State<SharedEngine>,
        Path(doc_name): Path<String>,
        Json(body): Json<serde_json::Value>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        evaluate_with_json_body(&engine, &doc_name, &[], &body).await
    }

    /// `POST /{doc_name}/{rules}` — evaluate selected rules, facts as JSON body.
    /// If the rules segment is empty after trimming, evaluates all rules.
    async fn evaluate_post_with_rules(
        State(engine): State<SharedEngine>,
        Path((doc_name, rules)): Path<(String, String)>,
        Json(body): Json<serde_json::Value>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let rule_names = parse_rule_names(&rules);
        if rule_names.is_empty() {
            return evaluate_with_json_body(&engine, &doc_name, &[], &body).await;
        }
        validate_rule_names(&engine, &doc_name, &rule_names).await?;
        evaluate_with_json_body(&engine, &doc_name, &rule_names, &body).await
    }

    // -----------------------------------------------------------------------
    // Shared evaluation logic
    // -----------------------------------------------------------------------

    async fn evaluate_with_query_params(
        engine: &SharedEngine,
        doc_name: &str,
        rule_names: &[String],
        params: HashMap<String, String>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let engine = engine.read().await;

        if engine.get_document(doc_name).is_none() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("Document '{}' not found", doc_name),
                }),
            ));
        }

        let response = engine
            .evaluate(doc_name, rule_names.to_vec(), params)
            .map_err(|err| {
                error!("Evaluation failed for '{}': {}", doc_name, err);
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("Evaluation failed: {}", err),
                    }),
                )
            })?;

        let results = convert_response(&response);
        info!("Evaluated '{}' with {} results", doc_name, results.len());

        Ok(Json(results))
    }

    async fn evaluate_with_json_body(
        engine: &SharedEngine,
        doc_name: &str,
        rule_names: &[String],
        body: &serde_json::Value,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let engine = engine.read().await;

        if engine.get_document(doc_name).is_none() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("Document '{}' not found", doc_name),
                }),
            ));
        }

        let fact_values = lemma_openapi::json_body_to_fact_values(body);

        let response = engine
            .evaluate(doc_name, rule_names.to_vec(), fact_values)
            .map_err(|err| {
                error!("Evaluation failed for '{}': {}", doc_name, err);
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("Evaluation failed: {}", err),
                    }),
                )
            })?;

        let results = convert_response(&response);
        info!(
            "Evaluated '{}' (POST) with {} results",
            doc_name,
            results.len()
        );

        Ok(Json(results))
    }

    // -----------------------------------------------------------------------
    // Response conversion
    // -----------------------------------------------------------------------

    fn convert_response(response: &lemma::Response) -> HashMap<String, RuleResultJson> {
        response
            .results
            .iter()
            .map(|(name, rule_result)| {
                let (value, veto_reason) = match &rule_result.result {
                    lemma::OperationResult::Value(v) => (Some(v.to_string()), None),
                    lemma::OperationResult::Veto(msg) => (None, msg.clone()),
                };
                (name.clone(), RuleResultJson { value, veto_reason })
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Verify that all requested rule names exist in the document. Returns 404 if any are unknown.
    async fn validate_rule_names(
        engine: &SharedEngine,
        doc_name: &str,
        rule_names: &[String],
    ) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
        let engine = engine.read().await;
        if let Some(plan) = engine.get_execution_plan(doc_name) {
            let known: std::collections::HashSet<&str> = plan
                .rules
                .iter()
                .filter(|r| r.path.segments.is_empty())
                .map(|r| r.name.as_str())
                .collect();
            let unknown: Vec<&str> = rule_names
                .iter()
                .filter(|name| !known.contains(name.as_str()))
                .map(|s| s.as_str())
                .collect();
            if !unknown.is_empty() {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("Unknown rule(s) in '{}': {}", doc_name, unknown.join(", ")),
                    }),
                ));
            }
        }
        Ok(())
    }

    /// Parse comma-separated rule names from a URL path segment.
    /// Filters out empty strings and the literal `{rules}` placeholder that
    /// Scalar sends when the path parameter is left blank.
    fn parse_rule_names(rules_segment: &str) -> Vec<String> {
        rules_segment
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "{rules}")
            .collect()
    }

    // -----------------------------------------------------------------------
    // File watcher (--watch mode)
    // -----------------------------------------------------------------------

    /// Snapshot of the last-modified timestamps for all `.lemma` files in the
    /// workspace. Used to detect whether files have actually changed between
    /// watcher callbacks, avoiding needless reloads from access-only events.
    type ModifiedSnapshot = std::collections::BTreeMap<PathBuf, std::time::SystemTime>;

    /// Walk the workspace and collect `(path, modified)` for every `.lemma` file.
    fn collect_modified_times(workdir: &std::path::Path) -> ModifiedSnapshot {
        use walkdir::WalkDir;

        let mut snapshot = std::collections::BTreeMap::new();
        for entry in WalkDir::new(workdir).into_iter().flatten() {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("lemma") {
                if let Ok(metadata) = entry.path().metadata() {
                    if let Ok(modified) = metadata.modified() {
                        snapshot.insert(entry.path().to_path_buf(), modified);
                    }
                }
            }
        }
        snapshot
    }

    fn start_file_watcher(shared_engine: SharedEngine, workdir: PathBuf) -> anyhow::Result<()> {
        use notify_debouncer_mini::new_debouncer;
        use std::sync::Mutex;
        use std::time::Duration;

        let watch_dir = workdir.clone();

        // Track the last-known modified timestamps so we only reload when
        // file contents have actually changed, not on access-only events.
        let last_snapshot: Arc<Mutex<ModifiedSnapshot>> =
            Arc::new(Mutex::new(collect_modified_times(&workdir)));

        // The debouncer thread runs in the background. We intentionally
        // "forget" the handle so the watcher stays alive for the lifetime
        // of the process. Dropping it would stop watching.
        let mut debouncer = new_debouncer(
            Duration::from_millis(500),
            move |result: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
                match result {
                    Ok(events) => {
                        let has_lemma_events = events.iter().any(|event| {
                            event
                                .path
                                .extension()
                                .and_then(|ext| ext.to_str())
                                .map(|ext| ext == "lemma")
                                .unwrap_or(false)
                        });

                        if !has_lemma_events {
                            return;
                        }

                        // Check if any file was actually modified by comparing
                        // the current timestamps to the last known snapshot.
                        let current_snapshot = collect_modified_times(&workdir);

                        let files_changed = {
                            let previous = match last_snapshot.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            current_snapshot != *previous
                        };

                        if !files_changed {
                            return;
                        }

                        // Store the new snapshot before starting the reload so
                        // that subsequent callbacks see the up-to-date times.
                        {
                            let mut previous = match last_snapshot.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            *previous = current_snapshot;
                        }

                        info!("Detected .lemma file changes, reloading...");
                        let engine_clone = shared_engine.clone();
                        let workdir_clone = workdir.clone();

                        // Spawn a dedicated OS thread for reloading. The notify
                        // callback is synchronous, so we create a fresh tokio
                        // runtime on a new thread to run the async reload.
                        std::thread::spawn(move || {
                            let runtime = match tokio::runtime::Runtime::new() {
                                Ok(rt) => rt,
                                Err(err) => {
                                    error!("Failed to create tokio runtime for reload: {}", err);
                                    return;
                                }
                            };

                            runtime.block_on(async {
                                match reload_engine(&workdir_clone).await {
                                    Ok(new_engine) => {
                                        let document_count = new_engine.list_documents().len();
                                        let mut engine = engine_clone.write().await;
                                        *engine = new_engine;
                                        info!(
                                            "Reloaded engine with {} document(s)",
                                            document_count
                                        );
                                    }
                                    Err(err) => {
                                        warn!("Reload failed (keeping previous state): {}", err);
                                    }
                                }
                            });
                        });
                    }
                    Err(err) => {
                        error!("File watcher error: {}", err);
                    }
                }
            },
        )?;

        debouncer
            .watcher()
            .watch(&watch_dir, notify::RecursiveMode::Recursive)?;

        info!("Watching {:?} for .lemma file changes", watch_dir);

        // Leak the debouncer so the watcher thread stays alive.
        // This is intentional: the watcher should run for the lifetime of the process.
        std::mem::forget(debouncer);

        Ok(())
    }

    /// Create a fresh engine by loading all .lemma files from the workspace directory.
    /// Uses `add_lemma_files` so registry resolution runs once and all errors are collected.
    async fn reload_engine(workdir: &std::path::Path) -> anyhow::Result<Engine> {
        use walkdir::WalkDir;

        let mut engine = Engine::new();
        let mut files = std::collections::HashMap::new();
        for entry in WalkDir::new(workdir) {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("lemma") {
                let path = entry.path();
                let source_id = path.to_string_lossy().to_string();
                let code = std::fs::read_to_string(path)?;
                files.insert(source_id, code);
            }
        }

        engine
            .add_lemma_files(files)
            .await
            .map_err(lemma::LemmaError::MultipleErrors)?;

        Ok(engine)
    }
}
