use lemma::FactPath;
use lemma::{Bound, Domain, Shape};
use serde_json::json;

fn lit_bool(b: bool) -> lemma::LiteralValue {
    lemma::LiteralValue::Boolean(b.into())
}

fn expr_lit(l: lemma::LiteralValue) -> lemma::Expression {
    lemma::Expression::new(
        lemma::ExpressionKind::Literal(l),
        None,
        lemma::ExpressionId::new(0),
    )
}

#[test]
fn display_equation() {
    use lemma::{BranchOutcome, ShapeBranch};

    let shape = Shape::new(
        vec![ShapeBranch {
            condition: expr_lit(lit_bool(true)),
            outcome: BranchOutcome::Value(expr_lit(lemma::LiteralValue::number(42))),
        }],
        vec![],
    );
    let s = shape.to_string();
    assert!(s.contains("42"));
}

#[test]
fn display_piecewise_and_domain() {
    // Domain display basic sanity
    let d1 = Domain::Range {
        min: Bound::Unbounded,
        max: Bound::Inclusive(lemma::LiteralValue::number(10)),
    };
    assert_eq!(d1.to_string(), "(-inf, 10]");

    let d2 = Domain::Enumeration(vec![lemma::LiteralValue::number(5)]);
    // Union prints parts with a pipe separator
    let du = Domain::Union(vec![d2, d1]);
    let su = du.to_string();
    assert!(su.contains("{") && su.contains("|"));
}

#[test]
fn serialize_equation() {
    use lemma::{BranchOutcome, ShapeBranch};

    let shape = Shape::new(
        vec![ShapeBranch {
            condition: expr_lit(lit_bool(true)),
            outcome: BranchOutcome::Value(expr_lit(lemma::LiteralValue::number(7))),
        }],
        vec![FactPath::from_path(vec![
            "doc".to_string(),
            "y".to_string(),
        ])],
    );
    let v = serde_json::to_value(&shape).expect("serialize shape");

    // Shape serializes as a struct with branches and free_variables fields
    assert!(v["branches"].is_array());
    // FactPath serializes as an object with segments and fact fields
    assert!(v["free_variables"].is_array());
    assert_eq!(v["free_variables"][0]["fact"], json!("y"));
}

#[test]
fn serialize_domain_range() {
    let d = Domain::Range {
        min: Bound::Inclusive(lemma::LiteralValue::number(0)),
        max: Bound::Exclusive(lemma::LiteralValue::number(10)),
    };
    let v = serde_json::to_value(&d).expect("serialize domain");

    assert_eq!(v["type"], json!("range"));
    assert_eq!(v["min"]["type"], json!("inclusive"));
    assert!(v["min"]["value"].is_string() || v["min"]["value"].is_object());
    assert_eq!(v["max"]["type"], json!("exclusive"));
}
