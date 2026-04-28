/// Integration tests for the data reference (value-copy) feature.
///
/// QA NOTE: tests in this file encode the INTENDED behavior of data
/// references. Do NOT weaken, mask, `#[ignore]`, or `#[should_panic]`
/// these tests. If a test goes red, fix the regression — do not soften
/// the assertion.
use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use std::collections::HashMap;

fn rule_value(result: &lemma::evaluation::Response, rule_name: &str) -> String {
    let rr = result
        .results
        .get(rule_name)
        .unwrap_or_else(|| panic!("rule '{}' not found", rule_name));
    match &rr.result {
        OperationResult::Value(v) => v.to_string(),
        OperationResult::Veto(v) => format!("VETO({})", v),
    }
}

fn load_err_joined(engine_res: Result<(), lemma::Errors>) -> String {
    let err = engine_res.expect_err("expected load to fail");
    err.iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn local_reference_to_nested_spec_data_copies_value() {
    let code = r#"
spec law
data other: number -> default 42

spec license
with l: law
data license2: l.other
rule check: license2 > 10
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let result = engine
        .run("license", Some(&now), HashMap::new(), false)
        .expect("should run");

    assert_eq!(rule_value(&result, "check"), "true");
}

#[test]
fn binding_reference_copies_cross_spec_target_value() {
    let code = r#"
spec law
data other: number -> default 99

spec inner
with l: law
data slot: number

spec top
with lic: inner
with lw: law
data lic.slot: lw.other
rule answer: lic.slot
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let result = engine
        .run("top", Some(&now), HashMap::new(), false)
        .expect("should run");

    assert_eq!(rule_value(&result, "answer"), "99");
}

#[test]
fn user_value_overrides_reference() {
    let code = r#"
spec law
data other: number -> default 42

spec license
with l: law
data license2: l.other
rule check: license2
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .unwrap();

    let mut data = HashMap::new();
    data.insert("license2".to_string(), "777".to_string());

    let now = DateTimeValue::now();
    let result = engine
        .run("license", Some(&now), data, false)
        .expect("should run");

    assert_eq!(rule_value(&result, "check"), "777");
}

#[test]
fn reference_chain_resolves_in_dependency_order() {
    let code = r#"
spec base
data other: number -> default 5

spec mid
with b: base
data m2: b.other

spec top
with mm: mid
data t2: mm.m2
rule result: t2
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .unwrap();

    let now = DateTimeValue::now();
    let result = engine
        .run("top", Some(&now), HashMap::new(), false)
        .expect("should run");

    assert_eq!(rule_value(&result, "result"), "5");
}

/// Closed data-reference cycle: two bindings in the same spec point at each
/// other via the shared binding path. Planning MUST reject this with a
/// circular reference error. Previous iteration of this test did not close
/// the cycle and just asserted `load` succeeded, which is the opposite of
/// the invariant.
#[test]
fn closed_reference_cycle_is_rejected() {
    let code = r#"
spec inner
data a: number
data b: number

spec outer
with i: inner
data i.a: i.b
data i.b: i.a
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, lemma::SourceType::Labeled("reference.lemma")));

    assert!(
        joined.contains("Circular data reference"),
        "closed reference cycle must be reported as a circular data reference, got: {joined}"
    );
}

/// Self-referential reference: `data x: outer.x` where outer.x resolves back
/// to itself. A 1-node cycle must still be rejected.
#[test]
fn self_referential_reference_is_rejected() {
    let code = r#"
spec inner
data x: number

spec outer
with i: inner
data i.x: i.x
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, lemma::SourceType::Labeled("reference.lemma")));

    assert!(
        joined.contains("Circular data reference"),
        "self-referential reference must be reported as a circular data reference, got: {joined}"
    );
}

#[test]
fn unknown_reference_target_is_rejected_with_exact_error() {
    let code = r#"
spec test
data a: number -> default 1
data b: a.nonexistent
rule r: b
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, lemma::SourceType::Labeled("reference.lemma")));

    assert!(
        joined.contains("'a' is not a spec reference")
            || joined.contains("'nonexistent' not found")
            || joined.contains("target 'a.nonexistent' does not exist"),
        "unknown reference target must identify the missing path, got: {joined}"
    );
}

/// Reference target is a `SpecRef` data (i.e. a `with` binding). Planning
/// must reject this because a spec reference has no value to copy.
#[test]
fn reference_target_is_spec_reference_rejected() {
    let code = r#"
spec inner
data x: number -> default 1

spec outer
with i: inner
data copy_of_i: i
rule r: copy_of_i
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, lemma::SourceType::Labeled("reference.lemma")));

    assert!(
        joined.contains("is a spec reference and cannot carry a value"),
        "referencing a spec reference must be rejected with the exact error, got: {joined}"
    );
}

