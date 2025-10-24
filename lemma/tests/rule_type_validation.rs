use lemma::Engine;

#[test]
fn test_number_vs_percentage_type_mismatch() {
    let code = r#"
doc test

fact income = 100000
fact total_tax = 20000

rule effective_tax_rate = (total_tax / income)
  unless income == 0 then 0%
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject mixing number and percentage types"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("incompatible return types")
            || err.contains("number") && err.contains("percentage"),
        "Error should mention type incompatibility: {}",
        err
    );
}

#[test]
fn test_percentage_vs_number_type_mismatch() {
    let code = r#"
doc test

fact rate = 10%

rule adjusted_rate = rate
  unless rate > 5% then 100
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject mixing percentage and number types"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("incompatible return types")
            || err.contains("percentage") && err.contains("number"),
        "Error should mention type incompatibility: {}",
        err
    );
}

#[test]
fn test_mass_vs_length_type_mismatch() {
    let code = r#"
doc test

fact weight = 50 kilograms

rule measurement = weight
  unless weight > 100 kilograms then 10 meters
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject mixing mass and length types"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("incompatible return types") || err.contains("mass") && err.contains("length"),
        "Error should mention type incompatibility: {}",
        err
    );
}

#[test]
fn test_volume_vs_duration_type_mismatch() {
    let code = r#"
doc test

fact capacity = 100 liters

rule result = capacity
  unless capacity > 50 liters then 5 hours
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject mixing volume and duration types"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("incompatible return types")
            || err.contains("volume") && err.contains("duration"),
        "Error should mention type incompatibility: {}",
        err
    );
}

#[test]
fn test_power_vs_energy_type_mismatch() {
    let code = r#"
doc test

fact consumption = 1000 watts

rule result = consumption
  unless consumption > 500 watts then 100 joules
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject mixing power and energy types"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("incompatible return types")
            || err.contains("power") && err.contains("energy"),
        "Error should mention type incompatibility: {}",
        err
    );
}

#[test]
fn test_frequency_vs_pressure_type_mismatch() {
    let code = r#"
doc test

fact freq = 100 hertz

rule result = freq
  unless freq > 50 hertz then 10 pascals
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject mixing frequency and pressure types"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("incompatible return types")
            || err.contains("frequency") && err.contains("pressure"),
        "Error should mention type incompatibility: {}",
        err
    );
}

#[test]
fn test_data_size_vs_force_type_mismatch() {
    let code = r#"
doc test

fact size = 1024 megabytes

rule result = size
  unless size > 500 megabytes then 100 newtons
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject mixing data size and force types"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("incompatible return types")
            || err.contains("data size") && err.contains("force"),
        "Error should mention type incompatibility: {}",
        err
    );
}

#[test]
fn test_temperature_vs_money_type_mismatch() {
    let code = r#"
doc test

fact temp = 25 celsius

rule result = temp
  unless temp > 30 celsius then 100 USD
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_err(),
        "Should reject mixing temperature and money types"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("incompatible return types")
            || err.contains("temperature") && err.contains("money"),
        "Error should mention type incompatibility: {}",
        err
    );
}

#[test]
fn test_conversion_preserves_type_consistency() {
    let code = r#"
doc test

fact income = 100000
fact tax = 20000

rule rate_decimal = tax / income
rule rate_percentage = (tax / income) in percentage
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_ok(),
        "Should allow separate rules with different types"
    );
}

#[test]
fn test_same_unit_type_allowed() {
    let code = r#"
doc test

fact weight = 50 kilograms

rule adjusted_weight = weight
  unless weight > 100 kilograms then 75 grams
"#;

    let mut engine = Engine::new();
    let result = engine.add_lemma_code(code, "test.lemma");

    assert!(
        result.is_ok(),
        "Should allow same category units (mass vs mass)"
    );
}
