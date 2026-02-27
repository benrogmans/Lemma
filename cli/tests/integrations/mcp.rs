use assert_cmd::cargo::cargo_bin_cmd;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn test_mcp_server_starts() {
    let mut help_cmd = cargo_bin_cmd!("lemma");
    help_cmd.arg("--help");
    help_cmd
        .assert()
        .success()
        .stdout(predicates::str::contains("mcp"));
}

#[test]
fn test_mcp_help_shows_admin_flag() {
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.args(["mcp", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("--admin"));
}

/// Send JSON-RPC messages to the MCP server and collect responses.
fn mcp_session(
    workdir: &std::path::Path,
    admin: bool,
    messages: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut cmd = Command::new(bin);
    cmd.arg("mcp").arg("--dir").arg(workdir);
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
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
}

#[test]
fn test_mcp_list_documents_includes_schema() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("pricing.lemma"),
        "doc pricing\nfact quantity: [number]\nfact base_price: 10\nrule total: quantity * base_price\n",
    )
    .unwrap();

    let responses = mcp_session(
        temp_dir.path(),
        false,
        &[
            make_request(1, "initialize", serde_json::json!({})),
            make_request(2, "tools/list", serde_json::json!({})),
            make_request(
                3,
                "tools/call",
                serde_json::json!({
                    "name": "list_documents",
                    "arguments": {}
                }),
            ),
        ],
    );

    assert!(responses.len() >= 3, "Expected at least 3 responses");

    let list_result = &responses[2]["result"]["content"][0]["text"];
    let text = list_result
        .as_str()
        .expect("list_documents should return text");

    assert!(
        text.contains("Document: pricing"),
        "Should contain document name, got: {text}"
    );
    assert!(
        text.contains("quantity"),
        "Should list fact names, got: {text}"
    );
    assert!(
        text.contains("base_price"),
        "Should list fact names, got: {text}"
    );
    assert!(
        text.contains("total"),
        "Should list rule names, got: {text}"
    );
}

#[test]
fn test_mcp_evaluate_includes_reasoning() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("discount.lemma"),
        "doc discount\nfact quantity: [number]\nrule rate: 0 percent\n unless quantity >= 10 then 10 percent\n unless quantity >= 50 then 20 percent\n",
    )
    .unwrap();

    let responses = mcp_session(
        temp_dir.path(),
        false,
        &[
            make_request(1, "initialize", serde_json::json!({})),
            make_request(
                2,
                "tools/call",
                serde_json::json!({
                    "name": "evaluate",
                    "arguments": {
                        "document": "discount",
                        "rule": "rate",
                        "facts": ["quantity=25"]
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
        "Should show fact value in reasoning, got: {text}"
    );
}

#[test]
fn test_mcp_read_only_by_default() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        temp_dir.path(),
        false,
        &[
            make_request(1, "initialize", serde_json::json!({})),
            make_request(2, "tools/list", serde_json::json!({})),
            make_request(
                3,
                "tools/call",
                serde_json::json!({
                    "name": "add_document",
                    "arguments": {
                        "code": "doc test\nfact x: 5\nrule y: x"
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
        !tool_names.contains(&"add_document"),
        "add_document should not be listed in read-only mode, got: {:?}",
        tool_names
    );
    assert!(
        !tool_names.contains(&"get_document_source"),
        "get_document_source should not be listed in read-only mode, got: {:?}",
        tool_names
    );

    // Calling add_document should return an error
    let error = &responses[2]["error"];
    assert!(
        error.is_object(),
        "add_document should return an error in read-only mode"
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
fn test_mcp_admin_enables_add_document() {
    let temp_dir = tempfile::tempdir().unwrap();

    let responses = mcp_session(
        temp_dir.path(),
        true,
        &[
            make_request(1, "initialize", serde_json::json!({})),
            make_request(2, "tools/list", serde_json::json!({})),
            make_request(
                3,
                "tools/call",
                serde_json::json!({
                    "name": "add_document",
                    "arguments": {
                        "code": "doc test_doc\nfact x: 5\nrule y: x * 2"
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
        tool_names.contains(&"add_document"),
        "add_document should be listed with --admin, got: {:?}",
        tool_names
    );
    assert!(
        tool_names.contains(&"get_document_source"),
        "get_document_source should be listed with --admin, got: {:?}",
        tool_names
    );

    // add_document should succeed and return schema
    let add_result = &responses[2]["result"]["content"][0]["text"];
    let text = add_result
        .as_str()
        .expect("add_document should return text");
    assert!(
        text.contains("Document added successfully"),
        "Should confirm success, got: {text}"
    );
    assert!(
        text.contains("Document: test_doc"),
        "Should include document name in schema, got: {text}"
    );
    assert!(
        text.contains("y"),
        "Should include rule name in schema, got: {text}"
    );
}

#[test]
fn test_mcp_get_document_source() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("pricing.lemma"),
        "doc pricing\nfact quantity: [number]\nfact base_price: 10\nrule total: quantity * base_price\n",
    )
    .unwrap();

    let responses = mcp_session(
        temp_dir.path(),
        true,
        &[
            make_request(1, "initialize", serde_json::json!({})),
            make_request(
                2,
                "tools/call",
                serde_json::json!({
                    "name": "get_document_source",
                    "arguments": {
                        "document": "pricing"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2, "Expected at least 2 responses");

    let source_result = &responses[1]["result"]["content"][0]["text"];
    let text = source_result
        .as_str()
        .expect("get_document_source should return text");

    assert!(
        text.contains("doc pricing"),
        "Should contain doc declaration, got: {text}"
    );
    assert!(
        text.contains("fact quantity"),
        "Should contain fact declarations, got: {text}"
    );
    assert!(
        text.contains("rule total"),
        "Should contain rule declarations, got: {text}"
    );
}

#[test]
fn test_mcp_get_document_source_blocked_without_admin() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("pricing.lemma"),
        "doc pricing\nfact x: 5\nrule y: x\n",
    )
    .unwrap();

    let responses = mcp_session(
        temp_dir.path(),
        false,
        &[
            make_request(1, "initialize", serde_json::json!({})),
            make_request(
                2,
                "tools/call",
                serde_json::json!({
                    "name": "get_document_source",
                    "arguments": {
                        "document": "pricing"
                    }
                }),
            ),
        ],
    );

    assert!(responses.len() >= 2, "Expected at least 2 responses");

    let error = &responses[1]["error"];
    assert!(
        error.is_object(),
        "get_document_source should return an error without --admin"
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
