//! Cross-spec interface contract tests.
//!
//! Locks in two guarantees:
//! - Plan hash (behavior lock): changes when any semantic content changes, including imported types
//! - SpecSchema (IO lock): exposes fact types and rule types to consumers
//!
//! Interface validation enforces structural type compatibility at spec boundaries:
//! base kind, units, scale family, and numeric bounds. Veto is control flow
//! (not a type incompatibility) and propagates through cross-spec references.

use lemma::{DateTimeValue, Engine, TypeSpecification};

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

// =============================================================================
// A. Plan hash locks behavior (PASS)
// =============================================================================

#[test]
fn hash_changes_when_imported_type_constraints_change() {
    let dep_v1 = r#"
spec dep
type temp: scale
  -> unit c 1
  -> minimum -273
"#;
    let dep_v2 = r#"
spec dep
type temp: scale
  -> unit c 1
  -> minimum 0
"#;
    let consumer = r#"
spec consumer
type temp from dep
fact t: [temp]
rule r: t
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h1 = plan_hash(&e1, "consumer", &eff);

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h2 = plan_hash(&e2, "consumer", &eff);

    assert_ne!(
        h1, h2,
        "consumer hash must change when dep type constraints change"
    );
}

#[test]
fn hash_changes_when_imported_type_adds_unit() {
    let dep_v1 = r#"
spec dep
type money: scale
  -> unit eur 1
"#;
    let dep_v2 = r#"
spec dep
type money: scale
  -> unit eur 1
  -> unit usd 1.1
"#;
    let consumer = r#"
spec consumer
type money from dep
fact p: [money]
rule r: p
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h1 = plan_hash(&e1, "consumer", &eff);

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h2 = plan_hash(&e2, "consumer", &eff);

    assert_ne!(h1, h2, "consumer hash must change when dep type adds unit");
}

#[test]
fn hash_changes_when_imported_type_removes_unit() {
    let dep_v1 = r#"
spec dep
type money: scale
  -> unit eur 1
  -> unit usd 1.1
"#;
    let dep_v2 = r#"
spec dep
type money: scale
  -> unit eur 1
"#;
    let consumer_uses_usd = r#"
spec consumer
type money from dep
fact p: [money]
rule r: p in usd
"#;
    let consumer_eur_only = r#"
spec consumer
type money from dep
fact p: [money]
rule r: p
"#;
    let eff = date(2025, 1, 1);

    // Consumer using removed unit must fail planning
    let mut e_fail = Engine::new();
    e_fail
        .load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let err = e_fail.load(
        consumer_uses_usd,
        lemma::SourceType::Labeled("consumer.lemma"),
    );
    assert!(
        err.is_err(),
        "planning must fail when imported type no longer has used unit"
    );

    // Consumer not using removed unit: hash must still differ (type shape changed)
    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(
        consumer_eur_only,
        lemma::SourceType::Labeled("consumer.lemma"),
    )
    .unwrap();
    let h1 = plan_hash(&e1, "consumer", &eff);

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e2.load(
        consumer_eur_only,
        lemma::SourceType::Labeled("consumer.lemma"),
    )
    .unwrap();
    let h2 = plan_hash(&e2, "consumer", &eff);

    assert_ne!(
        h1, h2,
        "consumer hash must change when dep type removes unit"
    );
}

#[test]
fn hash_changes_when_dep_rule_result_type_changes() {
    let dep_v1 = r#"
spec dep
fact x: 42
rule result: x
"#;
    let dep_v2 = r#"
spec dep
type money: scale -> unit eur 1
fact x: 42 eur
rule result: x
"#;
    let consumer = r#"
spec consumer
fact d: spec dep
rule val: d.result
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h1 = plan_hash(&e1, "consumer", &eff);

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h2 = plan_hash(&e2, "consumer", &eff);

    assert_ne!(
        h1, h2,
        "consumer hash must change when dep rule result type changes"
    );
}

