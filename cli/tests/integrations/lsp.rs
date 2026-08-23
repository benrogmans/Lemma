use super::lsp_session::{path_to_uri, write_lemma_file, LspSession};
use serde_json::json;
use std::fs;
use std::path::Path;

const LSP_ERROR_SEVERITY: i64 = 1;

fn coffee_order_fixture() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/integrations/fixtures_coffee_order.lemma");
    fs::read_to_string(path).expect("read coffee_order fixture")
}

#[test]
fn lsp_initialize_reports_capabilities() {
    let mut session = LspSession::spawn_lemma_lsp();
    let result = session.initialize(None);
    session.initialized();

    let caps = &result["capabilities"];
    assert!(
        caps.get("documentFormattingProvider").is_some(),
        "missing documentFormattingProvider: {caps}"
    );
    assert!(
        caps.get("semanticTokensProvider").is_some(),
        "missing semanticTokensProvider: {caps}"
    );
    assert!(
        caps.get("documentLinkProvider").is_some(),
        "missing documentLinkProvider: {caps}"
    );

    session.shutdown();
}

#[test]
fn lsp_stdio_flag_initializes() {
    let mut session = LspSession::spawn_lemma_lsp_with_stdio_flag();
    let result = session.initialize(None);
    session.initialized();
    assert!(
        result.get("capabilities").is_some(),
        "lemma lsp --stdio must initialize: {result}"
    );
    session.shutdown();
}

#[test]
fn lsp_parse_error_publishes_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_lemma_file(dir.path(), "broken.lemma", "not lemma syntax");
    let uri = path_to_uri(&path);

    let mut session = LspSession::spawn_lemma_lsp();
    session.initialize(None);
    session.initialized();
    session.did_open(&uri, "not lemma syntax");

    let notification = session.wait_for_diagnostics(&uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        !diagnostics.is_empty(),
        "expected parse diagnostics, got: {notification}"
    );
    assert_eq!(
        diagnostics[0]["severity"].as_i64(),
        Some(LSP_ERROR_SEVERITY),
        "expected Error severity"
    );

    session.shutdown();
}

#[test]
fn lsp_valid_spec_has_no_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"spec pricing 2026-01-01
"""
Bulk pricing.
"""

data qty: number
  -> minimum 0
  -> maximum 1000000
  -> help "Order quantity."
  -> suggest 10

rule total: qty
"#;
    let path = write_lemma_file(dir.path(), "valid.lemma", source);
    let uri = path_to_uri(&path);

    let mut session = LspSession::spawn_lemma_lsp();
    session.initialize(None);
    session.initialized();
    session.did_open(&uri, source);

    let notification = session.wait_for_diagnostics(&uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics for valid quality-clean spec, got: {diagnostics:?}"
    );

    session.shutdown();
}

#[test]
fn lsp_quality_issues_are_not_hint_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = r#"spec pricing
data qty: number
rule total: qty
"#;
    let path = write_lemma_file(dir.path(), "quality_gaps.lemma", source);
    let uri = path_to_uri(&path);

    let mut session = LspSession::spawn_lemma_lsp();
    session.initialize(None);
    session.initialized();
    session.did_open(&uri, source);

    let notification = session.wait_for_diagnostics(&uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics.is_empty(),
        "quality recommendations must not publish as LSP diagnostics, got: {diagnostics:?}"
    );

    session.shutdown();
}

#[test]
fn lsp_formatting_returns_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let messy = "spec messy\ndata   x:1\nrule y:x+1\n";
    let path = write_lemma_file(dir.path(), "messy.lemma", messy);
    let uri = path_to_uri(&path);

    let mut session = LspSession::spawn_lemma_lsp();
    session.initialize(None);
    session.initialized();
    session.did_open(&uri, messy);

    let result = session.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": uri },
            "options": { "tabSize": 2, "insertSpaces": true }
        }),
    );

    let edits = result.as_array().expect("formatting edits array");
    assert!(
        !edits.is_empty(),
        "expected formatting TextEdit[], got: {result:?}"
    );

    session.shutdown();
}

