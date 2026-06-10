use lemma::{DateTimeValue, Engine, SourceType};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn rule_vetoes_when_result_exceeds_decimal_value_limit() {
    let max_decimal = Decimal::MAX.normalize().to_string();
    let code = format!(
        r#"
spec decimal_limit
data max_val: {max_decimal}
data two: 2
rule over_limit: max_val * two
"#
    );

    let mut engine = Engine::new();
    engine.load(&code, SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "decimal_limit",
            Some(&now),
            HashMap::new(),
            false,
            None,
        )
        .expect("evaluation must complete");

    let rule = response
        .results
        .get("over_limit")
        .expect("over_limit rule must be present");

    assert!(rule.vetoed);
    assert_eq!(
        rule.veto_reason.as_deref(),
        Some("Calculated result exceeds decimal value limit")
    );
}
