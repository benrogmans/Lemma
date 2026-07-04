use lemma::{DateTimeValue, Engine, SourceType};
use std::collections::HashMap;

#[test]
fn delivery_cost_materializes_converted_unit_with_schema_decimals() {
    let code = r#"
spec delivery 2026-01-01

data distance: measure
  -> unit meter 1
  -> unit kilometer 1000

data money: measure
  -> decimals 2
  -> unit eur 1.00
  -> unit usd 0.84

data rate: measure
  -> unit eur_per_km eur/kilometer

rule delivery_cost: 0.26 eur_per_km * distance
"#;

    let mut engine = Engine::new();
    engine.load(code, SourceType::Volatile).unwrap();

    let effective = DateTimeValue {
        year: 2026,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,
        granularity: lemma::DateGranularity::Full,
    };

    let response = engine
        .run(
            None,
            "delivery",
            Some(&effective),
            HashMap::from([("distance".to_string(), "12 kilometer".to_string())]),
            false,
            None,
        )
        .expect("run");

    let delivery_cost = response
        .results
        .get("delivery_cost")
        .expect("delivery_cost rule");

    assert!(!delivery_cost.vetoed);
    let measure = delivery_cost
        .measure
        .as_ref()
        .expect("measure map on delivery_cost");
    assert_eq!(measure.get("eur"), Some(&"3.12".to_string()));
    assert_eq!(measure.get("usd"), Some(&"3.71".to_string()));
}
