use std::net::TcpStream;
use std::time::{Duration, Instant};

const SERVER_TEST_PORT: u16 = 19998;

fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn test_get_spec_route_returns_200() {
    let temp_dir = tempfile::tempdir().unwrap();
    let lemma_file = temp_dir.path().join("single.lemma");
    std::fs::write(
        &lemma_file,
        r#"spec single_spec
data x: number
rule result: x
"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut child = std::process::Command::new(bin)
        .arg("server")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(SERVER_TEST_PORT.to_string())
        .spawn()
        .unwrap();

    let ok = wait_for_port(SERVER_TEST_PORT, Duration::from_secs(5));
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within 5s");
    }

    let url = format!("http://127.0.0.1:{}/single_spec?x=42", SERVER_TEST_PORT);
    let resp = reqwest::blocking::get(&url).expect("GET request");
    let status = resp.status();
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        status.is_success(),
        "GET /single_spec should return 2xx, got {}",
        status
    );
}

#[test]
fn test_get_with_x_explanations_header_returns_explanation_when_explanations_enabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let lemma_file = temp_dir.path().join("single.lemma");
    std::fs::write(
        &lemma_file,
        r#"spec single_spec
data x: number
rule result: x
"#,
    )
    .unwrap();

    let port = SERVER_TEST_PORT + 1;
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut child = std::process::Command::new(bin)
        .arg("server")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .arg("--explanations")
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, Duration::from_secs(5));
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within 5s");
    }

    let client = reqwest::blocking::Client::new();
    let url = format!("http://127.0.0.1:{}/single_spec", port);
    let resp = client
        .post(&url)
        .header("x-explanations", "true")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("x=42")
        .send()
        .expect("POST request");
    let status = resp.status();
    let body: serde_json::Value =
        serde_json::from_str(&resp.text().expect("response body")).expect("JSON body");
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        status.is_success(),
        "POST with x-explanations should return 2xx, got {}",
        status
    );
    let results = body
        .get("result")
        .expect("response should have envelope 'result' key");
    let rule_result = results
        .get("result")
        .expect("results should have 'result' rule");
    assert!(
        rule_result.get("explanation").is_some(),
        "response should include explanation when x-explanations header sent: {:?}",
        body
    );
    assert_eq!(
        rule_result
            .get("value")
            .and_then(|v: &serde_json::Value| v.as_i64()),
        Some(42)
    );
    assert!(body.get("spec").is_some(), "envelope should include spec");
}

#[test]
fn post_evaluate_accept_datetime_selects_temporal_version() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("temporal.lemma"),
        r#"spec pricing 2025-01-01
data base: 10
rule total: base

spec pricing 2026-01-01
data base: 99
rule total: base
"#,
    )
    .unwrap();

    let port = SERVER_TEST_PORT + 2;
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut child = std::process::Command::new(bin)
        .arg("server")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, std::time::Duration::from_secs(5));
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within 5s");
    }

    let client = reqwest::blocking::Client::new();
    let url = format!("http://127.0.0.1:{}/pricing", port);

    let post = |accept_dt: &str| -> serde_json::Value {
        let resp = client
            .post(&url)
            .header("Accept-Datetime", accept_dt)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("")
            .send()
            .expect("POST");
        let text = resp.text().expect("body");
        serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!("invalid JSON: {e}; body: {text}");
        })
    };

    let j2025 = post("2025-06-01");
    let j2026 = post("2026-06-01");
    let _ = child.kill();
    let _ = child.wait();

    let v2025 = j2025["result"]["total"]["value"]
        .as_i64()
        .expect("total 2025");
    let v2026 = j2026["result"]["total"]["value"]
        .as_i64()
        .expect("total 2026");
    assert_eq!(
        v2025, 10,
        "Accept-Datetime 2025 should resolve pricing v1: {j2025:?}"
    );
    assert_eq!(
        v2026, 99,
        "Accept-Datetime 2026 should resolve pricing v2: {j2026:?}"
    );
}

/// GET `/{spec}` must expose each temporal version's half-open
/// `[effective_from, effective_to)` range. The latest version's `effective_to`
/// is `null` (no successor); earlier versions' `effective_to` equals the next
/// version's `effective_from`.
#[test]
fn get_schema_versions_expose_effective_to_range() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("temporal.lemma"),
        r#"spec pricing 2025-01-01
data base: 10
rule total: base

spec pricing 2026-01-01
data base: 99
rule total: base
"#,
    )
    .unwrap();

    let port = SERVER_TEST_PORT + 3;
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut child = std::process::Command::new(bin)
        .arg("server")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, Duration::from_secs(5));
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within 5s");
    }

    let url = format!("http://127.0.0.1:{}/pricing", port);
    let resp = reqwest::blocking::get(&url).expect("GET request");
    let status = resp.status();
    let body_text = resp.text().expect("response body");
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        status.is_success(),
        "GET /pricing should return 2xx, got {status}: {body_text}"
    );
    let body: serde_json::Value = serde_json::from_str(&body_text)
        .unwrap_or_else(|e| panic!("invalid JSON: {e}; {body_text}"));

    let versions = body["versions"]
        .as_array()
        .unwrap_or_else(|| panic!("'versions' must be an array: {body}"));
    assert_eq!(
        versions.len(),
        2,
        "two temporal versions loaded, got: {body}"
    );

    let earlier = &versions[0];
    assert_eq!(
        earlier["effective_from"].as_str(),
        Some("2025-01-01"),
        "earlier version effective_from: {earlier}"
    );
    assert_eq!(
        earlier["effective_to"].as_str(),
        Some("2026-01-01"),
        "earlier version effective_to equals next version's effective_from: {earlier}"
    );

    let latest = &versions[1];
    assert_eq!(
        latest["effective_from"].as_str(),
        Some("2026-01-01"),
        "latest version effective_from: {latest}"
    );
    assert!(
        latest["effective_to"].is_null(),
        "latest version effective_to must be null (no successor): {latest}"
    );
}
