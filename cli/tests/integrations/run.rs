use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_cli_run_simple_document() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
doc simple_test
fact x = 10
fact y = 5
rule sum = x + y
rule product = x * y
"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("lemma").unwrap();
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
fn test_cli_run_with_fact_override() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
doc override_test
fact base = [number]
rule doubled = base * 2
"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("lemma").unwrap();
    cmd.arg("run")
        .arg("override_test")
        .arg("base=7")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("doubled"))
        .stdout(predicate::str::contains("14"));
}

#[test]
fn test_cli_run_nonexistent_document() {
    let temp_dir = TempDir::new().unwrap();

    let mut cmd = Command::cargo_bin("lemma").unwrap();
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
doc discount_test
fact quantity = 15
rule discount = 0
  unless quantity >= 10 then 10
  unless quantity >= 20 then 20
"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("lemma").unwrap();
    cmd.arg("run")
        .arg("discount_test")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("discount"))
        .stdout(predicate::str::contains("10"));
}

#[test]
fn test_cli_show_document() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
doc inspect_test
fact name = "Test"
fact value = 42
rule doubled = value * 2
"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("lemma").unwrap();
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
        temp_dir.path().join("doc1.lemma"),
        r#"
doc doc1
fact x = 1
"#,
    )
    .unwrap();

    fs::write(
        temp_dir.path().join("doc2.lemma"),
        r#"
doc doc2
fact y = 2
"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("lemma").unwrap();
    cmd.arg("list").arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("2 files"))
        .stdout(predicate::str::contains("2 documents"))
        .stdout(predicate::str::contains("doc1"))
        .stdout(predicate::str::contains("doc2"));
}

#[test]
fn test_cli_run_with_money_units() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
doc money_test
fact price = 100 USD
rule with_tax = price * 1.21
"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("lemma").unwrap();
    cmd.arg("run")
        .arg("money_test")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("with_tax"))
        .stdout(predicate::str::contains("121"));
}

#[test]
fn test_cli_parse_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    let lemma_file = temp_dir.path().join("test.lemma");

    fs::write(
        &lemma_file,
        r#"
doc invalid
this is not valid lemma syntax
"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("lemma").unwrap();
    cmd.arg("run")
        .arg("invalid")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("error").or(predicate::str::contains("Error")));
}
