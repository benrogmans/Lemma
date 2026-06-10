//! Failing repro: [`Engine::load_batch`]`(…, Some(dep))` WASM-style bundle + duplicate basename `finance` across
//! `repo x` and `repo collide` → [`Engine::apply_planning_result`] keys only [`LemmaSpec::name`], so
//! `get_plan` returns identical plan pointers. Parser needs `repo @benrogmans/test` + `spec constants` (not
//! `repo @benrogmans/test constants`); `uses b: @benrogmans/tess x` needs tess `spec x`.

use lemma::{DateGranularity, DateTimeValue, Engine, SourceType};
use std::collections::HashMap;
use std::sync::Arc;

#[test]
fn wasm_dep_batch_user_dual_slice_finance_must_not_share_plan_blob_with_foreign_finance() {
    let mut engine = Engine::new();
    engine
        .load_batch(
            HashMap::from([
                (
                    SourceType::Path(Arc::new(std::path::PathBuf::from("x/consumer.lemma"))),
                    r#"repo x

spec finance

  rule x:
    4


spec finance 2024-01-02

  uses c: @benrogmans/test constants

  uses b: @benrogmans/tess x

  rule x:
    c.pi
"#
                    .to_string(),
                ),
                (
                    SourceType::Path(Arc::new(std::path::PathBuf::from("deps/constants.lemma"))),
                    r#"repo @benrogmans/test

spec constants
data pi: 3.14
"#
                    .to_string(),
                ),
                (
                    SourceType::Path(Arc::new(std::path::PathBuf::from("deps/tess.lemma"))),
                    r#"repo @benrogmans/tess

spec x
rule placeholder: true
"#
                    .to_string(),
                ),
                (
                    SourceType::Path(Arc::new(std::path::PathBuf::from("collide/other.lemma"))),
                    r#"repo collide

spec finance
rule sentinel: false
"#
                    .to_string(),
                ),
            ]),
            Some("wasm-bundle-dep"),
        )
        .expect("fixture must parse and plan");

    let effective = DateTimeValue {
        year: 2026,
        month: 6,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,

        granularity: DateGranularity::Full,
    };

    let plan_x_repo = engine
        .get_plan(Some("x"), "finance", Some(&effective))
        .expect("repo x / finance");
    let plan_collide = engine
        .get_plan(Some("collide"), "finance", Some(&effective))
        .expect("repo collide / finance");

    assert_ne!(
        std::ptr::from_ref(plan_x_repo),
        std::ptr::from_ref(plan_collide),
        "`plan_sets` keyed only by LemmaSpec.name — x::finance and collide::finance wrongly alias"
    );

    assert!(
        plan_collide.rules.iter().any(|r| r.name == "sentinel"),
        "collide must keep its own rule `sentinel` once plan storage is per repository"
    );
}