#[test]
fn hash_stable_when_dep_only_changes_meta() {
    let dep_v1 = r#"
spec dep
meta author: "alice"
fact x: 1
rule r: x
"#;
    let dep_v2 = r#"
spec dep
meta author: "bob"
fact x: 1
rule r: x
"#;
    let consumer = r#"
spec consumer
fact d: spec dep
rule val: d.r
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h1 = plan_hash(&e1, "consumer", &eff);

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h2 = plan_hash(&e2, "consumer", &eff);

    assert_eq!(
        h1, h2,
        "consumer hash must be stable when dep only changes meta"
    );
}

#[test]
fn hash_changes_when_type_used_only_by_rules_changes() {
    let dep_v1 = r#"
spec dep
type temp: scale
  -> unit c 1
  -> unit f 1.8
  -> minimum -273
"#;
    let dep_v2 = r#"
spec dep
type temp: scale
  -> unit c 1
  -> unit f 1.8
  -> minimum 0
"#;
    let consumer = r#"
spec consumer
type temp from dep
fact x: [number]
rule r: x in c
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h1 = plan_hash(&e1, "consumer", &eff);

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let h2 = plan_hash(&e2, "consumer", &eff);

    assert_ne!(
        h1, h2,
        "consumer hash must change when imported type (used only by rules, not facts) changes"
    );
}

#[test]
fn hash_pin_catches_dep_type_change() {
    let dep_v1 = r#"
spec dep
type money: scale -> unit eur 1
fact x: 1 eur
rule r: x
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let dep_hash = plan_hash(&e1, "dep", &eff);

    let dep_v2 = r#"
spec dep
type money: scale -> unit eur 1 -> unit usd 1.1
fact x: 1 eur
rule r: x
"#;
    let consumer_pinned = format!(
        r#"
spec consumer
type money from dep~{}
fact p: [money]
rule r: p
"#,
        dep_hash
    );

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let err = e2.load(
        &consumer_pinned,
        lemma::SourceType::Labeled("consumer.lemma"),
    );
    assert!(
        err.is_err(),
        "planning must fail when hash_pin no longer matches changed dep"
    );
}

// =============================================================================
// B. SpecSchema IO contract (PASS)
// =============================================================================

#[test]
fn schema_fact_carries_imported_type_constraints() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
type money: scale
  -> unit eur 1
  -> unit usd 1.1
  -> minimum 0 eur
  -> decimals 2

spec consumer
type money from dep
fact price: [money]
rule r: price
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let plan = engine.get_plan("consumer", Some(&eff)).unwrap();
    let schema = plan.schema();

    let (price_type, _) = schema
        .facts
        .get("price")
        .expect("price fact must exist in schema");
    assert!(price_type.is_scale(), "price must be scale type");
    assert_eq!(price_type.name(), "money");

    match &price_type.specifications {
        TypeSpecification::Scale {
            minimum,
            decimals,
            units,
            ..
        } => {
            assert_eq!(*decimals, Some(2));
            assert!(minimum.is_some(), "minimum must be present");
            let unit_names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            assert!(unit_names.contains(&"eur"), "must contain eur");
            assert!(unit_names.contains(&"usd"), "must contain usd");
        }
        other => panic!("expected Scale, got {:?}", other),
    }
}

#[test]
fn schema_fact_reflects_dep_type_constraint_change() {
    let dep_v1 = r#"
spec dep
type temp: scale -> unit c 1 -> maximum 1000
"#;
    let dep_v2 = r#"
spec dep
type temp: scale -> unit c 1 -> maximum 500
"#;
    let consumer = r#"
spec consumer
type temp from dep
fact t: [temp]
rule r: t
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let schema1 = e1.get_plan("consumer", Some(&eff)).unwrap().schema();
    let (t1, _) = schema1.facts.get("t").unwrap();
    let max1 = match &t1.specifications {
        TypeSpecification::Scale { maximum, .. } => maximum.unwrap(),
        _ => panic!("expected scale"),
    };

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let schema2 = e2.get_plan("consumer", Some(&eff)).unwrap().schema();
    let (t2, _) = schema2.facts.get("t").unwrap();
    let max2 = match &t2.specifications {
        TypeSpecification::Scale { maximum, .. } => maximum.unwrap(),
        _ => panic!("expected scale"),
    };

    assert_ne!(
        max1, max2,
        "schema fact type must reflect dep constraint change"
    );
    assert!(max2 < max1, "max must have narrowed from 1000 to 500");
}

