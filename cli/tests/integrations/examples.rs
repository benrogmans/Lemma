//! Tests for all CLI integration example files
//!
//! Ensures all example files in cli/tests/integrations/examples/ are valid and can be evaluated
//! This validates that the examples work correctly through the CLI command interface.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("integrations")
        .join("examples")
}

#[test]
fn test_example_01_simple_facts() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("01_simple_facts.lemma");

    fs::copy(&example_file, temp_dir.path().join("01_simple_facts.lemma")).unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("simple_facts")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();
}

#[test]
fn test_show_includes_hash() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("01_simple_facts.lemma");
    fs::copy(&example_file, temp_dir.path().join("01_simple_facts.lemma")).unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("show")
        .arg("simple_facts")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("hash:"))
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn test_show_hash_only_outputs_hash() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("01_simple_facts.lemma");
    fs::copy(&example_file, temp_dir.path().join("01_simple_facts.lemma")).unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("show")
        .arg("simple_facts")
        .arg("--hash")
        .arg("--dir")
        .arg(temp_dir.path());

    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let hash = stdout.trim();
    assert!(
        hash.len() == 8 && hash.chars().all(|c| c.is_ascii_hexdigit()),
        "expected 8-char hex hash, got: {:?}",
        hash
    );
    assert!(!stdout.contains("facts"), "should not contain structure");
}

#[test]
fn test_example_02_rules_and_unless() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("02_rules_and_unless.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("02_rules_and_unless.lemma"),
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("rules_and_unless")
        .arg("base_price=100.00")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("final_total").or(predicate::str::is_empty()));
}

#[test]
fn test_example_03_spec_references() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("03_spec_references.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("03_spec_references.lemma"),
    )
    .unwrap();

    // Test base_employee spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("base_employee")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("annual_salary").or(predicate::str::is_empty()));

    // Test specific_employee spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("specific_employee")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test contractor spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("contractor")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();
}

#[test]
fn test_example_04_unit_conversions() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("04_unit_conversions.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("04_unit_conversions.lemma"),
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("unit_conversions")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("duration_hours").or(predicate::str::is_empty()));
}

#[test]
fn test_example_05_date_handling() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("05_date_handling.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("05_date_handling.lemma"),
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("date_handling")
        .arg("current_date=2024-06-15")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("employee_age").or(predicate::str::is_empty()));
}

#[test]
fn test_example_06_tax_calculation() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("06_tax_calculation.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("06_tax_calculation.lemma"),
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("tax_calculation")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("taxable_income").or(predicate::str::is_empty()));
}

#[test]
fn test_example_07_shipping_policy() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("07_shipping_policy.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("07_shipping_policy.lemma"),
    )
    .unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("shipping_policy")
        .arg("order_total=75.00")
        .arg("item_weight=8")
        .arg("destination_country=NL")
        .arg("destination_region=North Holland")
        .arg("is_po_box=false")
        .arg("is_expedited=false")
        .arg("is_hazardous=false")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("final_shipping"))
        .stdout(predicate::str::contains("23.6")) // NL base 22.00 + weight 7.50 = 29.50, gold discount 20% = 5.90, final = 23.60
        .stdout(predicate::str::contains("estimated_delivery_days"))
        .stdout(predicate::str::contains("2")); // NL delivery is 2 days
}

#[test]
fn test_example_08_rule_references() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("08_rule_references.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("08_rule_references.lemma"),
    )
    .unwrap();

    // Test rule_references spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("rule_references")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("can_drive_legally").or(predicate::str::is_empty()));

    // Test eligibility_check spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("eligibility_check")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success().stdout(
        predicate::str::contains("can_travel_internationally").or(predicate::str::is_empty()),
    );
}

#[test]
fn test_example_09_stress_test() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("09_stress_test.lemma");

    fs::copy(&example_file, temp_dir.path().join("09_stress_test.lemma")).unwrap();

    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("stress_test")
        .arg("base_price=100.00")
        .arg("quantity=50")
        .arg("customer_tier=premium")
        .arg("loyalty_points=5000")
        .arg("package_weight=25")
        .arg("delivery_distance=300")
        .arg("is_express=false")
        .arg("is_fragile=false")
        .arg("payment_method=credit")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test stress_test_config spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("stress_test_config")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test stress_test_extended spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("stress_test_extended")
        .arg("order.base_price=100.00")
        .arg("order.quantity=100")
        .arg("order.customer_tier=vip")
        .arg("order.loyalty_points=10000")
        .arg("order.package_weight=30")
        .arg("order.delivery_distance=250")
        .arg("order.is_express=true")
        .arg("order.is_fragile=true")
        .arg("order.payment_method=debit")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();
}

