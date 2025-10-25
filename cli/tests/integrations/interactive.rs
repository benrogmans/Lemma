use assert_cmd::Command;

#[test]
fn test_interactive_mode_help() {
    // Since interactive mode requires stdin, we'll just test that the command exists
    // and doesn't crash immediately with --help
    let mut cmd = Command::cargo_bin("lemma").unwrap();
    cmd.arg("--help");

    let output = cmd.assert().success();
    output.stdout(predicates::str::contains("lemma"));
}
