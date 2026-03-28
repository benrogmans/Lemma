//! Integration tests for plan fingerprint hashing.
//!
//! Locks in behaviour: semantic equivalence => same hash, semantic difference => different hash.
//! Source-only changes (labels, etc.) must not affect the hash.
//!
//! Golden hashes below: load Lemma source into `Engine`, `get_plan`, then `plan_hash`
//! (format version `lemma::planning::fingerprint::FINGERPRINT_FORMAT_VERSION`, same encoding as unit goldens).

use lemma::{DateTimeValue, Engine};

fn date(year: i32, month: u32, day: u32) -> DateTimeValue {
    DateTimeValue {
        year,
        month,
        day,
        hour: 0,
        minute: 0,
        second: 0,
        microsecond: 0,
        timezone: None,
    }
}

fn plan_hash(engine: &Engine, spec: &str, effective: &DateTimeValue) -> String {
    engine
        .get_plan_hash(spec, effective)
        .unwrap()
        .expect("spec must have plan")
}

fn plan_hash_from_loaded_plan(engine: &Engine, spec: &str, effective: &DateTimeValue) -> String {
    engine.get_plan(spec, Some(effective)).unwrap().plan_hash()
}

// -----------------------------------------------------------------------------
// Golden plan hashes (Lemma source -> engine -> get_plan -> plan_hash)
//
// Covers: minimal/unless/facts-only; temporal slice + effective query; commentary + meta +
// custom type; cross-spec ref with `spec dep~plan_hash` pin (hash matches engine plan hash).
// -----------------------------------------------------------------------------

#[test]
fn golden_loaded_minimal_fact_and_rule() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec golden_loaded_minimal\nfact x: 1\nrule r: x",
            lemma::SourceType::Labeled("golden.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    assert_eq!(
        plan_hash_from_loaded_plan(&engine, "golden_loaded_minimal", &eff),
        "56c29a91"
    );
}

#[test]
fn golden_loaded_unless_rule() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec golden_loaded_unless\nfact q: 10\nrule r: 0\n unless q >= 5 then 5",
            lemma::SourceType::Labeled("golden.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    assert_eq!(
        plan_hash_from_loaded_plan(&engine, "golden_loaded_unless", &eff),
        "6a33b890"
    );
}

#[test]
fn golden_loaded_facts_only_no_rules() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec golden_loaded_facts\nfact a: 1\nfact b: 2",
            lemma::SourceType::Labeled("golden.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    assert_eq!(
        plan_hash_from_loaded_plan(&engine, "golden_loaded_facts", &eff),
        "696ce61c"
    );
}

#[test]
fn golden_loaded_temporal_effective_slice() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"spec golden_temporal_adv 2025-06-01

fact x: 42
rule r: x"#,
            lemma::SourceType::Labeled("golden.lemma"),
        )
        .unwrap();
    let eff = date(2025, 7, 1);
    assert_eq!(
        plan_hash_from_loaded_plan(&engine, "golden_temporal_adv", &eff),
        "6d8cc867"
    );
}

#[test]
fn golden_loaded_commentary_meta_and_custom_type() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"spec golden_rich_adv
"""
Golden integration fixture: commentary and meta are not part of the semantic plan hash.
"""
meta title: "Golden rich"
meta version: v2.0.0

type money: scale
 -> unit eur 1.00
 -> decimals 2

fact balance: 50 eur
rule total: balance"#,
            lemma::SourceType::Labeled("golden.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    assert_eq!(
        plan_hash_from_loaded_plan(&engine, "golden_rich_adv", &eff),
        "d5f0b7ae"
    );
}

#[test]
fn golden_loaded_spec_ref_hash_pinned() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"spec golden_dep_ref
fact seed: 7
rule computed: seed + 1"#,
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();
    let dep_h = plan_hash(&engine, "golden_dep_ref", &date(2025, 1, 1));
    engine
        .load(
            &format!(
                r#"spec golden_consumer_ref
fact link: spec golden_dep_ref~{}
rule out: link.computed"#,
                dep_h
            ),
            lemma::SourceType::Labeled("consumer.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    assert_eq!(
        plan_hash_from_loaded_plan(&engine, "golden_consumer_ref", &eff),
        "c161b54e"
    );
}

