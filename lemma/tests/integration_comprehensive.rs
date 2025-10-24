use lemma::Engine;

#[test]
fn test_employee_contract_comprehensive() {
    let mut engine = Engine::new();

    let base_contract = r#"
doc base_contract
fact min_salary = 30000
fact max_salary = 200000
fact standard_vacation_days = 20 days
fact probation_period = 90 days
fact min_age = 18 years
"#;

    let employment_terms = r#"
doc employment_terms
fact base = doc base_contract
fact salary = 75000
fact bonus_percentage = 10%
fact start_date = 2024-01-15
fact vacation_days = 20 days
fact employee_age = 28 years

rule total_compensation = salary + (salary * bonus_percentage)
rule is_salary_valid = salary >= base.min_salary and salary <= base.max_salary
rule vacation_days_ok = vacation_days >= base.standard_vacation_days
rule is_adult = employee_age >= base.min_age
rule probation_end_date = start_date + base.probation_period

rule contract_valid = is_salary_valid? and vacation_days_ok? and is_adult?
    unless not is_adult? then veto "Employee must be 18 or older"
"#;

    engine.add_lemma_code(base_contract, "test.lemma").unwrap();
    engine
        .add_lemma_code(employment_terms, "test.lemma")
        .unwrap();

    let response = engine.evaluate("employment_terms", None, None).unwrap();

    let total_comp = response
        .results
        .iter()
        .find(|r| r.rule_name == "total_compensation")
        .unwrap();
    assert!(total_comp
        .result
        .as_ref()
        .unwrap()
        .to_string()
        .contains("82500"));

    let contract_valid = response
        .results
        .iter()
        .find(|r| r.rule_name == "contract_valid")
        .unwrap();
    assert_eq!(contract_valid.result.as_ref().unwrap().to_string(), "true");

    engine.remove_document("employment_terms");
    engine.remove_document("base_contract");
}

#[test]
fn test_shipping_calculation_with_units() {
    let mut engine = Engine::new();

    let shipping_doc = r#"
doc shipping
fact package_weight = 5 kilograms
fact package_dimensions_cm = 50 centimeters
fact distance = 500 kilometers
fact is_express = true
fact base_rate = 10

rule weight_in_pounds = package_weight in pounds
rule distance_in_miles = distance in miles
rule dimensions_in_inches = package_dimensions_cm in inches

rule weight_surcharge = weight_in_pounds? > 10
rule is_long_distance = distance_in_miles? > 100
rule oversized = dimensions_in_inches? > 20

rule total_surcharges = 0
  unless weight_surcharge? then 5
rule distance_fee = 0
  unless is_long_distance? then distance_in_miles? * 0.1

rule base_shipping = base_rate + total_surcharges?
rule express_multiplier = 1
  unless is_express then 2
rule final_cost = (base_shipping? + distance_fee?) * express_multiplier?
"#;

    engine.add_lemma_code(shipping_doc, "test.lemma").unwrap();

    let response = engine.evaluate("shipping", None, None).unwrap();

    let weight_pounds = response
        .results
        .iter()
        .find(|r| r.rule_name == "weight_in_pounds")
        .unwrap();
    assert!(weight_pounds
        .result
        .as_ref()
        .unwrap()
        .to_string()
        .contains("11.02"));

    let weight_surcharge = response
        .results
        .iter()
        .find(|r| r.rule_name == "weight_surcharge")
        .unwrap();
    assert_eq!(
        weight_surcharge.result.as_ref().unwrap().to_string(),
        "true"
    );

    engine.remove_document("shipping");
}

