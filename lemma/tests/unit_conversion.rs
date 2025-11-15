use lemma::*;
use rust_decimal::Decimal;
use std::str::FromStr;

#[test]
fn test_mass_conversions() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
doc test1
fact weight_kg = 10 kilograms
rule weight_lbs = weight_kg in pounds
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test1", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "weight_lbs")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            let expected = Decimal::from_str("22.0462").unwrap();
            let diff = (amount - expected).abs();
            assert!(
                diff < Decimal::from_str("0.01").unwrap(),
                "Expected ~22.05, got {}",
                amount
            );
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test1");

    engine
        .add_lemma_code(
            r#"
doc test2
fact weight_tons = 2 tons
rule weight_kg = weight_tons in kilograms
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test2", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "weight_kg")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("2000").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test2");
}

#[test]
fn test_length_conversions() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
doc test1
fact distance_km = 100 kilometers
rule distance_miles = distance_km in miles
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test1", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "distance_miles")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert!(
                (amount - Decimal::from_str("62.1371").unwrap()).abs()
                    < Decimal::from_str("0.01").unwrap(),
                "Expected ~62.14, got {}",
                amount
            );
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test1");

    engine
        .add_lemma_code(
            r#"
doc test2
fact distance_yards = 100 yards
rule distance_meters = distance_yards in meters
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test2", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "distance_meters")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert!(
                (amount - Decimal::from_str("91.44").unwrap()).abs()
                    < Decimal::from_str("0.01").unwrap(),
                "Expected ~91.44, got {}",
                amount
            );
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test2");

    engine
        .add_lemma_code(
            r#"
doc test3
fact length_dm = 50 decimeters
rule length_cm = length_dm in centimeters
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test3", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "length_cm")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("500.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test3");
}

#[test]
fn test_volume_conversions() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
doc test1
fact volume_l = 2 liters
rule volume_ml = volume_l in milliliters
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test1", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "volume_ml")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("2000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test1");

    engine
        .add_lemma_code(
            r#"
doc test2
fact volume_gal = 5 gallons
rule volume_l = volume_gal in liters
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test2", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "volume_l")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert!(
                (amount - Decimal::from_str("18.927").unwrap()).abs()
                    < Decimal::from_str("0.01").unwrap(),
                "Expected ~18.93 liters, got {}",
                amount
            );
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test2");

    engine
        .add_lemma_code(
            r#"
doc test3
fact volume_qt = 8 quarts
rule volume_pt = volume_qt in pints
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test3", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "volume_pt")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("16.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test3");

    engine
        .add_lemma_code(
            r#"
doc test4
fact volume_cl = 75 centiliters
rule volume_dl = volume_cl in deciliters
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test4", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "volume_dl")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert!(
                (amount - Decimal::from_str("7.5").unwrap()).abs()
                    < Decimal::from_str("0.01").unwrap(),
                "Expected 7.5 deciliters, got {}",
                amount
            );
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test4");
}

