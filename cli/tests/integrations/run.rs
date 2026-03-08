use assert_cmd::cargo::cargo_bin_cmd;

use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_cli_run_simple_spec() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
spec simple_test
fact x: 10
fact y: 5
rule sum: x + y
rule product: x * y
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("simple_test")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("sum"))
        .stdout(predicate::str::contains("15"))
        .stdout(predicate::str::contains("product"))
        .stdout(predicate::str::contains("50"));
}

#[test]
fn test_cli_run_with_fact_values() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
spec override_test
fact base: [number]
rule doubled: base * 2
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("override_test")
        .arg("base=7")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("14"));
}

#[test]
fn test_cli_run_nonexistent_spec() {
    let temp_dir = TempDir::new().unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("nonexistent")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_cli_run_with_unless_clause() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
spec discount_test
fact quantity: 15
rule discount: 0
  unless quantity >= 10 then 10
  unless quantity >= 20 then 20
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("discount_test")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("10"));
}

#[test]
fn test_cli_show_spec() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
spec inspect_test
fact name: "Test"
fact value: 42
rule doubled: value * 2
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("show")
        .arg("inspect_test")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("inspect_test"))
        .stdout(predicate::str::contains("facts"))
        .stdout(predicate::str::contains("rules"));
}

#[test]
fn test_cli_list_summary() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("spec1.lemma"),
        r#"
spec spec1
fact x: 1
"#,
    )
    .unwrap();

    fs::write(
        temp_dir.path().join("spec2.lemma"),
        r#"
spec spec2
fact y: 2
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("list").arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("spec1"))
        .stdout(predicate::str::contains("spec2"));
}

#[test]
fn test_cli_run_with_arithmetic() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
spec arithmetic_test
fact price: 100
rule with_tax: price * 1.21
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("arithmetic_test")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("121"));
}

#[test]
fn test_cli_parse_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
spec invalid
this is not valid lemma syntax
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("invalid")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("error").or(predicate::str::contains("Error")));
}

#[test]
fn test_cli_reports_errors_from_all_files() {
    let temp_dir = TempDir::new().unwrap();

    // File 1: valid
    fs::write(
        temp_dir.path().join("valid.lemma"),
        r#"
spec valid_spec
fact price: 100
rule doubled: price * 2
"#,
    )
    .unwrap();

    // File 2: broken (parse error)
    fs::write(
        temp_dir.path().join("broken_a.lemma"),
        r#"
spec broken_a
this is not valid lemma
"#,
    )
    .unwrap();

    // File 3: broken (different parse error)
    fs::write(
        temp_dir.path().join("broken_b.lemma"),
        r#"
spec broken_b
also invalid lemma syntax
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("valid_spec")
        .arg("--dir")
        .arg(temp_dir.path());

    // Should fail, and the error output should mention BOTH broken files
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "Should fail when workspace has broken files"
    );
    assert!(
        stderr.contains("broken_a") && stderr.contains("broken_b"),
        "Should report errors from both broken files, got:\n{}",
        stderr
    );
}

#[test]
fn test_cli_explain_shows_negated_comparison_not_false() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec explain_test
rule out: true
 unless 5 < 3 then false
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("explain_test")
        .arg("--explain")
        .arg("--dir")
        .arg(temp_dir.path());

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "run --explain should succeed: {}",
        stdout
    );

    assert!(
        stdout.contains(">="),
        "explain should show negated comparison (e.g. 5 >= 3), got:\n{}",
        stdout
    );
}
