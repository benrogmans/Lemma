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

    fn parse_spec_path(path: &str) -> String {
        path.trim_matches('/').to_string()
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
    struct EffectiveQuery {
        effective: Option<String>,
    }

    #[derive(Deserialize, Default)]
    struct SpecQuery {
        rules: Option<String>,
    }

    fn resolve_effective(
        raw: Option<&str>,
    ) -> Result<DateTimeValue, (StatusCode, Json<ErrorResponse>)> {
        match raw {
            Some(s) => s.parse::<DateTimeValue>().ok().ok_or_else(|| {
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
        explanations_enabled: bool,
    }

    #[derive(Debug, serde::Serialize)]
    struct ErrorResponse {
        error: String,
    }

    #[derive(serde::Serialize)]
    struct GetSpecResponse {
        spec_set_id: String,
        effective_from: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        rules: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        meta: Option<serde_json::Value>,
        versions: Vec<VersionEntry>,
    }

    /// Temporal version entry for a single spec row.
    ///
    /// Ranges are half-open: `[effective_from, effective_to)`. `effective_from = None`
    /// means the range is unbounded at the start (no earlier version exists).
    /// `effective_to = None` means the range is unbounded at the end (no later
    /// version exists; this row is still the latest).
    #[derive(serde::Serialize)]
    struct VersionEntry {
        effective_from: Option<String>,
        effective_to: Option<String>,
    }

    /// Build Memento-Datetime, Vary for the resolved spec.
    fn spec_response_headers(
        effective_from: Option<&DateTimeValue>,
    ) -> Vec<(axum::http::header::HeaderName, HeaderValue)> {
        let mut h = Vec::new();
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
    ///         The server auto-generates typed REST endpoints for each loaded spec:
    /// - `GET /{spec}/{rules?}` — evaluate rules (all if rules omitted), data as query params
    /// - `POST /{spec}/{rules?}` — evaluate rules (all if rules omitted), data as JSON body
    ///
    /// Meta routes:
    /// - `GET /` — list all specs
    /// - `GET /health` — health check
    /// - `GET /openapi.json` — OpenAPI 3.1 specification
    /// - `GET /docs` — Scalar interactive documentation
    pub async fn start_server(
        engine: Engine,
        host: &str,
        port: u16,
        watch: bool,
        explanations: bool,
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
            explanations_enabled: explanations,
        };

        let app = Router::new()
            .route("/", get(list_specs))
            .route("/health", get(health_check))
            .route("/openapi.json", get(openapi_spec))
            .route("/docs", get(scalar_docs))
            .route("/scalar.js", get(scalar_js))
            .route("/schema/{spec_name}", get(schema_all_rules))
            .route("/schema/{spec_name}/{rules}", get(schema_for_rules))
            .route("/{*path}", get(spec_get_schema).post(spec_post_evaluate))
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

    async fn list_specs(
        State(state): State<AppState>,
        Query(q): Query<EffectiveQuery>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let now = resolve_effective(q.effective.as_deref())?;
        let engine = state.engine.read().await;

        let specs: Vec<lemma::SpecSchema> = engine
            .list_specs_effective(&now)
            .iter()
            .filter_map(|s| engine.schema(&s.name, Some(&now)).ok())
            .collect();

        Ok(Json(specs))
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
                error: "Not found. Use GET / for spec list, GET /docs for API docs.".to_string(),
            }),
        )
    }

    async fn openapi_spec(
        State(state): State<AppState>,
        Query(q): Query<EffectiveQuery>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let effective = resolve_effective(q.effective.as_deref())?;
        let engine = state.engine.read().await;
        let spec = lemma_openapi::generate_openapi_effective(
            &engine,
            state.explanations_enabled,
            &effective,
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
      documentDownloadType: 'both', // Scalar UI option, not Lemma
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

    /// `GET /{*path}` — schema of resolved version; path = SpecSet id. `Accept-Datetime` for temporal, `?rules=` to scope.
    async fn spec_get_schema(
        State(state): State<AppState>,
        Path(path): Path<String>,
        Query(q): Query<SpecQuery>,
        headers: HeaderMap,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let spec_set_id = parse_spec_path(&path);
        let effective = accept_datetime_from_headers(&headers)?;
        let engine = state.engine.read().await;

        let schema = engine.schema(&spec_set_id, Some(&effective)).map_err(|e| {
            (
                lemma_error_to_status(&e),
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

        let rule_names = q.rules.as_deref().map(parse_rule_names).unwrap_or_default();
        let schema = if rule_names.is_empty() {
            schema
        } else {
            let plan = engine
                .get_plan(&spec_set_id, Some(&effective))
                .map_err(|e| {
                    (
                        lemma_error_to_status(&e),
                        Json(ErrorResponse {
                            error: e.to_string(),
                        }),
                    )
                })?;
            plan.schema_for_rules(&rule_names).map_err(|err| {
                (
                    lemma_error_to_status(&err),
                    Json(ErrorResponse {
                        error: err.to_string(),
                    }),
                )
            })?
        };

        let spec_arc = engine
            .get_spec(&spec_set_id, Some(&effective))
            .map_err(|e| {
                (
                    lemma_error_to_status(&e),
                    Json(ErrorResponse {
                        error: e.to_string(),
                    }),
                )
            })?;

        let spec_set = engine
            .get_spec_set(&spec_arc.name)
            .expect("BUG: spec resolved by get_spec but spec set missing from engine");
        let versions: Vec<VersionEntry> = spec_set
            .iter_with_ranges()
            .map(|(_, effective_from, effective_to)| VersionEntry {
                effective_from: effective_from.map(|d| d.to_string()),
                effective_to: effective_to.map(|d| d.to_string()),
            })
            .collect();

        let effective_from_str = spec_arc.effective_from().map(|d| d.to_string());

        let body = GetSpecResponse {
            spec_set_id,
            effective_from: effective_from_str,
            data: Some(
                serde_json::to_value(&schema.data).expect("BUG: failed to serialize schema data"),
            ),
            rules: Some(
                serde_json::to_value(&schema.rules).expect("BUG: failed to serialize schema rules"),
            ),
            meta: Some(
                serde_json::to_value(&schema.meta).expect("BUG: failed to serialize schema meta"),
            ),
            versions,
        };

        let mut response = Json(body).into_response();
        let headers_mut = response.headers_mut();
        for (k, v) in spec_response_headers(spec_arc.effective_from()) {
            headers_mut.insert(k, v);
        }
        Ok(response)
    }

    /// `POST /{*path}` — evaluate; path = SpecSet id. `Accept-Datetime` for temporal, `?rules=` to limit. Body = form-encoded data.
    async fn spec_post_evaluate(
        State(state): State<AppState>,
        Path(path): Path<String>,
        Query(q): Query<SpecQuery>,
        headers: HeaderMap,
        Form(data_values): Form<std::collections::HashMap<String, String>>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let spec_set_id = parse_spec_path(&path);
        let effective = accept_datetime_from_headers(&headers)?;
        let rule_names = q.rules.as_deref().map(parse_rule_names).unwrap_or_default();
        let engine = state.engine.read().await;

        let mut response = engine
            .run(&spec_set_id, Some(&effective), data_values, false)
            .map_err(|err| {
                (
                    lemma_error_to_status(&err),
                    Json(ErrorResponse {
                        error: err.to_string(),
                    }),
                )
            })?;
        if !rule_names.is_empty() {
            response.filter_rules(&rule_names);
        }

        let spec_arc = engine.get_spec(&spec_set_id, Some(&effective)).ok();
        let effective_from = spec_arc.as_ref().and_then(|a| a.effective_from());
        let results = response::convert_response_envelope(
            &response,
            want_explanations(&state, &headers),
            &response.spec_name,
            &effective,
        );
        let mut axum_response = Json(results).into_response();
        let headers_mut = axum_response.headers_mut();
        for (k, v) in spec_response_headers(effective_from) {
            headers_mut.insert(k, v);
        }
        Ok(axum_response)
    }

    // -----------------------------------------------------------------------
    // Schema routes (legacy — kept for backward compatibility only)
    // -----------------------------------------------------------------------
    //
    // These routes predate the spec envelope returned by `GET /{spec_name}`
    // (`spec_get_schema`). New clients must use `GET /{spec_name}` instead; it
    // returns the same schema plus `spec_set_id`, `effective_from`, `versions`,
    // and `meta`. The `/schema/*` routes are intentionally omitted from the
    // generated OpenAPI document and are not part of the public contract.

    /// `GET /schema/{spec_name}` — full spec schema (all data and rules).
    async fn schema_all_rules(
        State(state): State<AppState>,
        Path(spec_name): Path<String>,
        Query(q): Query<EffectiveQuery>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let now = resolve_effective(q.effective.as_deref())?;
        schema_inner(&state.engine, &spec_name, &[], &now).await
    }

    /// `GET /schema/{spec_name}/{rules}` — schema scoped to specific rules and
    /// only the data those rules need.
    async fn schema_for_rules(
        State(state): State<AppState>,
        Path((spec_name, rules)): Path<(String, String)>,
        Query(q): Query<EffectiveQuery>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let now = resolve_effective(q.effective.as_deref())?;
        let rule_names = parse_rule_names(&rules);
        schema_inner(&state.engine, &spec_name, &rule_names, &now).await
    }

    async fn schema_inner(
        engine: &SharedEngine,
        spec_name: &str,
        rule_names: &[String],
        now: &DateTimeValue,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let engine = engine.read().await;

        let plan = engine.get_plan(spec_name, Some(now)).map_err(|e| {
            (
                lemma_error_to_status(&e),
                Json(ErrorResponse {
                    error: e.to_string(),
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

    fn want_explanations(state: &AppState, headers: &HeaderMap) -> bool {
        state.explanations_enabled
            && headers
                .get("x-explanations")
                .and_then(|v: &axum::http::HeaderValue| v.to_str().ok())
                .map(|s: &str| !s.trim().is_empty())
                .unwrap_or(false)
    }

    /// Map a `Error` to an HTTP status code.
    ///
    /// SpecNotFound → 404; InvalidRequest → 400.
    fn lemma_error_to_status(err: &lemma::Error) -> StatusCode {
        use lemma::RequestErrorKind;
        match err {
            lemma::Error::Request {
                kind: RequestErrorKind::SpecNotFound,
                ..
            } => StatusCode::NOT_FOUND,
            _ => StatusCode::BAD_REQUEST,
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
                                        let spec_count = new_engine.list_specs().len();
                                        let mut engine = engine_clone.write().await;
                                        *engine = new_engine;
                                        info!("Reloaded engine with {} spec(s)", spec_count);
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

    /// Create a fresh engine by loading all .lemma files from the workspace
    /// directory (including `.deps/` for cached registry dependencies).
    async fn reload_engine(workdir: &std::path::Path) -> anyhow::Result<Engine> {
        use walkdir::WalkDir;

        let mut engine = Engine::new();
        let mut paths: Vec<std::path::PathBuf> = Vec::new();
        for entry in WalkDir::new(workdir) {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("lemma") {
                paths.push(entry.path().to_path_buf());
            }
        }
        if let Err(load_err) = engine.load_from_paths(&paths, false) {
            for e in load_err.iter() {
                tracing::error!(
                    "{}",
                    crate::error_formatter::format_error(e, &load_err.sources)
                );
            }
            anyhow::bail!("Workspace load failed ({} error(s))", load_err.errors.len());
        }
        Ok(engine)
    }
}
