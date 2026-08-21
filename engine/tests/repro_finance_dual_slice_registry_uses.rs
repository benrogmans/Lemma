//! Dependency-tagged load with duplicate basename `finance` across `repo x` and `repo collide`
//! must not alias compiled plans: distinct `(repository, name)` keys in `Engine::plans`.

use lemma::{DateGranularity, DateTimeValue, Engine, SourceType};

const WASM_BUNDLE_DEP: &str = "wasm-bundle-dep";

#[test]
fn wasm_dep_batch_user_dual_slice_finance_must_not_share_plan_blob_with_foreign_finance() {
    let mut engine = Engine::new();
    engine
        .load([(
            SourceType::Dependency(WASM_BUNDLE_DEP.to_string()),
            format!(
                r#"{consumer}

{constants}

{tess}

{collide}"#,
                consumer = r#"repo x

spec finance

  rule x:
    4


spec finance 2024-01-02

  uses c: @benrogmans/test constants

  uses b: @benrogmans/tess x

  rule x:
    c.pi"#,
                constants = r#"repo @benrogmans/test

spec constants
data pi: 3.14"#,
                tess = r#"repo @benrogmans/tess

spec x
rule ready: true"#,
                collide = r#"repo collide

spec finance
rule sentinel: false"#,
            ),
        )])
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

    let show_x = engine
        .show(Some("x"), "finance", Some(&effective))
        .expect("repo x / finance");
    let show_collide = engine
        .show(Some("collide"), "finance", Some(&effective))
        .expect("repo collide / finance");

    assert_ne!(
        show_x.rules.keys().collect::<Vec<_>>(),
        show_collide.rules.keys().collect::<Vec<_>>(),
        "x::finance and collide::finance must expose distinct compiled interfaces"
    );

    assert!(
        show_collide.rules.contains_key("sentinel"),
        "collide must keep its own rule `sentinel`"
    );
}
