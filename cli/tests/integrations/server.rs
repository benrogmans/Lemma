use std::net::TcpStream;
use std::time::{Duration, Instant};

const SERVER_TEST_PORT: u16 = 19998;
const SERVER_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

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
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(SERVER_TEST_PORT.to_string())
        .spawn()
        .unwrap();

    let ok = wait_for_port(SERVER_TEST_PORT, SERVER_STARTUP_TIMEOUT);
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within timeout");
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
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .arg("--explanations")
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, SERVER_STARTUP_TIMEOUT);
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within timeout");
    }

    let client = reqwest::blocking::Client::new();
    let url = format!("http://127.0.0.1:{}/single_spec", port);
    let resp = client
        .post(&url)
        .header("x-explanations", "true")
        .header("Content-Type", "application/json")
        .body(r#"{"x":"42"}"#)
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
        .get("results")
        .expect("response should have envelope 'results' key");
    let rule_result = results
        .get("result")
        .expect("results should have 'result' rule");
    assert!(
        rule_result.get("explanation").is_some(),
        "response should include explanation when x-explanations header sent: {:?}",
        body
    );
    assert_eq!(rule_result["number"].as_str(), Some("42"));
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
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, SERVER_STARTUP_TIMEOUT);
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within timeout");
    }

    let client = reqwest::blocking::Client::new();
    let url = format!("http://127.0.0.1:{}/pricing", port);

    let post = |accept_dt: &str| -> serde_json::Value {
        let resp = client
            .post(&url)
            .header("Accept-Datetime", accept_dt)
            .header("Content-Type", "application/json")
            .body("{}")
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

    assert_eq!(
        j2025["results"]["total"]["number"].as_str(),
        Some("10"),
        "Accept-Datetime 2025 should resolve pricing v1: {j2025:?}"
    );
    assert_eq!(
        j2026["results"]["total"]["number"].as_str(),
        Some("99"),
        "Accept-Datetime 2026 should resolve pricing v2: {j2026:?}"
    );
}

#[test]
fn post_evaluate_form_urlencoded_body() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("single.lemma"),
        r#"spec single_spec
data x: number
rule result: x
"#,
    )
    .unwrap();

    let port = SERVER_TEST_PORT + 4;
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut child = std::process::Command::new(bin)
        .arg("server")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, SERVER_STARTUP_TIMEOUT);
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within timeout");
    }

    let client = reqwest::blocking::Client::new();
    let url = format!("http://127.0.0.1:{}/single_spec", port);
    let resp = client
        .post(&url)
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
        "POST form body should return 2xx, got {status}: {body:?}"
    );
    assert_eq!(body["results"]["result"]["number"].as_str(), Some("42"));
}

/// GET `/` must return one entry per loaded spec: `{name, schema}` on success
/// or `{name, error}` when the schema cannot be produced at the requested
/// effective date. No spec may silently disappear from the list.
#[test]
fn list_specs_surfaces_per_spec_errors() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("specs.lemma"),
        r#"spec always_available
data x: number
rule result: x

spec future_only 2030-01-01
data y: number
rule result: y
"#,
    )
    .unwrap();

    let port = SERVER_TEST_PORT + 5;
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut child = std::process::Command::new(bin)
        .arg("server")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, SERVER_STARTUP_TIMEOUT);
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within timeout");
    }

    let url = format!("http://127.0.0.1:{}/?effective=2025-06-01", port);
    let resp = reqwest::blocking::get(&url).expect("GET request");
    let status = resp.status();
    let body_text = resp.text().expect("response body");
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        status.is_success(),
        "GET / should return 2xx, got {status}: {body_text}"
    );
    let body: serde_json::Value = serde_json::from_str(&body_text)
        .unwrap_or_else(|e| panic!("invalid JSON: {e}; {body_text}"));
    let entries = body
        .as_array()
        .unwrap_or_else(|| panic!("list response must be an array: {body}"));

    let available = entries
        .iter()
        .find(|e| e["name"].as_str() == Some("always_available"))
        .unwrap_or_else(|| panic!("always_available missing from list: {body}"));
    assert!(
        available.get("schema").is_some(),
        "resolvable spec must carry a schema entry: {available}"
    );

    let future = entries
        .iter()
        .find(|e| e["name"].as_str() == Some("future_only"))
        .unwrap_or_else(|| panic!("future_only must not silently disappear: {body}"));
    assert!(
        future.get("error").is_some(),
        "unresolvable spec must carry an error entry: {future}"
    );
}

/// By default the server sends no CORS headers: cross-origin browser
/// requests are denied. `--cors` opts in to permissive CORS.
#[test]
fn cors_denied_by_default_and_enabled_with_flag() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        temp_dir.path().join("single.lemma"),
        r#"spec single_spec
data x: number
rule result: x
"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_lemma");
    let run_case = |port: u16, cors_flag: bool| -> Option<String> {
        let mut cmd = std::process::Command::new(bin);
        cmd.arg("server")
            .arg("--prefix")
            .arg(temp_dir.path())
            .arg("--port")
            .arg(port.to_string());
        if cors_flag {
            cmd.arg("--cors");
        }
        let mut child = cmd.spawn().unwrap();

        let ok = wait_for_port(port, SERVER_STARTUP_TIMEOUT);
        if !ok {
            let _ = child.kill();
            let _ = child.wait();
            panic!("server did not start within timeout");
        }

        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .header("Origin", "https://evil.example")
            .send()
            .expect("GET request");
        let allow_origin = resp
            .headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let _ = child.kill();
        let _ = child.wait();
        allow_origin
    };

    let default_origin = run_case(SERVER_TEST_PORT + 6, false);
    assert!(
        default_origin.is_none(),
        "default must not send Access-Control-Allow-Origin, got {default_origin:?}"
    );

    let opt_in_origin = run_case(SERVER_TEST_PORT + 7, true);
    assert_eq!(
        opt_in_origin.as_deref(),
        Some("*"),
        "--cors must send permissive Access-Control-Allow-Origin"
    );
}

/// `--eval-timeout 0` makes every evaluation exceed the wall-clock budget;
/// the server must answer 503 with a JSON error instead of hanging.
/// The spec carries a long rule chain so plan+eval take real work and the
/// zero-length budget always elapses before the blocking task finishes.
#[test]
fn evaluation_timeout_returns_503() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut spec = String::from("spec slow_spec\ndata x: number\nrule r0: x + 1\n");
    for i in 1..100 {
        spec.push_str(&format!("rule r{i}: r{} * 2 + {i}\n", i - 1));
    }
    std::fs::write(temp_dir.path().join("slow.lemma"), spec).unwrap();

    let port = SERVER_TEST_PORT + 8;
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut child = std::process::Command::new(bin)
        .arg("server")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .arg("--eval-timeout")
        .arg("0")
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, SERVER_STARTUP_TIMEOUT);
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within timeout");
    }

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/slow_spec"))
        .header("Content-Type", "application/json")
        .body(r#"{"x":"42"}"#)
        .send()
        .expect("POST request");
    let status = resp.status();
    let body: serde_json::Value =
        serde_json::from_str(&resp.text().expect("response body")).expect("JSON body");
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(
        status.as_u16(),
        503,
        "zero timeout must yield 503, got {status}: {body:?}"
    );
    assert!(
        body["error"]
            .as_str()
            .expect("error message present")
            .contains("timed out"),
        "error must mention timeout: {body:?}"
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
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, SERVER_STARTUP_TIMEOUT);
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within timeout");
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