/// Target name is BOTH a data and a rule in the referenced spec. Reference
/// resolution must flag this as ambiguous.
#[test]
fn reference_target_is_ambiguous_data_and_rule() {
    let code = r#"
spec inner
data conflict: number -> default 1
rule conflict: 2

spec outer
with i: inner
data c: i.conflict
rule r: c
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, lemma::SourceType::Labeled("reference.lemma")));

    assert!(
        joined.contains("is ambiguous"),
        "duplicate data+rule name must be reported as ambiguous reference target, got: {joined}"
    );
}

/// Binding overrides a child-declared type with a reference whose target is
/// a different primitive kind. Planning must catch the primitive-kind
/// mismatch via the binding path: child-declared LHS (`number`) vs target
/// (`text`).
#[test]
fn binding_reference_target_type_incompatible_with_child_declared_type_is_rejected() {
    let code = r#"
spec inner
data n: number

spec source_spec
data s: text -> default "hello"

spec outer
with i: inner
with src: source_spec
data i.n: src.s
rule r: i.n
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, lemma::SourceType::Labeled("reference.lemma")));

    assert!(
        joined.contains("type mismatch"),
        "binding reference with target of a different base kind must be rejected with a \
         type mismatch error, got: {joined}"
    );
}

/// RULE-TARGET REFERENCE, value case. `data x: i.my_r` where `my_r` is a
/// rule in inner spec returning `42`. The reference MUST copy the rule's
/// evaluated result into the reference path so that downstream rules see
/// the value.
#[test]
fn rule_target_reference_copies_rule_value() {
    let code = r#"
spec inner
rule my_r: 42

spec top
with i: inner
data x: i.my_r
rule out: x
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .expect("rule-target reference must be accepted at plan time");

    let now = DateTimeValue::now();
    let result = engine
        .run("top", Some(&now), HashMap::new(), false)
        .expect("must evaluate without error");

    assert_eq!(rule_value(&result, "out"), "42");
}

/// RULE-TARGET REFERENCE, veto case. If the target rule returns a `Veto`,
/// the reference path becomes missing/vetoed and any consumer rule
/// propagates the veto with the SAME reason.
#[test]
fn rule_target_reference_propagates_veto() {
    let code = r#"
spec inner
data denom: number -> default 0
rule divided: 10 / denom

spec top
with i: inner
data x: i.divided
rule out: x
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .expect("rule-target reference must be accepted at plan time");

    let now = DateTimeValue::now();
    let result = engine
        .run("top", Some(&now), HashMap::new(), false)
        .expect("evaluator must run; veto is a domain result, not an error");

    let rr = result
        .results
        .get("out")
        .expect("rule 'out' must be present");
    match &rr.result {
        OperationResult::Veto(v) => {
            let s = v.to_string();
            assert!(
                s.contains("Division by zero"),
                "reference must propagate the target rule's division-by-zero veto reason, got: {s}"
            );
        }
        OperationResult::Value(v) => {
            panic!("expected propagated veto, got value: {v}");
        }
    }
}

/// RULE-TARGET REFERENCE, cycle case. The outer spec references inner-spec
/// data `i.slot` to outer's own rule `r`, and `r` reads `i.slot`. The
/// reference path injects an `r -> r` edge in the rule dependency graph,
/// which the topological sort MUST detect and reject as a circular
/// dependency.
#[test]
fn rule_target_reference_cycle_through_self_is_rejected() {
    let code = r#"
spec inner
data slot: number

spec outer
with i: inner
data i.slot: r
rule r: i.slot
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, lemma::SourceType::Labeled("reference.lemma")));

    assert!(
        joined.to_lowercase().contains("circular") || joined.to_lowercase().contains("cycle"),
        "rule-target reference forming a cycle with its target rule must be rejected at plan \
         time with a circular-dependency error, got: {joined}"
    );
}

