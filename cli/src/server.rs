pub mod http {
    use crate::formatter::Formatter;
    use axum::{
        body::Bytes,
        extract::{Path, Query, State},
        http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, StatusCode},
        response::{Html, IntoResponse, Json},
        routing::get,
        Router,
    };
    use lemma::DateTimeValue;
    use lemma::Engine;
    use lemma_cli::deps::{dependency_identifier_from_dependency_path, lemma_deps_dir};
    use serde::Deserialize;
    use std::fs;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;
    use tower_http::cors::CorsLayer;
    use tracing::{error, info, warn};

    /// Requests read-lock only long enough to clone the inner `Arc<Engine>`
    /// (a pointer copy) and never hold the lock during evaluation. The file
    /// watcher builds a reloaded `Engine` off to the side and write-locks only
    /// for the instant it takes to swap the `Arc`.
    type SharedEngine = Arc<RwLock<Arc<Engine>>>;

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
        lemma::resolve_effective(raw).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.message().to_string(),
                }),
            )
        })
    }

    #[derive(Clone)]
    struct AppState {
        engine: SharedEngine,
        explanations_enabled: bool,
        eval_timeout: Duration,
    }

    /// Owned snapshot of the engine for this request; the read guard lives
    /// only for the duration of the `Arc` clone.
    async fn engine_snapshot(state: &AppState) -> Arc<Engine> {
        Arc::clone(&*state.engine.read().await)
    }

    #[derive(Debug, serde::Serialize)]
    struct ErrorResponse {
        error: String,
    }

    fn catch_engine_panic<F, T>(f: F) -> Result<T, (StatusCode, Json<ErrorResponse>)>
    where
        F: FnOnce() -> T + std::panic::UnwindSafe,
    {
        catch_unwind(f).map_err(|panic_payload| {
            let msg = panic_payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic_payload.downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("unknown internal error");
            error!("engine panic caught: {}", msg);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "internal engine error".to_string(),
                }),
            )
        })
    }

    #[derive(serde::Serialize)]
    struct GetSpecResponse {
        spec_set_id: String,
        #[serde(flatten)]
        show: lemma::Show,
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
    /// The server auto-generates typed REST endpoints for each loaded spec:
    /// - `GET /{spec}` — show interface (data, rules, versions); no evaluate, no data query params
    /// - `POST /{spec}` — evaluate (`?rules=` scopes which rules to run); data as JSON or form body
    ///
    /// Meta routes:
    /// - `GET /` — list all specs
    /// - `GET /health` — health check
    /// - `GET /openapi.json` — OpenAPI 3.1 specification
    /// - `GET /docs` — Scalar interactive documentation
    ///
    /// Deployment posture: the server has no built-in authentication or TLS.
    /// It binds to localhost by default; for any non-localhost deployment, put
    /// it behind a reverse proxy that terminates TLS and enforces access
    /// control. Cross-origin browser access is denied unless `cors` is set.
    #[allow(clippy::too_many_arguments)]
    pub async fn start_server(
        engine: Engine,
        host: &str,
        port: u16,
        watch: bool,
        explanations: bool,
        workdir: PathBuf,
        eval_timeout_secs: u64,
        cors: bool,
    ) -> anyhow::Result<()> {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "lemma=info,tower_http=info".into()),
            )
            .init();

        let shared_engine: SharedEngine = Arc::new(RwLock::new(Arc::new(engine)));

        if watch {
            start_file_watcher(shared_engine.clone(), workdir)?;
        }

        let state = AppState {
            engine: shared_engine,
            explanations_enabled: explanations,
            eval_timeout: Duration::from_secs(eval_timeout_secs),
        };

        let router = Router::new()
            .route("/", get(list))
            .route("/health", get(health_check))
            .route("/openapi.json", get(openapi_spec))
            .route("/docs", get(scalar_docs))
            .route("/scalar.js", get(scalar_js))
            .route("/{*path}", get(spec_get_show).post(spec_post_evaluate))
            .fallback(fallback_404);
        let router = if cors {
            info!("Permissive CORS enabled (--cors): cross-origin browser requests allowed");
            router.layer(CorsLayer::permissive())
        } else {
            router
        };
        let app = router.with_state(state);

        if !matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]") {
            warn!(
                "Binding to non-localhost address {host}: the Lemma HTTP server has no \
                 built-in authentication or TLS. Deploy behind a reverse proxy that \
                 terminates TLS and enforces access control."
            );
        }

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

    async fn list(
        State(state): State<AppState>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let engine = engine_snapshot(&state).await;
        Ok(Json(engine.list()))
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
        let engine = engine_snapshot(&state).await;
        let spec = lemma_openapi::generate_openapi_effective(
            &engine,
            state.explanations_enabled,
            &effective,
        );
        Ok(Json(spec))
    }

    async fn scalar_docs(State(state): State<AppState>) -> impl IntoResponse {
        let engine = engine_snapshot(&state).await;
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
        /*
         * Evaluate client Response → Body (not .response-body-virtual).
         * Scalar hardcodes overflow-y-hidden + max-h-fit on the Body panel, so
         * CodeMirror grows to full JSON height and the pane clips with no scroll.
         */
        .scalar-app .response-section-content-body.overflow-y-hidden,
        .scalar-app .response-section-content-body {
          overflow-y: auto !important;
          max-height: 70vh !important;
          min-height: 0 !important;
        }
        .scalar-app .response-section-content-body.diclosure-panel,
        .scalar-app .response-section-content-body .diclosure-panel {
          max-height: 70vh !important;
          overflow-y: auto !important;
          min-height: 0 !important;
        }
        .scalar-app .response-section-content-body .body-raw,
        .scalar-app .response-section-content-body .body-raw-scroller {
          max-height: 70vh !important;
          min-height: 0 !important;
          overflow-y: auto !important;
        }
        .scalar-app .response-section-content-body .cm-editor {
          height: auto !important;
          max-height: 70vh !important;
        }
        .scalar-app .response-section-content-body .cm-scroller {
          max-height: 70vh !important;
          overflow-y: auto !important;
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
    // Doc path (wildcard): GET = show, POST = evaluate
    // -----------------------------------------------------------------------

    fn data_values_for_run(
        data: std::collections::HashMap<String, lemma::RunDataValue>,
    ) -> Result<std::collections::HashMap<String, String>, (StatusCode, Json<ErrorResponse>)> {
        data.into_iter()
            .map(|(key, value)| {
                let string_value = match value {
                    lemma::RunDataValue::String(s) => s,
                    lemma::RunDataValue::Boolean(b) => b.to_string(),
                    lemma::RunDataValue::MeasureMap(map) | lemma::RunDataValue::RatioMap(map) => {
                        if map.len() == 1 {
                            let (unit, magnitude) = map.into_iter().next().expect("BUG: map len checked");
                            format!("{magnitude} {unit}")
                        } else {
                            return Err((
                                StatusCode::BAD_REQUEST,
                                Json(ErrorResponse {
                                    error: format!(
                                        "data field '{key}' uses a multi-key unit map; pass a convenience string instead"
                                    ),
                                }),
                            ));
                        }
                    }
                };
                Ok((key, string_value))
            })
            .collect()
    }

    async fn spec_get_show(
        State(state): State<AppState>,
        Path(path): Path<String>,
        headers: HeaderMap,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let spec_set_id = parse_spec_path(&path);
        let effective = accept_datetime_from_headers(&headers)?;
        let engine = engine_snapshot(&state).await;

        let spec_name = lemma::parse_spec_set_id(&spec_set_id).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

        catch_engine_panic(AssertUnwindSafe(
            || -> Result<_, (StatusCode, Json<ErrorResponse>)> {
                let show = engine
                    .show(None, &spec_name, Some(&effective))
                    .map_err(|e| {
                        (
                            lemma_error_to_status(&e),
                            Json(ErrorResponse {
                                error: e.to_string(),
                            }),
                        )
                    })?;

                let effective_from = show.effective_from.clone();
                let body = GetSpecResponse { spec_set_id, show };

                let mut response = Json(body).into_response();
                let headers_mut = response.headers_mut();
                for (k, v) in spec_response_headers(effective_from.as_ref()) {
                    headers_mut.insert(k, v);
                }
                Ok(response)
            },
        ))?
    }

    fn parse_post_evaluate_body(
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<
        std::collections::HashMap<String, lemma::RunDataValue>,
        (StatusCode, Json<ErrorResponse>),
    > {
        if body.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let content_type = headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase())
            .unwrap_or_default();

        let data_values = match content_type.as_str() {
            "application/json" => {
                let map: std::collections::HashMap<String, serde_json::Value> =
                    serde_json::from_slice(body).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            Json(ErrorResponse {
                                error: format!("invalid JSON body: {e}"),
                            }),
                        )
                    })?;
                map.into_iter()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| {
                        crate::data_json::json_value_to_run_data_value(v).map(|input| (k, input))
                    })
                    .collect::<Result<_, _>>()
                    .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))?
            }
            "application/x-www-form-urlencoded" => {
                crate::data_json::form_urlencoded_to_data_values(body)
                    .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))?
            }
            "" => {
                return Err((
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    Json(ErrorResponse {
                        error: "Expected request with Content-Type: application/json or application/x-www-form-urlencoded".to_string(),
                    }),
                ));
            }
            other => {
                return Err((
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    Json(ErrorResponse {
                        error: format!(
                            "Unsupported Content-Type '{other}'; expected application/json or application/x-www-form-urlencoded"
                        ),
                    }),
                ));
            }
        };

        Ok(data_values)
    }

    /// `POST /{*path}` — evaluate; path = specset id. `Accept-Datetime` for temporal, `?rules=` to limit. Body = JSON or form data.
    async fn spec_post_evaluate(
        State(state): State<AppState>,
        Path(path): Path<String>,
        Query(q): Query<SpecQuery>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let spec_set_id = parse_spec_path(&path);
        let effective = accept_datetime_from_headers(&headers)?;

        let spec_name = lemma::parse_spec_set_id(&spec_set_id).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })?;

        let data_values = parse_post_evaluate_body(&headers, &body)?;

        let data_strings = data_values_for_run(data_values)?;

        let parsed_rules: Option<Vec<String>> = match q.rules.as_deref() {
            None => None,
            Some(rules_query) => {
                let parsed = parse_rule_names(rules_query);
                if parsed.is_empty() {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "at least one rule required".to_string(),
                        }),
                    ));
                }
                Some(parsed)
            }
        };

        let include_explanations = want_explanations(&state, &headers);
        let engine = engine_snapshot(&state).await;

        // Evaluate on a blocking thread with no lock held. Post-planning
        // evaluation is loop-free/terminating by design; the wall-clock
        // timeout is a boundary safeguard, not an engine mechanism.
        let eval_task = tokio::task::spawn_blocking(move || {
            let response = engine
                .run(
                    None,
                    &spec_name,
                    Some(&effective),
                    data_strings,
                    parsed_rules.as_deref(),
                    include_explanations,
                )
                .map_err(|err| {
                    (
                        lemma_error_to_status(&err),
                        Json(ErrorResponse {
                            error: err.to_string(),
                        }),
                    )
                })?;

            let effective_from = response.spec_effective_from.clone();
            let payload = Formatter.response_json_value(&response, include_explanations);
            Ok((payload, effective_from))
        });

        let (payload, effective_from) =
            match tokio::time::timeout(state.eval_timeout, eval_task).await {
                Err(_elapsed) => {
                    error!(
                        "evaluation timed out after {}s",
                        state.eval_timeout.as_secs()
                    );
                    return Err((
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(ErrorResponse {
                            error: format!(
                                "evaluation timed out after {}s",
                                state.eval_timeout.as_secs()
                            ),
                        }),
                    ));
                }
                // The blocking task panicked (JoinError); same mapping as catch_engine_panic.
                Ok(Err(join_error)) => {
                    error!("engine panic caught: {}", join_error);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: "internal engine error".to_string(),
                        }),
                    ));
                }
                Ok(Ok(result)) => result?,
            };

        let mut axum_response = Json(payload).into_response();
        let headers_mut = axum_response.headers_mut();
        for (k, v) in spec_response_headers(effective_from.as_ref()) {
            headers_mut.insert(k, v);
        }
        Ok(axum_response)
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
                                        let workspace_specs = new_engine
                                            .list()
                                            .into_iter()
                                            .find(|repository_group| repository_group.repository.is_none())
                                            .expect("BUG: workspace repository must exist after Engine::new")
                                            .specs;
                                        let unique_specs: std::collections::BTreeSet<&str> =
                                            workspace_specs
                                                .iter()
                                                .map(|ls| ls.name.as_str())
                                                .collect();
                                        let spec_count = unique_specs.len();
                                        // Write lock held only for the Arc swap.
                                        *engine_clone.write().await = Arc::new(new_engine);
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
    /// directory (including `lemma_deps/` for cached registry dependencies).
    /// `lemma_deps/` files are loaded as dependencies with IDs derived from their path.
    async fn reload_engine(workdir: &std::path::Path) -> anyhow::Result<Engine> {
        use walkdir::WalkDir;

        let mut engine = Engine::new();
        let deps_dir = lemma_deps_dir(workdir);
        let mut workspace_paths: Vec<std::path::PathBuf> = Vec::new();
        let mut deps_paths: Vec<std::path::PathBuf> = Vec::new();
        for entry in WalkDir::new(workdir) {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("lemma") {
                continue;
            }
            if entry.path().starts_with(&deps_dir) {
                deps_paths.push(entry.path().to_path_buf());
            } else {
                workspace_paths.push(entry.path().to_path_buf());
            }
        }

        let mut sources =
            Vec::with_capacity(deps_paths.len().saturating_add(workspace_paths.len()));

        for dep_path in &deps_paths {
            let dependency_id = dependency_identifier_from_dependency_path(workdir, dep_path);
            let content = fs::read_to_string(dep_path)?;
            sources.push((
                lemma::SourceType::Dependency(dependency_id.to_string()),
                content,
            ));
        }
        for path in &workspace_paths {
            let content = fs::read_to_string(path)?;
            sources.push((
                lemma::SourceType::Path(std::sync::Arc::new(path.clone())),
                content,
            ));
        }
        if !sources.is_empty() {
            if let Err(load_err) = engine.load(sources) {
                for err in load_err.iter() {
                    tracing::error!(
                        "{}",
                        crate::error_formatter::format_error(err, &load_err.sources)
                    );
                }
                anyhow::bail!("Workspace load failed ({} error(s))", load_err.errors.len());
            }
        }
        Ok(engine)
    }
}
