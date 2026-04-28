use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn test_mcp_help_shows_admin_flag() {
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.args(["mcp", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("--admin"));
}

/// Send JSON-RPC messages to the MCP server and collect responses.
/// `workdir: None` runs `lemma mcp` with no path (in-memory only; no disk read at startup).
fn mcp_session(
    workdir: Option<&std::path::Path>,
    admin: bool,
    messages: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp");
    if let Some(p) = workdir {
        cmd.arg(p);
    }
    if admin {
        cmd.arg("--admin");
    }
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("Failed to start MCP server");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    let mut input = String::new();
    for msg in messages {
        input.push_str(&serde_json::to_string(msg).unwrap());
        input.push('\n');
    }
    stdin.write_all(input.as_bytes()).unwrap();
    drop(stdin);

    let mut responses = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
            responses.push(val);
        }
    }

    child.wait().unwrap();
    responses
}

fn make_request(id: u64, method: &str, params: serde_json::Value) -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
}

fn pricing_spec() -> &'static str {
    "spec pricing\ndata quantity: number\ndata base_price: 10\nrule total: quantity * base_price\n"
}

fn write_spec(dir: &std::path::Path, filename: &str, content: &str) {
    std::fs::write(dir.join(filename), content).unwrap();
}

#[test]
fn test_mcp_list_specs_includes_schema() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(temp_dir.path(), "pricing.lemma", pricing_spec());

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(2, "tools/list", json!({})),
            make_request(
                3,
                "tools/call",
                json!({
                    "name": "list_specs",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 3, "Expected at least 3 responses");

    let list_result = &responses[2]["result"]["content"][0]["text"];
    let text = list_result.as_str().expect("list_specs should return text");

    assert!(
        text.contains("Spec: pricing"),
        "Should contain spec name, got: {text}"
    );
    assert!(
        text.contains("quantity"),
        "Should list data names, got: {text}"
    );
    assert!(
        text.contains("base_price"),
        "Should list data names, got: {text}"
    );
    assert!(
        text.contains("total"),
        "Should list rule names, got: {text}"
    );
}

#[test]
fn test_mcp_evaluate_includes_reasoning() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "discount.lemma",
        "spec discount\ndata quantity: number\nrule rate: 0 percent\n unless quantity >= 10 then 10 percent\n unless quantity >= 50 then 20 percent\n",
    );

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "evaluate",
                    "arguments": {
                        "spec": "discount",
                        "rule": "rate",
                        "data": ["quantity=25"]
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2, "Expected at least 2 responses");

    let eval_result = &responses[1]["result"]["content"][0]["text"];
    let text = eval_result.as_str().expect("evaluate should return text");

    assert!(
        text.contains("rate:"),
        "Should contain rule name, got: {text}"
    );
    assert!(
        text.contains("Reasoning:"),
        "Should contain reasoning section, got: {text}"
    );
    assert!(
        text.contains("quantity: 25"),
        "Should show data value in reasoning, got: {text}"
    );
}

