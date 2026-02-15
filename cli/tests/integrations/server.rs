use assert_cmd::cargo::cargo_bin_cmd;
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
fn test_get_document_route_returns_200() {
    let temp_dir = tempfile::tempdir().unwrap();
    let lemma_file = temp_dir.path().join("single.lemma");
    std::fs::write(
        &lemma_file,
        r#"doc single_doc
fact x = [number]
rule result = x
"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut child = std::process::Command::new(bin)
        .arg("server")
        .arg("--dir")
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

    let url = format!("http://127.0.0.1:{}/single_doc?x=42", SERVER_TEST_PORT);
    let resp = reqwest::blocking::get(&url).expect("GET request");
    let status = resp.status();
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        status.is_success(),
        "GET /single_doc should return 2xx, got {}",
        status
    );
}

#[test]
fn test_server_command_available() {
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stdout(predicates::str::contains("server"));
}

#[test]
fn test_server_help_shows_new_routes() {
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("server").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Workspace root directory"))
        .stdout(predicates::str::contains("--watch"))
        .stdout(predicates::str::contains("/docs"))
        .stdout(predicates::str::contains("/openapi.json"));
}

#[test]
fn test_server_help_shows_watch_flag() {
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("server").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicates::str::contains("--watch"))
        .stdout(predicates::str::contains("Watch workspace"));
}

#[test]
fn test_server_help_shows_proofs_flag() {
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("server").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicates::str::contains("--proofs"))
        .stdout(predicates::str::contains("x-proofs"));
}

#[test]
fn test_get_with_x_proofs_header_returns_proof_when_proofs_enabled() {
    let temp_dir = tempfile::tempdir().unwrap();
    let lemma_file = temp_dir.path().join("single.lemma");
    std::fs::write(
        &lemma_file,
        r#"doc single_doc
fact x = [number]
rule result = x
"#,
    )
    .unwrap();

    let port = SERVER_TEST_PORT + 1;
    let bin = env!("CARGO_BIN_EXE_lemma");
    let mut child = std::process::Command::new(bin)
        .arg("server")
        .arg("--dir")
        .arg(temp_dir.path())
        .arg("--port")
        .arg(port.to_string())
        .arg("--proofs")
        .spawn()
        .unwrap();

    let ok = wait_for_port(port, Duration::from_secs(5));
    if !ok {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not start within 5s");
    }

    let client = reqwest::blocking::Client::new();
    let url = format!("http://127.0.0.1:{}/single_doc?x=42", port);
    let resp = client
        .get(&url)
        .header("x-proofs", "true")
        .send()
        .expect("GET request");
    let status = resp.status();
    let body: serde_json::Value =
        serde_json::from_str(&resp.text().expect("response body")).expect("JSON body");
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        status.is_success(),
        "GET with x-proofs should return 2xx, got {}",
        status
    );
    let result = body
        .get("result")
        .expect("response should have 'result' key");
    assert!(
        result.get("proof").is_some(),
        "response should include proof when x-proofs header sent: {:?}",
        body
    );
    assert_eq!(
        result
            .get("value")
            .and_then(|v: &serde_json::Value| v.as_str()),
        Some("42")
    );
}
