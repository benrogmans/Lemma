use assert_cmd::cargo::cargo_bin_cmd;

use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_cli_run_simple_spec() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec simple_test
data x: 10
data y: 5
rule sum: x + y
rule product: x * y
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("simple_test");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("sum"))
        .stdout(predicate::str::contains("15"))
        .stdout(predicate::str::contains("product"))
        .stdout(predicate::str::contains("50"));
}

#[test]
fn test_cli_run_set_data_values() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec override_test
data base: number
rule doubled: base * 2
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("override_test")
        .arg("base=7");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("14"));
}

#[test]
fn test_cli_run_nonexistent_spec() {
    let temp_dir = TempDir::new().unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("nonexistent");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_cli_run_rejects_dash_source() {
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run").arg("-").arg("any_spec");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("--prefix"));
}

#[test]
fn test_cli_run_with_unless_clause() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec discount_test
data quantity: 15
rule discount: 0
  unless quantity >= 10 then 10
  unless quantity >= 20 then 20
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("discount_test");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("10"));
}

#[test]
fn test_cli_schema_spec() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec inspect_test
data name: text
  -> option "Test"
data value: number
  -> minimum 0
rule doubled: value * 2
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("schema")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("inspect_test");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("inspect_test"))
        .stdout(predicate::str::contains("Data"))
        .stdout(predicate::str::contains("Rules"))
        .stdout(predicate::str::contains("name"))
        .stdout(predicate::str::contains("options"))
        .stdout(predicate::str::contains("minimum"))
        .stdout(predicate::str::contains("text"))
        .stdout(predicate::str::contains("number"));
}

#[test]
fn test_cli_list_summary() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("spec1.lemma"),
        r#"
spec spec1
data x: 1
"#,
    )
    .unwrap();

    fs::write(
        temp_dir.path().join("spec2.lemma"),
        r#"
spec spec2
data y: 2
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("list").arg("--prefix").arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("spec1"))
        .stdout(predicate::str::contains("spec2"))
        .stdout(predicate::str::contains("Found").not())
        .stdout(predicate::str::contains("data").not());
}

#[test]
fn test_cli_list_scopes_main_repository() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("named.lemma"),
        r#"repo deps_repo

spec dep_only
data d: 1
"#,
    )
    .unwrap();

    fs::write(
        temp_dir.path().join("workspace.lemma"),
        r#"spec entry
data x: 1"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("list").arg("--prefix").arg(temp_dir.path());

    cmd.assert().success().stdout(
        predicate::str::contains("entry")
            .and(predicate::str::contains("deps_repo"))
            .and(predicate::str::contains("dep_only"))
            .and(predicate::str::contains("Found").not())
            .and(predicate::str::contains("Repositories:").not()),
    );
}

#[test]
fn test_cli_run_with_arithmetic() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec arithmetic_test
data price: 100
rule with_tax: price * 1.21
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("arithmetic_test");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("121"));
}

#[test]
fn test_cli_parse_error_handling() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec invalid
this is not valid lemma syntax
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("invalid");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("error").or(predicate::str::contains("Error")));
}