#[test]
fn test_mcp_read_only_by_default() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(2, "tools/list", json!({})),
            make_request(
                3,
                "tools/call",
                json!({
                    "name": "add_spec",
                    "arguments": {
                        "code": "spec test\ndata x: 5\nrule y: x"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 3, "Expected at least 3 responses");

    // tools/list should NOT include admin tools
    let tools = &responses[1]["result"]["tools"];
    let tool_names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        !tool_names.contains(&"add_spec"),
        "add_spec should not be listed in read-only mode, got: {:?}",
        tool_names
    );
    assert!(
        !tool_names.contains(&"get_spec_source"),
        "get_spec_source should not be listed in read-only mode, got: {:?}",
        tool_names
    );

    // Calling add_spec should return an error
    let error = &responses[2]["error"];
    assert!(
        error.is_object(),
        "add_spec should return an error in read-only mode"
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("Admin tools are disabled"),
        "Error should mention admin tools are disabled, got: {}",
        error["message"]
    );
}

#[test]
fn test_mcp_admin_enables_add_spec() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(2, "tools/list", json!({})),
            make_request(
                3,
                "tools/call",
                json!({
                    "name": "add_spec",
                    "arguments": {
                        "code": "spec test_spec\ndata x: 5\nrule y: x * 2"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 3, "Expected at least 3 responses");

    // tools/list should include admin tools
    let tools = &responses[1]["result"]["tools"];
    let tool_names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        tool_names.contains(&"add_spec"),
        "add_spec should be listed with --admin, got: {:?}",
        tool_names
    );
    assert!(
        tool_names.contains(&"get_spec_source"),
        "get_spec_source should be listed with --admin, got: {:?}",
        tool_names
    );

    // add_spec should succeed and return schema
    let add_result = &responses[2]["result"]["content"][0]["text"];
    let text = add_result.as_str().expect("add_spec should return text");
    assert!(
        text.contains("Spec added successfully"),
        "Should confirm success, got: {text}"
    );
    assert!(
        text.contains("Spec: test_spec"),
        "Should include spec name in schema, got: {text}"
    );
    assert!(
        text.contains("y"),
        "Should include rule name in schema, got: {text}"
    );
}

#[test]
fn test_mcp_get_spec_source() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(temp_dir.path(), "pricing.lemma", pricing_spec());

    let responses = mcp_session(
        Some(temp_dir.path()),
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "get_spec_source",
                    "arguments": {
                        "spec": "pricing"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2, "Expected at least 2 responses");

    let source_result = &responses[1]["result"]["content"][0]["text"];
    let text = source_result
        .as_str()
        .expect("get_spec_source should return text");

    assert!(
        text.contains("spec pricing"),
        "Should contain spec declaration, got: {text}"
    );
    assert!(
        text.contains("data quantity"),
        "Should contain data declarations, got: {text}"
    );
    assert!(
        text.contains("rule total"),
        "Should contain rule declarations, got: {text}"
    );
}

#[test]
fn test_mcp_get_spec_source_blocked_without_admin() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "pricing.lemma",
        "spec pricing\ndata x: 5\nrule y: x\n",
    );

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "get_spec_source",
                    "arguments": {
                        "spec": "pricing"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2, "Expected at least 2 responses");

    let error = &responses[1]["error"];
    assert!(
        error.is_object(),
        "get_spec_source should return an error without --admin"
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("Admin tools are disabled"),
        "Error should mention admin tools are disabled, got: {}",
        error["message"]
    );
}

// ── initialize ──────────────────────────────────────────────────────────

#[test]
fn test_mcp_initialize_response() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[make_request(1, "initialize", json!({}))],
    );

    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["serverInfo"]["name"], "lemma-mcp-server");
    assert!(
        result["serverInfo"]["version"].as_str().is_some(),
        "Should include server version"
    );
    assert!(
        result["capabilities"]["tools"].is_object(),
        "Should advertise tools capability"
    );
}

// ── get_schema ──────────────────────────────────────────────────────────

#[test]
fn test_mcp_get_schema_full_spec() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(temp_dir.path(), "pricing.lemma", pricing_spec());

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "get_schema",
                    "arguments": { "spec": "pricing" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("get_schema should return text");

    assert!(
        text.contains("pricing"),
        "Should mention spec name, got: {text}"
    );
    assert!(text.contains("quantity"), "Should list data, got: {text}");
    assert!(text.contains("base_price"), "Should list data, got: {text}");
    assert!(text.contains("total"), "Should list rules, got: {text}");
}

#[test]
fn test_mcp_get_schema_for_specific_rule() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "multi.lemma",
        "spec multi\ndata a: number\ndata b: number\nrule sum: a + b\nrule product: a * b\n",
    );

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "get_schema",
                    "arguments": { "spec": "multi", "rule": "sum" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("get_schema should return text");

    assert!(
        text.contains("sum"),
        "Should include the requested rule, got: {text}"
    );
}

#[test]
fn test_mcp_get_schema_missing_spec() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "get_schema",
                    "arguments": { "spec": "nonexistent" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(error.is_object(), "Should return an error for missing spec");
    assert!(
        error["message"].as_str().unwrap().contains("not found"),
        "Error should say spec not found, got: {}",
        error["message"]
    );
}

#[test]
fn test_mcp_get_schema_empty_spec_name() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "get_schema",
                    "arguments": { "spec": "" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(
        error.is_object(),
        "Should return an error for empty spec name"
    );
}

