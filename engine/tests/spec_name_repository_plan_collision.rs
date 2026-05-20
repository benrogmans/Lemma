//! Regression shield: [`crate::planning::plan`] stores results in an [`indexmap::IndexMap`] keyed
//! only by [`LemmaSpec::name`] (see [`engine/src/planning/mod.rs`] `plan_spec`), and
//! [`Engine::apply_planning_result`] then does `plan_sets.insert(r.name.clone(), ...)`.
//!
//! Different repositories legitimately reuse the same spec basename (names live per repository).
//! Until planning and `plan_sets` key by `(repository, spec)` (or equivalent), the second caller
//! clobbers slice identity and [`Engine::get_plan`] returns the wrong [`ExecutionPlan`] for at
//! least one repository.
//!
//! This test **must fail** until that design is fixed; remove or rewrite when the landmine is gone.

use lemma::parsing::ast::DateTimeValue;
use lemma::{Engine, SourceType};
use rust_decimal::Decimal;
use std::sync::Arc;

fn path_source(path: &str) -> SourceType {
    SourceType::Path(Arc::new(std::path::PathBuf::from(path)))
}

fn rule_answer_decimal(response: &lemma::evaluation::Response) -> Decimal {
    let rr = response
        .results
        .get("answer")
        .expect("spec defines rule answer");
    match &rr.result {
        lemma::OperationResult::Value(lit) => match &lit.value {
            lemma::ValueKind::Number(n) => lemma::commit_rational_to_decimal(n).unwrap(),
            other => panic!("expected number answer, got {:?}", other),
        },
        other => panic!("expected value result, got {:?}", other),
    }
}

#[test]
fn distinct_repositories_same_spec_basename_must_not_alias_execution_plan() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"repo alpha
spec duped
rule answer: 1
"#,
            path_source("alpha.lemma"),
        )
        .expect("alpha repository loads");

    engine
        .load(
            r#"repo beta
spec duped
rule answer: 99
"#,
            path_source("beta.lemma"),
        )
        .expect("beta repository loads");

    let now = DateTimeValue::now();

    let plan_alpha = engine
        .get_plan(Some("alpha"), "duped", Some(&now))
        .expect("alpha/duped plan exists");
    let plan_beta = engine
        .get_plan(Some("beta"), "duped", Some(&now))
        .expect("beta/duped plan exists");

    assert_ne!(
        std::ptr::from_ref(plan_alpha),
        std::ptr::from_ref(plan_beta),
        "BUG: plan_sets and planning results are keyed only by spec.name — \
         alpha::duped and beta::duped must not share the same &ExecutionPlan"
    );

    let run_alpha = engine
        .run(
            Some("alpha"),
            "duped",
            Some(&now),
            Default::default(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run alpha");
    let run_beta = engine
        .run(
            Some("beta"),
            "duped",
            Some(&now),
            Default::default(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("run beta");

    assert_eq!(
        rule_answer_decimal(&run_alpha),
        Decimal::ONE,
        "alpha::duped rule answer must be 1"
    );
    assert_eq!(
        rule_answer_decimal(&run_beta),
        Decimal::from(99),
        "beta::duped rule answer must be 99"
    );
}
