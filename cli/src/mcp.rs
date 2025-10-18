#[cfg(feature = "mcp")]
pub mod server {
    use anyhow::Result;
    use lemma::Engine;
    use serde::{Deserialize, Serialize};
    use std::io::{self, BufRead, Write};
    use tracing::{debug, error, info};

    const PROTOCOL_VERSION: &str = "2024-11-05";
    const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

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
                message: format!("Method not found: {}", method),
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

    struct McpServer {
        engine: Engine,
    }

    impl McpServer {
        fn new(engine: Engine) -> Self {
            Self { engine }
        }

        fn handle_request(&mut self, request: McpRequest) -> McpResponse {
            debug!("Handling request: method={}", request.method);

            if request.jsonrpc != "2.0" {
                return McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request.id,
                    result: None,
                    error: Some(McpError::invalid_request(
                        "Invalid JSON-RPC version, expected '2.0'".to_string(),
                    )),
                };
            }

            let result = match request.method.as_str() {
                "initialize" => self.initialize(),
                "tools/list" => self.list_tools(),
                "tools/call" => self.call_tool(request.params),
                _ => Err(McpError::method_not_found(request.method)),
            };

            match result {
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
            }
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
                    "tools": {}
                }
            }))
        }

        fn list_tools(&self) -> Result<serde_json::Value, McpError> {
            debug!("Listing tools");
            Ok(serde_json::json!({
                "tools": [
                    {
                        "name": "add_document",
                        "description": "Add a Lemma document to the engine. Provide the complete Lemma code and an optional identifier.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "code": {
                                    "type": "string",
                                    "description": "The complete Lemma code to add (e.g., 'doc example\\nfact x = 5\\nrule y = x * 2')"
                                },
                                "source_id": {
                                    "type": "string",
                                    "description": "Optional identifier for this document (will be auto-generated if not provided)"
                                }
                            },
                            "required": ["code"]
                        }
                    },
                    {
                        "name": "evaluate",
                        "description": "Evaluate all rules in a document with optional fact overrides. Returns computed values for all rules.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "document": {
                                    "type": "string",
                                    "description": "Name of the document to evaluate (from 'doc <name>' declaration)"
                                },
                                "facts": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Optional fact overrides in format 'name=value' (e.g., ['price=100', 'quantity=5'])",
                                    "default": []
                                },
                                "include_trace": {
                                    "type": "boolean",
                                    "description": "Include execution trace showing how each rule was evaluated",
                                    "default": false
                                }
                            },
                            "required": ["document"]
                        }
                    },
                    {
                        "name": "inspect",
                        "description": "Inspect a document's structure to see its facts and rules.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "document": {
                                    "type": "string",
                                    "description": "Name of the document to inspect"
                                }
                            },
                            "required": ["document"]
                        }
                    },
                    {
                        "name": "list_documents",
                        "description": "List all documents currently loaded in the engine.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {}
                        }
                    }
                ]
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
                "add_document" => self.tool_add_document(arguments),
                "evaluate" => self.tool_evaluate(arguments),
                "inspect" => self.tool_inspect(arguments),
                "list_documents" => self.tool_list_documents(),
                _ => Err(McpError::invalid_params(format!(
                    "Unknown tool: {}",
                    tool_name
                ))),
            }
        }

        fn tool_add_document(
            &mut self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let code = args["code"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'code' field".to_string()))?;

            if code.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Document code cannot be empty".to_string(),
                ));
            }

            let source_id = args["source_id"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| format!("doc_{}", chrono::Utc::now().timestamp_millis()));

            self.engine.add_lemma_code(code, &source_id).map_err(|e| {
                error!("Failed to add document: {}", e);
                McpError::internal_error(format!("Failed to parse document: {}", e))
            })?;

            info!("Document added: {}", source_id);

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!("Document added successfully\n\nSource ID: {}\n\nThe document has been parsed and loaded into the engine. You can now evaluate it using the 'evaluate' tool.", source_id)
                }]
            }))
        }

        fn tool_evaluate(
            &mut self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let document = args["document"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'document' field".to_string()))?;

            if document.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Document name cannot be empty".to_string(),
                ));
            }

            if self.engine.get_document(document).is_none() {
                return Err(McpError::invalid_params(format!(
                    "Document '{}' not found. Use list_documents to see available documents.",
                    document
                )));
            }

            let facts: Vec<&str> = args["facts"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            let include_trace = args["include_trace"].as_bool().unwrap_or(false);

            let response = self.engine.evaluate(document, facts).map_err(|e| {
                error!("Evaluation failed: {}", e);
                McpError::internal_error(format!("Evaluation failed: {}", e))
            })?;

            let mut output = String::new();
            output.push_str(&format!(
                "Evaluation complete for document '{}'\n\n",
                document
            ));

            if !response.results.is_empty() {
                output.push_str("## Results\n\n");
                for result in &response.results {
                    output.push_str(&format!("**{}**: ", result.rule_name));
                    if let Some(ref value) = result.result {
                        output.push_str(&value.to_string());
                    } else if let Some(ref veto) = result.veto_message {
                        output.push_str(&format!("VETO ({})", veto));
                    } else {
                        output.push_str("(no value)");
                    }
                    output.push('\n');
                }
            }

            if !response.warnings.is_empty() {
                output.push_str("\n## Warnings\n\n");
                for warning in &response.warnings {
                    output.push_str(&format!("- {}\n", warning));
                }
            }

            if include_trace {
                let traces_to_show: Vec<_> = response
                    .results
                    .iter()
                    .filter(|r| !r.operations.is_empty())
                    .collect();

                if !traces_to_show.is_empty() {
                    output.push_str("\n## Execution Trace\n\n");
                    for result in traces_to_show {
                        output.push_str(&format!("### Rule: {}\n\n", result.rule_name));
                        for (i, step) in result.operations.iter().enumerate() {
                            output.push_str(&format!("{}. {:?}\n", i + 1, step));
                        }
                        output.push('\n');
                    }
                }
            }

            info!(
                "Evaluated document '{}' with {} results",
                document,
                response.results.len()
            );

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }

        fn tool_inspect(&self, args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            let document = args["document"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'document' field".to_string()))?;

            if document.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Document name cannot be empty".to_string(),
                ));
            }

            self.engine.get_document(document).ok_or_else(|| {
                McpError::invalid_params(format!("Document '{}' not found", document))
            })?;

            let facts = self.engine.get_document_facts(document);
            let rules = self.engine.get_document_rules(document);

            let mut output = String::new();
            output.push_str(&format!("# Document: {}\n\n", document));

            output.push_str(&format!("## Facts ({})\n\n", facts.len()));
            if facts.is_empty() {
                output.push_str("(none)\n");
            } else {
                for fact in &facts {
                    let fact_name = lemma::analysis::fact_display_name(fact);
                    output.push_str(&format!("- **{}**: {}\n", fact_name, fact.value));
                }
            }

            output.push_str(&format!("\n## Rules ({})\n\n", rules.len()));
            if rules.is_empty() {
                output.push_str("(none)\n");
            } else {
                for rule in &rules {
                    output.push_str(&format!("- **{}**\n", rule.name));
                }
            }

            debug!("Inspected document: {}", document);

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }

        fn tool_list_documents(&self) -> Result<serde_json::Value, McpError> {
            let documents = self.engine.list_documents();

            let output = if documents.is_empty() {
                "No documents loaded.\n\nUse the 'add_document' tool to load Lemma code."
                    .to_string()
            } else {
                let mut s = format!("## Loaded Documents ({})\n\n", documents.len());
                for doc in &documents {
                    s.push_str(&format!("- {}\n", doc));
                }
                s
            };

            debug!("Listed {} documents", documents.len());

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }
    }

    pub fn start_server(engine: Engine) -> Result<()> {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "lemma_mcp=info".into()),
            )
            .with_writer(io::stderr)
            .init();

        info!("Starting Lemma MCP server v{}", SERVER_VERSION);
        info!("Protocol version: {}", PROTOCOL_VERSION);

        let mut server = McpServer::new(engine);
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        for line in stdin.lock().lines() {
            let line = line?;

            if line.trim().is_empty() {
                continue;
            }

            debug!("Received: {}", line);

            let response = match serde_json::from_str::<McpRequest>(&line) {
                Ok(request) => server.handle_request(request),
                Err(e) => {
                    error!("Parse error: {}", e);
                    McpResponse {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        result: None,
                        error: Some(McpError::parse_error(format!("Parse error: {}", e))),
                    }
                }
            };

            let response_json = serde_json::to_string(&response)?;
            writeln!(stdout, "{}", response_json)?;
            stdout.flush()?;

            debug!("Sent response");
        }

        info!("MCP server shutting down");
        Ok(())
    }
}

#[cfg(not(feature = "mcp"))]
pub mod server {
    use anyhow::Result;
    use lemma::Engine;

    pub fn start_server(_engine: Engine) -> Result<()> {
        anyhow::bail!("MCP feature not enabled. Recompile with --features mcp")
    }
}