/// RULE-TARGET REFERENCE, LHS-declared type mismatch. The bound data
/// declares `number` in the inner spec; the binding references it to a
/// rule that returns text. Planning MUST reject the kind mismatch via the
/// reference's LHS-vs-target check.
#[test]
fn rule_target_reference_lhs_type_mismatch_is_rejected() {
    let code = r#"
spec inner
data v: number

spec source_spec
rule greeting: "hello"

spec outer
with i: inner
with src: source_spec
data i.v: src.greeting
rule r: i.v
"#;

    let mut engine = Engine::new();
    let joined = load_err_joined(engine.load(code, lemma::SourceType::Labeled("reference.lemma")));

    assert!(
        joined.contains("type mismatch"),
        "rule-target reference whose target rule's type kind differs from the \
         child-declared LHS type must be rejected with a type mismatch error, \
         got: {joined}"
    );
}

/// RULE-TARGET REFERENCE in a chain. `top.y` is a data-target reference to
/// `mid.x`, which is itself a rule-target reference to `inner.my_r`.
/// Reading `y` must transitively resolve through `mid.x` to the rule's
/// value, yielding 42 at `y`.
///
/// Each hop uses a dotted RHS so the parser treats both as `Reference`
/// (typedef references are reserved for non-dotted local RHS like
/// `data y: x`).
#[test]
fn rule_target_reference_in_chain_resolves_value() {
    let code = r#"
spec inner
rule my_r: 42

spec mid
with i: inner
data x: i.my_r

spec top
with m: mid
data y: m.x
rule out: y
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .expect("rule-target reference chain must be accepted at plan time");

    let now = DateTimeValue::now();
    let result = engine
        .run("top", Some(&now), HashMap::new(), false)
        .expect("must evaluate without error");

    assert_eq!(rule_value(&result, "out"), "42");
}

/// RULE-TARGET REFERENCE, user override. A caller-supplied value for the
/// reference data path must win over the target rule's evaluated result.
/// The user value is injected at plan-finalization time, replacing the
/// reference entry with a `Value` definition; the lazy resolver is never
/// consulted.
#[test]
fn rule_target_reference_user_override_wins_over_rule_value() {
    let code = r#"
spec inner
rule my_r: 42

spec top
with i: inner
data x: i.my_r
rule out: x
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .expect("rule-target reference must be accepted at plan time");

    let now = DateTimeValue::now();
    let mut overrides = HashMap::new();
    overrides.insert("x".to_string(), "99".to_string());
    let result = engine
        .run("top", Some(&now), overrides, false)
        .expect("must evaluate without error");

    assert_eq!(
        rule_value(&result, "out"),
        "99",
        "user-provided override at the reference path must win over the target rule's value"
    );
}

/// RUNTIME CONSTRAINT CHECK on referenced value via binding. The child
/// declares `maximum 5`; the reference copies a value of 10 from a source
/// spec. The engine MUST reject the copied value against the child-declared
/// tighter constraint. Silently copying an out-of-range value is a
/// landmine.
#[test]
fn reference_value_violating_child_declared_max_is_rejected() {
    let code = r#"
spec inner
data limited: number -> maximum 5

spec source_spec
data v: number -> default 10

spec outer
with i: inner
with src: source_spec
data i.limited: src.v
rule r: i.limited
"#;

    let mut engine = Engine::new();
    let load_result = engine.load(code, lemma::SourceType::Labeled("reference.lemma"));

    match load_result {
        Ok(()) => {
            let now = DateTimeValue::now();
            let run_result = engine.run("outer", Some(&now), HashMap::new(), false);

            match run_result {
                Ok(resp) => {
                    let rr = resp.results.get("r").expect("rule 'r'");
                    match &rr.result {
                        OperationResult::Veto(v) => {
                            let s = v.to_string();
                            assert!(
                                s.contains("maximum") || s.contains("exceeds"),
                                "expected max-constraint veto, got: {s}"
                            );
                        }
                        OperationResult::Value(v) => {
                            panic!(
                                "expected constraint-violation veto or error; engine silently \
                                 accepted out-of-range referenced value {v} (planning landmine)"
                            );
                        }
                    }
                }
                Err(err) => {
                    let s = err.to_string();
                    assert!(
                        s.contains("maximum") || s.contains("exceeds") || s.contains("constraint"),
                        "expected constraint error at run time, got: {s}"
                    );
                }
            }
        }
        Err(errors) => {
            let joined = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                joined.contains("maximum")
                    || joined.contains("exceeds")
                    || joined.contains("constraint"),
                "expected constraint error at load time, got: {joined}"
            );
        }
    }
}

/// Local `default` constraint on a reference supplies a value when the
/// target value is missing.
#[test]
fn reference_local_default_supplies_value_when_target_missing() {
    let code = r#"
spec inner
data maybe: number

spec outer
with i: inner
data here: i.maybe -> default 77
rule r: here
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .expect("must load");

    let now = DateTimeValue::now();
    let result = engine
        .run("outer", Some(&now), HashMap::new(), false)
        .expect("must evaluate");

    assert_eq!(
        rule_value(&result, "r"),
        "77",
        "reference-local default must fill in when target is missing"
    );
}

