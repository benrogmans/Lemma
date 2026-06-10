//! Structural explanation trees for the contractor invoice spec.

use lemma::{format_explanation, DateTimeValue, Engine};
use std::collections::HashMap;

const CALC_SPEC: &str = r#"
spec calc

data money: quantity
  -> decimals 2
  -> unit eur 1

data hourly_rate: 85.00 eur
data hours_worked: 37.5
data is_rush: boolean
data is_super_rush: boolean

rule labor: hourly_rate * hours_worked
rule rush_surcharge: 0 eur
  unless is_rush then labor * 25%
  unless is_super_rush then labor * 50%
rule subtotal: labor + rush_surcharge
rule vat: subtotal * 21%
rule total: subtotal + vat
"#;

fn run_calc(data: HashMap<String, String>) -> lemma::Explanation {
    let mut engine = Engine::new();
    engine
        .load(CALC_SPEC, lemma::SourceType::Volatile)
        .expect("calc spec loads");
    let now = DateTimeValue::now();
    let response = engine
        .run(None, "calc", Some(&now), data, true, None)
        .expect("calc eval succeeds");
    response
        .results
        .values()
        .find(|r| r.rule.name == "total")
        .expect("total rule evaluated")
        .explanation
        .clone()
        .expect("explanation always built")
}

const NEITHER_UNLESS_MATCHES: &str = "\
total: 3856.88 eur
└─ subtotal + vat
   ├─ subtotal
   │  └─ labor + rush_surcharge
   │     ├─ labor: hourly_rate * hours_worked
   │     │  ├─ hourly_rate: 85.00 eur
   │     │  └─ hours_worked: 37.5
   │     └─ rush_surcharge: 0.00 eur
   │        ├─ is_rush is false
   │        └─ is_super_rush is false
   └─ vat: subtotal * 21%";

const IS_RUSH_ONLY: &str = "\
total: 4821.09 eur
└─ subtotal + vat
   ├─ subtotal
   │  └─ labor + rush_surcharge
   │     ├─ labor: hourly_rate * hours_worked
   │     │  ├─ hourly_rate: 85.00 eur
   │     │  └─ hours_worked: 37.5
   │     └─ rush_surcharge: labor * 25%
   │        ├─ is_rush is true
   │        └─ is_super_rush is false
   └─ vat: subtotal * 21%";

const IS_SUPER_RUSH: &str = "\
total: 5785.31 eur
└─ subtotal + vat
   ├─ subtotal
   │  └─ labor + rush_surcharge
   │     ├─ labor: hourly_rate * hours_worked
   │     │  ├─ hourly_rate: 85.00 eur
   │     │  └─ hours_worked: 37.5
   │     └─ rush_surcharge: labor * 50%
   │        └─ is_super_rush is true
   └─ vat: subtotal * 21%";

#[test]
fn contractor_invoice_neither_unless_matches() {
    let mut data = HashMap::new();
    data.insert("is_rush".into(), "false".into());
    data.insert("is_super_rush".into(), "false".into());
    let explanation = run_calc(data);
    assert_eq!(format_explanation(&explanation), NEITHER_UNLESS_MATCHES);
}

#[test]
fn contractor_invoice_is_rush_only() {
    let mut data = HashMap::new();
    data.insert("is_rush".into(), "true".into());
    data.insert("is_super_rush".into(), "false".into());
    let explanation = run_calc(data);
    assert_eq!(format_explanation(&explanation), IS_RUSH_ONLY);
}

#[test]
fn contractor_invoice_is_super_rush() {
    let mut data = HashMap::new();
    data.insert("is_rush".into(), "true".into());
    data.insert("is_super_rush".into(), "true".into());
    let explanation = run_calc(data);
    assert_eq!(format_explanation(&explanation), IS_SUPER_RUSH);
}
