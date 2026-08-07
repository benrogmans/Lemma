mod imp {
    use anyhow::Result;
    use lemma::DateTimeValue;
    use lemma::Engine;
    use serde::{Deserialize, Serialize};
    use std::fs;
    use std::io::{self, BufRead, Write};
    use std::path::{Component, Path, PathBuf};
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
        lemma::resolve_effective(raw).map_err(|e| McpError::invalid_params(e.message().to_string()))
    }

    /// Configuration for the MCP server.
    pub struct McpConfig {
        /// When true, admin tools (`add_spec`, `update_spec`, `remove_spec`, `clear`, `install`)
        /// are advertised and allowed. When false (default), the server is read-only.
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
        /// Workspace directory for admin persist (add / update / remove / clear / install).
        workdir: PathBuf,
        registry: Box<dyn lemma::Registry>,
    }

    /// Resolve `workdir/source_path` for relative source ids. Rejects absolute paths and `..`.
    fn validated_write_path(workdir: &Path, source_path: &Path) -> Result<PathBuf, McpError> {
        if source_path.as_os_str().is_empty() {
            return Err(McpError::invalid_params(
                "source_id path must be non-empty".to_string(),
            ));
        }
        if source_path.is_absolute() {
            return Err(McpError::invalid_params(
                "source_id must be a relative path within the workspace".to_string(),
            ));
        }
        for component in source_path.components() {
            match component {
                Component::Normal(_) | Component::CurDir => {}
                Component::ParentDir => {
                    return Err(McpError::invalid_params(
                        "source_id must not contain '..'".to_string(),
                    ));
                }
                _ => {
                    return Err(McpError::invalid_params(
                        "source_id must be a relative path within the workspace".to_string(),
                    ));
                }
            }
        }
        Ok(workdir.join(source_path))
    }

    /// Absolute Path sources (workspace load) must still resolve under workdir.
    fn absolute_path_under_workdir(
        workdir: &Path,
        source_path: &Path,
    ) -> Result<PathBuf, McpError> {
        let workdir_canon = fs::canonicalize(workdir).map_err(|e| {
            McpError::internal_error(format!(
                "Failed to resolve workspace {}: {e}",
                workdir.display()
            ))
        })?;
        let path_canon =
            fs::canonicalize(source_path).unwrap_or_else(|_| source_path.to_path_buf());
        if !path_canon.starts_with(&workdir_canon) {
            return Err(McpError::invalid_params(format!(
                "path {} is outside the workspace",
                source_path.display()
            )));
        }
        Ok(path_canon)
    }

    /// Atomic write: temp file in same directory, fsync, rename.
    fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
        lemma_cli::install::atomic_write(path, contents)
    }

    impl McpServer {
        fn new(
            engine: Engine,
            config: McpConfig,
            workdir: PathBuf,
            registry: Box<dyn lemma::Registry>,
        ) -> Self {
            Self {
                engine,
                config,
                workdir,
                registry,
            }
        }

        /// Disk path for this source. Always a concrete path — never optional skip.
        fn disk_path(&self, source_type: &lemma::SourceType) -> Result<PathBuf, McpError> {
            match source_type {
                lemma::SourceType::Path(source_path) => {
                    let path = source_path.as_ref();
                    if path.is_absolute() {
                        absolute_path_under_workdir(&self.workdir, path)
                    } else {
                        validated_write_path(&self.workdir, path)
                    }
                }
                lemma::SourceType::Dependency(id) if id == lemma::EMBEDDED_STDLIB_REPOSITORY => {
                    Err(McpError::invalid_params(
                        "cannot mutate the embedded standard library".to_string(),
                    ))
                }
                lemma::SourceType::Dependency(id) => {
                    Ok(lemma::deps::dependency_cache_file(&self.workdir, id))
                }
                lemma::SourceType::Volatile => Err(McpError::invalid_params(
                    "volatile sources cannot be persisted".to_string(),
                )),
            }
        }

        fn formatted_persist_source(code: &str, source_type: &lemma::SourceType) -> String {
            lemma::format_source(code, source_type.clone()).expect(
                "BUG: format_source must succeed after engine load/update accepted the source",
            )
        }

        /// Remove every spec that `code` defined (reverse parse order) so dependents
        /// of an add can be torn down after a failed disk write.
        fn rollback_added_source(&mut self, code: &str, source_type: &lemma::SourceType) {
            let parsed = lemma::parse(code, source_type.clone(), &lemma::ResourceLimits::default())
                .expect("BUG: source already loaded successfully must parse for rollback");
            let mut removals: Vec<(Option<String>, String, Option<DateTimeValue>)> = Vec::new();
            for (repo, specs) in parsed.repositories {
                let repo_name = repo.name.clone();
                for spec in specs {
                    removals.push((
                        repo_name.clone(),
                        spec.name.clone(),
                        spec.effective_from().cloned(),
                    ));
                }
            }
            for (repo, name, eff) in removals.into_iter().rev() {
                self.engine
                    .remove(repo.as_deref(), &name, eff.as_ref())
                    .expect("BUG: rollback remove after failed persist must succeed");
            }
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
                "resources/list" => self.list_resources(),
                "resources/read" => self.read_resource(request.params),
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
                    },
                    "resources": {
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
                    "description": "Evaluate rules. Pass `rule` to target one rule; omit for all. For human intake: call guide (default = evaluate guide; not topic full), then list, show once, evaluate. missing_data lines include name, type, and help. Primary loop: after each user turn, bind every field that utterance decides (entailments), re-evaluate; ask at most one open topic-question when something remains. Never ask the user what the policy means. Never dispose interpretation as truth; use “should” when a judgment call cannot be answered. When the rule answers, present details+answer in domain language for user verify (no tooling jargon to the user) before treating as done. No questionnaire dumps. No re-call show between asks. Do not dump every show data field into evaluate. Returns display values, unit maps, reasoning, and missing_data when inputs are still needed.",
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
                                "description": "Optional data values as 'name=value' (e.g. ['price=100', 'measure=5']). Partial is fine.",
                                "default": []
                            },
                            "effective": {
                                "type": "string",
                                "description": "Optional: evaluate at a specific effective datetime (e.g. '2026', '2026-03', '2026-03-04', '2026-03-04T10:30:00Z')"
                            }
                        },
                        "required": ["spec"]
                    }
                }),
                serde_json::json!({
                    "name": "list",
                    "description": "List loaded specs by repository (name, effective_from, effective_to). Call this first when you do not already know the exact spec name. Do not invent or guess spec names. For human intake call guide (default evaluate guide), then show once and evaluate.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                serde_json::json!({
                    "name": "show",
                    "description": "Return JSON Show for a spec: data catalog (types, constraints, suggestions, units, help) and rule output types. Call once after list. Static interface — not a required-input list, not a questionnaire, not something to re-call between evaluate/ask turns. Human intake: call guide (default = evaluate guide).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "spec": {
                                "type": "string",
                                "description": "Spec set id, e.g. pricing"
                            },
                            "effective": {
                                "type": "string",
                                "description": "Optional: show at a specific effective datetime"
                            }
                        },
                        "required": ["spec"]
                    }
                }),
                serde_json::json!({
                    "name": "source",
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
                }),
                serde_json::json!({
                    "name": "check",
                    "description": "Validate Lemma sources (does not load). On success confirms syntax is valid. Call add_spec to load after check passes. On failure returns structured diagnostics (kind, message, suggestion, source line/column). Sources resolve cross-file `uses` within the batch. A leading `@` label loads as a dependency. Lemma has no `#` or `//` comments; commentary is valid only as a docstring immediately after the `spec` line. Before drafting new specs, call guide with topic full.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "sources": {
                                "type": "array",
                                "items": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "minItems": 2,
                                    "maxItems": 2
                                },
                                "description": "Array of [label, code] pairs. Label is the source path (e.g. 'pricing.lemma') or a dependency identifier (e.g. '@org/repo')."
                            }
                        },
                        "required": ["sources"]
                    }
                }),
                serde_json::json!({
                    "name": "guide",
                    "description": "Return a Lemma guide. Omit topic for the evaluate guide (CS intake with loaded specs — default). Pass topic full only when authoring new Lemma specs (complete authoring guide). Other topics are authoring sections: method, syntax, data, rules, units, veto, composition, natural_language, anti_patterns; topic evaluate is the same as the default.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "topic": {
                                "type": "string",
                                "enum": ["method", "syntax", "data", "rules", "units", "veto", "composition", "natural_language", "anti_patterns", "evaluate", "full"],
                                "description": "Optional. Omit for evaluate guide. Use full only when authoring new specs."
                            }
                        }
                    }
                }),
            ];

            if self.config.admin {
                tools.push(serde_json::json!({
                    "name": "add_spec",
                    "description": "Load Lemma source as one or more specs (persists). Prefer check first; check alone does not load. On failure returns structured diagnostics. After success, present the full source in chat for user verify.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "code": {
                                "type": "string",
                                "description": "The complete Lemma code to add"
                            },
                            "source_id": {
                                "type": "string",
                                "description": "Stable id for this source (e.g. pricing.lemma)"
                            }
                        },
                        "required": ["code", "source_id"]
                    }
                }));
                tools.push(serde_json::json!({
                    "name": "update_spec",
                    "description": "Replace an existing spec with new source. After success, present the full source in chat for user verify.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "spec": {
                                "type": "string",
                                "description": "Name of the spec to replace"
                            },
                            "code": {
                                "type": "string",
                                "description": "The complete new Lemma source"
                            },
                            "source_id": {
                                "type": "string",
                                "description": "Stable id for this source (e.g. pricing.lemma)"
                            },
                            "repository": {
                                "type": "string",
                                "description": "Repository qualifier when the spec is not in the workspace"
                            },
                            "effective": {
                                "type": "string",
                                "description": "Effective datetime of the version to replace"
                            }
                        },
                        "required": ["spec", "code", "source_id"]
                    }
                }));
                tools.push(serde_json::json!({
                    "name": "remove_spec",
                    "description": "Remove one spec version. Omit effective to remove the origin version; pass effective to remove that version.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "spec": {
                                "type": "string",
                                "description": "Name of the spec to remove"
                            },
                            "repository": {
                                "type": "string",
                                "description": "Repository qualifier when the spec is not in the workspace"
                            },
                            "effective": {
                                "type": "string",
                                "description": "Effective datetime of the version to remove"
                            }
                        },
                        "required": ["spec"]
                    }
                }));
                tools.push(serde_json::json!({
                    "name": "clear",
                    "description": "Remove all specs.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }));
                tools.push(serde_json::json!({
                    "name": "install",
                    "description": "Download a registry dependency (e.g. @iso/countries), persist under lemma_deps/, and load it. Pass force=true to overwrite an existing copy.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "dependency": {
                                "type": "string",
                                "description": "Registry identifier, e.g. @iso/countries"
                            },
                            "force": {
                                "type": "boolean",
                                "description": "Overwrite if already present (default false)",
                                "default": false
                            }
                        },
                        "required": ["dependency"]
                    }
                }));
            }

            Ok(serde_json::json!({ "tools": tools }))
        }

        fn list_resources(&self) -> Result<serde_json::Value, McpError> {
            let mut resources = vec![serde_json::json!({
                "uri": "lemma://guide",
                "name": "Lemma evaluate guide",
                "mimeType": "text/plain",
                "description": "Default evaluate guide for CS intake. Same as guide tool with no topic. Use lemma://guide/full only when authoring."
            })];
            for topic in crate::mcp::guide::GuideTopic::ALL {
                resources.push(serde_json::json!({
                    "uri": format!("lemma://guide/{}", topic.as_str()),
                    "name": format!("Lemma guide: {}", topic.as_str()),
                    "mimeType": "text/plain",
                    "description": format!("Guide section '{}'", topic.as_str())
                }));
            }
            for example in crate::mcp::guide::EXAMPLE_RESOURCES {
                resources.push(serde_json::json!({
                    "uri": format!("lemma://examples/{}", example.path),
                    "name": example.path,
                    "mimeType": "text/plain",
                    "description": format!("Example Lemma source: {}", example.path)
                }));
            }
            Ok(serde_json::json!({ "resources": resources }))
        }

        fn read_resource(
            &self,
            params: Option<serde_json::Value>,
        ) -> Result<serde_json::Value, McpError> {
            let params =
                params.ok_or_else(|| McpError::invalid_params("Missing params".to_string()))?;
            let uri = params["uri"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'uri' field".to_string()))?;
            let text = self.resource_text(uri)?;
            Ok(serde_json::json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "text/plain",
                    "text": text
                }]
            }))
        }

        fn resource_text(&self, uri: &str) -> Result<&'static str, McpError> {
            if uri == "lemma://guide" {
                return Ok(crate::mcp::guide::EVALUATE_GUIDE);
            }
            if let Some(topic_name) = uri.strip_prefix("lemma://guide/") {
                let topic = crate::mcp::guide::GuideTopic::parse(topic_name).ok_or_else(|| {
                    McpError::invalid_params(format!(
                        "Unknown guide topic '{topic_name}'. Valid: {}",
                        crate::mcp::guide::GuideTopic::VALID_LIST
                    ))
                })?;
                return Ok(topic.section_text());
            }
            if let Some(path) = uri.strip_prefix("lemma://examples/") {
                return crate::mcp::guide::example_by_path(path).ok_or_else(|| {
                    McpError::invalid_params(format!("Unknown example resource: {uri}"))
                });
            }
            Err(McpError::invalid_params(format!(
                "Unknown resource URI: {uri}. Use resources/list."
            )))
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
                "add_spec" | "update_spec" | "remove_spec" | "clear" | "install"
                    if !self.config.admin =>
                {
                    Err(McpError::invalid_params(
                        "Admin tools are disabled. Start the server with --admin to enable them."
                            .to_string(),
                    ))
                }
                "add_spec" => self.tool_add_spec(arguments),
                "update_spec" => self.tool_update_spec(arguments),
                "remove_spec" => self.tool_remove_spec(arguments),
                "clear" => self.tool_clear(arguments),
                "install" => self.tool_install(arguments),
                "source" => self.tool_source(arguments),
                "evaluate" => self.tool_evaluate(arguments),
                "list" => self.tool_list(arguments),
                "show" => self.tool_show(arguments),
                "check" => Self::tool_check(arguments),
                "guide" => self.tool_guide(arguments),
                _ => Err(McpError::invalid_params(format!(
                    "Unknown tool: {}",
                    tool_name
                ))),
            }
        }

        fn load_diagnostics_tool_result(load_err: lemma::Errors) -> serde_json::Value {
            for e in load_err.iter() {
                error!(
                    "{}",
                    crate::error_formatter::format_error(e, &load_err.sources)
                );
            }
            let diagnostics: Vec<lemma::EngineError> = load_err
                .errors
                .iter()
                .map(lemma::EngineError::from)
                .collect();
            let text = serde_json::to_string_pretty(&diagnostics)
                .expect("BUG: EngineError diagnostics must serialize");
            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }],
                "isError": true
            })
        }

        fn append_unit_map(output: &mut String, map: &std::collections::BTreeMap<String, String>) {
            let parts: Vec<String> = map
                .iter()
                .map(|(unit, magnitude)| format!("{unit} {magnitude}"))
                .collect();
            output.push_str(" (");
            output.push_str(&parts.join(", "));
            output.push(')');
        }

        fn tool_add_spec(
            &mut self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let (code, source_type) = Self::parse_code_and_source(args)?;
            let path = self.disk_path(&source_type)?;

            if let Err(load_err) = self.engine.load([(source_type.clone(), code.to_string())]) {
                return Ok(Self::load_diagnostics_tool_result(load_err));
            }

            let formatted = Self::formatted_persist_source(code, &source_type);
            if let Err(e) = atomic_write(&path, &formatted) {
                self.rollback_added_source(code, &source_type);
                return Err(McpError::internal_error(format!(
                    "Failed to persist source to {}: {e}",
                    path.display()
                )));
            }

            info!("Spec added from source '{source_type}'");

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": "Spec added successfully."
                }]
            }))
        }

        fn tool_update_spec(
            &mut self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let spec = args["spec"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| McpError::invalid_params("Missing 'spec' field".to_string()))?;

            let (code, source_type) = Self::parse_code_and_source(args)?;
            let path = self.disk_path(&source_type)?;

            let repository = args["repository"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let effective = match args.get("effective").and_then(|v| v.as_str()) {
                None => None,
                Some(raw) => Some(
                    lemma::resolve_effective(Some(raw))
                        .map_err(|e| McpError::invalid_params(e.to_string()))?,
                ),
            };

            let rollback_snapshot =
                match self
                    .engine
                    .source(repository, Some(spec), effective.as_ref())
                {
                    Ok(old_code) => {
                        let show = self
                            .engine
                            .show(repository, spec, effective.as_ref())
                            .expect("BUG: source succeeded so show must succeed for same identity");
                        let old_source_type = show.source_type.expect(
                            "BUG: loaded spec must carry source_type for update persist rollback",
                        );
                        Some((old_source_type, old_code))
                    }
                    Err(_) => None,
                };

            if let Err(load_err) = self.engine.update(
                repository,
                spec,
                effective.as_ref(),
                source_type.clone(),
                code.to_string(),
            ) {
                return Ok(Self::load_diagnostics_tool_result(load_err));
            }

            let formatted = Self::formatted_persist_source(code, &source_type);
            if let Err(e) = atomic_write(&path, &formatted) {
                let (old_source_type, old_code) = rollback_snapshot
                    .expect("BUG: successful update with persist requires rollback snapshot");
                self.engine
                    .update(
                        repository,
                        spec,
                        effective.as_ref(),
                        old_source_type,
                        old_code,
                    )
                    .expect("BUG: restore previous spec after failed persist must succeed");
                return Err(McpError::internal_error(format!(
                    "Failed to persist source to {}: {e}",
                    path.display()
                )));
            }

            info!("Spec '{spec}' updated from source '{source_type}'");

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": "Spec updated successfully."
                }]
            }))
        }

        /// Remove every loaded temporal row whose `show.source_type` equals `source_type`.
        fn remove_all_with_source_type(&mut self, source_type: &lemma::SourceType) {
            let mut removals: Vec<(Option<String>, String, Option<DateTimeValue>)> = Vec::new();
            for repo_group in self.engine.list() {
                let repo = repo_group.repository.clone();
                for listed in repo_group.specs {
                    let show = self
                        .engine
                        .show(
                            repo.as_deref(),
                            &listed.name,
                            listed.effective_from.as_ref(),
                        )
                        .expect("BUG: listed spec must show for source_type scan");
                    if show.source_type.as_ref() == Some(source_type) {
                        removals.push((repo.clone(), listed.name, listed.effective_from));
                    }
                }
            }
            for (repo, name, eff) in removals {
                self.engine
                    .remove(repo.as_deref(), &name, eff.as_ref())
                    .expect("BUG: remove of previously listed source_type row must succeed");
            }
        }

        fn tool_remove_spec(
            &mut self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let spec = args["spec"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| McpError::invalid_params("Missing 'spec' field".to_string()))?;

            let repository = args["repository"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let effective = match args.get("effective").and_then(|v| v.as_str()) {
                None => None,
                Some(raw) => Some(
                    lemma::resolve_effective(Some(raw))
                        .map_err(|e| McpError::invalid_params(e.to_string()))?,
                ),
            };

            let show = match self.engine.show(repository, spec, effective.as_ref()) {
                Ok(show) => show,
                Err(err) => {
                    error!("{}", err);
                    let diagnostics = vec![lemma::EngineError::from(&err)];
                    let text = serde_json::to_string_pretty(&diagnostics)
                        .expect("BUG: EngineError diagnostics must serialize");
                    return Ok(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": text
                        }],
                        "isError": true
                    }));
                }
            };

            let source_type = show
                .source_type
                .expect("BUG: loaded spec must carry source_type for remove");
            let path = self.disk_path(&source_type)?;
            let file_snapshot = fs::read_to_string(&path).map_err(|e| {
                McpError::internal_error(format!(
                    "Failed to snapshot {} before remove: {e}",
                    path.display()
                ))
            })?;

            // Sibling temporal rows that share this Path (exclude the row about to be removed).
            let mut siblings: Vec<(Option<String>, String, Option<DateTimeValue>)> = Vec::new();
            for repo_group in self.engine.list() {
                let repo = repo_group.repository.clone();
                for listed in repo_group.specs {
                    let same_identity = repo.as_deref() == repository
                        && listed.name == spec
                        && listed.effective_from == effective;
                    if same_identity {
                        continue;
                    }
                    let sibling_show = self
                        .engine
                        .show(
                            repo.as_deref(),
                            &listed.name,
                            listed.effective_from.as_ref(),
                        )
                        .expect("BUG: listed sibling must show");
                    if sibling_show.source_type.as_ref() == Some(&source_type) {
                        siblings.push((repo.clone(), listed.name, listed.effective_from));
                    }
                }
            }

            if let Err(err) = self.engine.remove(repository, spec, effective.as_ref()) {
                error!("{}", err);
                let diagnostics = vec![lemma::EngineError::from(&err)];
                let text = serde_json::to_string_pretty(&diagnostics)
                    .expect("BUG: EngineError diagnostics must serialize");
                return Ok(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": text
                    }],
                    "isError": true
                }));
            }

            let disk_result = if siblings.is_empty() {
                match fs::remove_file(&path) {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e),
                }
            } else {
                let mut pieces: Vec<String> = Vec::with_capacity(siblings.len());
                for (repo, name, eff) in &siblings {
                    let piece = self
                        .engine
                        .source(repo.as_deref(), Some(name), eff.as_ref())
                        .expect("BUG: sibling remaining after remove must have source");
                    pieces.push(piece);
                }
                atomic_write(&path, &pieces.join("\n\n"))
            };

            if let Err(e) = disk_result {
                self.remove_all_with_source_type(&source_type);
                self.engine
                    .load([(source_type, file_snapshot)])
                    .expect("BUG: restore file after failed remove persist must succeed");
                return Err(McpError::internal_error(format!(
                    "Failed to persist remove to {}: {e}",
                    path.display()
                )));
            }

            info!("Spec '{spec}' removed");

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!("Spec '{spec}' removed.")
                }]
            }))
        }

        fn tool_clear(&mut self, _args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            for path in Self::workspace_lemma_files(&self.workdir)? {
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(McpError::internal_error(format!(
                            "Failed to delete {}: {e}",
                            path.display()
                        )));
                    }
                }
            }

            self.engine = Engine::new();
            info!("Engine cleared");

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": "Removed all specs."
                }]
            }))
        }

        /// Every `.lemma` file under `workdir` (or `workdir` itself when it is a file).
        fn workspace_lemma_files(workdir: &Path) -> Result<Vec<PathBuf>, McpError> {
            if workdir.is_file() {
                return Ok(vec![workdir.to_path_buf()]);
            }
            if !workdir.is_dir() {
                return Ok(Vec::new());
            }
            let mut paths = Vec::new();
            for entry in walkdir::WalkDir::new(workdir) {
                let entry = entry.map_err(|e| {
                    McpError::internal_error(format!("Failed to walk workspace during clear: {e}"))
                })?;
                if entry.file_type().is_file()
                    && entry.path().extension().and_then(|s| s.to_str()) == Some("lemma")
                {
                    paths.push(entry.path().to_path_buf());
                }
            }
            Ok(paths)
        }

        fn dependency_in_engine(engine: &Engine, dependency: &str) -> bool {
            engine
                .list()
                .iter()
                .any(|repo| repo.repository.as_deref() == Some(dependency))
        }

        /// Remove every temporal row currently loaded under `dependency`.
        fn unload_dependency(engine: &mut Engine, dependency: &str) {
            let rows: Vec<(String, Option<DateTimeValue>)> = engine
                .list()
                .into_iter()
                .find(|repo| repo.repository.as_deref() == Some(dependency))
                .map(|repo| {
                    repo.specs
                        .into_iter()
                        .map(|spec| (spec.name, spec.effective_from))
                        .collect()
                })
                .unwrap_or_default();
            for (name, effective) in rows {
                engine
                    .remove(Some(dependency), &name, effective.as_ref())
                    .expect("BUG: unload of previously listed dependency row must succeed");
            }
        }

        fn tool_install(
            &mut self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let dependency = args["dependency"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    McpError::invalid_params("Missing 'dependency' field".to_string())
                })?;

            let force = match args.get("force") {
                None => false,
                Some(v) => v.as_bool().ok_or_else(|| {
                    McpError::invalid_params("'force' must be a boolean".to_string())
                })?,
            };

            let source_type = lemma::SourceType::Dependency(dependency.to_string());
            let outcome = lemma_cli::install::install_registry_dependency(
                &self.workdir,
                dependency,
                force,
                self.registry.as_ref(),
            );

            let (relative_path, source, freshly_written) = match outcome {
                Ok(lemma_cli::install::InstallOutcome::AlreadyUpToDate {
                    relative_path,
                    source,
                }) => (relative_path, source, false),
                Ok(lemma_cli::install::InstallOutcome::Written {
                    relative_path,
                    source,
                }) => (relative_path, source, true),
                Err(lemma_cli::install::InstallError::Plan(load_err)) => {
                    return Ok(Self::load_diagnostics_tool_result(load_err));
                }
                Err(lemma_cli::install::InstallError::Registry(error)) => {
                    return Err(McpError::internal_error(format!(
                        "Registry error for {dependency}: {}",
                        error.message
                    )));
                }
                Err(lemma_cli::install::InstallError::UnparseableRegistry(error)) => {
                    return Err(McpError::internal_error(format!(
                        "Registry returned unparseable dependency: {error}"
                    )));
                }
                Err(
                    error @ (lemma_cli::install::InstallError::Conflict { .. }
                    | lemma_cli::install::InstallError::Io(_)
                    | lemma_cli::install::InstallError::Workspace(_)),
                ) => {
                    return Err(McpError::invalid_params(error.to_string()));
                }
            };

            let already_loaded = Self::dependency_in_engine(&self.engine, dependency);
            let rollback_source = if already_loaded {
                Some(
                    self.engine
                        .source(Some(dependency), None, None)
                        .map_err(|e| {
                            McpError::internal_error(format!(
                                "Failed to snapshot existing dependency '{dependency}' for rollback: {e}"
                            ))
                        })?,
                )
            } else {
                None
            };

            if already_loaded {
                Self::unload_dependency(&mut self.engine, dependency);
            }

            if let Err(load_err) = self.engine.load([(source_type, source)]) {
                if let Some(old) = rollback_source.as_ref() {
                    self.engine
                        .load([(
                            lemma::SourceType::Dependency(dependency.to_string()),
                            old.clone(),
                        )])
                        .expect("BUG: restore previous dependency after failed load must succeed");
                }
                return Ok(Self::load_diagnostics_tool_result(load_err));
            }

            let message = if freshly_written {
                format!("Installed {} -> {}", dependency, relative_path.display())
            } else {
                format!(
                    "Already up to date: {} -> {}",
                    dependency,
                    relative_path.display()
                )
            };
            info!("{message}");
            Ok(serde_json::json!({
                "content": [{ "type": "text", "text": message }]
            }))
        }

        fn parse_code_and_source(
            args: &serde_json::Value,
        ) -> Result<(&str, lemma::SourceType), McpError> {
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
                .ok_or_else(|| McpError::invalid_params("Missing 'source_id' field".to_string()))?;

            let source_type = lemma::SourceType::from_binding_label(source_id)
                .map_err(McpError::invalid_params)?;

            Ok((code, source_type))
        }

        fn tool_check(args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            let sources_arr = args
                .get("sources")
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    McpError::invalid_params("Missing 'sources' array field".to_string())
                })?;

            if sources_arr.is_empty() {
                return Err(McpError::invalid_params(
                    "'sources' must be a non-empty array of [label, code] pairs".to_string(),
                ));
            }

            let mut sources: Vec<(lemma::SourceType, String)> =
                Vec::with_capacity(sources_arr.len());
            for (i, entry) in sources_arr.iter().enumerate() {
                let pair = entry.as_array().ok_or_else(|| {
                    McpError::invalid_params(format!("sources[{i}] must be a [label, code] array"))
                })?;
                if pair.len() != 2 {
                    return Err(McpError::invalid_params(format!(
                        "sources[{i}] must have exactly 2 elements [label, code]"
                    )));
                }
                let label = pair[0].as_str().ok_or_else(|| {
                    McpError::invalid_params(format!("sources[{i}][0] (label) must be a string"))
                })?;
                let code = pair[1].as_str().ok_or_else(|| {
                    McpError::invalid_params(format!("sources[{i}][1] (code) must be a string"))
                })?;
                let source_type = lemma::SourceType::from_binding_label(label)
                    .map_err(McpError::invalid_params)?;
                sources.push((source_type, code.to_string()));
            }

            let mut engine = Engine::new();
            if let Err(load_err) = engine.load(sources) {
                return Ok(Self::load_diagnostics_tool_result(load_err));
            }

            let recommendations = engine.quality();
            let mut text = String::from(
                "Parsed and planned. Syntax is valid; this does not verify the policy is correct.",
            );
            const MAX_RECOMMENDATIONS: usize = 20;
            if !recommendations.is_empty() {
                text.push_str("\n\nRecommendations:");
                for (i, rec) in recommendations.iter().take(MAX_RECOMMENDATIONS).enumerate() {
                    text.push_str(&format!("\n{}. {}", i + 1, rec));
                }
                let omitted = recommendations.len().saturating_sub(MAX_RECOMMENDATIONS);
                if omitted > 0 {
                    text.push_str(&format!("\n… and {omitted} more"));
                }
            }

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }

        fn tool_guide(&self, args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            let text = match args.get("topic").and_then(|v| v.as_str()) {
                None => crate::mcp::guide::EVALUATE_GUIDE,
                Some(topic_name) => {
                    let topic =
                        crate::mcp::guide::GuideTopic::parse(topic_name).ok_or_else(|| {
                            McpError::invalid_params(format!(
                                "Unknown guide topic '{topic_name}'. Valid: {}",
                                crate::mcp::guide::GuideTopic::VALID_LIST
                            ))
                        })?;
                    topic.section_text()
                }
            };
            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            }))
        }

        fn tool_source(&self, args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            if let Some(repo) = args
                .get("repository")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let source = self.engine.source(Some(repo), None, None).map_err(|e| {
                    McpError::invalid_params(format!(
                        "Repository '{}' not found: {}. Use list to see loaded repositories.",
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
            let source = self
                .engine
                .source(None, Some(&spec_name), Some(&now))
                .map_err(|e| {
                    McpError::invalid_params(format!(
                        "Spec '{}' not found: {}. Use list to see available specs.",
                        spec_set_id, e
                    ))
                })?;

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

            let rules = if rule_names.is_empty() {
                None
            } else {
                Some(rule_names.as_slice())
            };
            let response = self
                .engine
                .run(None, &spec_name, Some(&now), data_values, rules, true)
                .map_err(|e| {
                    error!("Evaluation failed: {}", e);
                    McpError::internal_error(format!("Evaluation failed: {e}"))
                })?;

            let show_for_missing = if response
                .results
                .values()
                .any(|result| !result.missing_data.is_empty())
            {
                Some(
                    self.engine
                        .show(None, &spec_name, Some(&now))
                        .map_err(|e| {
                            error!(
                                "show failed after evaluate for '{}': {}",
                                spec_set_id.trim(),
                                e
                            );
                            McpError::internal_error(format!(
                                "Failed to show spec for missing_data: {e}"
                            ))
                        })?,
                )
            } else {
                None
            };

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
                    let display = result.display().ok_or_else(|| {
                        McpError::internal_error(format!(
                            "Rule '{}' evaluated without display after evaluation",
                            result.rule.name
                        ))
                    })?;
                    output.push_str(display);
                    if let Some(value) = &result.value {
                        if let Some(measure) = &value.measure {
                            Self::append_unit_map(&mut output, measure);
                        } else if let Some(ratio) = &value.ratio {
                            Self::append_unit_map(&mut output, ratio);
                        }
                    }
                }
                output.push('\n');

                if !result.missing_data.is_empty() {
                    let show = show_for_missing
                        .as_ref()
                        .expect("BUG: missing_data nonempty so show_for_missing must be Some");
                    output.push_str("missing_data:\n");
                    for name in &result.missing_data {
                        let entry = show.data.get(name).unwrap_or_else(|| {
                            panic!(
                                "BUG: missing_data key {name:?} must exist in show.data after evaluate"
                            )
                        });
                        let type_name = entry.lemma_type.specifications.to_string();
                        let help = entry.lemma_type.specifications.help();
                        if help.is_empty() {
                            output.push_str(&format!("  {name}: {type_name}\n"));
                        } else {
                            output.push_str(&format!("  {name}: {type_name} — {help}\n"));
                        }
                    }
                }

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

        fn tool_list(&self, _args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            let list = self.engine.list();
            let mut output = serde_json::to_string_pretty(&list)
                .map_err(|e| McpError::internal_error(format!("Failed to serialize list: {e}")))?;

            let workspace_empty = self
                .engine
                .list()
                .into_iter()
                .find(|repository_group| repository_group.repository.is_none())
                .expect("BUG: workspace repository must exist after Engine::new")
                .specs
                .is_empty();
            if self.config.admin && workspace_empty {
                output.push_str("\n\nUse the 'add_spec' tool to load workspace Lemma source.");
            }

            let spec_count: usize = list.iter().map(|r| r.specs.len()).sum();
            debug!("Listed {} spec rows across repositories", spec_count);

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }

        fn tool_show(&self, args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
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

            let show = self
                .engine
                .show(None, &spec_name, Some(&now))
                .map_err(|e| {
                    error!("show failed for '{}': {}", spec_set_id.trim(), e);
                    McpError::internal_error(format!("Failed to show spec: {e}"))
                })?;

            let output = serde_json::to_string_pretty(&show).map_err(|e| {
                McpError::internal_error(format!("Failed to serialize show response: {e}"))
            })?;

            info!(
                "Returned show for '{}' ({} data, {} rules)",
                spec_set_id.trim(),
                show.data.len(),
                show.rules.len()
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

    pub fn start_server(engine: Engine, config: McpConfig, workdir: &Path) -> Result<()> {
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
        let workdir = workdir.to_path_buf();

        // Requests are handled on a dedicated worker thread that owns the
        // engine state, so the reader loop can enforce a wall-clock timeout
        // per request. The worker sends exactly one response per request; a
        // timed-out request's late response is counted in `abandoned` and
        // discarded when it eventually arrives.
        let (request_tx, request_rx) = std::sync::mpsc::channel::<McpRequest>();
        let (response_tx, response_rx) = std::sync::mpsc::channel::<Option<McpResponse>>();
        std::thread::spawn(move || {
            let mut server =
                McpServer::new(engine, config, workdir, Box::new(lemma::LemmaBase::new()));
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
            McpServer::new(
                Engine::new(),
                McpConfig::default(),
                std::env::temp_dir(),
                Box::new(lemma::LemmaBase::test()),
            )
        }

        fn admin_server(workdir: PathBuf) -> McpServer {
            McpServer::new(
                Engine::new(),
                McpConfig {
                    admin: true,
                    ..McpConfig::default()
                },
                workdir,
                Box::new(lemma::LemmaBase::test()),
            )
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
        fn initialize_advertises_tools_and_resources() {
            let mut s = server();
            let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
            let resp = s.handle_request(req).expect("request must yield response");
            let result = resp.result.expect("result expected");
            assert_eq!(result["capabilities"]["tools"]["listChanged"], false);
            assert_eq!(result["capabilities"]["resources"]["listChanged"], false);
        }

        fn tools_call(id: u64, name: &str, arguments: serde_json::Value) -> McpRequest {
            McpRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(serde_json::json!(id)),
                method: "tools/call".to_string(),
                params: Some(serde_json::json!({
                    "name": name,
                    "arguments": arguments,
                })),
            }
        }

        #[test]
        fn install_writes_file_and_loads() {
            let dir = tempfile::tempdir().unwrap();
            let mut s = admin_server(dir.path().to_path_buf());
            let resp = s
                .handle_request(tools_call(
                    1,
                    "install",
                    serde_json::json!({ "dependency": "@iso/countries" }),
                ))
                .expect("response");
            let text = resp.result.as_ref().unwrap()["content"][0]["text"]
                .as_str()
                .unwrap();
            assert!(text.contains("Installed @iso/countries"), "got: {text}");
            let dep_path = dir.path().join("lemma_deps/@iso/countries.lemma");
            assert!(dep_path.exists(), "must write {}", dep_path.display());
            assert!(fs::read_to_string(&dep_path)
                .unwrap()
                .contains("spec alpha2"));

            let list = s
                .handle_request(tools_call(2, "list", serde_json::json!({})))
                .expect("list");
            let list_text = list.result.as_ref().unwrap()["content"][0]["text"]
                .as_str()
                .unwrap();
            assert!(
                list_text.contains("@iso/countries") && list_text.contains("alpha2"),
                "got: {list_text}"
            );
        }

        #[test]
        fn install_force_must_be_bool() {
            let dir = tempfile::tempdir().unwrap();
            let mut s = admin_server(dir.path().to_path_buf());
            for bad in [serde_json::json!("true"), serde_json::json!(1)] {
                let resp = s
                    .handle_request(tools_call(
                        1,
                        "install",
                        serde_json::json!({ "dependency": "@iso/countries", "force": bad }),
                    ))
                    .expect("response");
                let err = resp.error.as_ref().expect("invalid_params");
                assert_eq!(err.code, -32602);
                assert!(err.message.contains("force"), "got: {}", err.message);
            }
        }

        #[test]
        fn install_skips_unchanged() {
            let dir = tempfile::tempdir().unwrap();
            let mut s = admin_server(dir.path().to_path_buf());
            let first = s
                .handle_request(tools_call(
                    1,
                    "install",
                    serde_json::json!({ "dependency": "@iso/countries" }),
                ))
                .expect("response");
            assert!(first.result.as_ref().unwrap()["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Installed"));

            let second = s
                .handle_request(tools_call(
                    2,
                    "install",
                    serde_json::json!({ "dependency": "@iso/countries" }),
                ))
                .expect("response");
            let skip = second.result.as_ref().unwrap()["content"][0]["text"]
                .as_str()
                .unwrap();
            assert!(skip.contains("Already up to date"), "got: {skip}");
        }

        #[test]
        fn install_missing_registry_spec_errors() {
            let dir = tempfile::tempdir().unwrap();
            let mut s = admin_server(dir.path().to_path_buf());
            let resp = s
                .handle_request(tools_call(
                    1,
                    "install",
                    serde_json::json!({ "dependency": "@org/does-not-exist" }),
                ))
                .expect("response");
            let err = resp.error.as_ref().expect("registry miss");
            assert!(
                err.message.contains("@org/does-not-exist")
                    || err.message.contains("Registry error"),
                "got: {}",
                err.message
            );
            assert!(!dir
                .path()
                .join("lemma_deps/@org/does-not-exist.lemma")
                .exists());
        }

        #[test]
        fn install_non_at_id_reaches_registry() {
            let dir = tempfile::tempdir().unwrap();
            let mut s = admin_server(dir.path().to_path_buf());
            let resp = s
                .handle_request(tools_call(
                    1,
                    "install",
                    serde_json::json!({ "dependency": "not-a-registry-id" }),
                ))
                .expect("response");
            let err = resp.error.as_ref().expect("registry miss");
            assert!(
                err.message.contains("Registry error")
                    || err.message.contains("must start with '@'")
                    || err.message.contains("not-a-registry-id"),
                "got: {}",
                err.message
            );
        }

        #[test]
        fn install_identical_content_elsewhere_conflicts_without_force() {
            let dir = tempfile::tempdir().unwrap();
            let fixture = fs::read_to_string(
                lemma::LemmaBase::test_fixtures_dir()
                    .join("@iso")
                    .join("countries.lemma"),
            )
            .unwrap();
            let other = dir.path().join("lemma_deps/@other/copy.lemma");
            fs::create_dir_all(other.parent().unwrap()).unwrap();
            fs::write(&other, &fixture).unwrap();

            let mut s = admin_server(dir.path().to_path_buf());
            let resp = s
                .handle_request(tools_call(
                    1,
                    "install",
                    serde_json::json!({ "dependency": "@iso/countries" }),
                ))
                .expect("response");
            let err = resp.error.expect("overlapping foreign copy must conflict");
            assert!(
                err.message.contains("already exists") || err.message.contains("force"),
                "got: {}",
                err.message
            );
            assert!(!dir.path().join("lemma_deps/@iso/countries.lemma").exists());
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

        #[test]
        fn validated_write_path_accepts_relative_file() {
            let workdir = Path::new("/tmp/lemma-workspace");
            let path = validated_write_path(workdir, Path::new("pricing.lemma"))
                .expect("relative path must be accepted");
            assert_eq!(path, PathBuf::from("/tmp/lemma-workspace/pricing.lemma"));
        }

        #[test]
        fn validated_write_path_rejects_parent_dir() {
            let err = validated_write_path(Path::new("/tmp/ws"), Path::new("../escape.lemma"))
                .expect_err(".. must be rejected");
            assert!(err.message.contains(".."));
        }

        #[test]
        fn validated_write_path_rejects_absolute() {
            let err = validated_write_path(Path::new("/tmp/ws"), Path::new("/etc/passwd"))
                .expect_err("absolute path must be rejected");
            assert!(err.message.contains("relative"));
        }

        #[test]
        fn atomic_write_round_trip() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("out.lemma");
            atomic_write(&path, "spec x\ndata a: 1\n").unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), "spec x\ndata a: 1\n");
            assert!(
                !dir.path().join(".out.lemma.tmp").exists(),
                "temp file must be cleaned up"
            );
        }
    }
}

pub use imp::start_server;
pub use imp::McpConfig;
