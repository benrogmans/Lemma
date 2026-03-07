mod imp {
    use anyhow::Result;
    use lemma::parsing::ast::DateTimeValue;
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

    fn resolve_effective(args: &serde_json::Value) -> Result<DateTimeValue, McpError> {
        match args.get("effective").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => DateTimeValue::parse(s.trim()).ok_or_else(|| {
                McpError::invalid_params(format!(
                    "Invalid effective value '{}'. Expected: YYYY, YYYY-MM, YYYY-MM-DD, or ISO 8601 datetime",
                    s
                ))
            }),
            _ => Ok(DateTimeValue::now()),
        }
    }

    /// Configuration for the MCP server.
    #[derive(Default)]
    pub struct McpConfig {
        /// When true, admin tools (`add_document`, `get_document_source`) are
        /// advertised and allowed. When false (default), the server is read-only.
        pub admin: bool,
    }

    struct McpServer {
        engine: Engine,
        config: McpConfig,
    }

    impl McpServer {
        fn new(engine: Engine, config: McpConfig) -> Self {
            Self { engine, config }
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

            let mut tools = vec![
                serde_json::json!({
                    "name": "evaluate",
                    "description": "Evaluate rules in a Lemma document. Returns the result and a step-by-step reasoning trace showing which facts were used and which conditions matched. Omit 'rule' to evaluate all rules.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "document": {
                                "type": "string",
                                "description": "Name of the document (from 'doc <name>' declaration)"
                            },
                            "rule": {
                                "type": "string",
                                "description": "Optional: name of a specific rule to evaluate. Omit to evaluate all rules."
                            },
                            "facts": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional fact values as 'name=value' (e.g. ['price=100', 'quantity=5'])",
                                "default": []
                            },
                            "effective": {
                                "type": "string",
                                "description": "Optional: evaluate at a specific effective datetime (e.g. '2026', '2026-03', '2026-03-04', '2026-03-04T10:30:00Z')"
                            }
                        },
                        "required": ["document"]
                    }
                }),
                serde_json::json!({
                    "name": "list_documents",
                    "description": "List all loaded Lemma documents with their schemas: fact names, types, defaults, and rule names with return types.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "effective": {
                                "type": "string",
                                "description": "Optional: list documents at a specific effective datetime (e.g. '2026', '2026-03-04')"
                            }
                        }
                    }
                }),
                serde_json::json!({
                    "name": "get_schema",
                    "description": "Get a document's schema: its facts (inputs with types, constraints, and defaults) and rules (outputs with types). Optionally scope to a specific rule to see only the facts it needs. Use this before calling evaluate to know which facts to provide.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "document": {
                                "type": "string",
                                "description": "Name of the document (from 'doc <name>' declaration)"
                            },
                            "rule": {
                                "type": "string",
                                "description": "Optional: name of a specific rule. Omit to get the full document schema."
                            },
                            "effective": {
                                "type": "string",
                                "description": "Optional: get schema at a specific effective datetime"
                            }
                        },
                        "required": ["document"]
                    }
                }),
            ];

            if self.config.admin {
                tools.push(serde_json::json!({
                    "name": "add_document",
                    "description": "Add a Lemma document to the engine. Returns the document schema on success.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "code": {
                                "type": "string",
                                "description": "The complete Lemma code to add"
                            },
                            "source_id": {
                                "type": "string",
                                "description": "Optional identifier for this document source"
                            }
                        },
                        "required": ["code"]
                    }
                }));
                tools.push(serde_json::json!({
                    "name": "get_document_source",
                    "description": "Return the Lemma source code for a document. Useful for inspecting or debugging the rules that produce evaluation results.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "document": {
                                "type": "string",
                                "description": "Name of the document"
                            },
                            "effective": {
                                "type": "string",
                                "description": "Optional: get source at a specific effective datetime"
                            }
                        },
                        "required": ["document"]
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
                "add_document" | "get_document_source" if !self.config.admin => {
                    Err(McpError::invalid_params(
                        "Admin tools are disabled. Start the server with --admin to enable them."
                            .to_string(),
                    ))
                }
                "add_document" => self.tool_add_document(arguments),
                "get_document_source" => self.tool_get_document_source(arguments),
                "evaluate" => self.tool_evaluate(arguments),
                "list_documents" => self.tool_list_documents(arguments),
                "get_schema" => self.tool_get_schema(arguments),
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

            let names_before: std::collections::HashSet<String> = self
                .engine
                .list_documents()
                .iter()
                .map(|d| d.name.clone())
                .collect();

            let files: std::collections::HashMap<String, String> =
                std::iter::once((source_id.clone(), code.to_string())).collect();
            tokio::runtime::Runtime::new()
                .map_err(|e| McpError::internal_error(e.to_string()))?
                .block_on(self.engine.add_lemma_files(files))
                .map_err(|errs| {
                    for e in &errs {
                        error!("{}", e);
                    }
                    let msg = errs
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join("; ");
                    McpError::internal_error(format!("Failed to parse document: {}", msg))
                })?;

            let new_doc_names: Vec<String> = self
                .engine
                .list_documents()
                .iter()
                .filter(|d| !names_before.contains(&d.name))
                .map(|d| d.name.clone())
                .collect();

            let mut output = String::from("Document added successfully.\n\n");

            let now = DateTimeValue::now();
            for doc_name in &new_doc_names {
                if let Some(plan) = self.engine.get_execution_plan(doc_name, None, &now) {
                    output.push_str(&plan.schema().to_string());
                    output.push('\n');
                }
            }

            info!(
                "Document added from source '{}': {:?}",
                source_id, new_doc_names
            );

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }

        fn tool_get_document_source(
            &self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let doc_name = args["document"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'document' field".to_string()))?;

            let now = resolve_effective(args)?;
            let doc = self.engine.get_document(doc_name, &now).ok_or_else(|| {
                McpError::invalid_params(format!(
                    "Document '{}' not found. Use list_documents to see available documents.",
                    doc_name
                ))
            })?;

            let source = lemma::format_docs(std::slice::from_ref(doc.as_ref()));

            debug!("Returned source for document '{}'", doc_name);

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
            let document = args["document"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'document' field".to_string()))?;

            if document.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Document name cannot be empty".to_string(),
                ));
            }

            let rule_names: Vec<String> = match args.get("rule").and_then(|v| v.as_str()) {
                Some(rule) if !rule.trim().is_empty() => vec![rule.trim().to_string()],
                _ => Vec::new(),
            };

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

            let now = resolve_effective(args)?;
            let hash_pin = args.get("hash_pin").and_then(|v| v.as_str());
            let response = self
                .engine
                .evaluate(document, hash_pin, &now, rule_names, fact_values)
                .map_err(|e| {
                    error!("Evaluation failed: {}", e);
                    McpError::internal_error(format!("Evaluation failed: {e}"))
                })?;

            let mut output = String::new();

            for result in response.results.values() {
                output.push_str(&format!("{}: ", result.rule.name));
                match &result.result {
                    lemma::OperationResult::Value(value) => {
                        output.push_str(&value.to_string());
                    }
                    lemma::OperationResult::Veto(msg) => {
                        if let Some(veto) = msg {
                            output.push_str(&format!("veto ({})", veto));
                        } else {
                            output.push_str("veto");
                        }
                    }
                }
                output.push('\n');

                if let Some(proof) = &result.proof {
                    let steps = format_proof_steps(proof);
                    if !steps.is_empty() {
                        output.push_str("\nReasoning:\n");
                        output.push_str(&steps);
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

        fn tool_list_documents(
            &self,
            args: &serde_json::Value,
        ) -> Result<serde_json::Value, McpError> {
            let now = resolve_effective(args)?;
            let docs = self.engine.list_documents_effective(&now);

            let output = if docs.is_empty() {
                if self.config.admin {
                    "No documents loaded.\n\nUse the 'add_document' tool to load Lemma code."
                        .to_string()
                } else {
                    "No documents loaded.".to_string()
                }
            } else {
                let schemas: Vec<lemma::DocumentSchema> = docs
                    .iter()
                    .filter_map(|doc| self.engine.get_execution_plan(&doc.name, None, &now))
                    .map(|plan| plan.schema())
                    .collect();

                schemas
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join("\n\n")
            };

            debug!("Listed {} documents", docs.len());

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }

        fn tool_get_schema(&self, args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            let document = args["document"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'document' field".to_string()))?;

            if document.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Document name cannot be empty".to_string(),
                ));
            }

            let now = resolve_effective(args)?;
            let plan = self
                .engine
                .get_execution_plan(document, None, &now)
                .ok_or_else(|| {
                    McpError::invalid_params(format!(
                        "Document '{}' not found. Use list_documents to see available documents.",
                        document
                    ))
                })?;

            let rule_names: Vec<String> = match args.get("rule").and_then(|v| v.as_str()) {
                Some(rule) if !rule.trim().is_empty() => vec![rule.trim().to_string()],
                _ => Vec::new(),
            };

            let schema = if rule_names.is_empty() {
                plan.schema()
            } else {
                plan.schema_for_rules(&rule_names).map_err(|e| {
                    error!("schema_for_rules failed: {}", e);
                    McpError::internal_error(format!("Failed to get schema for rules: {e}"))
                })?
            };

            let scope = if rule_names.is_empty() {
                format!("{} (all rules)", document)
            } else {
                format!("{}.{}", document, rule_names[0])
            };

            let output = format!("Schema for {}:\n\n{}", scope, schema);

            info!(
                "Returned schema for '{}' ({} facts, {} rules)",
                scope,
                schema.facts.len(),
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

    // ── Proof formatting ─────────────────────────────────────────────────

    /// Linearise a proof tree into plain-English reasoning steps.
    fn format_proof_steps(proof: &lemma::proof::Proof) -> String {
        let mut steps = Vec::new();
        let mut seen_facts = std::collections::HashSet::new();
        let mut seen_rules = std::collections::HashSet::new();
        walk_proof_node(&proof.tree, &mut steps, &mut seen_facts, &mut seen_rules);
        steps.join("\n")
    }

    fn walk_proof_node(
        node: &lemma::proof::ProofNode,
        steps: &mut Vec<String>,
        seen_facts: &mut std::collections::HashSet<String>,
        seen_rules: &mut std::collections::HashSet<String>,
    ) {
        use lemma::proof::{ProofNode, ValueSource};

        match node {
            ProofNode::Value {
                value,
                source: ValueSource::Fact { fact_ref },
                ..
            } => {
                let key = fact_ref.to_string();
                if seen_facts.insert(key.clone()) {
                    steps.push(format!("{}: {}", key, value));
                }
            }

            ProofNode::Value { .. } => {}

            ProofNode::RuleReference {
                rule_path,
                result,
                expansion,
                ..
            } => {
                let key = rule_path.to_string();
                if !seen_rules.insert(key) {
                    return;
                }
                walk_proof_node(expansion, steps, seen_facts, seen_rules);
                match result {
                    lemma::OperationResult::Value(v) => {
                        steps.push(format!("{}: {}", rule_path.rule, v));
                    }
                    lemma::OperationResult::Veto(msg) => match msg {
                        Some(reason) => {
                            steps.push(format!("{}: veto ({})", rule_path.rule, reason));
                        }
                        None => {
                            steps.push(format!("{}: veto", rule_path.rule));
                        }
                    },
                }
            }

            ProofNode::Computation {
                original_expression,
                expression,
                result,
                operands,
                ..
            } => {
                for op in operands {
                    walk_proof_node(op, steps, seen_facts, seen_rules);
                }
                let expr = if original_expression != expression && !original_expression.is_empty() {
                    original_expression.as_str()
                } else {
                    expression.as_str()
                };
                steps.push(format!("{}: {}", expr, result));
            }

            ProofNode::Branches {
                matched,
                non_matched,
                ..
            } => {
                // Collect facts from all branch conditions first
                for branch in non_matched {
                    collect_branch_facts(&branch.condition, steps, seen_facts);
                }
                if let Some(cond) = &matched.condition {
                    collect_branch_facts(cond, steps, seen_facts);
                }

                // Emit non-matched branch decisions
                for branch in non_matched {
                    let cond_text = node_expression(&branch.condition);
                    let clause = match branch.clause_index {
                        Some(i) => format!("unless clause {}", i + 1),
                        None => "default".to_string(),
                    };
                    steps.push(format!("{}: {} is false, skipped", clause, cond_text));
                }

                // Emit matched branch decision
                if let Some(cond) = &matched.condition {
                    let cond_text = node_expression(cond);
                    let clause = match matched.clause_index {
                        Some(i) => format!("unless clause {}", i + 1),
                        None => "clause".to_string(),
                    };
                    steps.push(format!("{}: {} is true, matched", clause, cond_text));
                } else {
                    steps.push("default value applies".to_string());
                }

                // Walk the matched result
                walk_proof_node(&matched.result, steps, seen_facts, seen_rules);
            }

            ProofNode::Condition {
                original_expression,
                expression,
                result,
                operands,
                ..
            } => {
                for op in operands {
                    walk_proof_node(op, steps, seen_facts, seen_rules);
                }
                let expr = if original_expression != expression && !original_expression.is_empty() {
                    original_expression.as_str()
                } else {
                    expression.as_str()
                };
                let verdict = if *result { "true" } else { "false" };
                steps.push(format!("{} is {}", expr, verdict));
            }

            ProofNode::Veto { message, .. } => match message {
                Some(msg) => steps.push(format!("veto: {}", msg)),
                None => steps.push("veto".to_string()),
            },
        }
    }

    /// Walk a proof node collecting only fact values (no reasoning steps).
    /// Used to gather facts from branch conditions before emitting branch decisions.
    fn collect_branch_facts(
        node: &lemma::proof::ProofNode,
        steps: &mut Vec<String>,
        seen_facts: &mut std::collections::HashSet<String>,
    ) {
        use lemma::proof::{ProofNode, ValueSource};

        match node {
            ProofNode::Value {
                value,
                source: ValueSource::Fact { fact_ref },
                ..
            } => {
                let key = fact_ref.to_string();
                if seen_facts.insert(key.clone()) {
                    steps.push(format!("{}: {}", key, value));
                }
            }
            ProofNode::Condition { operands, .. } | ProofNode::Computation { operands, .. } => {
                for op in operands {
                    collect_branch_facts(op, steps, seen_facts);
                }
            }
            ProofNode::RuleReference { expansion, .. } => {
                collect_branch_facts(expansion, steps, seen_facts);
            }
            ProofNode::Branches {
                matched,
                non_matched,
                ..
            } => {
                for b in non_matched {
                    collect_branch_facts(&b.condition, steps, seen_facts);
                }
                if let Some(c) = &matched.condition {
                    collect_branch_facts(c, steps, seen_facts);
                }
                collect_branch_facts(&matched.result, steps, seen_facts);
            }
            _ => {}
        }
    }

    /// Extract the human-readable expression from any proof node.
    fn node_expression(node: &lemma::proof::ProofNode) -> String {
        use lemma::proof::{ProofNode, ValueSource};

        match node {
            ProofNode::Condition {
                original_expression,
                expression,
                ..
            }
            | ProofNode::Computation {
                original_expression,
                expression,
                ..
            } => {
                if original_expression != expression && !original_expression.is_empty() {
                    original_expression.clone()
                } else {
                    expression.clone()
                }
            }
            ProofNode::RuleReference { rule_path, .. } => rule_path.rule.to_string(),
            ProofNode::Value {
                source: ValueSource::Fact { fact_ref },
                ..
            } => fact_ref.to_string(),
            ProofNode::Value { value, .. } => value.to_string(),
            ProofNode::Branches { .. } => "branch".to_string(),
            ProofNode::Veto { message, .. } => {
                message.clone().unwrap_or_else(|| "veto".to_string())
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

        let mut server = McpServer::new(engine, config);
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
pub use imp::McpConfig;
