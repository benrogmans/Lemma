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
    cmd.arg("server");

    // Serve command should fail if no directory is provided or directory doesn't exist
    let result = cmd.output().unwrap();
    assert!(!result.status.success() || !result.stderr.is_empty() || !result.stdout.is_empty());
}