#[test]
fn test_tax_calculation_with_percentages() {
    let mut engine = Engine::new();

    let tax_doc = r#"
doc tax_calculation
fact income = 80000
fact deductions = 10000
fact tax_rate_low = 10%
fact tax_rate_mid = 20%
fact tax_rate_high = 30%
fact bracket_low = 40000
fact bracket_mid = 80000

rule taxable_income = income - deductions
rule in_low_bracket = taxable_income? <= bracket_low
rule in_mid_bracket = taxable_income? > bracket_low and taxable_income? <= bracket_mid
rule in_high_bracket = taxable_income? > bracket_mid

rule tax_rate = tax_rate_low
    unless in_mid_bracket? then tax_rate_mid
    unless in_high_bracket? then tax_rate_high

rule tax_amount = taxable_income? * tax_rate?
rule net_income = income - tax_amount?
rule effective_rate = (tax_amount? / income) * 100%
"#;

    engine.add_lemma_code(tax_doc, "test.lemma").unwrap();

    let response = engine.evaluate("tax_calculation", None, None).unwrap();

    let taxable = response
        .results
        .iter()
        .find(|r| r.rule_name == "taxable_income")
        .unwrap();
    assert!(taxable
        .result
        .as_ref()
        .unwrap()
        .to_string()
        .contains("70000"));

    let in_mid = response
        .results
        .iter()
        .find(|r| r.rule_name == "in_mid_bracket")
        .unwrap();
    assert_eq!(in_mid.result.as_ref().unwrap().to_string(), "true");

    let tax_rate = response
        .results
        .iter()
        .find(|r| r.rule_name == "tax_rate")
        .unwrap();
    assert!(tax_rate.result.as_ref().unwrap().to_string().contains("20"));

    engine.remove_document("tax_calculation");
}

#[test]
fn test_multi_document_with_overrides() {
    let mut engine = Engine::new();

    let config_doc = r#"
doc config
fact max_temperature = 30 celsius
fact min_temperature = 15 celsius
fact alert_threshold = 90%
fact check_interval = 5 minutes
"#;

    let monitoring_doc = r#"
doc monitoring
fact config = doc config
fact current_temp = 28 celsius
fact current_usage = 85%
fact last_check = 2024-01-15T10:00:00Z

rule temp_in_fahrenheit = current_temp in fahrenheit
rule max_temp_f = config.max_temperature in fahrenheit
rule min_temp_f = config.min_temperature in fahrenheit

rule temp_ok = current_temp >= config.min_temperature and current_temp <= config.max_temperature
rule usage_ok = current_usage < config.alert_threshold
rule system_healthy = temp_ok? and usage_ok?

rule status = "OK"
    unless not temp_ok? then "TEMP_ALERT"
    unless not usage_ok? then "USAGE_ALERT"
"#;

    engine.add_lemma_code(config_doc, "test.lemma").unwrap();
    engine.add_lemma_code(monitoring_doc, "test.lemma").unwrap();

    let response = engine.evaluate("monitoring", None, None).unwrap();

    let system_healthy = response
        .results
        .iter()
        .find(|r| r.rule_name == "system_healthy")
        .unwrap();
    assert_eq!(system_healthy.result.as_ref().unwrap().to_string(), "true");

    let status = response
        .results
        .iter()
        .find(|r| r.rule_name == "status")
        .unwrap();
    assert_eq!(status.result.as_ref().unwrap().to_string(), "\"OK\"");

    engine.remove_document("monitoring");

    let monitoring_override = r#"
doc monitoring
fact config = doc config
fact current_temp = 35 celsius
fact current_usage = 95%
fact last_check = 2024-01-15T10:00:00Z

rule temp_in_fahrenheit = current_temp in fahrenheit
rule max_temp_f = config.max_temperature in fahrenheit
rule min_temp_f = config.min_temperature in fahrenheit

rule temp_ok = current_temp >= config.min_temperature and current_temp <= config.max_temperature
rule usage_ok = current_usage < config.alert_threshold
rule system_healthy = temp_ok? and usage_ok?

rule status = "OK"
    unless not temp_ok? then "TEMP_ALERT"
    unless not usage_ok? then "USAGE_ALERT"
"#;

    engine
        .add_lemma_code(monitoring_override, "test.lemma")
        .unwrap();

    let response2 = engine.evaluate("monitoring", None, None).unwrap();

    let system_healthy2 = response2
        .results
        .iter()
        .find(|r| r.rule_name == "system_healthy")
        .unwrap();
    assert_eq!(
        system_healthy2.result.as_ref().unwrap().to_string(),
        "false"
    );

    let status2 = response2
        .results
        .iter()
        .find(|r| r.rule_name == "status")
        .unwrap();
    assert_eq!(
        status2.result.as_ref().unwrap().to_string(),
        "\"USAGE_ALERT\""
    );

    engine.remove_document("monitoring");
    engine.remove_document("config");
}