#[test]
fn schema_rule_carries_type_from_unit_conversion() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec s
type money: scale
  -> unit eur 1
  -> unit usd 1.1
fact price: [money]
rule converted: price in usd
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("s", Some(&eff)).unwrap().schema();
    let rule_type = schema
        .rules
        .get("converted")
        .expect("converted rule must exist");
    assert!(rule_type.is_scale(), "unit conversion result must be scale");
}

#[test]
fn schema_rule_type_changes_when_dep_rule_type_changes() {
    let dep_v1 = r#"
spec dep
fact x: 42
rule result: x
"#;
    let dep_v2 = r#"
spec dep
fact x: true
rule result: x
"#;
    let consumer = r#"
spec consumer
fact d: spec dep
rule val: d.result
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let s1 = e1.get_plan("consumer", Some(&eff)).unwrap().schema();
    let rt1 = s1.rules.get("val").expect("val rule");

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let s2 = e2.get_plan("consumer", Some(&eff)).unwrap().schema();
    let rt2 = s2.rules.get("val").expect("val rule");

    assert!(rt1.is_number(), "v1 rule type must be number");
    assert!(rt2.is_boolean(), "v2 rule type must be boolean");
}

#[test]
fn schema_facts_exclude_spec_ref_facts() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
fact x: 1
rule r: x

spec consumer
fact d: spec dep
rule val: d.r
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();
    assert!(
        !schema.facts.contains_key("d"),
        "spec-ref fact must not appear in schema.facts"
    );
}

#[test]
fn schema_for_rules_scopes_facts_correctly() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec s
fact a: [number]
fact b: [number]
rule total: a + b
rule just_a: a
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let plan = engine.get_plan("s", Some(&eff)).unwrap();
    let scoped = plan
        .schema_for_rules(&["just_a".to_string()])
        .expect("schema_for_rules must succeed");

    let (a_type, _) = scoped.facts.get("a").expect("scoped schema must include a");
    assert!(
        a_type.is_number(),
        "scoped fact a must still be typed as number"
    );
    assert!(
        !scoped.facts.contains_key("b"),
        "scoped schema must not include b (not needed by just_a)"
    );
}

// =============================================================================
// C. SpecSchema gap -- type used only by rules (PASS, documents limitation)
// =============================================================================

#[test]
fn schema_does_not_expose_named_type_used_only_by_rules() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
type temp: scale -> unit c 1 -> unit f 1.8

spec consumer
type temp from dep
fact x: [number]
rule result: x in c
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();

    let (x_type, _) = schema.facts.get("x").expect("x fact must exist");
    assert!(x_type.is_number(), "fact x must be plain number");

    let result_type = schema.rules.get("result").expect("result rule must exist");
    assert!(
        result_type.is_scale(),
        "result type is scale (from unit conversion) -- but the imported temp type itself is not in SpecSchema"
    );
}

// =============================================================================
// D. Facts lock in type constraints (PASS)
// =============================================================================

#[test]
fn fact_inline_type_import_carries_full_constraints() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
type money: scale
  -> unit eur 1
  -> unit usd 1.1
  -> minimum 0 eur
  -> decimals 2

spec consumer
fact price: [money from dep]
rule r: price
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();
    let (price_type, _) = schema.facts.get("price").expect("price fact");

    assert!(price_type.is_scale());
    match &price_type.specifications {
        TypeSpecification::Scale {
            minimum,
            decimals,
            units,
            ..
        } => {
            assert!(minimum.is_some());
            assert_eq!(*decimals, Some(2));
            assert_eq!(units.iter().count(), 2);
        }
        other => panic!("expected Scale, got {:?}", other),
    }
}

