#[cfg(feature = "server")]
pub mod http {
    use axum::{
        extract::{Path, Query, State},
        http::StatusCode,
        response::{IntoResponse, Json},
        routing::{get, post},
        Router,
    };
    use lemma::{Engine, Response};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower_http::cors::CorsLayer;
    use tracing::{error, info};

    type SharedEngine = Arc<RwLock<Engine>>;

    #[derive(Debug, Deserialize)]
    struct EvaluateRequest {
        code: String,
        #[serde(default)]
        facts: HashMap<String, serde_json::Value>,
    }

    #[derive(Debug, Serialize)]
    struct EvaluateResponse {
        results: Vec<RuleResultJson>,
        warnings: Vec<String>,
    }

    #[derive(Debug, Serialize)]
    struct RuleResultJson {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        veto_reason: Option<String>,
    }

    #[derive(Debug, Serialize)]
    struct ErrorResponse {
        error: String,
    }

    pub async fn start_server(engine: Engine, host: &str, port: u16) -> anyhow::Result<()> {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "lemma=info,tower_http=info".into()),
            )
            .init();

        let shared_engine = Arc::new(RwLock::new(engine));

        let app = Router::new()
            .route("/health", get(health_check))
            .route("/evaluate/:doc_name", get(evaluate_get))
            .route("/evaluate", post(evaluate_post))
            .layer(CorsLayer::permissive())
            .with_state(shared_engine);

        let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
        info!("Lemma server listening on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    async fn health_check() -> impl IntoResponse {
        Json(serde_json::json!({
            "status": "ok",
            "service": "lemma",
            "version": env!("CARGO_PKG_VERSION")
        }))
    }

    async fn evaluate_get(
        State(engine): State<SharedEngine>,
        Path(doc_name): Path<String>,
        Query(params): Query<HashMap<String, String>>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        let engine = engine.read().await;

        if engine.get_document(&doc_name).is_none() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("Document '{}' not found", doc_name),
                }),
            ));
        }

        let facts: Vec<String> = params.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        let fact_refs: Vec<&str> = facts.iter().map(|s| s.as_str()).collect();

        let response: Response = engine.evaluate(&doc_name, fact_refs).map_err(|e| {
            error!("Evaluation failed: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Evaluation failed: {}", e),
                }),
            )
        })?;

        let results = convert_results(&response);
        info!(
            "Evaluated document '{}' with {} results",
            doc_name,
            results.len()
        );

        Ok(Json(EvaluateResponse {
            results,
            warnings: response.warnings,
        }))
    }

    async fn evaluate_post(
        State(_engine): State<SharedEngine>,
        Json(payload): Json<EvaluateRequest>,
    ) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
        if payload.code.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Code cannot be empty".to_string(),
                }),
            ));
        }

        let mut temp_engine = Engine::new();
        let source_id = format!("inline_{}", chrono::Utc::now().timestamp_millis());

        temp_engine
            .add_lemma_code(&payload.code, &source_id)
            .map_err(|e| {
                error!("Failed to parse code: {}", e);
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("Failed to parse code: {}", e),
                    }),
                )
            })?;

        let documents = temp_engine.list_documents();
        if documents.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "No document found in provided code".to_string(),
                }),
            ));
        }

        let doc_name = &documents[0];

        let facts: Vec<String> = payload
            .facts
            .iter()
            .map(|(k, v)| format!("{}={}", k, json_value_to_lemma(v)))
            .collect();
        let fact_refs: Vec<&str> = facts.iter().map(|s| s.as_str()).collect();

        let response: Response = temp_engine.evaluate(doc_name, fact_refs).map_err(|e| {
            error!("Evaluation failed: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Evaluation failed: {}", e),
                }),
            )
        })?;

        let results = convert_results(&response);

        info!(
            "Evaluated inline document '{}' with {} results",
            doc_name,
            results.len()
        );

        Ok(Json(EvaluateResponse {
            results,
            warnings: response.warnings,
        }))
    }

    fn convert_results(response: &Response) -> Vec<RuleResultJson> {
        response
            .results
            .iter()
            .map(|r| RuleResultJson {
                name: r.rule_name.clone(),
                value: r.result.as_ref().map(|v| v.to_string()),
                veto_reason: r.veto_message.clone(),
            })
            .collect()
    }

    fn json_value_to_lemma(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => format!("\"{}\"", value.to_string().replace("\"", "\\\"")),
        }
    }
}

#[cfg(not(feature = "server"))]
pub mod http {
    pub async fn start_server(
        _engine: lemma::Engine,
        _host: &str,
        _port: u16,
    ) -> anyhow::Result<()> {
        anyhow::bail!("Server feature not enabled. Recompile with --features server")
    }
}
