use lemma::{Bound, Domain};
use serde_json::json;
use std::sync::Arc;

#[test]
fn display_piecewise_and_domain() {
    // Domain display basic sanity
    let d1 = Domain::Range {
        min: Bound::Unbounded,
        max: Bound::Inclusive(Arc::new(lemma::LiteralValue::number(10.into()))),
    };
    assert_eq!(d1.to_string(), "(-inf, 10]");

    let d2 = Domain::Enumeration(Arc::new(vec![lemma::LiteralValue::number(5.into())]));
    // Union prints parts with a pipe separator
    let du = Domain::Union(Arc::new(vec![d2, d1]));
    let su = du.to_string();
    assert!(su.contains("{") && su.contains("|"));
}

#[test]
fn serialize_domain_range() {
    let d = Domain::Range {
        min: Bound::Inclusive(Arc::new(lemma::LiteralValue::number(0.into()))),
        max: Bound::Exclusive(Arc::new(lemma::LiteralValue::number(10.into()))),
    };
    let v = serde_json::to_value(&d).expect("serialize domain");

    assert_eq!(v["type"], json!("range"));
    assert_eq!(v["min"]["type"], json!("inclusive"));
    assert!(v["min"]["value"].is_string() || v["min"]["value"].is_object());
    assert_eq!(v["max"]["type"], json!("exclusive"));
}
