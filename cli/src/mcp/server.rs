mod imp {
    use anyhow::Result;
    use lemma::mcp::ToolError;
    use lemma::DateTimeValue;
    use lemma::Engine;
    use serde::{Deserialize, Serialize};
    use std::fs;
    use std::io::{self, BufRead, Write};
    use std::path::{Component, Path, PathBuf};
    use std::time::Duration;
    use tracing::{debug, error, info};

    const PROTOCOL_VERSION: &str = "2026-07-28";
    /// Legacy handshake revision spoken via `initialize` (`2025-11-25` and earlier).
    const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
    const UNSUPPORTED_PROTOCOL_VERSION: i32 = -32022;
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

        fn unsupported_protocol_version(requested: &str) -> Self {
            Self {
                code: UNSUPPORTED_PROTOCOL_VERSION,
                message: format!("Unsupported protocol version: {requested}"),
                data: Some(serde_json::json!({
                    "supported": [PROTOCOL_VERSION],
                    "requested": requested,
                })),
            }
        }
    }

    fn requested_protocol_version(params: &Option<serde_json::Value>) -> Option<&str> {
        params
            .as_ref()
            .and_then(|value| value.get("_meta"))
            .and_then(|meta| meta.get("io.modelcontextprotocol/protocolVersion"))
            .and_then(|value| value.as_str())
    }

    fn server_info() -> serde_json::Value {
        serde_json::json!({
            "name": "lemma-mcp-server",
            "version": SERVER_VERSION
        })
    }

    fn with_server_meta(mut result: serde_json::Value) -> serde_json::Value {
        let meta = result
            .as_object_mut()
            .expect("BUG: MCP result must be an object")
            .entry("_meta")
            .or_insert_with(|| serde_json::json!({}));
        meta.as_object_mut()
            .expect("BUG: result _meta must be an object")
            .insert(
                "io.modelcontextprotocol/serverInfo".to_string(),
                server_info(),
            );
        result
    }

    fn text_content(text: String) -> serde_json::Value {
        serde_json::json!({
            "content": [{
                "type": "text",
                "text": text
            }]
        })
    }

    fn map_tool_result(result: Result<String, ToolError>) -> Result<serde_json::Value, McpError> {
        match result {
            Ok(text) => Ok(text_content(text)),
            Err(ToolError::Diagnostics(text)) => Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }],
                "isError": true
            })),
            Err(ToolError::InvalidArguments(message) | ToolError::NotFound(message)) => {
                Err(McpError::invalid_params(message))
            }
        }
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
        /// Set after a successful legacy `initialize`. Process-scoped (stdio lifetime).
        legacy_session: bool,
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
                legacy_session: false,
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
                debug!("Ignoring notification: {}", request.method);
                return None;
            }

            // Legacy `initialize` is not gated on `_meta`. It selects legacy
            // semantics for the rest of this stdio process.
            if request.method == "initialize" {
                return Some(match self.initialize(&request.params) {
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
                });
            }

            match requested_protocol_version(&request.params) {
                Some(PROTOCOL_VERSION) => {}
                Some(other) => {
                    return Some(McpResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request.id,
                        result: None,
                        error: Some(McpError::unsupported_protocol_version(other)),
                    });
                }
                None if self.legacy_session => {}
                None => {
                    return Some(McpResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request.id,
                        result: None,
                        error: Some(McpError::invalid_params(
                            "Missing required _meta.io.modelcontextprotocol/protocolVersion (or call initialize for a legacy session)"
                                .to_string(),
                        )),
                    });
                }
            }

            let result = match request.method.as_str() {
                "server/discover" => self.discover(),
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
                    result: Some(with_server_meta(result)),
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

        fn initialize(
            &mut self,
            params: &Option<serde_json::Value>,
        ) -> Result<serde_json::Value, McpError> {
            params
                .as_ref()
                .and_then(|value| value.get("protocolVersion"))
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    McpError::invalid_params("Missing params.protocolVersion".to_string())
                })?;

            // `initialize` always negotiates legacy. Asking for the modern
            // revision (or any older legacy) still yields LEGACY_PROTOCOL_VERSION.
            self.legacy_session = true;

            Ok(serde_json::json!({
                "protocolVersion": LEGACY_PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": server_info()
            }))
        }

        fn discover(&self) -> Result<serde_json::Value, McpError> {
            Ok(serde_json::json!({
                "resultType": "complete",
                "supportedVersions": [PROTOCOL_VERSION],
                "capabilities": {
                    "tools": {},
                    "resources": {}
                }
            }))
        }

        fn list_tools(&self) -> Result<serde_json::Value, McpError> {
            debug!("Listing tools");

            let mut catalog = serde_json::to_value(lemma::mcp::list_tools())
                .unwrap_or_else(|error| panic!("BUG: MCP tool catalog must serialize: {error}"));
            let tools = catalog
                .as_array_mut()
                .expect("BUG: lemma::mcp::list_tools serializes as an array");

            if self.config.admin {
                tools.push(serde_json::json!({
                    "name": "add_spec",
                    "description": "Load Lemma source as one or more specs (persists). Prefer check first; check alone does not load. On failure returns structured diagnostics. After success, call source and present that formatted text in chat for user verify; do not paste the unformatted draft.",
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
                    "description": "Replace an existing spec with new source. After success, call source and present that formatted text in chat for user verify; do not paste the unformatted draft.",
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

            Ok(serde_json::json!({ "tools": catalog }))
        }

        fn list_resources(&self) -> Result<serde_json::Value, McpError> {
            Ok(serde_json::json!({ "resources": lemma::mcp::list_resources() }))
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
            let text = lemma::mcp::read_resource(uri)
                .map_err(|error| McpError::invalid_params(error.to_string()))?;
            Ok(serde_json::json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "text/plain",
                    "text": text
                }]
            }))
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
                "evaluate" => map_tool_result(lemma::mcp::evaluate(&self.engine, arguments)),
                "list" => self.tool_list(arguments),
                "show" => map_tool_result(lemma::mcp::show(&self.engine, arguments)),
                "source" => map_tool_result(lemma::mcp::source(&self.engine, arguments)),
                "check" => map_tool_result(lemma::mcp::check(arguments)),
                "guide" => map_tool_result(lemma::mcp::guide(arguments)),
                other => panic!("BUG: unknown MCP tool {other}"),
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

        fn tool_list(&self, args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            let mut output = match lemma::mcp::list(&self.engine, args) {
                Ok(text) => text,
                Err(error) => return map_tool_result(Err(error)),
            };

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

            Ok(text_content(output))
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

        fn request_meta() -> serde_json::Value {
            serde_json::json!({
                "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION,
                "io.modelcontextprotocol/clientInfo": {
                    "name": "lemma-mcp-test",
                    "version": "0"
                },
                "io.modelcontextprotocol/clientCapabilities": {}
            })
        }

        fn parse(line: &str) -> McpRequest {
            let mut value: serde_json::Value =
                serde_json::from_str(line).expect("test fixture must be valid JSON-RPC");
            if value.get("id").is_some() {
                let params = value
                    .as_object_mut()
                    .expect("request object")
                    .entry("params")
                    .or_insert_with(|| serde_json::json!({}));
                if let Some(params) = params.as_object_mut() {
                    params.entry("_meta").or_insert_with(request_meta);
                }
            }
            serde_json::from_value(value).expect("test fixture must be valid JSON-RPC")
        }

        /// Deserialize the wire JSON as sent. Does not inject `_meta`.
        fn parse_as_sent(line: &str) -> McpRequest {
            serde_json::from_str(line).expect("test fixture must be valid JSON-RPC")
        }

        fn assert_legacy_initialize_result(resp: &McpResponse, expected_id: serde_json::Value) {
            assert_eq!(resp.id, Some(expected_id));
            assert!(
                resp.error.is_none(),
                "initialize must not error: {:?}",
                resp.error
            );
            let result = resp.result.as_ref().expect("InitializeResult");
            let negotiated = result["protocolVersion"].as_str().expect("protocolVersion");
            assert_eq!(
                negotiated, "2025-11-25",
                "initialize must negotiate legacy 2025-11-25, got: {negotiated}"
            );
            assert!(result["capabilities"]["tools"].is_object());
            assert!(result["capabilities"]["resources"].is_object());
            assert_eq!(result["serverInfo"]["name"], "lemma-mcp-server");
            assert!(result["serverInfo"]["version"]
                .as_str()
                .is_some_and(|v| !v.is_empty()));
        }

        #[test]
        fn initialize_without_meta_returns_legacy_result() {
            let mut s = server();
            let req = parse_as_sent(
                r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"lemma-mcp-test","version":"0"}}}"#,
            );
            let resp = s
                .handle_request(req)
                .expect("initialize must yield response");
            assert_legacy_initialize_result(&resp, serde_json::json!(0));
            assert_eq!(
                resp.result.as_ref().unwrap()["protocolVersion"],
                "2025-11-25"
            );
        }

        #[test]
        fn initialize_with_modern_meta_still_returns_legacy_result() {
            let mut s = server();
            let req = parse_as_sent(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"lemma-mcp-test","version":"0"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"lemma-mcp-test","version":"0"}}}}"#,
            );
            let resp = s
                .handle_request(req)
                .expect("initialize must yield response");
            assert_legacy_initialize_result(&resp, serde_json::json!(1));
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
            let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#);
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
            let req = parse(r#"{"jsonrpc":"1.0","id":2,"method":"server/discover"}"#);
            let resp = s.handle_request(req).expect("request must yield response");
            assert_eq!(resp.error.as_ref().expect("error expected").code, -32600);
        }

        #[test]
        fn discover_advertises_tools_and_resources() {
            let mut s = server();
            let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#);
            let resp = s.handle_request(req).expect("request must yield response");
            let result = resp.result.expect("result expected");
            assert_eq!(result["supportedVersions"][0], PROTOCOL_VERSION);
            assert!(result["capabilities"]["tools"].is_object());
            assert!(result["capabilities"]["resources"].is_object());
            assert_eq!(
                result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
                "lemma-mcp-server"
            );
        }

        fn tools_call(id: u64, name: &str, arguments: serde_json::Value) -> McpRequest {
            McpRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(serde_json::json!(id)),
                method: "tools/call".to_string(),
                params: Some(serde_json::json!({
                    "name": name,
                    "arguments": arguments,
                    "_meta": request_meta(),
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
