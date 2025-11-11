//! Tests for the inversion relation graph

use lemma::Engine;

#[test]
fn test_relation_graph_extraction() {
    let code = r#"
        doc pricing
        fact price = 10 EUR
        fact quantity = 5

        rule total = price * quantity
        rule discounted = total? * 0.9
    "#;

    let mut engine = Engine::new();
    engine
        .add_lemma_code(code, "test")
        .expect("Failed to add code");

    // The relation graph should now have extracted the relations
    // We can't directly access it yet (private field), but we can verify
    // the document was added successfully
    assert!(engine.get_document("pricing").is_some());

    // TODO: Once we add public methods to query the relation graph,
    // we can verify:
    // - Relations were extracted for "total" and "discounted"
    // - Dependencies were mapped correctly
    // - Branches were extracted from unless clauses
}

#[test]
fn test_relation_graph_with_unless_clauses() {
    let code = r#"
        doc shipping
        fact weight = [mass]

        rule shipping_cost = 5 EUR
             unless weight >= 10 kilograms then 10 EUR
             unless weight >= 50 kilograms then 25 EUR
             unless weight < 0 kilograms then veto "invalid"
    "#;

    let mut engine = Engine::new();
    engine
        .add_lemma_code(code, "test")
        .expect("Failed to add code");

    assert!(engine.get_document("shipping").is_some());

    // TODO: Verify that branches were extracted:
    // - Branch 1: weight >= 10 kg -> 10 EUR
    // - Branch 2: weight >= 50 kg -> 25 EUR
    // - Branch 3: weight < 0 kg -> veto "invalid"
}

#[test]
fn test_relation_graph_cross_document() {
    let doc1 = r#"
        doc base
        fact rate = 10%
    "#;

    let doc2 = r#"
        doc derived
        fact amount = [money]

        rule discount = amount * base.rate?
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(doc1, "doc1").unwrap();
    engine.add_lemma_code(doc2, "doc2").unwrap();

    // Both documents should be loaded
    assert!(engine.get_document("base").is_some());
    assert!(engine.get_document("derived").is_some());

    // TODO: Verify cross-document dependencies were captured
}