// ── evaluate edge cases ─────────────────────────────────────────────────

#[test]
fn test_mcp_evaluate_all_rules() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "multi.lemma",
        "spec multi\ndata x: 3\nrule double: x * 2\nrule triple: x * 3\n",
    );

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "evaluate",
                    "arguments": { "spec": "multi" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("evaluate should return text");

    assert!(
        text.contains("double:"),
        "Should contain double rule, got: {text}"
    );
    assert!(
        text.contains("triple:"),
        "Should contain triple rule, got: {text}"
    );
    assert!(text.contains("6"), "double should be 6, got: {text}");
    assert!(text.contains("9"), "triple should be 9, got: {text}");
}

#[test]
fn test_mcp_evaluate_missing_spec() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "evaluate",
                    "arguments": { "spec": "nonexistent" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(error.is_object(), "Should return an error for missing spec");
}

#[test]
fn test_mcp_evaluate_empty_spec_name() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "evaluate",
                    "arguments": { "spec": "" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(
        error.is_object(),
        "Should return an error for empty spec name"
    );
    assert!(
        error["message"].as_str().unwrap().contains("empty"),
        "Error should mention empty, got: {}",
        error["message"]
    );
}

#[test]
fn test_mcp_evaluate_veto_result() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "vetoed.lemma",
        "spec vetoed\ndata price: -5\nrule validated: price\n unless price < 0 then veto \"Price cannot be negative\"\n",
    );

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "evaluate",
                    "arguments": { "spec": "vetoed" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("evaluate should return text");

    assert!(
        text.contains("veto"),
        "Should contain veto in output, got: {text}"
    );
    assert!(
        text.contains("Price cannot be negative"),
        "Should contain veto reason, got: {text}"
    );
}

#[test]
fn test_mcp_evaluate_with_effective_datetime() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "simple.lemma",
        "spec simple\ndata x: 42\nrule y: x\n",
    );

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "evaluate",
                    "arguments": {
                        "spec": "simple",
                        "effective": "2026-01-01"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("evaluate should return text");

    assert!(
        text.contains("y:"),
        "Should contain rule result, got: {text}"
    );
    assert!(
        text.contains("2026-01-01"),
        "Should show effective datetime, got: {text}"
    );
}

// ── list_specs edge cases ───────────────────────────────────────────────

#[test]
fn test_mcp_list_specs_empty_workspace() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "list_specs",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("list_specs should return text");

    assert!(
        text.contains("No specs loaded"),
        "Should indicate no specs loaded, got: {text}"
    );
}

#[test]
fn test_mcp_list_specs_empty_workspace_admin_suggests_add() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "list_specs",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("list_specs should return text");

    assert!(
        text.contains("add_spec"),
        "Admin mode should suggest using add_spec, got: {text}"
    );
}

#[test]
fn test_mcp_omit_path_no_disk_at_startup() {
    let responses = mcp_session(
        None,
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "list_specs",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("list_specs should return text");
    assert!(
        text.contains("No specs loaded"),
        "Omitting path should start with empty engine, got: {text}"
    );
    assert!(text.contains("add_spec"));
}

// ── error handling ──────────────────────────────────────────────────────

#[test]
fn test_mcp_invalid_jsonrpc_version() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[json!({
            "jsonrpc": "1.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        })],
    );

    assert_eq!(responses.len(), 1);
    let error = &responses[0]["error"];
    assert!(
        error.is_object(),
        "Should return an error for bad JSON-RPC version"
    );
    assert_eq!(error["code"], -32600, "Should be invalid request code");
}

#[test]
fn test_mcp_unknown_method() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[make_request(1, "nonexistent/method", json!({}))],
    );

    assert_eq!(responses.len(), 1);
    let error = &responses[0]["error"];
    assert!(
        error.is_object(),
        "Should return an error for unknown method"
    );
    assert_eq!(error["code"], -32601, "Should be method not found code");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("nonexistent/method"),
        "Error should name the unknown method, got: {}",
        error["message"]
    );
}

