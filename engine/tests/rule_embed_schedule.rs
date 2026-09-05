//! Rule-embed evaluation schedule: deps evaluate in plan order; tip-only
//! missing_data matches full-run missing_data for gated dependencies.

use lemma::{DateTimeValue, Engine, SourceType};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

#[test]
fn unless_gated_dep_flag_false_empty_missing_data() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec gated
data amount: number
data flag: boolean
rule dep: amount * 2
rule total: 0
  unless flag then dep
"#
            .to_string(),
        )])
        .expect("load");
    let now = DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("flag".to_string(), "false".to_string());
    let response = engine
        .run(
            None,
            "gated",
            Some(&now),
            data,
            Some(&["total".to_string()]),
            false,
        )
        .expect("run");
    let total = response.results.get("total").expect("total");
    assert!(!total.vetoed);
    assert!(
        total.missing_data().is_empty(),
        "flag=false must not surface amount: {:?}",
        total.missing_data()
    );
    let number = Decimal::from_str(
        total
            .value
            .as_ref()
            .expect("value")
            .number
            .as_ref()
            .expect("number"),
    )
    .expect("decimal");
    assert_eq!(number, Decimal::ZERO);
}

#[test]
fn unless_gated_dep_flag_true_lists_amount() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec gated
data amount: number
data flag: boolean
rule dep: amount * 2
rule total: 0
  unless flag then dep
"#
            .to_string(),
        )])
        .expect("load");
    let now = DateTimeValue::now();
    let mut data = HashMap::new();
    data.insert("flag".to_string(), "true".to_string());
    let response = engine
        .run(
            None,
            "gated",
            Some(&now),
            data,
            Some(&["total".to_string()]),
            false,
        )
        .expect("run");
    let total = response.results.get("total").expect("total");
    assert!(total.vetoed);
    assert!(
        total.awaits_missing_data(),
        "flag=true unbound amount must await: {:?}",
        total.veto_reason
    );
    assert_eq!(
        total.missing_data(),
        &["amount".to_string()][..],
        "live dep arm must list amount: {:?}",
        total.missing_data()
    );
}

#[test]
fn cross_spec_tip_only_run() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Volatile,
            r#"
spec a
rule x: 7

spec b
uses a: a
rule y: a.x + 1
"#
            .to_string(),
        )])
        .expect("load");
    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "b",
            Some(&now),
            HashMap::new(),
            Some(&["y".to_string()]),
            false,
        )
        .expect("run");
    assert_eq!(response.results.len(), 1);
    let y = response.results.get("y").expect("y");
    assert!(!y.vetoed);
    let number = Decimal::from_str(
        y.value
            .as_ref()
            .expect("value")
            .number
            .as_ref()
            .expect("number"),
    )
    .expect("decimal");
    assert_eq!(number, Decimal::from(8));
}
