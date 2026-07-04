mod imp {
    use anyhow::Result;
    use lemma::DateTimeValue;
    use lemma::Engine;
    use serde::{Deserialize, Serialize};
    use std::io::{self, BufRead, Write};
    use std::time::Duration;
    use tracing::{debug, error, info};

    const PROTOCOL_VERSION: &str = "2024-11-05";
    const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

    /// Upper bound on a single stdin JSON-RPC line. Lines beyond this are
    /// consumed and rejected with a JSON-RPC error instead of being buffered
    /// unboundedly.
    const MAX_STDIN_LINE_BYTES: usize = 10 * 1024 * 1024;

    #[derive(Debug, Deserialize)]
    struct McpRequest {
        jsonrpc: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<serde_json::Value>,
        method: String,
        #[serde(default)]
        params: Option<serde_json::Value>,
    }

    #[derive(Debug, Serialize)]
    struct McpResponse {
        jsonrpc: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<McpError>,
    }

    #[derive(Debug, Serialize)]
    struct McpError {
        code: i32,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
    }

    impl McpError {
        fn parse_error(message: String) -> Self {
            Self {
                code: -32700,
                message,
                data: None,
            }
        }

        fn invalid_request(message: String) -> Self {
            Self {
                code: -32600,
                message,
                data: None,
            }
        }

        fn method_not_found(method: String) -> Self {
            Self {
                code: -32601,
                message: format!("Method not found: {method}"),
                data: None,
            }
        }

        fn invalid_params(message: String) -> Self {
            Self {
                code: -32602,
                message,
                data: None,
            }
        }

        fn internal_error(message: String) -> Self {
            Self {
                code: -32603,
                message,
                data: None,
            }
        }
    }

    fn resolve_effective(args: &serde_json::Value) -> Result<DateTimeValue, McpError> {
        let raw = args.get("effective").and_then(|v| v.as_str());
        lemma::Engine::resolve_effective(raw)
            .map_err(|e| McpError::invalid_params(e.message().to_string()))
    }

    /// Configuration for the MCP server.
    pub struct McpConfig {
        /// When true, admin tools (`add_spec`, `get_spec_source`) are
        /// advertised and allowed. When false (default), the server is read-only.
        pub admin: bool,
        /// Wall-clock budget for handling a single request. Requests that
        /// exceed it get a JSON-RPC internal error; the worker finishes the
        /// stale request in the background and its late response is discarded.
        pub request_timeout: Duration,
    }

    impl Default for McpConfig {
        fn default() -> Self {
            Self {
                admin: false,
                request_timeout: Duration::from_secs(10),
            }
        }
    }

    struct McpServer {
        engine: Engine,
        config: McpConfig,
    }

    impl McpServer {
        fn new(engine: Engine, config: McpConfig) -> Self {
            Self { engine, config }
        }

        /// JSON-RPC 2.0: requests with no `id` are notifications and MUST NOT
        /// receive a response (§4.1). Returns `None` for notifications, even
        /// on error, so the transport layer skips the write entirely.
        fn handle_request(&mut self, request: McpRequest) -> Option<McpResponse> {
            debug!("Handling request: method={}", request.method);

            let is_notification = request.id.is_none();

            if request.jsonrpc != "2.0" {
                if is_notification {
                    debug!("Dropping notification with bad jsonrpc version");
                    return None;
                }
                return Some(McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: None,
                    error: Some(McpError::invalid_request(
                        "Invalid JSON-RPC version, expected '2.0'".to_string(),
                    )),
                });
            }

            if is_notification {
                match request.method.as_str() {
                    "notifications/initialized" => {
                        debug!("Client signalled notifications/initialized");
                    }
                    other => {
                        debug!("Ignoring notification: {}", other);
                    }
                }
                return None;
            }

            let result = match request.method.as_str() {
                "initialize" => self.initialize(),
                "tools/list" => self.list_tools(),
                "tools/call" => self.call_tool(request.params),
                _ => Err(McpError::method_not_found(request.method)),
            };

            Some(match result {
                Ok(result) => McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: Some(result),
                    error: None,
                },
                Err(error) => McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: None,
                    error: Some(error),
                },
            })
        }

        fn initialize(&self) -> Result<serde_json::Value, McpError> {
            info!("Initializing MCP server");
            Ok(serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "serverInfo": {
                    "name": "lemma-mcp-server",
                    "version": SERVER_VERSION
                },
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                }
            }))
        }

        fn list_tools(&self) -> Result<serde_json::Value, McpError> {
            debug!("Listing tools");

            let mut tools = vec![
                serde_json::json!({
                    "name": "evaluate",
                "description": "Evaluate rules in a Lemma spec. Returns the result and a step-by-step reasoning trace showing which data were used and which conditions matched. Omit 'rule' to evaluate all rules.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "spec": {
                                        "type": "string",
                                        "description": "Spec set id, e.g. pricing"
                            },
                            "rule": {
                                "type": "string",
                                "description": "Optional: name of a specific rule to evaluate. Omit to evaluate all rules."
                            },
                            "data": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional data values as 'name=value' (e.g. ['price=100', 'measure=5'])",
                                "default": []
                            },
                            "effective": {
                                "type": "string",
                                "description": "Optional: evaluate at a specific effective datetime (e.g. '2026', '2026-03', '2026-03-04', '2026-03-04T10:30:00Z')"
                            },
                            "conversions": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional measure unit conversions as 'rule=unit' or 'rule:unit' (e.g. ['total=usd'])",
                                "default": []
                            }
                        },
                        "required": ["spec"]
                    }
                }),
                serde_json::json!({
                    "name": "list_specs",
                    "description": "List all loaded Lemma specs with their schemas: data names, types, defaults, and rule names with return types.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "effective": {
                                "type": "string",
                                "description": "Optional: list specs at a specific effective datetime (e.g. '2026', '2026-03-04')"
                            }
                        }
                    }
                }),
                serde_json::json!({
                    "name": "get_schema",
                "description": "Get a spec's schema: its data (inputs with types, constraints, and defaults) and rules (outputs with types). Optionally scope to a specific rule to see only the data it needs. Use this before calling evaluate to know which data to provide.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "spec": {
                                        "type": "string",
                                        "description": "Spec set id, e.g. pricing"
                                    },
                                    "rule": {
                                        "type": "string",
                                        "description": "Optional: name of a specific rule. Omit to get the full spec schema."
                                    },
                            "effective": {
                                "type": "string",
                                "description": "Optional: get schema at a specific effective datetime"
                            }
                        },
                        "required": ["spec"]
                    }
                }),
            ];

            if self.config.admin {
                tools.push(serde_json::json!({
                    "name": "add_spec",
                    "description": "Add Lemma source to the engine. Returns each new spec schema on success.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "code": {
                                "type": "string",
                                "description": "The complete Lemma code to add"
                            },
                            "source_id": {
                                "type": "string",
                                "description": "Identifier for this source fragment (used as load path)"
                            }
                        },
                        "required": ["code", "source_id"]
                    }
                }));
                tools.push(serde_json::json!({
                    "name": "get_spec_source",
                    "description": "Return formatted Lemma source. Pass `repository` (e.g. `lemma` for embedded units stdlib) for the whole repo, or `spec` for a workspace spec.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "repository": {
                                        "type": "string",
                                        "description": "Repository qualifier (e.g. lemma). When set, returns formatted source for the entire repository."
                                    },
                                    "spec": {
                                        "type": "string",
                                        "description": "Workspace spec set id (when repository is omitted)"
                                    },
                            "effective": {
                                "type": "string",
                                "description": "Optional: get source at a specific effective datetime"
                            }
                        }
                    }
                }));
            }

            Ok(serde_json::json!({ "tools": tools }))
        }

        fn call_tool(
            &mut self,
            params: Option<serde_json::Value>,
        ) -> Result<serde_json::Value, McpError> {
            let params =
                params.ok_or_else(|| McpError::invalid_params("Missing params".to_string()))?;

            let tool_name = params["name"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing tool name".to_string()))?;

            let arguments = params
                .get("arguments")
                .ok_or_else(|| McpError::invalid_params("Missing arguments".to_string()))?;

            debug!("Calling tool: {}", tool_name);

            match tool_name {
                "add_spec" | "get_spec_source" if !self.config.admin => {
                    Err(McpError::invalid_params(
                        "Admin tools are disabled. Start the server with --admin to enable them."
                            .to_string(),
                    ))
                }
                "add_spec" => self.tool_add_spec(arguments),
                "get_spec_source" => self.tool_get_spec_source(arguments),
                "evaluate" => self.tool_evaluate(arguments),
                "list_specs" => self.tool_list_specs(arguments),
                "get_schema" => self.tool_get_schema(arguments),
                _ => Err(McpError::invalid_params(format!(
                    "Unknown tool: {}",
                    tool_name
                ))),
            }
        }

        fn tool_add_spec(
            &mut self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let code = args["code"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'code' field".to_string()))?;

            if code.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Lemma source cannot be empty".to_string(),
                ));
            }

            let source_id = args["source_id"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| McpError::invalid_params("Missing 'source_id' field".to_string()))?
                .to_string();

            let names_before: std::collections::HashSet<String> = self
                .engine
                .get_workspace()
                .specs
                .iter()
                .map(|ss| ss.name.clone())
                .collect();

            let source_type =
                lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(&source_id)));
            self.engine.load(code, source_type).map_err(|load_err| {
                for e in load_err.iter() {
                    error!(
                        "{}",
                        crate::error_formatter::format_error(e, &load_err.sources)
                    );
                }
                McpError::internal_error(format!(
                    "Failed to load Lemma source ({} error(s))",
                    load_err.errors.len()
                ))
            })?;

            let new_spec_names: Vec<String> = self
                .engine
                .get_workspace()
                .specs
                .iter()
                .filter(|ss| !names_before.contains(&ss.name))
                .map(|ss| ss.name.clone())
                .collect();

            let mut output = String::from("Spec added successfully.\n\n");

            let now = DateTimeValue::now();
            for spec_name in &new_spec_names {
                if let Ok(plan) = self.engine.get_plan(None, spec_name, Some(&now)) {
                    output.push_str(&plan.schema(&lemma::DataOverlay::default()).to_string());
                    output.push('\n');
                }
            }

            info!(
                "Spec added from source '{}': {:?}",
                source_id, new_spec_names
            );

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }

        fn tool_get_spec_source(
            &self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            if let Some(repo) = args
                .get("repository")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let source = self.engine.format_repository(repo).map_err(|e| {
                    McpError::invalid_params(format!(
                        "Repository '{}' not found: {}. Use list_specs to see loaded repositories.",
                        repo, e
                    ))
                })?;
                debug!("Returned formatted source for repository '{}'", repo);
                return Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": source
                    }]
                }));
            }

            let spec_set_id = args["spec"].as_str().ok_or_else(|| {
                McpError::invalid_params("Missing 'spec' or 'repository' field".to_string())
            })?;

            let spec_name = lemma::parse_spec_set_id(spec_set_id.trim())
                .map_err(|e| McpError::invalid_params(format!("{}", e)))?;

            let now = resolve_effective(args)?;
            let spec = self.engine.get_spec(&spec_name, Some(&now)).map_err(|e| {
                McpError::invalid_params(format!(
                    "Spec '{}' not found: {}. Use list_specs to see available specs.",
                    spec_set_id, e
                ))
            })?;

            let source = lemma::format_specs(std::slice::from_ref(spec.as_ref()));

            debug!("Returned source for spec '{}'", spec_name);

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": source
                }]
            }))
        }

        fn tool_evaluate(
            &mut self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let spec_set_id = args["spec"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'spec' field".to_string()))?;

            if spec_set_id.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Spec set id cannot be empty".to_string(),
                ));
            }

            let spec_name = lemma::parse_spec_set_id(spec_set_id.trim())
                .map_err(|e| McpError::invalid_params(format!("{}", e)))?;

            let rule_names: Vec<String> = match args.get("rule").and_then(|v| v.as_str()) {
                Some(rule) if !rule.trim().is_empty() => vec![rule.trim().to_string()],
                _ => Vec::new(),
            };

            let data: Vec<&str> = args["data"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            let data_values: std::collections::HashMap<String, String> = data
                .iter()
                .filter_map(|s| {
                    s.split_once('=')
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                })
                .collect();

            let now = resolve_effective(args)?;

            let plan = self
                .engine
                .get_plan(None, &spec_name, Some(&now))
                .map_err(|e| {
                    error!("Evaluation failed: {}", e);
                    McpError::internal_error(format!("Evaluation failed: {e}"))
                })?;
            let rules = if rule_names.is_empty() {
                None
            } else {
                Some(rule_names.as_slice())
            };
            let data_input: std::collections::HashMap<String, lemma::DataValueInput> = data_values
                .into_iter()
                .map(|(k, v)| (k, lemma::DataValueInput::convenience(v)))
                .collect();
            let response = self
                .engine
                .run_plan(plan, Some(&now), data_input, true, rules)
                .map_err(|e| {
                    error!("Evaluation failed: {}", e);
                    McpError::internal_error(format!("Evaluation failed: {e}"))
                })?;

            let mut output = String::new();
            output.push_str(&format!("spec: {}\n", spec_set_id.trim()));
            output.push_str(&format!("effective: {}\n", now));
            output.push('\n');

            for result in response.results.values() {
                output.push_str(&format!("{}: ", result.rule.name));
                if result.vetoed {
                    if let Some(reason) = result.veto_reason.as_deref() {
                        output.push_str(reason);
                    }
                } else {
                    let display = result.display.as_deref().ok_or_else(|| {
                        McpError::internal_error(format!(
                            "Rule '{}' evaluated without display after evaluation",
                            result.rule.name
                        ))
                    })?;
                    output.push_str(display);
                }
                output.push('\n');

                if let Some(explanation) = &result.explanation {
                    let steps = lemma::format_explanation(explanation);
                    if !steps.is_empty() {
                        output.push_str("\nReasoning:\n");
                        output.push_str(&steps);
                        output.push('\n');
                    }
                }
            }

            info!(
                "Evaluated spec '{}' with {} results",
                spec_set_id.trim(),
                response.results.len()
            );

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }

        fn tool_list_specs(&self, args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            let now = resolve_effective(args)?;
            let mut sections: Vec<String> = Vec::new();
            let mut spec_count = 0usize;

            for resolved in self.engine.list() {
                let label = crate::interactive::repo_label(resolved.repository.as_ref());
                let repo_q = resolved.repository.name.as_deref();
                let schemas: Vec<String> = resolved
                    .specs
                    .iter()
                    .flat_map(|ss| ss.iter_specs())
                    .filter_map(|spec| {
                        let effective = spec
                            .effective_from()
                            .cloned()
                            .unwrap_or_else(|| now.clone());
                        self.engine
                            .schema(repo_q, &spec.name, Some(&effective))
                            .ok()
                            .map(|s| s.to_string())
                    })
                    .collect();
                spec_count += schemas.len();
                if !schemas.is_empty() {
                    sections.push(format!("Repository: {}\n\n{}", label, schemas.join("\n\n")));
                }
            }

            let workspace_empty = self
                .engine
                .get_workspace()
                .specs
                .iter()
                .all(|ss| ss.iter_specs().next().is_none());

            let output = if spec_count == 0 {
                if self.config.admin {
                    "No specs loaded.\n\nUse the 'add_spec' tool to load Lemma source.".to_string()
                } else {
                    "No specs loaded.".to_string()
                }
            } else {
                let mut out = sections.join("\n\n---\n\n");
                if self.config.admin && workspace_empty {
                    out.push_str("\n\nUse the 'add_spec' tool to load workspace Lemma source.");
                }
                out
            };

            debug!("Listed {} specs across repositories", spec_count);

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }

        fn tool_get_schema(&self, args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            let spec_set_id = args["spec"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'spec' field".to_string()))?;

            if spec_set_id.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Spec set id cannot be empty".to_string(),
                ));
            }

            let spec_name = lemma::parse_spec_set_id(spec_set_id.trim())
                .map_err(|e| McpError::invalid_params(format!("{}", e)))?;

            let now = resolve_effective(args)?;
            let plan = self
                .engine
                .get_plan(None, &spec_name, Some(&now))
                .map_err(|e| {
                    error!("get_plan failed for '{}': {}", spec_set_id.trim(), e);
                    McpError::internal_error(format!("Failed to get schema: {e}"))
                })?;

            let rule_names: Vec<String> = match args.get("rule").and_then(|v| v.as_str()) {
                Some(rule) if !rule.trim().is_empty() => vec![rule.trim().to_string()],
                _ => Vec::new(),
            };

            let schema = if rule_names.is_empty() {
                plan.schema(&lemma::DataOverlay::default())
            } else {
                plan.schema_for_rules(&rule_names, &lemma::DataOverlay::default())
                    .map_err(|e| {
                        error!("schema_for_rules failed: {}", e);
                        McpError::internal_error(format!("Failed to get schema for rules: {e}"))
                    })?
            };

            let scope = if rule_names.is_empty() {
                format!("{} (all rules)", spec_set_id.trim())
            } else {
                format!("{}.{}", spec_set_id.trim(), rule_names[0])
            };

            let output = format!("Schema for {}:\n\n{}", scope, schema);

            info!(
                "Returned schema for '{}' ({} data, {} rules)",
                scope,
                schema.data.len(),
                schema.rules.len()
            );

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }
    }

    /// Result of reading one stdin line under a byte cap.
    enum CappedLine {
        Eof,
        Line(String),
        /// Line exceeded the cap; its bytes were consumed up to and including
        /// the terminating newline so the stream stays in sync.
        TooLong,
        /// Line was within the cap but is not valid UTF-8.
        InvalidUtf8,
    }

    /// Read one `\n`-terminated line, buffering at most `cap` bytes. Oversized
    /// lines are drained (not buffered) and reported as `TooLong`. A trailing
    /// `\r` is stripped, matching `BufRead::lines`.
    fn read_line_capped(reader: &mut impl BufRead, cap: usize) -> io::Result<CappedLine> {
        let mut buf: Vec<u8> = Vec::new();
        let mut over_cap = false;
        loop {
            let (consume_len, line_done) = {
                let available = reader.fill_buf()?;
                if available.is_empty() {
                    if buf.is_empty() && !over_cap {
                        return Ok(CappedLine::Eof);
                    }
                    (0, true)
                } else if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                    if !over_cap {
                        if buf.len() + pos > cap {
                            over_cap = true;
                        } else {
                            buf.extend_from_slice(&available[..pos]);
                        }
                    }
                    (pos + 1, true)
                } else {
                    if !over_cap {
                        if buf.len() + available.len() > cap {
                            over_cap = true;
                        } else {
                            buf.extend_from_slice(available);
                        }
                    }
                    (available.len(), false)
                }
            };
            reader.consume(consume_len);
            if line_done {
                if over_cap {
                    return Ok(CappedLine::TooLong);
                }
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                return match String::from_utf8(buf) {
                    Ok(s) => Ok(CappedLine::Line(s)),
                    Err(_) => Ok(CappedLine::InvalidUtf8),
                };
            }
        }
    }

    pub fn start_server(engine: Engine, config: McpConfig) -> Result<()> {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "lemma_mcp=info".into()),
            )
            .with_writer(io::stderr)
            .init();

        info!("Starting Lemma MCP server v{}", SERVER_VERSION);
        info!("Protocol version: {}", PROTOCOL_VERSION);
        if config.admin {
            info!("Admin mode enabled (--admin)");
        } else {
            info!("Read-only mode (default)");
        }

        let request_timeout = config.request_timeout;

        // Requests are handled on a dedicated worker thread that owns the
        // engine state, so the reader loop can enforce a wall-clock timeout
        // per request. The worker sends exactly one response per request; a
        // timed-out request's late response is counted in `abandoned` and
        // discarded when it eventually arrives.
        let (request_tx, request_rx) = std::sync::mpsc::channel::<McpRequest>();
        let (response_tx, response_rx) = std::sync::mpsc::channel::<Option<McpResponse>>();
        std::thread::spawn(move || {
            let mut server = McpServer::new(engine, config);
            for request in request_rx {
                let request_id = request.id.clone();
                let response = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    server.handle_request(request)
                })) {
                    Ok(resp) => resp,
                    Err(panic_payload) => {
                        let msg = panic_payload
                            .downcast_ref::<&str>()
                            .copied()
                            .or_else(|| panic_payload.downcast_ref::<String>().map(|s| s.as_str()))
                            .unwrap_or("unknown internal error");
                        error!("engine panic caught: {}", msg);
                        Some(McpResponse {
                            jsonrpc: "2.0".to_string(),
                            id: request_id,
                            result: None,
                            error: Some(McpError::internal_error(
                                "internal engine error".to_string(),
                            )),
                        })
                    }
                };
                if response_tx.send(response).is_err() {
                    break;
                }
            }
        });

        let mut stdin = io::stdin().lock();
        let mut stdout = io::stdout();
        let mut abandoned: usize = 0;

        loop {
            let line = match read_line_capped(&mut stdin, MAX_STDIN_LINE_BYTES)? {
                CappedLine::Eof => break,
                CappedLine::Line(line) => line,
                CappedLine::TooLong => {
                    error!(
                        "stdin line exceeds {} bytes, rejected",
                        MAX_STDIN_LINE_BYTES
                    );
                    let response = McpResponse {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        result: None,
                        error: Some(McpError::parse_error(format!(
                            "Request line exceeds {MAX_STDIN_LINE_BYTES} bytes"
                        ))),
                    };
                    writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
                    stdout.flush()?;
                    continue;
                }
                CappedLine::InvalidUtf8 => {
                    error!("stdin line is not valid UTF-8, rejected");
                    let response = McpResponse {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        result: None,
                        error: Some(McpError::parse_error(
                            "Request line is not valid UTF-8".to_string(),
                        )),
                    };
                    writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
                    stdout.flush()?;
                    continue;
                }
            };

            if line.trim().is_empty() {
                continue;
            }

            debug!("Received: {}", line);

            // Drain late responses from previously timed-out requests so the
            // response channel stays aligned with the request we are about to
            // send.
            while abandoned > 0 {
                match response_rx.try_recv() {
                    Ok(_) => abandoned -= 1,
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        anyhow::bail!("BUG: MCP worker thread exited while requests pending")
                    }
                }
            }

            // Parse error responds with id: null (JSON-RPC 2.0 §4.2). For
            // any successfully-parsed notification, handle_request returns
            // None and we MUST NOT write anything back.
            let response = match serde_json::from_str::<McpRequest>(&line) {
                Ok(request) => {
                    let request_id = request.id.clone();
                    let is_notification = request_id.is_none();
                    request_tx
                        .send(request)
                        .map_err(|_| anyhow::anyhow!("BUG: MCP worker thread exited"))?;
                    loop {
                        match response_rx.recv_timeout(request_timeout) {
                            Ok(response) => {
                                if abandoned > 0 {
                                    abandoned -= 1;
                                    continue;
                                }
                                break response;
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                abandoned += 1;
                                error!("request timed out after {}s", request_timeout.as_secs());
                                break if is_notification {
                                    None
                                } else {
                                    Some(McpResponse {
                                        jsonrpc: "2.0".to_string(),
                                        id: request_id,
                                        result: None,
                                        error: Some(McpError::internal_error(format!(
                                            "Request timed out after {}s",
                                            request_timeout.as_secs()
                                        ))),
                                    })
                                };
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                                anyhow::bail!("BUG: MCP worker thread exited mid-request")
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Parse error: {}", e);
                    Some(McpResponse {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        result: None,
                        error: Some(McpError::parse_error(format!("Parse error: {e}"))),
                    })
                }
            };

            if let Some(response) = response {
                let response_json = serde_json::to_string(&response)?;
                writeln!(stdout, "{}", response_json)?;
                stdout.flush()?;
                debug!("Sent response");
            } else {
                debug!("No response (notification)");
            }
        }

        info!("MCP server shutting down");
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn server() -> McpServer {
            McpServer::new(Engine::new(), McpConfig::default())
        }

        fn parse(line: &str) -> McpRequest {
            serde_json::from_str(line).expect("test fixture must be valid JSON-RPC")
        }

        #[test]
        fn notification_returns_no_response() {
            let mut s = server();
            let req = parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
            assert!(s.handle_request(req).is_none());
        }

        #[test]
        fn notification_with_unknown_method_still_silent() {
            let mut s = server();
            let req = parse(r#"{"jsonrpc":"2.0","method":"some/random/notification"}"#);
            assert!(s.handle_request(req).is_none());
        }

        #[test]
        fn notification_with_bad_jsonrpc_version_silent() {
            let mut s = server();
            let req = parse(r#"{"jsonrpc":"1.0","method":"notifications/initialized"}"#);
            assert!(s.handle_request(req).is_none());
        }

        #[test]
        fn request_with_id_gets_response() {
            let mut s = server();
            let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
            let resp = s.handle_request(req).expect("request must yield response");
            assert_eq!(resp.id, Some(serde_json::json!(1)));
            assert!(resp.result.is_some());
            assert!(resp.error.is_none());
        }

        #[test]
        fn request_with_unknown_method_returns_method_not_found() {
            let mut s = server();
            let req = parse(r#"{"jsonrpc":"2.0","id":7,"method":"does/not/exist"}"#);
            let resp = s.handle_request(req).expect("request must yield response");
            assert_eq!(resp.id, Some(serde_json::json!(7)));
            assert_eq!(resp.error.as_ref().expect("error expected").code, -32601);
        }

        #[test]
        fn request_with_bad_jsonrpc_version_returns_invalid_request() {
            let mut s = server();
            let req = parse(r#"{"jsonrpc":"1.0","id":2,"method":"initialize"}"#);
            let resp = s.handle_request(req).expect("request must yield response");
            assert_eq!(resp.error.as_ref().expect("error expected").code, -32600);
        }

        #[test]
        fn initialize_advertises_tools_list_changed_false() {
            let mut s = server();
            let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
            let resp = s.handle_request(req).expect("request must yield response");
            let result = resp.result.expect("result expected");
            assert_eq!(result["capabilities"]["tools"]["listChanged"], false);
        }

        fn read_all_capped(input: &[u8], cap: usize) -> Vec<CappedLine> {
            let mut reader = io::BufReader::with_capacity(8, input);
            let mut out = Vec::new();
            loop {
                let line = read_line_capped(&mut reader, cap).expect("in-memory read");
                let eof = matches!(line, CappedLine::Eof);
                out.push(line);
                if eof {
                    break;
                }
            }
            out
        }

        #[test]
        fn capped_reader_returns_lines_within_cap() {
            let lines = read_all_capped(b"hello\nworld\n", 100);
            assert!(matches!(&lines[0], CappedLine::Line(s) if s == "hello"));
            assert!(matches!(&lines[1], CappedLine::Line(s) if s == "world"));
            assert!(matches!(lines[2], CappedLine::Eof));
        }

        #[test]
        fn capped_reader_strips_trailing_cr() {
            let lines = read_all_capped(b"hello\r\n", 100);
            assert!(matches!(&lines[0], CappedLine::Line(s) if s == "hello"));
        }

        #[test]
        fn capped_reader_handles_final_line_without_newline() {
            let lines = read_all_capped(b"no newline", 100);
            assert!(matches!(&lines[0], CappedLine::Line(s) if s == "no newline"));
            assert!(matches!(lines[1], CappedLine::Eof));
        }

        #[test]
        fn capped_reader_rejects_over_cap_line_and_resyncs() {
            let mut input = vec![b'x'; 50];
            input.push(b'\n');
            input.extend_from_slice(b"ok\n");
            let lines = read_all_capped(&input, 10);
            assert!(matches!(lines[0], CappedLine::TooLong));
            assert!(
                matches!(&lines[1], CappedLine::Line(s) if s == "ok"),
                "stream must stay in sync after an oversized line"
            );
        }

        #[test]
        fn capped_reader_rejects_over_cap_line_at_eof_without_newline() {
            let input = vec![b'x'; 50];
            let lines = read_all_capped(&input, 10);
            assert!(matches!(lines[0], CappedLine::TooLong));
            assert!(matches!(lines[1], CappedLine::Eof));
        }

        #[test]
        fn capped_reader_reports_invalid_utf8() {
            let lines = read_all_capped(&[0xff, 0xfe, b'\n'], 100);
            assert!(matches!(lines[0], CappedLine::InvalidUtf8));
        }

        #[test]
        fn capped_reader_line_exactly_at_cap_is_accepted() {
            let mut input = vec![b'x'; 10];
            input.push(b'\n');
            let lines = read_all_capped(&input, 10);
            assert!(matches!(&lines[0], CappedLine::Line(s) if s.len() == 10));
        }
    }
}

pub use imp::start_server;
pub use imp::McpConfig;
