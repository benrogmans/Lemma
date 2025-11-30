use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn test_interactive_mode_help() {
    // Since interactive mode requires stdin, we'll just test that the command exists
    // and doesn't crash immediately with --help
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("--help");

    let output = cmd.assert().success();
    output.stdout(predicates::str::contains("lemma"));
}
