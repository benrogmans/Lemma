use assert_cmd::Command;

#[test]
fn test_server_command_available() {
    let mut cmd = Command::cargo_bin("lemma").unwrap();
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stdout(predicates::str::contains("server"));
}

#[test]
fn test_serve_requires_dir() {
    let mut cmd = Command::cargo_bin("lemma").unwrap();
    cmd.arg("server")
        .arg("--dir")
        .arg("/nonexistent/directory/that/does/not/exist");

    // Serve command should fail if directory doesn't exist
    cmd.assert().failure();
}