// -----------------------------------------------------------------------------
// Semantic equivalence: same hash
// -----------------------------------------------------------------------------

#[test]
fn same_spec_different_source_labels_same_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("a.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("b.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_eq!(
        plan_hash(&e1, "t", &eff),
        plan_hash(&e2, "t", &eff),
        "Source label must not affect hash"
    );
}

#[test]
fn hash_deterministic_same_plan_twice() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec t\nfact x: 1\nrule r: x",
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    let h1 = plan_hash(&engine, "t", &eff);
    let h2 = plan_hash(&engine, "t", &eff);
    assert_eq!(h1, h2, "Hash must be deterministic");
}

#[test]
fn empty_plan_same_hash_across_instances() {
    let mut e1 = Engine::new();
    e1.load(
        "spec empty\nfact x: 1",
        lemma::SourceType::Labeled("a.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec empty\nfact x: 1",
        lemma::SourceType::Labeled("b.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_eq!(plan_hash(&e1, "empty", &eff), plan_hash(&e2, "empty", &eff));
}

// -----------------------------------------------------------------------------
// Semantic difference: different hash
// -----------------------------------------------------------------------------

#[test]
fn different_fact_value_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 2\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn different_fact_type_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: \"a\"\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn different_rule_expression_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 1\nrule r: x + 1",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn add_fact_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 1\nfact y: 2\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn remove_fact_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 1\nfact y: 2\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn add_rule_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 1\nrule r: x\nrule s: x + 1",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn different_unless_clause_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 5\nrule r: 0\n unless x >= 10 then 10",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 5\nrule r: 0\n unless x >= 20 then 20",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn different_spec_name_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec a\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec b\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "a", &eff), plan_hash(&e2, "b", &eff));
}

#[test]
fn different_spec_ref_dependency_different_hash() {
    let mut e1 = Engine::new();
    e1.load("spec dep1\nfact x: 1\nrule r: x\nspec dep2\nfact x: 2\nrule r: x\nspec consumer\nfact d: spec dep1\nrule val: d.r", lemma::SourceType::Labeled("t.lemma",))
    .unwrap();
    let mut e2 = Engine::new();
    e2.load("spec dep1\nfact x: 1\nrule r: x\nspec dep2\nfact x: 2\nrule r: x\nspec consumer\nfact d: spec dep2\nrule val: d.r", lemma::SourceType::Labeled("t.lemma",))
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(
        plan_hash(&e1, "consumer", &eff),
        plan_hash(&e2, "consumer", &eff)
    );
}

#[test]
fn different_temporal_slice_different_hash() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"spec t
fact x: 1
rule r: x

spec t 2025-04-01
fact x: 2
rule r: x"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();
    let h1 = plan_hash(&engine, "t", &date(2025, 1, 1));
    let h2 = plan_hash(&engine, "t", &date(2025, 6, 1));
    assert_ne!(
        h1, h2,
        "Different temporal slices must have different hashes"
    );
}

#[test]
fn adding_later_version_does_not_change_earlier_plan_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        r#"spec t
fact x: 1
rule r: x

spec t 2025-04-01
fact x: 2
rule r: x"#,
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let h_before = plan_hash(&e1, "t", &date(2025, 1, 1));
    let h_after = plan_hash(&e2, "t", &date(2025, 1, 1));
    assert_eq!(
        h_before, h_after,
        "valid_to must not affect hash; adding later version must not change earlier slice hash"
    );
}

#[test]
fn type_import_pinned_vs_unpinned_same_resolution_same_hash() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"spec dep
type money: scale -> unit eur 1
fact x: 1 eur

spec consumer
type money from dep
fact p: [money]
rule r: p"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();
    let dep_hash = plan_hash(&engine, "dep", &date(2025, 1, 1));
    let consumer_unpinned = plan_hash(&engine, "consumer", &date(2025, 1, 1));

    let mut e2 = Engine::new();
    e2.load(
        &format!(
            r#"spec dep
type money: scale -> unit eur 1
fact x: 1 eur

spec consumer
type money from dep~{}
fact p: [money]
rule r: p"#,
            dep_hash
        ),
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let consumer_pinned = plan_hash(&e2, "consumer", &date(2025, 1, 1));

    assert_eq!(
        consumer_unpinned, consumer_pinned,
        "unpinned and pinned type import resolving to same dep must yield same plan hash"
    );
}

