pub mod http {
    use crate::response;
    use axum::{
        extract::{Form, Path, Query, State},
        http::{HeaderMap, HeaderValue, StatusCode},
        response::{Html, IntoResponse, Json},
        routing::get,
        Router,
    };
    use lemma::parsing::ast::DateTimeValue;
    use lemma::Engine;
    use serde::Deserialize;

    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower_http::cors::CorsLayer;
    use tracing::{error, info, warn};

    type SharedEngine = Arc<RwLock<Engine>>;

    /// Parse doc path into (doc_name, optional hash_pin). Path like "pricing", "pricing~abc1234", "ind/kennismigrant/aanvraag~xyz".
    fn parse_doc_path(path: &str) -> (String, Option<String>) {
        let path = path.trim_matches('/');
        if path.is_empty() {
            return (String::new(), None);
        }
        let segments: Vec<&str> = path.split('/').collect();
        let last = segments.last().copied().unwrap_or("");
        if let Some(tilde_pos) = last.rfind('~') {
            let (base, hash) = last.split_at(tilde_pos);
            let hash = hash[1..].to_string();
            let doc_name = if segments.len() == 1 {
                base.to_string()
            } else {
                format!("{}/{}", segments[..segments.len() - 1].join("/"), base)
            };
            (doc_name, if hash.is_empty() { None } else { Some(hash) })
        } else {
            (path.to_string(), None)
        }
    }