#[test]
fn fact_named_type_import_carries_full_constraints() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
type money: scale
  -> unit eur 1
  -> unit usd 1.1
  -> minimum 0 eur
  -> decimals 2

spec consumer
type money from dep
fact price: [money]
rule r: price
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();
    let (price_type, _) = schema.facts.get("price").expect("price fact");

    assert!(price_type.is_scale());
    match &price_type.specifications {
        TypeSpecification::Scale {
            minimum,
            decimals,
            units,
            ..
        } => {
            assert!(minimum.is_some());
            assert_eq!(*decimals, Some(2));
            assert_eq!(units.iter().count(), 2);
        }
        other => panic!("expected Scale, got {:?}", other),
    }
}

#[test]
fn fact_type_changes_when_dep_type_changes() {
    let dep_v1 = r#"
spec dep
type money: scale -> unit eur 1 -> decimals 2
"#;
    let dep_v2 = r#"
spec dep
type money: scale -> unit eur 1 -> decimals 4
"#;
    let consumer = r#"
spec consumer
type money from dep
fact price: [money]
rule r: price
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep_v1, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let s1 = e1.get_plan("consumer", Some(&eff)).unwrap().schema();
    let (t1, _) = s1.facts.get("price").unwrap();

    let mut e2 = Engine::new();
    e2.load(dep_v2, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();
    let s2 = e2.get_plan("consumer", Some(&eff)).unwrap().schema();
    let (t2, _) = s2.facts.get("price").unwrap();

    let d1 = match &t1.specifications {
        TypeSpecification::Scale { decimals, .. } => decimals.unwrap(),
        _ => panic!("expected scale"),
    };
    let d2 = match &t2.specifications {
        TypeSpecification::Scale { decimals, .. } => decimals.unwrap(),
        _ => panic!("expected scale"),
    };

    assert_ne!(d1, d2, "fact type must update when dep type changes");
    assert_eq!(d1, 2);
    assert_eq!(d2, 4);
}

// =============================================================================
// E. validate_spec_interfaces catches category mismatch (PASS)
// =============================================================================

#[test]
fn interface_rejects_missing_rule_in_dep() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
fact x: 1
rule other: x
"#,
            lemma::SourceType::Labeled("dep.lemma"),
        )
        .unwrap();

    let err = engine.load(
        r#"
spec consumer
fact d: spec dep
rule val: d.result
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );
    assert!(
        err.is_err(),
        "planning must reject reference to missing rule in dep"
    );
}

