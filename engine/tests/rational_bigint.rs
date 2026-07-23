use lemma::{DateTimeValue, Engine, SourceType};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn decimal_max_multiply_vetoes_on_decimal_limit_not_numeric_overflow() {
    let max_decimal = Decimal::MAX.normalize().to_string();
    let code = format!(
        r#"
spec bigint_exact
data max_val: {max_decimal}
data two: 2
rule product: max_val * two
"#
    );

    let mut engine = Engine::new();
    engine
        .load([(SourceType::Volatile, &code.to_string())])
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "bigint_exact",
            Some(&now),
            HashMap::new(),
            None,
            false,
        )
        .expect("evaluation must complete");

    let rule = response
        .results
        .get("product")
        .expect("product rule must be present");

    assert!(rule.vetoed);
    assert_eq!(
        rule.veto_reason.as_deref(),
        Some("Calculated result exceeds decimal value limit"),
        "response materialization must veto magnitude overflow, not fail with numeric overflow"
    );
    assert_ne!(
        rule.veto_reason.as_deref(),
        Some("numeric overflow"),
        "must not spuriously veto during exact rational evaluation before response materialization"
    );
}
