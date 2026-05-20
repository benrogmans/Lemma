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
uses b
data x: b.x

spec b
uses a
data x: a.x
"#,
            SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("cycle.lemma"))),
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
uses b
data x: b.x

spec b
uses a
data x: a.x

spec c 2025-01-01
uses b
data y: b.x
rule r: y
"#,
            SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "cycle2.lemma",
            ))),
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
            SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                "rule_cycle.lemma",
            ))),
        )
        .expect_err("rule cycle");

    assert!(err.errors.iter().any(|e| matches!(e, Error::Validation(_))));
}
