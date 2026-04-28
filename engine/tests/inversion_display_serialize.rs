use lemma::{Bound, Domain};
use serde_json::json;
use std::sync::Arc;

#[test]
fn serialize_domain_range() {
    let d = Domain::Range {
        min: Bound::Inclusive(Arc::new(lemma::LiteralValue::number(0.into()))),
        max: Bound::Exclusive(Arc::new(lemma::LiteralValue::number(10.into()))),
    };
    let v = serde_json::to_value(&d).expect("serialize domain");

    assert_eq!(v["type"], json!("range"));
    assert_eq!(v["min"]["type"], json!("inclusive"));
    assert!(
        v["min"]["value"].is_object(),
        "LiteralValue serializes as object: {:?}",
        v["min"]["value"]
    );
    assert_eq!(v["max"]["type"], json!("exclusive"));
}