#[test]
fn test_duration_conversions() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
doc test1
fact time_h = 3 hours
rule time_min = time_h in minutes
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test1", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "time_min")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(n) => {
            // 3 hours = 180 minutes
            assert_eq!(*n, Decimal::from_str("180.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test1");

    engine
        .add_lemma_code(
            r#"
doc test2
fact time_s = 5 seconds
rule time_ms = time_s in milliseconds
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test2", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "time_ms")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(n) => {
            // 5 seconds = 5000 milliseconds
            assert_eq!(*n, Decimal::from_str("5000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test2");

    engine
        .add_lemma_code(
            r#"
doc test3
fact time_ms = 1000 milliseconds
rule time_us = time_ms in microseconds
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test3", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "time_us")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(n) => {
            // 1 second = 1000000 microseconds
            assert_eq!(*n, Decimal::from_str("1000000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test3");
}

#[test]
fn test_power_conversions() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
doc test1
fact power_kw = 5 kilowatts
rule power_w = power_kw in watts
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test1", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "power_w")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("5000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test1");

    engine
        .add_lemma_code(
            r#"
doc test2
fact power_w = 2000 watts
rule power_mw = power_w in milliwatts
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test2", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "power_mw")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("2000000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test2");
}

#[test]
fn test_energy_conversions() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
doc test1
fact energy_kj = 10 kilojoules
rule energy_j = energy_kj in joules
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test1", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "energy_j")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("10000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test1");

    engine
        .add_lemma_code(
            r#"
doc test2
fact energy_kwh = 2 kilowatthours
rule energy_wh = energy_kwh in watthours
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test2", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "energy_wh")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("2000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test2");

    engine
        .add_lemma_code(
            r#"
doc test3
fact energy_kcal = 5 kilocalories
rule energy_cal = energy_kcal in calories
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test3", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "energy_cal")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("5000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test3");
}

#[test]
fn test_pressure_conversions() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
doc test1
fact pressure_kpa = 250 kilopascals
rule pressure_pa = pressure_kpa in pascals
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test1", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "pressure_pa")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("250000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test1");

    engine
        .add_lemma_code(
            r#"
doc test2
fact pressure_mpa = 3 megapascals
rule pressure_kpa = pressure_mpa in kilopascals
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test2", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "pressure_kpa")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("3000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test2");
}

#[test]
fn test_datasize_conversions() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
doc test1
fact size_gb = 10 gigabytes
rule size_mb = size_gb in megabytes
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test1", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "size_mb")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("10000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test1");

    engine
        .add_lemma_code(
            r#"
doc test2
fact size_tb = 2 terabytes
rule size_gb = size_tb in gigabytes
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test2", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "size_gb")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("2000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test2");

    engine
        .add_lemma_code(
            r#"
doc test3
fact size_pb = 1 petabytes
rule size_tb = size_pb in terabytes
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test3", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "size_tb")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("1000.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test3");

    engine
        .add_lemma_code(
            r#"
doc test4
fact size_gib = 8 gibibytes
rule size_mib = size_gib in mebibytes
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test4", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "size_mib")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("8192.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test4");

    engine
        .add_lemma_code(
            r#"
doc test5
fact size_kib = 2048 kibibytes
rule size_b = size_kib in bytes
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("test5", None, None).unwrap();
    let result = response
        .results
        .iter()
        .find(|r| r.rule.name == "size_b")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match result {
        LiteralValue::Number(amount) => {
            assert_eq!(*amount, Decimal::from_str("2097152.0").unwrap());
        }
        _ => panic!("Expected Number (from 'in' conversion), got {:?}", result),
    }
    engine.remove_document("test5");
}

#[test]
fn test_complex_multi_unit_scenario() {
    let mut engine = Engine::new();

    engine
        .add_lemma_code(
            r#"
doc complex
fact base_price = 100
fact weight = 5 kilograms
fact distance = 1000 kilometers
fact discount = 10%

rule weight_lbs = weight in pounds
rule distance_miles = distance in miles
rule discounted_price = base_price * (1 - discount)
"#,
            "test.lemma",
        )
        .unwrap();
    let response = engine.evaluate("complex", None, None).unwrap();

    let weight_result = response
        .results
        .iter()
        .find(|r| r.rule.name == "weight_lbs")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match weight_result {
        LiteralValue::Number(amount) => {
            assert!(
                (amount - Decimal::from_str("11.0231").unwrap()).abs()
                    < Decimal::from_str("0.01").unwrap()
            );
        }
        _ => panic!("Expected Number (from 'in' conversion)"),
    }

    let distance_result = response
        .results
        .iter()
        .find(|r| r.rule.name == "distance_miles")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match distance_result {
        LiteralValue::Number(amount) => {
            assert!(
                (amount - Decimal::from_str("621.371").unwrap()).abs()
                    < Decimal::from_str("0.01").unwrap()
            );
        }
        _ => panic!("Expected Number (from 'in' conversion)"),
    }

    let price_result = response
        .results
        .iter()
        .find(|r| r.rule.name == "discounted_price")
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    match price_result {
        LiteralValue::Number(n) => {
            assert_eq!(*n, Decimal::from_str("90.0").unwrap());
        }
        _ => panic!("Expected Number"),
    }
    engine.remove_document("complex");
}
