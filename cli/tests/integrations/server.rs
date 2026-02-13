use assert_cmd::cargo::cargo_bin_cmd;

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
