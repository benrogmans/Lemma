//! End-to-end tests for strict veto semantics under normalization.
//!
//! Normalization may only delete a subexpression when it is total (a literal
//! that can never veto). These tests pin the contract that the folded and
//! unfolded forms of the same expression produce identical evaluation
//! results for all of: value present, value missing, and operand vetoes.

use lemma::{DateTimeValue, Engine};
use std::collections::HashMap;

fn load(code: &str) -> Engine {
    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Volatile)
        .expect("spec must load");
    engine
}

fn run(engine: &Engine, spec: &str, inputs: &[(&str, &str)]) -> lemma::Response {
    let now = DateTimeValue::now();
    let data: HashMap<String, String> = inputs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    engine
        .run(None, spec, Some(&now), data, false, None)
        .expect("evaluation must succeed")
}

fn rule_result<'response>(
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
fn multiply_by_zero_with_missing_data_vetoes() {
    let engine = load(
        r#"
spec strict_multiply
data x: number
rule r: x * 0
"#,
    );

    let missing = run(&engine, "strict_multiply", &[]);
    let result = rule_result(&missing, "r");
    assert!(
        result.vetoed,
        "x is missing: the multiply must veto, not fold to 0"
    );

    let supplied = run(&engine, "strict_multiply", &[("x", "7")]);
    let result = rule_result(&supplied, "r");
    assert!(!result.vetoed);
    assert_eq!(result.display.as_deref(), Some("0"));
}

#[test]
fn multiply_measure_data_by_zero_keeps_unit() {
    let engine = load(
        r#"
spec strict_measure_multiply
data money: measure -> unit eur 1
data price: money
rule r: price * 0
"#,
    );

    let response = run(&engine, "strict_measure_multiply", &[("price", "5 eur")]);
    let result = rule_result(&response, "r");
    assert!(!result.vetoed);
    assert_eq!(
        result.display.as_deref(),
        Some("0 eur"),
        "the runtime multiply must keep the measure's unit"
    );

    let missing = run(&engine, "strict_measure_multiply", &[]);
    let result = rule_result(&missing, "r");
    assert!(result.vetoed, "price is missing: the multiply must veto");
}

#[test]
fn multiply_measure_literal_by_zero_folds_to_typed_zero() {
    let engine = load(
        r#"
spec folded_measure_multiply
data money: measure -> unit eur 1
data rate: 5 eur
rule r: rate * 0
"#,
    );

    // `rate` is bound in the spec, so the inlined literal product folds at
    // planning time; the folded zero must keep the unit signature.
    let response = run(&engine, "folded_measure_multiply", &[]);
    let result = rule_result(&response, "r");
    assert!(!result.vetoed);
    assert_eq!(result.display.as_deref(), Some("0 eur"));
}

#[test]
fn multiply_vetoing_rule_by_zero_propagates_the_veto() {
    let engine = load(
        r#"
spec strict_veto_multiply
rule blocked: veto "credit check failed"
rule r: blocked * 0
"#,
    );

    let response = run(&engine, "strict_veto_multiply", &[]);
    let result = rule_result(&response, "r");
    assert!(
        result.vetoed,
        "the inlined veto must propagate through the multiply"
    );
    assert_eq!(result.veto_reason.as_deref(), Some("credit check failed"));
}

#[test]
fn and_with_literal_false_keeps_missing_data_veto() {
    let engine = load(
        r#"
spec strict_and
data flag: boolean
rule r: flag and (1 > 2)
"#,
    );

    // `1 > 2` constant-folds to `false`; the conjunction must still demand
    // `flag` so its missing-data veto propagates.
    let missing = run(&engine, "strict_and", &[]);
    let result = rule_result(&missing, "r");
    assert!(
        result.vetoed,
        "flag is missing: the conjunction must veto, not fold to false"
    );

    let supplied = run(&engine, "strict_and", &[("flag", "yes")]);
    let result = rule_result(&supplied, "r");
    assert!(!result.vetoed);
    assert_eq!(result.display.as_deref(), Some("false"));
}

#[test]
fn exp_of_log_keeps_domain_veto() {
    let engine = load(
        r#"
spec strict_exp_log
data offset: number
rule r: exp (log offset)
"#,
    );

    let negative = run(&engine, "strict_exp_log", &[("offset", "-5")]);
    let result = rule_result(&negative, "r");
    assert!(
        result.vetoed,
        "log of a negative operand must veto; the exp/log collapse would erase it"
    );

    let positive = run(&engine, "strict_exp_log", &[("offset", "5")]);
    let result = rule_result(&positive, "r");
    assert!(!result.vetoed);
}

#[test]
fn power_zero_with_missing_data_vetoes() {
    let engine = load(
        r#"
spec strict_power_zero
data x: number
rule r: x ^ 0
"#,
    );

    let missing = run(&engine, "strict_power_zero", &[]);
    let result = rule_result(&missing, "r");
    assert!(
        result.vetoed,
        "x is missing: the power must veto, not fold to 1"
    );

    let supplied = run(&engine, "strict_power_zero", &[("x", "9")]);
    let result = rule_result(&supplied, "r");
    assert!(!result.vetoed);
    assert_eq!(result.display.as_deref(), Some("1"));
}

#[test]
fn self_doubling_rule_chain_is_a_planning_error_not_a_crash() {
    // Each rule references its predecessor twice; transitive inlining
    // doubles the materialized expression per step. Planning must reject
    // the chain with a resource-limit error in bounded time — never
    // exhaust memory or overflow the compiler's operand indices.
    let mut code = String::from("\nspec doubling\ndata input: number\nrule r0: input > 0\n");
    for index in 1..=40 {
        code.push_str(&format!(
            "rule r{index}: r{previous} and not r{previous}\n",
            previous = index - 1
        ));
    }

    let mut engine = Engine::new();
    let load_result = engine.load(&code, lemma::SourceType::Volatile);
    let error_text = match load_result {
        Err(errors) => errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        Ok(()) => panic!("the self-doubling chain must be rejected at planning time"),
    };
    assert!(
        error_text.contains("max_normalized_expression_nodes"),
        "expected the expression node limit error, got: {error_text}"
    );
}