#[test]
fn test_complex_arithmetic_with_multiple_units() {
    let mut engine = Engine::new();

    let physics_doc = r#"
doc physics_calculation
fact mass = 10 kilograms
fact velocity = 15 meters
fact time = 3 seconds
fact distance_traveled = 100 kilometers
fact power_consumption = 500 watts

rule mass_in_pounds = mass in pounds
rule velocity_per_second = velocity / time
rule distance_in_miles = distance_traveled in miles

rule kinetic_energy_approx = (mass * velocity * velocity) / 2
rule power_in_kilowatts = power_consumption in kilowatts
rule energy_in_hours = power_consumption * 2 hours

rule is_high_speed = velocity_per_second? > 3
rule is_long_distance = distance_in_miles? > 50
rule is_high_power = power_in_kilowatts? > 0.4

rule trip_summary = is_high_speed? and is_long_distance? and is_high_power?
"#;

    engine.add_lemma_code(physics_doc, "test.lemma").unwrap();

    let response = engine.evaluate("physics_calculation", None, None).unwrap();

    let mass_pounds = response
        .results
        .iter()
        .find(|r| r.rule_name == "mass_in_pounds")
        .unwrap();
    assert!(mass_pounds
        .result
        .as_ref()
        .unwrap()
        .to_string()
        .contains("22.04"));

    let trip_summary = response
        .results
        .iter()
        .find(|r| r.rule_name == "trip_summary")
        .unwrap();
    assert_eq!(trip_summary.result.as_ref().unwrap().to_string(), "true");

    engine.remove_document("physics_calculation");
}

#[test]
fn test_cli_fact_overrides_integration() {
    let mut engine = Engine::new();

    let config_doc = r#"
doc dynamic_config
fact threshold = [number]
fact multiplier = [number]
fact base_value = 100

rule calculated_value = base_value * multiplier
rule exceeds_threshold = calculated_value? > threshold
rule status = "LOW"
  unless exceeds_threshold? then "HIGH"
"#;

    engine.add_lemma_code(config_doc, "test.lemma").unwrap();

    let facts = lemma::parse_facts(&["threshold=500", "multiplier=2"]).unwrap();
    let response = engine
        .evaluate("dynamic_config", None, Some(facts))
        .unwrap();

    let calculated = response
        .results
        .iter()
        .find(|r| r.rule_name == "calculated_value")
        .unwrap();
    assert_eq!(calculated.result.as_ref().unwrap().to_string(), "200");

    let status = response
        .results
        .iter()
        .find(|r| r.rule_name == "status")
        .unwrap();
    assert_eq!(status.result.as_ref().unwrap().to_string(), "\"LOW\"");

    let facts2 = lemma::parse_facts(&["threshold=150", "multiplier=2"]).unwrap();
    let response2 = engine
        .evaluate("dynamic_config", None, Some(facts2))
        .unwrap();

    let status2 = response2
        .results
        .iter()
        .find(|r| r.rule_name == "status")
        .unwrap();
    assert_eq!(status2.result.as_ref().unwrap().to_string(), "\"HIGH\"");

    engine.remove_document("dynamic_config");
}