    /// Read Accept-Datetime (RFC 7089) from headers; fallback to now.
    fn accept_datetime_from_headers(
        headers: &HeaderMap,
    ) -> Result<DateTimeValue, (StatusCode, Json<ErrorResponse>)> {
        let raw = headers
            .get("Accept-Datetime")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim());
        resolve_effective(raw)
    }

    #[derive(Deserialize, Default)]
    struct DocQuery {
        effective: Option<String>,
    }

    #[derive(Deserialize, Default)]
    struct RulesQuery {
        rules: Option<String>,
    }

    fn resolve_effective(
        raw: Option<&str>,
    ) -> Result<DateTimeValue, (StatusCode, Json<ErrorResponse>)> {
        match raw {
            Some(s) => DateTimeValue::parse(s).ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("Invalid effective value '{}'. Expected: YYYY, YYYY-MM, YYYY-MM-DD, or ISO 8601 datetime", s),
                    }),
                )
            }),
            None => Ok(DateTimeValue::now()),
        }
    }

    #[derive(Clone)]
    struct AppState {
        engine: SharedEngine,
        proofs_enabled: bool,
    }

    #[derive(Debug, serde::Serialize)]
    struct ErrorResponse {
        error: String,
    }

    #[derive(serde::Serialize)]
    struct GetDocResponse {
        doc: String,
        effective_from: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        facts: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        rules: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        meta: Option<serde_json::Value>,
        versions: Vec<VersionEntry>,
    }

    #[derive(serde::Serialize)]
    struct VersionEntry {
        effective_from: Option<String>,
        hash: String,
        permalink: String,
    }

    /// Build ETag, Memento-Datetime, Vary for the resolved doc.
    fn doc_response_headers(
        _doc_name: &str,
        effective_from: Option<&DateTimeValue>,
        hash: Option<&str>,
    ) -> Vec<(axum::http::header::HeaderName, HeaderValue)> {
        let mut h = Vec::new();
        if let Some(hash) = hash {
            if let Ok(v) = HeaderValue::from_str(&format!("\"{}\"", hash)) {
                h.push((axum::http::header::ETAG, v));
            }
        }
        if let Some(af) = effective_from {
            if let Ok(v) = HeaderValue::from_str(&af.to_string()) {
                h.push((
                    axum::http::header::HeaderName::from_static("memento-datetime"),
                    v,
                ));
            }
        }
        h.push((
            axum::http::header::VARY,
            HeaderValue::from_static("Accept-Datetime"),
        ));
        h
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
        proofs: bool,
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

        let state = AppState {
            engine: shared_engine,
            proofs_enabled: proofs,
        };

        let app = Router::new()
            .route("/", get(list_documents))
            .route("/health", get(health_check))
            .route("/openapi.json", get(openapi_spec))
            .route("/docs", get(scalar_docs))
            .route("/scalar.js", get(scalar_js))
            .route("/schema/{doc_name}", get(schema_all_rules))
            .route("/schema/{doc_name}/{rules}", get(schema_for_rules))
            .route("/{*path}", get(doc_get_schema).post(doc_post_evaluate))
            .fallback(fallback_404)
            .layer(CorsLayer::permissive())
            .with_state(state);

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

    async fn list_documents(
        State(state): State<AppState>,
        Query(q): Query<DocQuery>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let now = resolve_effective(q.effective.as_deref())?;
        let engine = state.engine.read().await;

        let documents: Vec<lemma::DocumentSchema> = engine
            .list_documents_effective(&now)
            .iter()
            .filter_map(|doc| engine.get_execution_plan(&doc.name, None, &now))
            .map(|plan| plan.schema())
            .collect();

        Ok(Json(documents))
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

    async fn openapi_spec(
        State(state): State<AppState>,
        Query(q): Query<DocQuery>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let effective = resolve_effective(q.effective.as_deref())?;
        let engine = state.engine.read().await;
        let use_permalink = q.effective.is_some();
        let spec = lemma_openapi::generate_openapi_effective(
            &engine,
            state.proofs_enabled,
            &effective,
            use_permalink,
        );
        Ok(Json(spec))
    }

    async fn scalar_docs(State(state): State<AppState>) -> impl IntoResponse {
        let engine = state.engine.read().await;
        let sources = lemma_openapi::temporal_api_sources(&engine);

        let shared_opts = r#"layout: 'modern',
      theme: 'solarized',
      agent: { disabled: true },
      hideClientButton: true,
      hideTestRequestButton: false,
      showSidebar: true,
      showDeveloperTools: 'never',
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
      `"#;

        let config_js = if sources.len() == 1 {
            format!("{{ url: '{}', {} }}", sources[0].url, shared_opts)
        } else {
            let sources_js: Vec<String> = sources
                .iter()
                .map(|s| {
                    format!(
                        "{{ title: '{}', slug: '{}', url: '{}' }}",
                        s.title, s.slug, s.url
                    )
                })
                .collect();
            format!(
                "{{ sources: [{}], {} }}",
                sources_js.join(", "),
                shared_opts
            )
        };

        let html = format!(
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
    Scalar.createApiReference('#app', {config_js})
  </script>
</body>
</html>"#
        );

        Html(html)
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
    // Doc path (wildcard): GET = schema with versions, POST = evaluate
    // -----------------------------------------------------------------------

    /// `GET /{*path}` — schema of resolved version; path = doc name with optional ~hash. Accept-Datetime for temporal. ?rules= to scope.
    async fn doc_get_schema(
        State(state): State<AppState>,
        Path(path): Path<String>,
        Query(q): Query<RulesQuery>,
        headers: HeaderMap,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let (doc_name, hash_pin) = parse_doc_path(&path);
        if doc_name.is_empty() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Document path required".to_string(),
                }),
            ));
        }
        let effective = accept_datetime_from_headers(&headers)?;
        let engine = state.engine.read().await;

        let doc_arc = hash_pin
            .as_deref()
            .and_then(|pin| engine.get_document_by_hash_pin(&doc_name, pin))
            .or_else(|| engine.get_document(&doc_name, &effective));
        let doc_arc = match doc_arc {
            Some(a) => a,
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("Document '{}' not found", doc_name),
                    }),
                ));
            }
        };

        let plan = match engine.get_execution_plan(&doc_name, hash_pin.as_deref(), &effective) {
            Some(p) => p,
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("Document '{}' not found", doc_name),
                    }),
                ));
            }
        };

        let rule_names = q.rules.as_deref().map(parse_rule_names).unwrap_or_default();
        let schema = if rule_names.is_empty() {
            plan.schema()
        } else {
            plan.schema_for_rules(&rule_names).map_err(|err| {
                (
                    lemma_error_to_status(&err),
                    Json(ErrorResponse {
                        error: err.to_string(),
                    }),
                )
            })?
        };

        let versions: Vec<VersionEntry> = engine
            .all_hash_pins()
            .into_iter()
            .filter(|(name, _, _)| *name == doc_name)
            .map(|(_, effective_from, hash)| VersionEntry {
                effective_from: effective_from.clone(),
                hash: hash.to_string(),
                permalink: format!("/{}~{}", doc_name, hash),
            })
            .collect();

        let effective_from_str = doc_arc.effective_from().map(|d| d.to_string());
        let hash = engine.hash_pin_for_doc(&doc_arc);

        let body = GetDocResponse {
            doc: schema.doc.clone(),
            effective_from: effective_from_str,
            facts: serde_json::to_value(&schema.facts).ok(),
            rules: serde_json::to_value(&schema.rules).ok(),
            meta: serde_json::to_value(&schema.meta).ok(),
            versions,
        };

        let mut response = Json(body).into_response();
        let headers_mut = response.headers_mut();
        for (k, v) in doc_response_headers(&doc_name, doc_arc.effective_from(), hash) {
            headers_mut.insert(k, v);
        }
        Ok(response)
    }

    /// `POST /{*path}` — evaluate; path = doc name with optional ~hash. Accept-Datetime for temporal. ?rules= to limit. Body = form-encoded facts.
    async fn doc_post_evaluate(
        State(state): State<AppState>,
        Path(path): Path<String>,
        Query(q): Query<RulesQuery>,
        headers: HeaderMap,
        Form(fact_values): Form<std::collections::HashMap<String, String>>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let (doc_name, hash_pin) = parse_doc_path(&path);
        if doc_name.is_empty() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Document path required".to_string(),
                }),
            ));
        }
        let effective = accept_datetime_from_headers(&headers)?;
        let rule_names = q.rules.as_deref().map(parse_rule_names).unwrap_or_default();
        let engine = state.engine.read().await;

        let doc_arc = hash_pin
            .as_deref()
            .and_then(|pin| engine.get_document_by_hash_pin(&doc_name, pin))
            .or_else(|| engine.get_document(&doc_name, &effective));
        let doc_arc = match doc_arc {
            Some(a) => a,
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("Document '{}' not found", doc_name),
                    }),
                ));
            }
        };

        let response = engine
            .evaluate(
                &doc_name,
                hash_pin.as_deref(),
                &effective,
                rule_names,
                fact_values,
            )
            .map_err(|err| {
                (
                    lemma_error_to_status(&err),
                    Json(ErrorResponse {
                        error: err.to_string(),
                    }),
                )
            })?;

        let hash = engine.hash_pin_for_doc(&doc_arc);
        let results = response::convert_response(&response, want_proofs(&state, &headers));
        let mut axum_response = Json(results).into_response();
        let headers_mut = axum_response.headers_mut();
        for (k, v) in doc_response_headers(&doc_name, doc_arc.effective_from(), hash) {
            headers_mut.insert(k, v);
        }
        Ok(axum_response)
    }

    // -----------------------------------------------------------------------
    // Schema routes (legacy)
    // -----------------------------------------------------------------------

    /// `GET /schema/{doc_name}` — full document schema (all facts and rules).
    async fn schema_all_rules(
        State(state): State<AppState>,
        Path(doc_name): Path<String>,
        Query(q): Query<DocQuery>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let now = resolve_effective(q.effective.as_deref())?;
        schema_inner(&state.engine, &doc_name, &[], &now).await
    }

    /// `GET /schema/{doc_name}/{rules}` — schema scoped to specific rules and
    /// only the facts those rules need.
    async fn schema_for_rules(
        State(state): State<AppState>,
        Path((doc_name, rules)): Path<(String, String)>,
        Query(q): Query<DocQuery>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let now = resolve_effective(q.effective.as_deref())?;
        let rule_names = parse_rule_names(&rules);
        schema_inner(&state.engine, &doc_name, &rule_names, &now).await
    }

    async fn schema_inner(
        engine: &SharedEngine,
        doc_name: &str,
        rule_names: &[String],
        now: &DateTimeValue,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let engine = engine.read().await;

        let plan = engine
            .get_execution_plan(doc_name, None, now)
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("Document '{}' not found", doc_name),
                    }),
                )
            })?;

        if rule_names.is_empty() {
            return Ok(Json(plan.schema()));
        }

        let schema = plan.schema_for_rules(rule_names).map_err(|err| {
            (
                lemma_error_to_status(&err),
                Json(ErrorResponse {
                    error: err.to_string(),
                }),
            )
        })?;

        Ok(Json(schema))
    }

    fn want_proofs(state: &AppState, headers: &HeaderMap) -> bool {
        state.proofs_enabled
            && headers
                .get("x-proofs")
                .and_then(|v: &axum::http::HeaderValue| v.to_str().ok())
                .map(|s: &str| !s.trim().is_empty())
                .unwrap_or(false)
    }

    /// Map a `Error` to an HTTP status code.
    ///
    /// Engine errors mentioning "not found" → 404; everything else → 400.
    fn lemma_error_to_status(err: &lemma::Error) -> StatusCode {
        let msg = err.to_string();
        if msg.contains("not found") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

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

        if let Err(errs) = engine.add_lemma_files(files).await {
            for e in &errs {
                tracing::error!("{}", crate::error_formatter::format_error(e));
            }
            anyhow::bail!("Workspace load failed ({} error(s))", errs.len());
        }
        Ok(engine)
    }
}