#[test]
fn test_mcp_malformed_json() {
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("Failed to start MCP server");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    stdin.write_all(b"this is not json\n").unwrap();
    drop(stdin);

    let mut responses = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
            responses.push(val);
        }
    }
    child.wait().unwrap();

    assert_eq!(responses.len(), 1);
    let error = &responses[0]["error"];
    assert!(error.is_object(), "Should return a parse error");
    assert_eq!(error["code"], -32700, "Should be parse error code");
}

#[test]
fn test_mcp_tools_call_missing_params() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call"
            }),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(
        error.is_object(),
        "Should return an error for missing params"
    );
    assert_eq!(error["code"], -32602, "Should be invalid params code");
}

#[test]
fn test_mcp_tools_call_missing_tool_name() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(2, "tools/call", json!({ "arguments": {} })),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(
        error.is_object(),
        "Should return an error for missing tool name"
    );
    assert_eq!(error["code"], -32602, "Should be invalid params code");
}

#[test]
fn test_mcp_tools_call_unknown_tool() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "nonexistent_tool",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(error.is_object(), "Should return an error for unknown tool");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("nonexistent_tool"),
        "Error should name the unknown tool, got: {}",
        error["message"]
    );
}

// ── add_spec error cases ────────────────────────────────────────────────

#[test]
fn test_mcp_add_spec_empty_code() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "add_spec",
                    "arguments": { "code": "" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(error.is_object(), "Should return an error for empty code");
    assert!(
        error["message"].as_str().unwrap().contains("empty"),
        "Error should mention empty, got: {}",
        error["message"]
    );
}

#[test]
fn test_mcp_add_spec_invalid_code() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "add_spec",
                    "arguments": { "code": "this is not valid lemma code !!!" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(
        error.is_object(),
        "Should return an error for invalid Lemma code"
    );
}

// ── tools/list structure ────────────────────────────────────────────────

#[test]
fn test_mcp_tools_list_read_only_tools() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(2, "tools/list", json!({})),
        ],
    );

    assert!(responses.len() >= 2);
    let tools = responses[1]["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    let tool_names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    assert!(
        tool_names.contains(&"evaluate"),
        "Should list evaluate tool"
    );
    assert!(
        tool_names.contains(&"list_specs"),
        "Should list list_specs tool"
    );
    assert!(
        tool_names.contains(&"get_schema"),
        "Should list get_schema tool"
    );
    assert_eq!(
        tool_names.len(),
        3,
        "Read-only mode should have exactly 3 tools, got: {:?}",
        tool_names
    );
}

#[test]
fn test_mcp_tools_list_admin_tools() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(2, "tools/list", json!({})),
        ],
    );

    assert!(responses.len() >= 2);
    let tools = responses[1]["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    let tool_names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    assert!(
        tool_names.contains(&"evaluate"),
        "Should list evaluate tool"
    );
    assert!(
        tool_names.contains(&"list_specs"),
        "Should list list_specs tool"
    );
    assert!(
        tool_names.contains(&"get_schema"),
        "Should list get_schema tool"
    );
    assert!(
        tool_names.contains(&"add_spec"),
        "Should list add_spec tool in admin mode"
    );
    assert!(
        tool_names.contains(&"get_spec_source"),
        "Should list get_spec_source tool in admin mode"
    );
    assert_eq!(
        tool_names.len(),
        5,
        "Admin mode should have exactly 5 tools, got: {:?}",
        tool_names
    );
}

#[test]
fn test_mcp_tools_have_input_schemas() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(2, "tools/list", json!({})),
        ],
    );

    assert!(responses.len() >= 2);
    let tools = responses[1]["result"]["tools"]
        .as_array()
        .expect("tools should be an array");

    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            tool["description"].as_str().is_some_and(|d| !d.is_empty()),
            "Tool '{}' should have a non-empty description",
            name
        );
        assert!(
            tool["inputSchema"].is_object(),
            "Tool '{}' should have an inputSchema",
            name
        );
        assert_eq!(
            tool["inputSchema"]["type"], "object",
            "Tool '{}' inputSchema type should be 'object'",
            name
        );
    }
}

// ── evaluate with data overrides ────────────────────────────────────────