#[test]
fn test_date_arithmetic_comprehensive() {
    let mut engine = Engine::new();

    let timeline_doc = r#"
doc project_timeline
fact project_start = 2024-01-15
fact phase1_duration = 30 days
fact phase2_duration = 45 days
fact phase3_duration = 60 days
fact today = 2024-02-15

rule phase1_end = project_start + phase1_duration
rule phase2_end = phase1_end? + phase2_duration
rule phase3_end = phase2_end? + phase3_duration

rule project_duration = phase1_duration + phase2_duration + phase3_duration
rule elapsed_time = today - project_start
rule days_remaining = phase3_end? - today

rule is_phase1_complete = today > phase1_end?
rule is_phase2_complete = today > phase2_end?
rule is_on_schedule = elapsed_time? <= phase1_duration + phase2_duration
"#;

    engine.add_lemma_code(timeline_doc, "test.lemma").unwrap();

    let response = engine.evaluate("project_timeline", None, None).unwrap();

    let phase1_complete = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_phase1_complete")
        .unwrap();
    assert_eq!(phase1_complete.result.as_ref().unwrap().to_string(), "true");

    let phase2_complete = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_phase2_complete")
        .unwrap();
    assert_eq!(
        phase2_complete.result.as_ref().unwrap().to_string(),
        "false"
    );

    engine.remove_document("project_timeline");
}

// ============================================================================
// Date Arithmetic Regression Tests
// ============================================================================

#[test]
fn test_date_plus_duration() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact start = 2024-01-15
fact duration = 30 days
rule end_date = start + duration
"#;

    engine.add_lemma_code(doc, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    let end_date = response
        .results
        .iter()
        .find(|r| r.rule_name == "end_date")
        .unwrap();

    assert!(end_date.result.is_some());
    let result_str = end_date.result.as_ref().unwrap().to_string();
    assert!(result_str.contains("2024"));
    assert!(result_str.contains("2") && result_str.contains("14"));
}

#[test]
fn test_date_minus_duration() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact end = 2024-02-14
fact duration = 30 days
rule start_date = end - duration
"#;

    engine.add_lemma_code(doc, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    let start_date = response
        .results
        .iter()
        .find(|r| r.rule_name == "start_date")
        .unwrap();

    assert!(start_date.result.is_some());
    let result_str = start_date.result.as_ref().unwrap().to_string();
    assert!(result_str.contains("2024"));
    assert!(result_str.contains("1") && result_str.contains("15"));
}

#[test]
fn test_date_minus_date() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact start = 2024-01-15
fact end = 2024-02-14
rule duration = end - start
"#;

    engine.add_lemma_code(doc, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    let duration = response
        .results
        .iter()
        .find(|r| r.rule_name == "duration")
        .unwrap();

    assert!(duration.result.is_some());
    let result_str = duration.result.as_ref().unwrap().to_string();
    // Date - Date returns seconds (30 days = 2,592,000 seconds)
    assert!(result_str.contains("2592000"));
    assert!(result_str.contains("second"));
}

#[test]
fn test_date_comparison() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact date1 = 2024-01-15
fact date2 = 2024-02-14
rule date1_before_date2 = date1 < date2
rule date1_after_date2 = date1 > date2
"#;

    engine.add_lemma_code(doc, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    let before = response
        .results
        .iter()
        .find(|r| r.rule_name == "date1_before_date2")
        .unwrap();
    assert_eq!(before.result.as_ref().unwrap().to_string(), "true");

    let after = response
        .results
        .iter()
        .find(|r| r.rule_name == "date1_after_date2")
        .unwrap();
    assert_eq!(after.result.as_ref().unwrap().to_string(), "false");
}

// ============================================================================
// Type Validation Regression Tests
// ============================================================================

#[test]
fn test_type_validation_boolean_and_number() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact flag = true
rule result_true = flag and 100 or 50
"#;

    let result = engine.add_lemma_code(doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean and number in logical expression"
    );
}

#[test]
fn test_type_validation_boolean_and_money() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact needs_extra = true
rule extra_charge = needs_extra and 10 or 0
"#;

    let result = engine.add_lemma_code(doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean and money in logical expression"
    );
}