/// PARSER PIN: `data x: notdotted` in local (non-binding) context MUST remain
/// a `TypeDeclaration`, NOT be parsed as a `Reference`. The AST doc claims
/// this; the parser agrees. This test pins that behavior so a future refactor
/// does not silently change it.
#[test]
fn local_non_dotted_rhs_stays_type_declaration() {
    let code = r#"
spec s
data age: number -> default 30
data person: age
rule r: person
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .expect("loads: `data person: age` is a typedef reference, not a value-copy reference");

    let now = DateTimeValue::now();
    let result = engine
        .run("s", Some(&now), HashMap::new(), false)
        .expect("evaluates; `person` is typed 'age' and inherits its default");

    assert_eq!(
        rule_value(&result, "r"),
        "30",
        "typedef inheritance must propagate default; if this becomes a value-copy reference \
         instead, the parser silently changed shape"
    );
}

/// PARSER+PLANNER PIN: `data x.y: notdotted` in binding context IS parsed as
/// a Reference (value-copy). When the referenced name `src` exists in the
/// SAME (outer) spec where the binding lives, the reference must resolve and
/// copy the source's value to the bound child data.
#[test]
fn binding_non_dotted_rhs_resolves_in_enclosing_spec() {
    let code = r#"
spec inner
data slot: number

spec outer
with i: inner
data src: number -> default 123
data i.slot: src
rule r: i.slot
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .expect("non-dotted RHS in binding context must resolve in the enclosing spec");

    let now = DateTimeValue::now();
    let result = engine
        .run("outer", Some(&now), HashMap::new(), false)
        .expect("evaluates");
    assert_eq!(
        rule_value(&result, "r"),
        "123",
        "non-dotted RHS in binding context must resolve as reference and copy 'src' value"
    );
}

/// SCHEMA SURFACE: a reference's `-> default N` tail must appear on the
/// schema's `default` field, just like `data x: number -> default N` does.
/// Both forms are user-equivalent ways to declare a default — the schema
/// surface (HTTP `/schema/{name}`, OpenAPI, WASM/Hex `list`) must not
/// silently drop one of them.
#[test]
fn reference_local_default_appears_in_schema() {
    let code = r#"
spec inner
data maybe: number

spec outer
with i: inner
data here: i.maybe -> default 77
rule r: here
"#;

    let mut engine = Engine::new();
    engine
        .load(code, lemma::SourceType::Labeled("reference.lemma"))
        .expect("must load");

    let now = DateTimeValue::now();
    let schema = engine
        .schema("outer", Some(&now))
        .expect("schema must build");

    let here_entry = schema
        .data
        .get("here")
        .expect("schema must include 'here' data entry");

    let default = here_entry
        .default
        .as_ref()
        .expect("schema must surface the reference's `-> default 77` value");

    let rendered = default.to_string();
    assert!(
        rendered.contains("77"),
        "schema default must render as 77; got: {rendered}"
    );
}

/// HEURISTIC TIGHTENING: a discriminant-only kind compatibility check
/// treats two scale types in different families as compatible (both are
/// `TypeSpecification::Scale`). Per `error-model.mdc`, a temperature-scale
/// reference whose target is a money-scale value is invalid Lemma and
/// must be rejected at planning, not silently propagated.
///
/// The LHS-side scale family is established by the binding's child-spec
/// type declaration (`inner.payment` extends a money family); the RHS
/// reference target is in a temperature family. Same `Scale` discriminant,
/// different families. Planning must reject.
#[test]
fn binding_reference_scale_family_mismatch_is_rejected() {
    let code = r#"
spec inner
data money: scale -> unit eur 1.00
data payment: money

spec source_spec
data temp_unit: scale -> unit celsius 1.0
data temperature: temp_unit

spec outer
with i: inner
with src: source_spec
data i.payment: src.temperature
rule r: i.payment
"#;

    let mut engine = Engine::new();
    let res = engine.load(code, lemma::SourceType::Labeled("reference.lemma"));
    let joined = load_err_joined(res);
    assert!(
        joined.contains("scale family")
            || joined.contains("scale_family")
            || joined.contains("family")
            || joined.contains("type mismatch"),
        "expected scale-family-mismatch error, got: {joined}"
    );
}
