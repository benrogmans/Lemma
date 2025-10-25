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
    // Just verify the server command is recognized in help
    // We don't actually start the server as it would hang the test
    let mut cmd = Command::cargo_bin("lemma").unwrap();
    cmd.arg("server").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicates::str::contains("Workspace root directory"));
}