#[test]
fn test_type_validation_comparison_and_number() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact value = 100
rule multiplier = value > 50 and 2 or 1
rule result = value * multiplier
"#;

    let result = engine.add_lemma_code(doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean comparison result and numbers in logical expression"
    );
}

#[test]
fn test_type_validation_nested_with_text() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact temp = 25 celsius
rule status = temp < 15 celsius and "COLD"
    or temp > 30 celsius and "HOT"
    or "COMFORTABLE"
"#;

    let result = engine.add_lemma_code(doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean comparison result and strings in logical expression"
    );
}

// ============================================================================
// Type Error Message Validation Tests
// ============================================================================

#[test]
fn test_logical_operator_with_text_error_message() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact system_healthy = true
rule status = system_healthy and "OK"
"#;

    let result = engine.add_lemma_code(doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean and text in logical expression"
    );

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Type error"),
        "Error should mention 'Type error'"
    );
    assert!(
        error_msg.contains("and"),
        "Error should mention the 'and' operator"
    );
    assert!(
        error_msg.contains("boolean"),
        "Error should mention 'boolean'"
    );
    assert!(
        error_msg.contains("text"),
        "Error should mention 'text' type"
    );
}

#[test]
fn test_logical_or_with_text_error_message() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact flag = false
rule result = flag or "default"
"#;

    let result = engine.add_lemma_code(doc, "test.lemma");
    assert!(
        result.is_err(),
        "Should reject mixing boolean and text in 'or' expression"
    );

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Type error"),
        "Error should mention 'Type error'"
    );
    assert!(
        error_msg.contains("or"),
        "Error should mention the 'or' operator"
    );
    assert!(
        error_msg.contains("boolean"),
        "Error should mention 'boolean'"
    );
    assert!(
        error_msg.contains("text"),
        "Error should mention 'text' type"
    );
}

// ============================================================================
// Document Reference Field Access Tests
// ============================================================================

#[test]
fn test_doc_ref_field_access_simple() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact min_value = 100
fact max_value = 1000
"#;

    let child_doc = r#"
doc child
fact config = doc base
fact value = 500

rule is_valid = value >= config.min_value and value <= config.max_value
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(child_doc, "test.lemma").unwrap();

    let response = engine.evaluate("child", None, None).unwrap();

    let is_valid = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_valid")
        .unwrap();
    assert_eq!(is_valid.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_doc_ref_field_access_with_units() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact min_salary = 30000
fact max_salary = 200000
"#;

    let child_doc = r#"
doc child
fact base_contract = doc base
fact salary = 75000

rule is_valid = salary >= base_contract.min_salary and salary <= base_contract.max_salary
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(child_doc, "test.lemma").unwrap();

    let response = engine.evaluate("child", None, None).unwrap();

    let is_valid = response
        .results
        .iter()
        .find(|r| r.rule_name == "is_valid")
        .unwrap();
    assert_eq!(is_valid.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_doc_ref_field_access_arithmetic() {
    let mut engine = Engine::new();

    let base_doc = r#"
doc base
fact project_start = 2024-01-15
fact probation_period = 90 days
"#;

    let child_doc = r#"
doc child
fact base_contract = doc base

rule probation_end = base_contract.project_start + base_contract.probation_period
"#;

    engine.add_lemma_code(base_doc, "test.lemma").unwrap();
    engine.add_lemma_code(child_doc, "test.lemma").unwrap();

    let response = engine.evaluate("child", None, None).unwrap();

    let probation_end = response
        .results
        .iter()
        .find(|r| r.rule_name == "probation_end")
        .unwrap();

    assert!(probation_end.result.is_some());
    let result_str = probation_end.result.as_ref().unwrap().to_string();
    assert!(result_str.contains("2024"));
    assert!(result_str.contains("4") && result_str.contains("14"));
}
