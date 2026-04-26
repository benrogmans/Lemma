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
            Some(s) if !s.trim().is_empty() => s.trim().parse::<DateTimeValue>().ok().ok_or_else(|| {
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
        /// When true, admin tools (`add_spec`, `get_spec_source`) are
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
                "description": "Evaluate rules in a Lemma spec. Returns the result and a step-by-step reasoning trace showing which facts were used and which conditions matched. Omit 'rule' to evaluate all rules.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "spec": {
                                        "type": "string",
                                        "description": "Spec id: name or name~hash (8 hex) to pin content, e.g. pricing or pricing~a1b2c3d4"
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
                        "required": ["spec"]
                    }
                }),
                serde_json::json!({
                    "name": "list_specs",
                    "description": "List all loaded Lemma specs with their schemas: fact names, types, defaults, and rule names with return types.",
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
                "description": "Get a spec's schema: its facts (inputs with types, constraints, and defaults) and rules (outputs with types). Optionally scope to a specific rule to see only the facts it needs. Use this before calling evaluate to know which facts to provide.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "spec": {
                                        "type": "string",
                                        "description": "Spec id: name or name~hash (8 hex), e.g. pricing or pricing~a1b2c3d4"
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
                    "description": "Add a Lemma spec to the engine. Returns the spec schema on success.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "code": {
                                "type": "string",
                                "description": "The complete Lemma code to add"
                            },
                            "source_id": {
                                "type": "string",
                                "description": "Optional identifier for this spec source"
                            }
                        },
                        "required": ["code"]
                    }
                }));
                tools.push(serde_json::json!({
                    "name": "get_spec_source",
"description": "Return the Lemma source code for a spec. Useful for inspecting or debugging the rules that produce evaluation results.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "spec": {
                                        "type": "string",
                                        "description": "Name of the spec"
                                    },
                            "effective": {
                                "type": "string",
                                "description": "Optional: get source at a specific effective datetime"
                            }
                        },
                        "required": ["spec"]
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
                    "Spec code cannot be empty".to_string(),
                ));
            }

            let source_id = args["source_id"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| format!("spec_{}", chrono::Utc::now().timestamp_millis()));

            let names_before: std::collections::HashSet<String> = self
                .engine
                .list_specs()
                .iter()
                .map(|d| d.name.clone())
                .collect();

            self.engine
                .load(code, lemma::SourceType::Labeled(&source_id))
                .map_err(|load_err| {
                    for e in load_err.iter() {
                        error!(
                            "{}",
                            crate::error_formatter::format_error(e, &load_err.sources)
                        );
                    }
                    McpError::internal_error(format!(
                        "Failed to parse spec ({} error(s))",
                        load_err.errors.len()
                    ))
                })?;

            let new_spec_names: Vec<String> = self
                .engine
                .list_specs()
                .iter()
                .filter(|d| !names_before.contains(&d.name))
                .map(|d| d.name.clone())
                .collect();

            let mut output = String::from("Spec added successfully.\n\n");

            let now = DateTimeValue::now();
            for spec_name in &new_spec_names {
                if let Ok(plan) = self.engine.get_plan(spec_name, Some(&now)) {
                    output.push_str(&plan.schema().to_string());
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
            let spec_name = args["spec"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'spec' field".to_string()))?;

            let now = resolve_effective(args)?;
            let spec = self.engine.get_spec(spec_name, Some(&now)).map_err(|e| {
                McpError::invalid_params(format!(
                    "Spec '{}' not found: {}. Use list_specs to see available specs.",
                    spec_name, e
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
            let spec_id = args["spec"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'spec' field".to_string()))?;

            if spec_id.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Spec id cannot be empty".to_string(),
                ));
            }

            let (_base_name, _) = lemma::parse_spec_id(spec_id.trim())
                .map_err(|e| McpError::invalid_params(format!("{}", e)))?;

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
            let mut response = self
                .engine
                .run(spec_id.trim(), Some(&now), fact_values, false)
                .map_err(|e| {
                    error!("Evaluation failed: {}", e);
                    McpError::internal_error(format!("Evaluation failed: {e}"))
                })?;
            if !rule_names.is_empty() {
                response.filter_rules(&rule_names);
            }

            let hash = self
                .engine
                .get_plan(spec_id.trim(), Some(&now))
                .map_err(|e| {
                    McpError::internal_error(format!("BUG: run succeeded but get_plan failed: {e}"))
                })?
                .plan_hash();

            let mut output = String::new();
            output.push_str(&format!("spec: {}\n", spec_id.trim()));
            output.push_str(&format!("effective: {}\n", now));
            output.push_str(&format!("hash: {}\n", hash));
            output.push('\n');

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

                if let Some(explanation) = &result.explanation {
                    let steps = format_explanation_steps(explanation);
                    if !steps.is_empty() {
                        output.push_str("\nReasoning:\n");
                        output.push_str(&steps);
                        output.push('\n');
                    }
                }
            }

            info!(
                "Evaluated spec '{}' with {} results",
                spec_id.trim(),
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
            let specs_list = self.engine.list_specs_effective(&now);

            let output = if specs_list.is_empty() {
                if self.config.admin {
                    "No specs loaded.\n\nUse the 'add_spec' tool to load Lemma code.".to_string()
                } else {
                    "No specs loaded.".to_string()
                }
            } else {
                let schemas: Vec<lemma::SpecSchema> = specs_list
                    .iter()
                    .filter_map(|s| self.engine.schema(&s.name, Some(&now)).ok())
                    .collect();

                schemas
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join("\n\n")
            };

            debug!("Listed {} specs", specs_list.len());

            Ok(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": output
                }]
            }))
        }

        fn tool_get_schema(&self, args: &serde_json::Value) -> Result<serde_json::Value, McpError> {
            let spec_id = args["spec"]
                .as_str()
                .ok_or_else(|| McpError::invalid_params("Missing 'spec' field".to_string()))?;

            if spec_id.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "Spec id cannot be empty".to_string(),
                ));
            }

            lemma::parse_spec_id(spec_id.trim())
                .map_err(|e| McpError::invalid_params(format!("{}", e)))?;

            let now = resolve_effective(args)?;
            let plan = self
                .engine
                .get_plan(spec_id.trim(), Some(&now))
                .map_err(|_| {
                    McpError::invalid_params(format!(
                        "Spec '{}' not found. Use list_specs to see available specs.",
                        spec_id.trim()
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
                format!("{} (all rules)", spec_id.trim())
            } else {
                format!("{}.{}", spec_id.trim(), rule_names[0])
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

    // ── Explanation formatting ─────────────────────────────────────────────

    /// Linearise an explanation tree into plain-English reasoning steps.
    fn format_explanation_steps(explanation: &lemma::explanation::Explanation) -> String {
        let mut steps = Vec::new();
        let mut seen_facts = std::collections::HashSet::new();
        let mut seen_rules = std::collections::HashSet::new();
        walk_explanation_node(
            &explanation.tree,
            &mut steps,
            &mut seen_facts,
            &mut seen_rules,
        );
        steps.join("\n")
    }

    fn walk_explanation_node(
        node: &lemma::explanation::ExplanationNode,
        steps: &mut Vec<String>,
        seen_facts: &mut std::collections::HashSet<String>,
        seen_rules: &mut std::collections::HashSet<String>,
    ) {
        use lemma::explanation::{ExplanationNode, ValueSource};

        match node {
            ExplanationNode::Value {
                value,
                source: ValueSource::Fact { fact_ref },
                ..
            } => {
                let key = fact_ref.to_string();
                if seen_facts.insert(key.clone()) {
                    steps.push(format!("{}: {}", key, value));
                }
            }

            ExplanationNode::Value { .. } => {}

            ExplanationNode::RuleReference {
                rule_path,
                result,
                expansion,
                ..
            } => {
                let key = rule_path.to_string();
                if !seen_rules.insert(key) {
                    return;
                }
                walk_explanation_node(expansion, steps, seen_facts, seen_rules);
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

            ExplanationNode::Computation {
                original_expression,
                expression,
                result,
                operands,
                ..
            } => {
                for op in operands {
                    walk_explanation_node(op, steps, seen_facts, seen_rules);
                }
                let expr = if original_expression != expression && !original_expression.is_empty() {
                    original_expression.as_str()
                } else {
                    expression.as_str()
                };
                steps.push(format!("{}: {}", expr, result));
            }

            ExplanationNode::Branches {
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
                walk_explanation_node(&matched.result, steps, seen_facts, seen_rules);
            }

            ExplanationNode::Condition {
                original_expression,
                expression,
                result,
                operands,
                ..
            } => {
                for op in operands {
                    walk_explanation_node(op, steps, seen_facts, seen_rules);
                }
                let expr = if original_expression != expression && !original_expression.is_empty() {
                    original_expression.as_str()
                } else {
                    expression.as_str()
                };
                let verdict = if *result { "true" } else { "false" };
                steps.push(format!("{} is {}", expr, verdict));
            }

            ExplanationNode::Veto { message, .. } => match message {
                Some(msg) => steps.push(format!("veto: {}", msg)),
                None => steps.push("veto".to_string()),
            },
        }
    }

    /// Walk an explanation node collecting only fact values (no reasoning steps).
    /// Used to gather facts from branch conditions before emitting branch decisions.
    fn collect_branch_facts(
        node: &lemma::explanation::ExplanationNode,
        steps: &mut Vec<String>,
        seen_facts: &mut std::collections::HashSet<String>,
    ) {
        use lemma::explanation::{ExplanationNode, ValueSource};

        match node {
            ExplanationNode::Value {
                value,
                source: ValueSource::Fact { fact_ref },
                ..
            } => {
                let key = fact_ref.to_string();
                if seen_facts.insert(key.clone()) {
                    steps.push(format!("{}: {}", key, value));
                }
            }
            ExplanationNode::Condition { operands, .. }
            | ExplanationNode::Computation { operands, .. } => {
                for op in operands {
                    collect_branch_facts(op, steps, seen_facts);
                }
            }
            ExplanationNode::RuleReference { expansion, .. } => {
                collect_branch_facts(expansion, steps, seen_facts);
            }
            ExplanationNode::Branches {
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

    /// Extract the human-readable expression from any explanation node.
    fn node_expression(node: &lemma::explanation::ExplanationNode) -> String {
        use lemma::explanation::{ExplanationNode, ValueSource};

        match node {
            ExplanationNode::Condition {
                original_expression,
                expression,
                ..
            }
            | ExplanationNode::Computation {
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
            ExplanationNode::RuleReference { rule_path, .. } => rule_path.rule.to_string(),
            ExplanationNode::Value {
                source: ValueSource::Fact { fact_ref },
                ..
            } => fact_ref.to_string(),
            ExplanationNode::Value { value, .. } => value.to_string(),
            ExplanationNode::Branches { .. } => "branch".to_string(),
            ExplanationNode::Veto { message, .. } => {
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

            // Parse error responds with id: null (JSON-RPC 2.0 §4.2). For
            // any successfully-parsed notification, handle_request returns
            // None and we MUST NOT write anything back.
            let response = match serde_json::from_str::<McpRequest>(&line) {
                Ok(request) => server.handle_request(request),
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
    }
}

pub use imp::start_server;
pub use imp::McpConfig;