#[test]
fn interface_rejects_boolean_vs_scale_mismatch() {
    let mut engine = Engine::new();

    let dep_bool = r#"
spec dep
fact x: true
rule result: x
"#;
    engine
        .load(dep_bool, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();

    let err = engine.load(
        r#"
spec consumer
fact d: spec dep
rule val: d.result + 1
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );
    assert!(
        err.is_err(),
        "planning must reject boolean dep rule used in arithmetic"
    );
}

#[test]
fn interface_rejects_text_vs_number_mismatch() {
    let mut engine = Engine::new();

    let dep_text = r#"
spec dep
fact x: "hello"
rule result: x
"#;
    engine
        .load(dep_text, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();

    let err = engine.load(
        r#"
spec consumer
fact d: spec dep
rule val: d.result + 1
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );
    assert!(
        err.is_err(),
        "planning must reject text dep rule used in arithmetic"
    );
}

#[test]
fn interface_accepts_compatible_category() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
fact x: 42
rule result: x

spec consumer
fact d: spec dep
rule val: d.result + 1
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();
    let val_type = schema
        .rules
        .get("val")
        .expect("val rule must exist in schema");
    assert!(
        val_type.is_number(),
        "d.result + 1 must infer number, got {:?}",
        val_type.name()
    );
}

// =============================================================================
// F. validate_spec_interfaces shallow constraint gap (FAIL -- expose the bug)
//
// These tests assert behavior that MUST be rejected but currently isn't.
// `ExpectedRuleTypeConstraint` only checks type category (Scale, Boolean, etc.)
// instead of verifying full type compatibility across spec boundaries.
// =============================================================================

/// Dep rule returns celsius-only scale. Consumer converts dep.result in celsius.
/// Dep changes to fahrenheit-only. Consumer still references `dep.temp in c`.
/// This test only accepts an interface-layer rejection (fact binding message).
/// A downstream semantic rejection is not sufficient.
#[test]
fn interface_should_reject_unit_change_celsius_to_fahrenheit() {
    let dep_celsius = r#"
spec dep
type temp: scale -> unit c 1
fact x: [temp]
rule measured_temp: x
"#;
    let dep_fahrenheit = r#"
spec dep
type temp: scale -> unit f 1
fact x: [temp]
rule measured_temp: x
"#;

    // v1: celsius -- must work
    let mut e1 = Engine::new();
    e1.load(dep_celsius, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(
        r#"
spec consumer
type local_temp: scale -> unit c 1
fact d: spec dep
rule val: d.measured_temp in c
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    )
    .unwrap();

    // v2: dep switches to fahrenheit, consumer still does `d.temp in c`
    let mut e2 = Engine::new();
    e2.load(dep_fahrenheit, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let err = e2.load(
        r#"
spec consumer
type local_temp: scale -> unit c 1
fact d: spec dep
rule val: d.measured_temp in c
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );

    let errs = err.expect_err(
        "planner must reject: dep no longer provides unit 'c' but consumer uses 'd.measured_temp in c'",
    );
    let err_str = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        err_str.contains("Fact binding") && err_str.contains("sets spec reference to"),
        "must reject at interface layer, not only downstream semantics; got: {}",
        err_str
    );
}

/// Dep rule returns money (eur/usd). Dep changes to weight (kg/lb).
/// Consumer uses dep rule in arithmetic with its own money-typed facts.
/// This test only accepts an interface-layer rejection (fact binding message).
/// A downstream semantic rejection is not sufficient.
#[test]
fn interface_should_reject_scale_family_change() {
    let dep_money = r#"
spec dep
type money: scale -> unit eur 1 -> unit usd 1.1
fact x: 100 eur
rule price: x
"#;
    let dep_weight = r#"
spec dep
type weight: scale -> unit kg 1 -> unit lb 2.2
fact x: 100 kg
rule price: x
"#;
    let consumer = r#"
spec consumer
type money: scale -> unit eur 1 -> unit usd 1.1
fact d: spec dep
fact local: [money]
rule combined: d.price + local
"#;

    let mut e1 = Engine::new();
    e1.load(dep_money, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();

    let mut e2 = Engine::new();
    e2.load(dep_weight, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let err = e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"));

    let errs = err.expect_err(
        "planner must reject: dep changed from money to weight scale family, \
         but consumer adds dep.price to a money-typed fact",
    );
    let err_str = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    assert!(
        err_str.contains("Fact binding") && err_str.contains("sets spec reference to"),
        "must reject at interface layer, not only downstream semantics; got: {}",
        err_str
    );
}

/// Dep rule narrows from max: 1000 to max: 100.
/// Consumer has a comparison `dep.result > 500` that can never be true with the new max.
/// Must reject: narrowed constraint makes consumer logic unreachable.
#[test]
fn interface_should_reject_constraint_narrowing() {
    let dep_wide = r#"
spec dep
type val: number -> maximum 1000
fact x: [val]
rule result: x
"#;
    let dep_narrow = r#"
spec dep
type val: number -> maximum 100
fact x: [val]
rule result: x
"#;
    let consumer = r#"
spec consumer
fact d: spec dep
rule check: d.result > 500
"#;

    let mut e1 = Engine::new();
    e1.load(dep_wide, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();

    let mut e2 = Engine::new();
    e2.load(dep_narrow, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let err = e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"));

    assert!(
        err.is_err(),
        "planner must reject: dep narrowed max from 1000 to 100, \
         making consumer's `> 500` comparison unreachable"
    );
}

/// Dep rule changes from number to scale. Both satisfy Numeric.
/// Consumer uses dep rule in plain number arithmetic.
/// Semantics change silently (scale carries units, number does not). Must reject.
#[test]
fn interface_should_reject_number_to_scale_change() {
    let dep_number = r#"
spec dep
fact x: 42
rule result: x
"#;
    let dep_scale = r#"
spec dep
type money: scale -> unit eur 1
fact x: 42 eur
rule result: x
"#;
    let consumer = r#"
spec consumer
fact d: spec dep
fact local: 10
rule combined: d.result + local
"#;

    // number + number: ok
    let mut e1 = Engine::new();
    e1.load(dep_number, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    e1.load(consumer, lemma::SourceType::Labeled("consumer.lemma"))
        .unwrap();

    // scale + number: semantics changed, must reject
    let mut e2 = Engine::new();
    e2.load(dep_scale, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let err = e2.load(consumer, lemma::SourceType::Labeled("consumer.lemma"));

    assert!(
        err.is_err(),
        "planner must reject: dep changed from number to scale, \
         silently changing arithmetic semantics for consumer"
    );
}

// =============================================================================
// G-pre. Cross-spec veto composition (PASS)
//
// Veto is control flow, not a type incompatibility. A consumer referencing
// a dep rule that returns veto is legitimate -- veto propagates at runtime.
// =============================================================================

#[test]
fn interface_accepts_veto_rule_passthrough() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
rule status: veto "decommissioned"

spec consumer
fact d: spec dep
rule out: d.status
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();
    let out_type = schema
        .rules
        .get("out")
        .expect("out rule must exist in schema");
    assert!(
        out_type.vetoed(),
        "passthrough of veto dep rule must infer veto type"
    );
}

#[test]
fn interface_accepts_veto_rule_in_arithmetic() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
rule amount: veto "suspended"

spec consumer
fact d: spec dep
fact local: 10
rule combined: d.amount + local
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();
    let combined_type = schema
        .rules
        .get("combined")
        .expect("combined rule must exist in schema");
    assert!(
        combined_type.vetoed(),
        "veto propagates through arithmetic: veto + number = veto"
    );
}

#[test]
fn interface_rejects_temporal_value_then_veto() {
    let mut engine = Engine::new();
    let err = engine.load(
        r#"
spec dep
fact x: 42
rule status: x

spec dep 2026-01-01
rule status: veto "decommissioned"

spec consumer
fact d: spec dep
rule out: d.status
"#,
        lemma::SourceType::Labeled("t.lemma"),
    );
    assert!(
        err.is_err(),
        "rule changing from number to veto across temporal slices is an interface change"
    );
}

#[test]
fn interface_accepts_veto_rule_in_comparison() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
rule threshold: veto "unavailable"

spec consumer
fact d: spec dep
rule check: d.threshold > 100
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();
    let check_type = schema
        .rules
        .get("check")
        .expect("check rule must exist in schema");
    assert!(
        check_type.vetoed(),
        "veto propagates through comparison: veto > 100 = veto"
    );
}

// =============================================================================
// G. Cross-spec unit conversion (PASS)
// =============================================================================

#[test]
fn rule_uses_imported_type_units_for_conversion() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
type money: scale
  -> unit eur 1
  -> unit usd 1.1

spec consumer
type money from dep
fact price: [money]
rule in_usd: price in usd
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let response = engine
        .run(
            "consumer",
            Some(&eff),
            std::collections::HashMap::from([("price".to_string(), "100 eur".to_string())]),
            false,
        )
        .unwrap();

    let result = response
        .results
        .values()
        .find(|r| r.rule.name == "in_usd")
        .expect("in_usd rule");
    let val_str = result.result.value().unwrap().to_string();
    assert!(
        val_str.contains("110") || val_str.contains("usd"),
        "conversion must produce usd value, got: {}",
        val_str
    );
}

#[test]
fn rule_fails_when_dep_type_removes_used_unit() {
    let dep = r#"
spec dep
type money: scale -> unit eur 1
"#;
    let consumer = r#"
spec consumer
type money from dep
fact price: [money]
rule in_usd: price in usd
"#;

    let mut engine = Engine::new();
    engine
        .load(dep, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let err = engine.load(consumer, lemma::SourceType::Labeled("consumer.lemma"));
    let errs = err.expect_err("planning must fail when consumer uses unit not in imported type");
    let msg = errs
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        msg.to_lowercase().contains("usd")
            || msg.to_lowercase().contains("unit")
            || msg.to_lowercase().contains("unknown"),
        "error should reference missing/unknown unit; got: {}",
        msg
    );
}

#[test]
fn rule_uses_dep_type_unit_without_fact_using_type() {
    let mut engine = Engine::new();
    engine
        .load(
            r#"
spec dep
type money: scale
  -> unit eur 1
  -> unit usd 1.1

spec consumer
type money from dep
fact amount: [number]
rule in_eur: amount in eur
"#,
            lemma::SourceType::Labeled("t.lemma"),
        )
        .unwrap();

    let eff = date(2025, 1, 1);
    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();
    let (amount_type, _) = schema.facts.get("amount").expect("amount fact");
    assert!(
        amount_type.is_number(),
        "fact must remain plain number even though rule uses imported type's units"
    );
    let in_eur_type = schema
        .rules
        .get("in_eur")
        .expect("in_eur rule must exist in schema");
    assert!(
        in_eur_type.is_scale(),
        "amount in eur must infer scale (money family), got {:?}",
        in_eur_type.name()
    );
    assert_eq!(
        in_eur_type.name(),
        "money",
        "conversion must use imported money type name"
    );
    match &in_eur_type.specifications {
        TypeSpecification::Scale { units, .. } => {
            assert!(
                units.iter().any(|u| u.name == "eur"),
                "in_eur result type must include eur unit"
            );
        }
        other => panic!("expected Scale specifications, got {:?}", other),
    }
}

// =============================================================================
// H. hash_pin across type imports (PASS)
// =============================================================================

#[test]
fn hash_pin_on_type_import_succeeds_with_correct_hash() {
    let dep = r#"
spec dep
type money: scale -> unit eur 1 -> unit usd 1.1
fact x: 1 eur
rule r: x
"#;
    let eff = date(2025, 1, 1);

    let mut e1 = Engine::new();
    e1.load(dep, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let dep_hash = plan_hash(&e1, "dep", &eff);

    let mut engine = Engine::new();
    engine
        .load(dep, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    engine
        .load(
            &format!(
                r#"
spec consumer
type money from dep~{}
fact p: [money]
rule r: p
"#,
                dep_hash
            ),
            lemma::SourceType::Labeled("consumer.lemma"),
        )
        .unwrap();

    let schema = engine.get_plan("consumer", Some(&eff)).unwrap().schema();
    let (p_type, _) = schema.facts.get("p").expect("fact p must exist in schema");
    assert!(
        p_type.is_scale(),
        "pinned imported money must be scale in schema"
    );
    assert_eq!(p_type.name(), "money");
    match &p_type.specifications {
        TypeSpecification::Scale { units, .. } => {
            let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
            assert!(names.contains(&"eur"), "schema must carry eur from dep");
            assert!(names.contains(&"usd"), "schema must carry usd from dep");
        }
        other => panic!("expected Scale, got {:?}", other),
    }
    let r_type = schema.rules.get("r").expect("rule r must exist");
    assert!(
        r_type.is_scale() && r_type.name() == "money",
        "rule r: p must expose same scale type as fact p"
    );
}

#[test]
fn hash_pin_on_type_import_fails_with_wrong_hash() {
    let dep = r#"
spec dep
type money: scale -> unit eur 1
fact x: 1 eur
rule r: x
"#;

    let mut engine = Engine::new();
    engine
        .load(dep, lemma::SourceType::Labeled("dep.lemma"))
        .unwrap();
    let err = engine.load(
        r#"
spec consumer
type money from dep~deadbeef
fact p: [money]
rule r: p
"#,
        lemma::SourceType::Labeled("consumer.lemma"),
    );
    assert!(
        err.is_err(),
        "planning must fail when hash_pin does not match dep's actual plan hash"
    );
}