#[test]
fn test_mcp_evaluate_with_data_overrides() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(temp_dir.path(), "pricing.lemma", pricing_spec());

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "evaluate",
                    "arguments": {
                        "spec": "pricing",
                        "data": ["quantity=5"]
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("evaluate should return text");

    assert!(
        text.contains("total:"),
        "Should contain rule result, got: {text}"
    );
    assert!(
        text.contains("50"),
        "total should be 5 * 10 = 50, got: {text}"
    );
}

// ── add_spec then evaluate ──────────────────────────────────────────────

#[test]
fn test_mcp_add_spec_then_evaluate() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "add_spec",
                    "arguments": {
                        "code": "spec dynamic\ndata n: 7\nrule doubled: n * 2\n"
                    }
                }),
            ),
            make_request(
                3,
                "tools/call",
                json!({
                    "name": "evaluate",
                    "arguments": { "spec": "dynamic" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 3);

    let add_text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("add_spec should return text");
    assert!(
        add_text.contains("Spec added successfully"),
        "got: {add_text}"
    );

    let eval_text = responses[2]["result"]["content"][0]["text"]
        .as_str()
        .expect("evaluate should return text");
    assert!(
        eval_text.contains("doubled:"),
        "Should contain rule, got: {eval_text}"
    );
    assert!(
        eval_text.contains("14"),
        "doubled should be 14, got: {eval_text}"
    );
}

// ── get_spec_source for missing spec ────────────────────────────────────

#[test]
fn test_mcp_get_spec_source_missing_spec() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        Some(temp_dir.path()),
        true,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "get_spec_source",
                    "arguments": { "spec": "nonexistent" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(error.is_object(), "Should return an error for missing spec");
    assert!(
        error["message"].as_str().unwrap().contains("not found"),
        "Error should say spec not found, got: {}",
        error["message"]
    );
}

// ── evaluate with invalid effective ─────────────────────────────────────

#[test]
fn test_mcp_evaluate_invalid_effective() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "simple.lemma",
        "spec simple\ndata x: 1\nrule y: x\n",
    );

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "evaluate",
                    "arguments": {
                        "spec": "simple",
                        "effective": "not-a-date"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let error = &responses[1]["error"];
    assert!(
        error.is_object(),
        "Should return an error for invalid effective datetime"
    );
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("Invalid effective"),
        "Error should mention invalid effective, got: {}",
        error["message"]
    );
}

#[test]
fn test_mcp_evaluate_respects_effective_for_versioned_spec() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "temporal.lemma",
        r#"spec pricing 2025-01-01
data base: 10
rule total: base

spec pricing 2026-01-01
data base: 99
rule total: base
"#,
    );

    let run_eval = |effective: &str| -> String {
        let responses = mcp_session(
            Some(temp_dir.path()),
            false,
            &[
                make_request(1, "initialize", json!({})),
                make_request(
                    2,
                    "tools/call",
                    json!({
                        "name": "evaluate",
                        "arguments": {
                            "spec": "pricing",
                            "effective": effective,
                            "rule": "total"
                        }
                    }),
                ),
            ],
        );
        assert!(responses.len() >= 2, "expected evaluate response");
        responses[1]["result"]["content"][0]["text"]
            .as_str()
            .expect("evaluate text")
            .to_string()
    };

    let out_2025 = run_eval("2025-06-01");
    let out_2026 = run_eval("2026-06-01");
    assert!(
        out_2025.contains("10") && !out_2025.contains("99"),
        "2025 body should use v2025 base=10; got:\n{out_2025}"
    );
    assert!(
        out_2026.contains("99"),
        "2026 body should use v2026 base=99; got:\n{out_2026}"
    );
}

// ── response IDs match request IDs ──────────────────────────────────────

#[test]
fn test_mcp_response_ids_match_request_ids() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "simple.lemma",
        "spec simple\ndata x: 1\nrule y: x\n",
    );

    let responses = mcp_session(
        Some(temp_dir.path()),
        false,
        &[
            make_request(10, "initialize", json!({})),
            make_request(20, "tools/list", json!({})),
            make_request(
                30,
                "tools/call",
                json!({
                    "name": "list_specs",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["id"], 10, "First response should have id 10");
    assert_eq!(responses[1]["id"], 20, "Second response should have id 20");
    assert_eq!(responses[2]["id"], 30, "Third response should have id 30");
}