#[test]
fn test_example_10_compensation_policy() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("10_compensation_policy.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("10_compensation_policy.lemma"),
    )
    .unwrap();

    // Test base_policy spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("compensation/base_policy")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("annual_health_cost").or(predicate::str::is_empty()));

    // Test engineering_dept spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("compensation/engineering_dept")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("total_package").or(predicate::str::is_empty()));

    // Test senior_engineer spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("compensation/senior_engineer")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test principal_engineer spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("compensation/principal_engineer")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();
}

#[test]
fn test_example_11_spec_composition() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("11_spec_composition.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("11_spec_composition.lemma"),
    )
    .unwrap();

    // Test base_config spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("pricing/base_config")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("final_price").or(predicate::str::is_empty()));

    // Test wholesale spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("pricing/wholesale")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("wholesale_final").or(predicate::str::is_empty()));

    // Test wholesale_order spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("order/wholesale_order")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test comparison spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("order/comparison")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test custom_wholesale spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("order/custom_wholesale")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("custom_total").or(predicate::str::is_empty()));

    // Test multi_reference spec
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("complex/multi_reference")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();
}

#[test]
fn test_example_13_temporal_versioning() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("13_temporal_versioning.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("13_temporal_versioning.lemma"),
    )
    .unwrap();

    let dir = temp_dir.path().to_str().unwrap();

    // 2024: applicant age 28, salary 4000 EUR/month
    // Under-30 threshold in 2024 is 3672 → qualifies
    let output_2024 = std::process::Command::new(env!("CARGO_BIN_EXE_lemma"))
        .args([
            "run",
            "ind/kennismigrant/aanvraag",
            "--dir",
            dir,
            "--effective",
            "2024-06",
            "applicant_age=28",
            "gross_monthly_salary=4000 eur",
        ])
        .output()
        .unwrap();

    let stdout_2024 = String::from_utf8_lossy(&output_2024.stdout);
    assert!(
        output_2024.status.success(),
        "2024 eval failed: {}",
        String::from_utf8_lossy(&output_2024.stderr)
    );
    assert!(
        stdout_2024.contains("meets_salary_requirement") && stdout_2024.contains("true"),
        "Expected meets_salary_requirement = true in 2024, got: {}",
        stdout_2024
    );

    // 2025: same salary, but applicant is now 31
    // Age 30+ threshold in 2025 is 5331 → does not qualify
    let output_2025 = std::process::Command::new(env!("CARGO_BIN_EXE_lemma"))
        .args([
            "run",
            "ind/kennismigrant/aanvraag",
            "--dir",
            dir,
            "--effective",
            "2025-06",
            "applicant_age=31",
            "gross_monthly_salary=4000 eur",
        ])
        .output()
        .unwrap();

    let stdout_2025 = String::from_utf8_lossy(&output_2025.stdout);
    assert!(
        output_2025.status.success(),
        "2025 eval failed: {}",
        String::from_utf8_lossy(&output_2025.stderr)
    );
    assert!(
        stdout_2025.contains("meets_salary_requirement") && stdout_2025.contains("false"),
        "Expected meets_salary_requirement = false in 2025, got: {}",
        stdout_2025
    );
}

#[test]
fn test_all_examples_parse_via_cli() {
    // This test ensures all example files can be parsed by the CLI.
    // Files with external @... registry references are excluded because they
    // require network access (or a running registry) to resolve.
    let temp_dir = TempDir::new().unwrap();
    let examples_path = examples_dir();

    let registry_examples: &[&str] = &["12_registry_references.lemma"];

    // Copy all example files to temp directory (skip registry examples)
    for entry in fs::read_dir(&examples_path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map(|e| e == "lemma").unwrap_or(false) {
            let filename = path.file_name().unwrap().to_str().unwrap();
            if registry_examples.contains(&filename) {
                continue;
            }
            fs::copy(&path, temp_dir.path().join(filename)).unwrap();
        }
    }

    // Use list command to verify files are parseable
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("list").arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("simple_facts"));
}
