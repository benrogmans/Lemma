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
/// `prefix: None` runs `lemma mcp` without `--prefix` (defaults to process cwd).
fn mcp_session(
    prefix: Option<&std::path::Path>,
    admin: bool,
    messages: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    mcp_session_in_dir(prefix, None, admin, messages)
}

fn mcp_session_in_dir(
    prefix: Option<&std::path::Path>,
    current_dir: Option<&std::path::Path>,
    admin: bool,
    messages: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp");
    if let Some(p) = prefix {
        cmd.arg("--prefix").arg(p);
    }
    if let Some(dir) = current_dir {
        cmd.current_dir(dir);
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
fn test_mcp_list_returns_list() {
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
                    "name": "list",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 3, "Expected at least 3 responses");

    let list_result = &responses[2]["result"]["content"][0]["text"];
    let text = list_result.as_str().expect("list should return text");
    let list: serde_json::Value = serde_json::from_str(text).expect("list should return list JSON");

    let workspace = list
        .as_array()
        .and_then(|groups| {
            groups
                .iter()
                .find(|g| g["repository"].is_null())
                .map(|g| g["specs"].as_array())
        })
        .flatten()
        .expect("workspace group with specs");
    assert!(
        workspace.iter().any(|row| row["name"] == "pricing"),
        "list should include pricing, got: {text}"
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
        text.contains("quantity >= 10"),
        "Should state the matching condition as a fact in reasoning, got: {text}"
    );
    assert!(
        text.contains("quantity: 25"),
        "Should show the data value that drove the conditions, got: {text}"
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
        !tool_names.contains(&"source"),
        "source should not be listed in read-only mode, got: {:?}",
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
                        "code": "spec test_spec\ndata x: 5\nrule y: x * 2",
                        "source_id": "test_spec.lemma"
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
        tool_names.contains(&"source"),
        "source should be listed with --admin, got: {:?}",
        tool_names
    );

    // add_spec should succeed and return structured show JSON
    let add_result = &responses[2]["result"]["content"][0]["text"];
    let text = add_result.as_str().expect("add_spec should return text");
    let payload: serde_json::Value =
        serde_json::from_str(text).expect("add_spec should return JSON");
    assert_eq!(
        payload["message"].as_str(),
        Some("Spec added successfully.")
    );
    let specs = payload["specs"]
        .as_array()
        .expect("add_spec payload should include specs array");
    assert!(
        specs.iter().any(|s| s["spec"] == "test_spec"),
        "Should include show for test_spec, got: {text}"
    );
    assert!(
        specs[0]["rules"]["y"].is_object(),
        "Should include rule types in show JSON, got: {text}"
    );
}

#[test]
fn test_mcp_source() {
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
                    "name": "source",
                    "arguments": {
                        "spec": "pricing"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2, "Expected at least 2 responses");

    let source_result = &responses[1]["result"]["content"][0]["text"];
    let text = source_result.as_str().expect("source should return text");

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
fn test_mcp_source_embedded_lemma_repository() {
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
                    "name": "source",
                    "arguments": {
                        "repository": "lemma"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("source should return text");
    assert!(
        text.contains("repo lemma")
            && text.contains("spec units")
            && text.contains("trait duration"),
        "Should return formatted embedded stdlib, got: {text}"
    );
}

#[test]
fn test_mcp_source_blocked_without_admin() {
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
                    "name": "source",
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
        "source should return an error without --admin"
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

// ── show ──────────────────────────────────────────────────────────

#[test]
fn test_mcp_show_full_spec() {
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
                    "name": "show",
                    "arguments": { "spec": "pricing" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("show should return text");

    assert!(
        text.contains("pricing"),
        "Should mention spec name, got: {text}"
    );
    assert!(text.contains("quantity"), "Should list data, got: {text}");
    assert!(text.contains("base_price"), "Should list data, got: {text}");
    assert!(text.contains("total"), "Should list rules, got: {text}");
}

#[test]
fn test_mcp_show_full_spec_rules() {
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
                    "name": "show",
                    "arguments": { "spec": "multi" }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("show should return text");

    assert!(
        text.contains("sum") && text.contains("product"),
        "Should list all rules, got: {text}"
    );
}

#[test]
fn test_mcp_show_missing_spec() {
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
                    "name": "show",
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
fn test_mcp_show_empty_spec_name() {
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
                    "name": "show",
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

// ── list edge cases ─────────────────────────────────────────────────────

#[test]
fn test_mcp_list_empty_workspace() {
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
                    "name": "list",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("list should return text");
    let list: serde_json::Value = serde_json::from_str(text).expect("list should return JSON");
    let embedded = list
        .as_array()
        .and_then(|groups| groups.iter().find(|g| g["repository"] == "lemma"))
        .expect("embedded lemma repository group");
    assert!(
        embedded["specs"]
            .as_array()
            .and_then(|specs| specs.first())
            .and_then(|row| row["name"].as_str())
            == Some("units"),
        "embedded stdlib must appear in list, got: {text}"
    );
}

#[test]
fn test_mcp_list_empty_workspace_admin_suggests_add() {
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
                    "name": "list",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("list should return text");
    assert!(
        text.contains("\"repository\": \"lemma\"") && text.contains("\"name\": \"units\""),
        "embedded stdlib must appear, got: {text}"
    );
    assert!(
        text.contains("add_spec"),
        "Admin mode should suggest using add_spec when workspace is empty, got: {text}"
    );
}

#[test]
fn test_mcp_defaults_prefix_to_cwd() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(temp_dir.path(), "pricing.lemma", pricing_spec());

    let responses = mcp_session_in_dir(
        None,
        Some(temp_dir.path()),
        false,
        &[
            make_request(1, "initialize", json!({})),
            make_request(
                2,
                "tools/call",
                json!({
                    "name": "list",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .expect("list should return text");
    let list: serde_json::Value = serde_json::from_str(text).expect("list should return JSON");
    let workspace = list
        .as_array()
        .and_then(|groups| {
            groups
                .iter()
                .find(|g| g["repository"].is_null())
                .map(|g| g["specs"].as_array())
        })
        .flatten()
        .expect("workspace group");
    assert!(
        workspace.iter().any(|row| row["name"] == "pricing"),
        "workspace specs must load when --prefix is omitted, got: {text}"
    );
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
    let temp_dir = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp").current_dir(temp_dir.path());
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

// ── stdin hardening ─────────────────────────────────────────────────────

/// A stdin line over the 10 MiB cap must be rejected with a JSON-RPC parse
/// error, and the server must stay in sync to serve subsequent requests.
#[test]
fn test_mcp_oversized_line_rejected_then_recovers() {
    let temp_dir = tempfile::tempdir().unwrap();
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp").current_dir(temp_dir.path());
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("Failed to start MCP server");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    let mut input = Vec::with_capacity(11 * 1024 * 1024);
    input.resize(10 * 1024 * 1024 + 1, b'x');
    input.push(b'\n');
    input.extend_from_slice(
        serde_json::to_string(&make_request(1, "initialize", json!({})))
            .unwrap()
            .as_bytes(),
    );
    input.push(b'\n');
    stdin.write_all(&input).unwrap();
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

    assert_eq!(responses.len(), 2, "expected error + initialize response");
    assert_eq!(
        responses[0]["error"]["code"], -32700,
        "oversized line must yield parse error: {}",
        responses[0]
    );
    assert!(
        responses[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("exceeds"),
        "error should mention the byte cap: {}",
        responses[0]
    );
    assert!(
        responses[1]["result"].is_object(),
        "server must recover and answer the next request: {}",
        responses[1]
    );
}

/// `--request-timeout 0` makes every request exceed its wall-clock budget;
/// the server must answer with a JSON-RPC internal error instead of hanging.
/// The spec carries a rule chain so handling takes real work and the
/// zero-length budget always elapses before the worker finishes.
#[test]
fn test_mcp_request_timeout_returns_error() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut spec = String::from("spec slow_spec\ndata x: number\nrule r0: x + 1\n");
    for i in 1..100 {
        spec.push_str(&format!("rule r{i}: r{} * 2 + {i}\n", i - 1));
    }
    write_spec(temp_dir.path(), "slow.lemma", &spec);

    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("--request-timeout")
        .arg("0");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().expect("Failed to start MCP server");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    let request = make_request(
        1,
        "tools/call",
        json!({
            "name": "evaluate",
            "arguments": { "spec": "slow_spec", "data": ["x=1"] }
        }),
    );
    let mut input = serde_json::to_string(&request).unwrap();
    input.push('\n');
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

    assert_eq!(responses.len(), 1, "expected exactly one timeout response");
    assert_eq!(responses[0]["id"], 1, "response id must match request");
    assert!(
        responses[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("timed out"),
        "error should mention timeout: {}",
        responses[0]
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
                    "arguments": {
                        "code": "this is not valid lemma code !!!",
                        "source_id": "invalid.lemma"
                    }
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
    assert!(tool_names.contains(&"list"), "Should list list tool");
    assert!(tool_names.contains(&"show"), "Should list show tool");
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
    assert!(tool_names.contains(&"list"), "Should list list tool");
    assert!(tool_names.contains(&"show"), "Should list show tool");
    assert!(
        tool_names.contains(&"add_spec"),
        "Should list add_spec tool in admin mode"
    );
    assert!(
        tool_names.contains(&"source"),
        "Should list source tool in admin mode"
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
                        "code": "spec dynamic\ndata n: 7\nrule doubled: n * 2\n",
                        "source_id": "dynamic.lemma"
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
    let payload: serde_json::Value =
        serde_json::from_str(add_text).expect("add_spec should return JSON");
    assert_eq!(
        payload["message"].as_str(),
        Some("Spec added successfully.")
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

// ── source for missing spec ─────────────────────────────────────────────

#[test]
fn test_mcp_source_missing_spec() {
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
                    "name": "source",
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
                    "name": "list",
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

#[test]
fn mcp_add_spec_without_source_id_must_require_source_id() {
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
                        "code": "spec test_spec\ndata x: 5\nrule y: x * 2"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    assert!(
        responses[1]["error"].is_object(),
        "add_spec without source_id must return error, got: {}",
        responses[1]
    );
}

#[test]
fn mcp_evaluate_veto_must_not_invent_vetoed_placeholder() {
    let temp_dir = tempfile::tempdir().unwrap();
    write_spec(
        temp_dir.path(),
        "veto_no_message.lemma",
        "spec veto_no_message\ndata value: -5\nrule r: value > 0\n    unless value < 0 then veto\n",
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
                        "spec": "veto_no_message"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2);
    let eval_result = &responses[1]["result"]["content"][0]["text"];
    let text = eval_result.as_str().expect("evaluate should return text");
    assert!(
        !text.contains("Vetoed"),
        "MCP must not invent 'Vetoed' placeholder when veto_reason missing, got: {text}"
    );
}
