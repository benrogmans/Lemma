//! Adversarial: transitive unqualified deps — interface change in `leaf` must surface when
//! `middle` and `app` only reference names in the chain.

use lemma::{Engine, SourceType};

#[test]
fn transitive_leaf_interface_change_rejected_for_unqualified_chain() {
    let mut engine = Engine::new();
    let err = engine
        .load(
            r#"
spec leaf 2025-01-01
data rate: number

spec leaf 2025-07-01
data rate: text

spec middle 2025-01-01
with L: leaf
rule m: L.rate

spec app 2025-01-01
with M: middle
rule out: M.m
"#,
            SourceType::Labeled("chain.lemma"),
        )
        .expect_err("leaf rate type changes across slices");

    let msg = err
        .errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        msg.contains("middle") || msg.contains("leaf") || msg.contains("interface"),
        "unexpected: {msg}"
    );
}
