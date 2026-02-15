mod imp {
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
                        "description": "Evaluate a single rule in a document with optional fact values. Dependencies are computed automatically.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "document": {
                                    "type": "string",
                                    "description": "Name of the document (from 'doc <name>' declaration)"
                                },
                                "rule": {
                                    "type": "string",
                                    "description": "Name of the rule to evaluate (as declared in the document)"
                                },
                                "facts": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Optional fact values in format 'name=value' (e.g., ['price=100', 'quantity=5'])",
                                    "default": []
                                }
                            },
                            "required": ["document", "rule"]
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

            let files: std::collections::HashMap<String, String> =
                std::iter::once((source_id.clone(), code.to_string())).collect();
            tokio::runtime::Runtime::new()
                .map_err(|e| McpError::internal_error(e.to_string()))?
                .block_on(self.engine.add_lemma_files(files))
                .map_err(|errs| {
                    let error = match errs.len() {
                        0 => unreachable!("add_lemma_files returned Err with empty error list"),
                        1 => errs.into_iter().next().unwrap(),
                        _ => lemma::LemmaError::MultipleErrors(errs),
                    };
                    error!("Failed to add document: {}", error);
                    McpError::internal_error(format!("Failed to parse document: {error}"))
                })?;

            info!("Document added: {}", source_id);

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": format!("Document added successfully\n\nSource ID: {source_id}\n\nThe document has been parsed and loaded into the engine. You can now evaluate it using the 'evaluate' tool.")
                }]
            }))
        }

        /// Verify that all requested rule names exist in the document.
        fn validate_rule_names(
            &self,
            doc_name: &str,
            rule_names: &[String],
        ) -> Result<(), McpError> {
            let plan = self.engine.get_execution_plan(doc_name).ok_or_else(|| {
                McpError::invalid_params(format!("Document '{doc_name}' not found"))
            })?;
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
                return Err(McpError::invalid_params(format!(
                    "Unknown rule(s) in '{doc_name}': {}",
                    unknown.join(", ")
                )));
            }
            Ok(())
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

            let rule_name = args["rule"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'rule' field".to_string()))?
                .trim()
                .to_string();

            if rule_name.is_empty() {
                return Err(McpError::invalid_params(
                    "Rule name cannot be empty.".to_string(),
                ));
            }

            self.validate_rule_names(document, std::slice::from_ref(&rule_name))?;

            let facts: Vec<&str> = args["facts"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            let fact_values: std::collections::HashMap<String, String> = facts
                .iter()
                .filter_map(|s| {
                    s.split_once('=')
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                })
                .collect();

            let response = self
                .engine
                .evaluate(document, vec![rule_name], fact_values)
                .map_err(|e| {
                    error!("Evaluation failed: {}", e);
                    McpError::internal_error(format!("Evaluation failed: {e}"))
                })?;

            let mut output = String::default();
            output.push_str(&format!(
                "Evaluation complete for document '{document}'\n\n"
            ));

            if !response.results.is_empty() {
                output.push_str("## Results\n\n");
                for result in response.results.values() {
                    output.push_str(&format!("**{}**: ", result.rule.name));
                    match &result.result {
                        lemma::OperationResult::Value(value) => {
                            output.push_str(&value.to_string());
                        }
                        lemma::OperationResult::Veto(msg) => {
                            if let Some(veto) = msg {
                                output.push_str(&format!("Veto: {veto}"));
                            } else {
                                output.push_str("Veto");
                            }
                        }
                    }
                    output.push('\n');
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

        fn tool_list_documents(&self) -> Result<serde_json::Value, McpError> {
            let documents = self.engine.list_documents();

            let output = if documents.is_empty() {
                "No documents loaded.\n\nUse the 'add_document' tool to load Lemma code."
                    .to_string()
            } else {
                let mut s = format!("## Loaded Documents ({})\n\n", documents.len());
                for doc in &documents {
                    s.push_str(&format!("- {doc}\n"));
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
                        error: Some(McpError::parse_error(format!("Parse error: {e}"))),
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

pub use imp::start_server;
