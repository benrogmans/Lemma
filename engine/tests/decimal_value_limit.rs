use lemma::{DateTimeValue, Engine, SourceType};
use rust_decimal::Decimal;
use std::collections::HashMap;

fn commit_boundary_spec() -> String {
    let max_decimal = Decimal::MAX.normalize().to_string();
    format!(
        r#"
spec commit_boundary
data max_val: {max_decimal}
data two: 2
rule huge: max_val * two
rule safe: huge / two
"#
    )
}

fn rule_by_name<'response>(
    response: &'response lemma::Response,
    rule_name: &str,
) -> &'response lemma::RuleResult {
    response
        .results
        .values()
        .find(|result| result.rule.name == rule_name)
        .unwrap_or_else(|| panic!("rule '{rule_name}' must be in the response"))
}

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

#[test]
fn uncommittable_intermediate_stored_exactly_explain_result_not_vetoed() {
    let mut engine = Engine::new();
    engine
        .load(commit_boundary_spec(), SourceType::Volatile)
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "commit_boundary",
            Some(&now),
            HashMap::new(),
            true,
            None,
        )
        .expect("evaluation must complete");

    let huge = rule_by_name(&response, "huge");
    assert!(
        huge.vetoed,
        "response must veto when output materialization cannot commit to decimal"
    );
    assert_eq!(
        huge.veto_reason.as_deref(),
        Some("Calculated result exceeds decimal value limit")
    );

    let explanation = huge.explanation.as_ref().expect("huge explanation");
    assert!(
        !explanation.result.vetoed(),
        "rule_results must store the exact computed value; decimal commit applies only at response materialization"
    );
}

#[test]
fn commit_boundary_full_eval_huge_vetoes_safe_succeeds() {
    let mut engine = Engine::new();
    engine
        .load(commit_boundary_spec(), SourceType::Volatile)
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "commit_boundary",
            Some(&now),
            HashMap::new(),
            false,
            None,
        )
        .expect("evaluation must complete");

    let huge = response.results.get("huge").expect("huge");
    assert!(huge.vetoed);
    assert_eq!(
        huge.veto_reason.as_deref(),
        Some("Calculated result exceeds decimal value limit")
    );

    let safe = response.results.get("safe").expect("safe");
    assert!(
        !safe.vetoed,
        "safe must commit at response materialization even when huge is uncommittable"
    );
    assert_eq!(safe.number.as_deref(), Some(max_decimal_string().as_str()));
}

#[test]
fn commit_boundary_targeted_safe_succeeds() {
    let mut engine = Engine::new();
    engine
        .load(commit_boundary_spec(), SourceType::Volatile)
        .unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "commit_boundary",
            Some(&now),
            HashMap::new(),
            false,
            Some(&["safe".to_string()]),
        )
        .expect("evaluation must complete");

    assert_eq!(response.results.len(), 1);
    let safe = response.results.get("safe").expect("safe");
    assert!(!safe.vetoed);
    assert_eq!(safe.number.as_deref(), Some(max_decimal_string().as_str()));
}

fn max_decimal_string() -> String {
    Decimal::MAX.normalize().to_string()
}
