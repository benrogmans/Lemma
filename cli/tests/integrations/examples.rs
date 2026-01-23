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
fn test_example_03_document_references() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("03_document_references.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("03_document_references.lemma"),
    )
    .unwrap();

    // Test base_employee document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("base_employee")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("annual_salary").or(predicate::str::is_empty()));

    // Test specific_employee document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("specific_employee")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test contractor document
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

    // Test rule_references document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("rule_references")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("can_drive_legally").or(predicate::str::is_empty()));

    // Test eligibility_check document
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

    // Test stress_test_config document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("stress_test_config")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test stress_test_extended document
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

    // Test base_policy document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("compensation/base_policy")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("annual_health_cost").or(predicate::str::is_empty()));

    // Test engineering_dept document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("compensation/engineering_dept")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("total_package").or(predicate::str::is_empty()));

    // Test senior_engineer document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("compensation/senior_engineer")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test principal_engineer document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("compensation/principal_engineer")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();
}

#[test]
fn test_example_11_document_composition() {
    let temp_dir = TempDir::new().unwrap();
    let example_file = examples_dir().join("11_document_composition.lemma");

    fs::copy(
        &example_file,
        temp_dir.path().join("11_document_composition.lemma"),
    )
    .unwrap();

    // Test base_config document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("pricing/base_config")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("final_price").or(predicate::str::is_empty()));

    // Test wholesale document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("pricing/wholesale")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("wholesale_final").or(predicate::str::is_empty()));

    // Test wholesale_order document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("order/wholesale_order")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test comparison document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("order/comparison")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();

    // Test custom_wholesale document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("order/custom_wholesale")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("custom_total").or(predicate::str::is_empty()));

    // Test multi_reference document
    let mut cmd = cargo_bin_cmd!("lemma");
    cmd.arg("run")
        .arg("complex/multi_reference")
        .arg("--dir")
        .arg(temp_dir.path());

    cmd.assert().success();
}

#[test]
fn test_all_examples_parse_via_cli() {
    // This test ensures all example files can be parsed by the CLI
    let temp_dir = TempDir::new().unwrap();
    let examples_path = examples_dir();

    // Copy all example files to temp directory
    for entry in fs::read_dir(&examples_path).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map(|e| e == "lemma").unwrap_or(false) {
            let filename = path.file_name().unwrap();
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
