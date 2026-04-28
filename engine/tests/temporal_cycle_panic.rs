//! Adversarial: dependency cycles must surface as errors, never panic during
//! `validate_dependency_interfaces` (missing `SpecSetPlanningResult` for a dep name).

use lemma::{Engine, Error, SourceType};

#[test]
fn cross_spec_data_reference_cycle_surfaces_error_not_panic() {
    let mut engine = Engine::new();
    let err = engine
        .load(
            r#"
spec a
data x: number from b

spec b
data x: number from a
"#,
            SourceType::Labeled("cycle.lemma"),
        )
        .expect_err("cross-spec data cycle must fail load");

    let joined = err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.to_lowercase().contains("cycle") || joined.to_lowercase().contains("circular"),
        "expected cycle wording, got: {joined}"
    );
}

#[test]
fn third_spec_depending_on_cyclic_pair_gets_error_not_panic() {
    let mut engine = Engine::new();
    let err = engine
        .load(
            r#"
spec a
data x: number from b

spec b
data x: number from a

spec c 2025-01-01
data y: number from b
rule r: y
"#,
            SourceType::Labeled("cycle2.lemma"),
        )
        .expect_err("must fail");

    let joined = err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        joined.to_lowercase().contains("cycle") || joined.to_lowercase().contains("circular"),
        "expected cycle in errors: {joined}"
    );
}

#[test]
fn rule_only_cycle_still_errors_without_panic() {
    let mut engine = Engine::new();
    let err = engine
        .load(
            r#"
spec t
rule x: y
rule y: x
"#,
            SourceType::Labeled("rule_cycle.lemma"),
        )
        .expect_err("rule cycle");

    assert!(err.errors.iter().any(|e| matches!(e, Error::Validation(_))));
}