#[test]
fn type_import_different_dep_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        r#"spec dep1
type money: scale -> unit eur 1
fact x: 1 eur

spec dep2
type money: scale -> unit eur 1 -> unit usd 1.1
fact x: 1 eur

spec consumer
type money from dep1
fact p: [money]
rule r: p"#,
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        r#"spec dep1
type money: scale -> unit eur 1
fact x: 1 eur

spec dep2
type money: scale -> unit eur 1 -> unit usd 1.1
fact x: 1 eur

spec consumer
type money from dep2
fact p: [money]
rule r: p"#,
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(
        plan_hash(&e1, "consumer", &eff),
        plan_hash(&e2, "consumer", &eff),
        "type import from dep1 vs dep2 must yield different hashes"
    );
}

#[test]
fn is_default_vs_explicit_value_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\ntype n: number -> default 1\nfact x: [n]\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\ntype n: number -> default 1\nfact x: 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(
        plan_hash(&e1, "t", &eff),
        plan_hash(&e2, "t", &eff),
        "is_default must be in fingerprint"
    );
}

// -----------------------------------------------------------------------------
// Edge cases
// -----------------------------------------------------------------------------

#[test]
fn empty_plan_hash_stable() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec empty\nfact x: 1",
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    let h1 = plan_hash(&engine, "empty", &eff);
    let h2 = plan_hash(&engine, "empty", &eff);
    assert_eq!(h1, h2);
}

#[test]
fn facts_only_no_rules_hash_stable() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec t\nfact a: 1\nfact b: 2\nfact c: 3",
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    let h1 = plan_hash(&engine, "t", &eff);
    let h2 = plan_hash(&engine, "t", &eff);
    assert_eq!(h1, h2);
}

#[test]
fn type_only_facts_hash_stable() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec t\nfact x: [number]\nrule r: x",
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    let h1 = plan_hash(&engine, "t", &eff);
    let h2 = plan_hash(&engine, "t", &eff);
    assert_eq!(h1, h2);
}

#[test]
fn spec_ref_fact_hash_stable() {
    let mut engine = Engine::new();
    engine
        .load(
            "spec dep\nfact x: 1\nrule r: x\nspec consumer\nfact d: spec dep\nrule val: d.r",
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    let h1 = plan_hash(&engine, "consumer", &eff);
    let h2 = plan_hash(&engine, "consumer", &eff);
    assert_eq!(h1, h2);
}

#[test]
fn many_unless_branches_hash_stable() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"spec t
fact q: 25
rule r: 0 percent
 unless q >= 10 then 10 percent
 unless q >= 50 then 20 percent
 unless q >= 100 then 30 percent"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();
    let eff = date(2025, 1, 1);
    let h1 = plan_hash(&engine, "t", &eff);
    let h2 = plan_hash(&engine, "t", &eff);
    assert_eq!(h1, h2);
}

#[test]
fn logical_and_expression_different_from_arithmetic() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact a: true\nfact b: true\nrule r: a and b",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact a: 1\nfact b: 1\nrule r: a + b",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn comparison_vs_literal_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 5\nrule r: x > 0",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 5\nrule r: true",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn veto_expression_different_from_literal() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: -1\nrule r: x\n unless x < 0 then veto \"negative\"",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: -1\nrule r: 0",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn unit_conversion_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\ntype m: scale -> unit eur 1 -> unit usd 1.1\nfact x: 100 eur\nrule r: x in usd",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\ntype m: scale -> unit eur 1 -> unit usd 1.1\nfact x: 100 eur\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn math_function_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 4\nrule r: sqrt x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 4\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}

#[test]
fn rule_reference_different_hash() {
    let mut e1 = Engine::new();
    e1.load(
        "spec t\nfact x: 1\nrule base: x + 1\nrule r: base",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let mut e2 = Engine::new();
    e2.load(
        "spec t\nfact x: 1\nrule base: x + 1\nrule r: x",
        lemma::SourceType::Labeled("t.lemma"),
    )
    .unwrap();
    let eff = date(2025, 1, 1);
    assert_ne!(plan_hash(&e1, "t", &eff), plan_hash(&e2, "t", &eff));
}
