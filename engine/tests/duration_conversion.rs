use std::collections::HashMap;

use lemma::DateTimeValue;
use lemma::{Engine, ValueKind};

#[test]
fn test_duration_conversion_properties() {
    let mut engine = Engine::new();
    let code = r#"
spec test
uses lemma units
data duration: 60 minutes
rule to_hours: duration as hours
"#;
    engine
        .load(
            code,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test"))),
        )
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(None, "test", Some(&now), HashMap::new(), true, None)
        .unwrap();
    let rule_result = response
        .results
        .values()
        .find(|r| r.rule.name == "to_hours")
        .unwrap();
    let val = rule_result
        .explanation
        .as_ref()
        .expect("explanation")
        .result
        .value()
        .expect("value")
        .clone();

    if let ValueKind::Quantity(_, _) = &val.value {
        assert_eq!(
            rule_result
                .quantity
                .as_ref()
                .and_then(|m| m.get("hours"))
                .map(String::as_str),
            Some("1"),
            "60 minutes as hours"
        );
    } else {
        panic!(
            "to_hours should be a Quantity after conversion, got {:?}",
            val
        );
    }
}
