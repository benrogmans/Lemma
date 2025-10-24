use lemma::*;

#[test]
fn test_same_unit_mass_comparison() {
    let code = r#"
doc test
fact weight1 = 3 kilograms
fact weight2 = 300 grams

rule heavier = weight1 > weight2
rule lighter = weight1 < weight2
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    // 3 kg = 3000 g, so 3000 g > 300 g = true
    let heavier = response
        .results
        .iter()
        .find(|r| r.rule_name == "heavier")
        .unwrap();
    assert_eq!(heavier.result.as_ref().unwrap().to_string(), "true");

    // 3 kg < 300 g should be false
    let lighter = response
        .results
        .iter()
        .find(|r| r.rule_name == "lighter")
        .unwrap();
    assert_eq!(lighter.result.as_ref().unwrap().to_string(), "false");
}

#[test]
fn test_same_unit_length_comparison() {
    let code = r#"
doc test
fact distance1 = 100 meters
fact distance2 = 1 kilometer

rule shorter = distance1 < distance2
rule equal = 1000 meters == 1 kilometer
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    // 100 m < 1 km (1000 m) = true
    let shorter = response
        .results
        .iter()
        .find(|r| r.rule_name == "shorter")
        .unwrap();
    assert_eq!(shorter.result.as_ref().unwrap().to_string(), "true");

    // 1000 m == 1 km = true
    let equal = response
        .results
        .iter()
        .find(|r| r.rule_name == "equal")
        .unwrap();
    assert_eq!(equal.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_same_unit_duration_comparison() {
    let code = r#"
doc test
fact time1 = 90 seconds
fact time2 = 2 minutes

rule less_time = time1 < time2
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    // 90 seconds < 2 minutes (120 seconds) = true
    let less_time = response
        .results
        .iter()
        .find(|r| r.rule_name == "less_time")
        .unwrap();
    assert_eq!(less_time.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_cross_category_comparison() {
    let code = r#"
doc test
fact weight = 5 kilograms
fact distance = 3 meters

rule weight_greater = weight > distance
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    // Different categories: compares numeric values (5 > 3 = true)
    let weight_greater = response
        .results
        .iter()
        .find(|r| r.rule_name == "weight_greater")
        .unwrap();
    assert_eq!(weight_greater.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_unit_vs_number_comparison() {
    let code = r#"
doc test
fact weight = 5 kilograms

rule greater_than_3 = weight > 3
rule less_than_10 = weight < 10
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    // 5 kg > 3 (extracts value: 5 > 3 = true)
    let greater = response
        .results
        .iter()
        .find(|r| r.rule_name == "greater_than_3")
        .unwrap();
    assert_eq!(greater.result.as_ref().unwrap().to_string(), "true");

    // 5 kg < 10 (extracts value: 5 < 10 = true)
    let less = response
        .results
        .iter()
        .find(|r| r.rule_name == "less_than_10")
        .unwrap();
    assert_eq!(less.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_same_unit_arithmetic_preserves_unit() {
    let code = r#"
doc test
fact weight1 = 2 kilograms
fact weight2 = 500 grams

rule total = weight1 + weight2
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    // 2 kg + 500 g = 2.5 kg (preserved left unit)
    let total = response
        .results
        .iter()
        .find(|r| r.rule_name == "total")
        .unwrap();
    let result_str = total.result.as_ref().unwrap().to_string();
    assert!(result_str.contains("2.5") || result_str.contains("2.50"));
    assert!(result_str.contains("kilogram"));
}

#[test]
fn test_cross_category_arithmetic_produces_number() {
    let code = r#"
doc test
fact distance = 15 meters
fact time = 3 seconds

rule speed = distance / time
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    // 15 meters / 3 seconds = 5 (dimensionless number)
    let speed = response
        .results
        .iter()
        .find(|r| r.rule_name == "speed")
        .unwrap();
    assert_eq!(speed.result.as_ref().unwrap().to_string(), "5");
}

#[test]
fn test_temperature_comparison_with_conversion() {
    let code = r#"
doc test
fact temp1 = 0 celsius
fact temp2 = 32 fahrenheit

rule same_temp = temp1 == temp2
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    // 0°C == 32°F = true (both are freezing point)
    let same_temp = response
        .results
        .iter()
        .find(|r| r.rule_name == "same_temp")
        .unwrap();
    assert_eq!(same_temp.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_power_comparison_with_conversion() {
    let code = r#"
doc test
fact power1 = 500 watts
fact power2 = 1 kilowatt

rule less_power = power1 < power2
"#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test.lemma").unwrap();
    let response = engine.evaluate("test", None, None).unwrap();

    // 500 W < 1 kW (1000 W) = true
    let less_power = response
        .results
        .iter()
        .find(|r| r.rule_name == "less_power")
        .unwrap();
    assert_eq!(less_power.result.as_ref().unwrap().to_string(), "true");
}