#[test]
fn test_cli_reports_errors_from_all_files() {
    let temp_dir = TempDir::new().unwrap();

    fs::write(
        temp_dir.path().join("valid.lemma"),
        r#"
spec valid_spec
data price: 100
rule doubled: price * 2
"#,
    )
    .unwrap();

    fs::write(
        temp_dir.path().join("broken_a.lemma"),
        r#"
spec broken_a
this is not valid lemma
"#,
    )
    .unwrap();

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
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("valid_spec");

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
fn test_cli_explain_shows_all_operands_in_nested_arithmetic() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec nested_explanation
data x: 10
data y: 20
data z: 30
rule a: x + 1
rule b: y + 2
rule c: z + 3
rule total: a + b + c
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("nested_explanation")
        .arg("--rules=total")
        .arg("--explain");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "run --explain should succeed: {}",
        stdout
    );

    assert!(
        stdout.contains("a:") && stdout.contains("b:") && stdout.contains("c:"),
        "explain should expand all three rule operands (a, b, c), got:\n{}",
        stdout
    );

    let lines: Vec<&str> = stdout.lines().collect();
    let a_line = lines.iter().find(|l| l.contains("a:")).unwrap();
    let b_line = lines.iter().find(|l| l.contains("b:")).unwrap();
    let c_line = lines.iter().find(|l| l.contains("c:")).unwrap();
    let indent_of = |line: &str| line.len() - line.trim_start().len();
    assert_eq!(
        indent_of(a_line),
        indent_of(b_line),
        "a and b should be at the same depth (flattened), got:\n{}",
        stdout
    );
    assert_eq!(
        indent_of(b_line),
        indent_of(c_line),
        "b and c should be at the same depth (flattened), got:\n{}",
        stdout
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
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("explain_test")
        .arg("--explain");

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

#[test]
fn test_cli_explain_scalar_conversion_format() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec test_cli_conversion_explain
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> default 2 kilogram
rule result: mass as gram
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("test_cli_conversion_explain")
        .arg("--rules=result")
        .arg("--explain");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "run --explain should succeed: {}",
        stdout
    );
    assert!(stdout.contains("2 kilogram"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("1 kilogram is 1000 gram"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("The quantity of mass is 2 kilogram"),
        "stdout:\n{stdout}"
    );
    assert!(!stdout.contains('×'), "stdout:\n{stdout}");
    assert!(!stdout.contains("mass as gram is"), "stdout:\n{stdout}");
}

#[test]
fn test_cli_explain_date_range_conversion_format() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec test_cli_date_conversion_explain
uses lemma units
data age: date range -> default 2024-06-01...2024-06-15
rule result: age as days
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("test_cli_date_conversion_explain")
        .arg("--rules=result")
        .arg("--explain");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "run --explain should succeed: {}",
        stdout
    );
    assert!(
        stdout.contains("2 week") || (stdout.contains("14") && stdout.contains("day")),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains('−'), "stdout:\n{stdout}");
    assert!(
        stdout.contains("2024-06-01") && stdout.contains("2024-06-15"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("The date range of age is"),
        "stdout:\n{stdout}"
    );
    assert!(!stdout.contains("age as days is"), "stdout:\n{stdout}");
}

#[test]
fn test_cli_explain_conversion_nested_operand() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec test_cli_nested_conversion_explain
data mass: quantity
    -> unit kilogram 1.0
    -> unit gram 0.001
    -> default 2 kilogram
rule result: (mass * 2) as gram
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("test_cli_nested_conversion_explain")
        .arg("--rules=result")
        .arg("--explain");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "run --explain should succeed: {}",
        stdout
    );
    assert!(stdout.contains("4 kilogram"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("1 kilogram is 1000 gram"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("The quantity is 4 kilogram"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("mass"), "stdout:\n{stdout}");
}

#[test]
fn test_cli_run_quantity_rule_result_includes_all_units_json() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("money.lemma"),
        r#"
spec money
data price: quantity
    -> unit eur 1
    -> unit usd 0.91
    -> default 100 eur
rule total: price
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("money")
        .arg("--rules=total")
        .arg("--json");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("\"quantity\""))
        .stdout(predicate::str::contains("\"eur\""))
        .stdout(predicate::str::contains("\"usd\""));
}

#[test]
fn test_cli_explain_shows_multiply_trace_for_quantity_product() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec product_explanation
data price: quantity
    -> unit eur 1
    -> default 10 eur
data quantity: number -> default 3
rule product: price * quantity
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("product_explanation")
        .arg("--rules=product")
        .arg("--explain");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "run --explain should succeed: {}",
        stdout
    );
    assert!(
        stdout.contains("price is") && stdout.contains("quantity is"),
        "explain should show both data operands, got:\n{}",
        stdout
    );
}

#[test]
fn test_cli_explain_with_quantity_product_preserves_multiply_trace() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("test.lemma"),
        r#"
spec product_as_explanation
data price: quantity
    -> unit eur 1
    -> unit usd 0.91
    -> default 10 eur
data quantity: number -> default 3
rule product: price * quantity
"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("--prefix")
        .arg(temp_dir.path())
        .arg("product_as_explanation")
        .arg("--rules=product")
        .arg("--explain");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "run --explain should succeed: {}",
        stdout
    );
    assert!(
        stdout.contains("price is") && stdout.contains("quantity is"),
        "explain with --as should still show multiply data operands, got:\n{}",
        stdout
    );
}