#[test]
fn lsp_formatting_skips_parse_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let broken = "this is not lemma";
    let path = write_lemma_file(dir.path(), "broken.lemma", broken);
    let uri = path_to_uri(&path);

    let mut session = LspSession::spawn_lemma_lsp();
    session.initialize(None);
    session.initialized();
    session.did_open(&uri, broken);

    let result = session.request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": uri },
            "options": { "tabSize": 2, "insertSpaces": true }
        }),
    );

    assert!(
        result.is_null(),
        "formatting must return null for unparseable spec, got: {result:?}"
    );

    session.shutdown();
}

#[test]
fn lsp_semantic_tokens_on_valid_spec() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = coffee_order_fixture();
    let path = write_lemma_file(dir.path(), "coffee.lemma", &source);
    let uri = path_to_uri(&path);

    let mut session = LspSession::spawn_lemma_lsp();
    session.initialize(None);
    session.initialized();
    session.did_open(&uri, &source);

    let result = session.request(
        "textDocument/semanticTokens/full",
        json!({
            "textDocument": { "uri": uri }
        }),
    );

    let data = result["data"]
        .as_array()
        .expect("semantic token data array");
    assert!(
        !data.is_empty(),
        "expected semantic tokens for coffee_order, got: {result:?}"
    );

    session.shutdown();
}

#[test]
fn lsp_did_close_clears_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let broken = "not lemma syntax";
    let path = write_lemma_file(dir.path(), "broken.lemma", broken);
    let uri = path_to_uri(&path);

    let mut session = LspSession::spawn_lemma_lsp();
    session.initialize(None);
    session.initialized();
    session.did_open(&uri, broken);

    let with_errors = session.wait_for_diagnostics(&uri);
    assert!(
        !with_errors["params"]["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .is_empty(),
        "expected diagnostics before close"
    );

    session.did_close(&uri);
    let cleared = session.wait_for_diagnostics(&uri);
    let diagnostics = cleared["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics.is_empty(),
        "didClose must publish empty diagnostics, got: {diagnostics:?}"
    );

    session.shutdown();
}

#[test]
fn lsp_workspace_missing_ref_diagnostic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = "spec orphan\nuses other: nonexistent\n";
    let path = write_lemma_file(dir.path(), "orphan.lemma", source);
    let uri = path_to_uri(&path);
    let root_uri = path_to_uri(dir.path());

    let mut session = LspSession::spawn_lemma_lsp();
    session.initialize(Some(&root_uri));
    session.initialized();

    let notification = session.wait_for_diagnostics(&uri);
    let diagnostics = notification["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        !diagnostics.is_empty(),
        "workspace discovery must publish planning diagnostic for missing spec, got: {notification}"
    );

    session.shutdown();
}

#[test]
fn lsp_duplicate_spec_reports_conflict_on_discovered_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path_a = write_lemma_file(dir.path(), "a.lemma", "spec conflict_test\ndata x: 1");
    let path_b = write_lemma_file(dir.path(), "b.lemma", "spec conflict_test\ndata x: 2");
    let uri_a = path_to_uri(&path_a);
    let uri_b = path_to_uri(&path_b);
    let root_uri = path_to_uri(dir.path());

    let mut session = LspSession::spawn_lemma_lsp();
    session.initialize(Some(&root_uri));
    session.initialized();

    let notification_a = session.wait_for_diagnostics(&uri_a);
    let diagnostics_a = notification_a["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        !diagnostics_a.is_empty(),
        "workspace must publish duplicate spec diagnostic on first file, got: {notification_a}"
    );
    assert_eq!(
        diagnostics_a[0]["severity"].as_i64(),
        Some(LSP_ERROR_SEVERITY),
        "expected Error severity on first file"
    );
    let message_a = diagnostics_a[0]["message"].as_str().unwrap_or("");
    assert!(
        message_a.contains("conflict_test") && message_a.contains("also declared"),
        "expected conflict message on first file, got: {message_a}"
    );

    let notification_b = session.wait_for_diagnostics(&uri_b);
    let diagnostics_b = notification_b["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        !diagnostics_b.is_empty(),
        "workspace must publish duplicate spec diagnostic on second file, got: {notification_b}"
    );
    let message_b = diagnostics_b[0]["message"].as_str().unwrap_or("");
    assert!(
        message_b.contains("conflict_test") && message_b.contains("also declared"),
        "expected conflict message on second file, got: {message_b}"
    );

    session.shutdown();
}
