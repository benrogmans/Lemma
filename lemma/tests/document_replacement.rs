use lemma::{Engine, LemmaError};

#[test]
fn test_cannot_add_existing_document() {
    let mut engine = Engine::new();

    let doc1 = r#"
doc test
fact x = 10
"#;

    let doc2 = r#"
doc test
fact y = 20
"#;

    engine.add_lemma_code(doc1, "file1.lemma").unwrap();

    let result = engine.add_lemma_code(doc2, "file2.lemma");
    assert!(result.is_err());
    match result {
        Err(LemmaError::Engine(msg)) => {
            assert!(msg.contains("already exists"));
            assert!(msg.contains("test"));
        }
        _ => panic!("Expected Engine error"),
    }
}

#[test]
fn test_can_add_after_removal() {
    let mut engine = Engine::new();

    let doc1 = r#"
doc test
fact x = 10
"#;

    let doc2 = r#"
doc test
fact y = 20
"#;

    engine.add_lemma_code(doc1, "file1.lemma").unwrap();
    engine.remove_document("test");
    engine.add_lemma_code(doc2, "file2.lemma").unwrap();

    let doc = engine.get_document("test").unwrap();
    let facts = doc.facts.iter().find(|f| f.reference.fact == "y").unwrap();
    assert_eq!(facts.value.to_string(), "20");
}

#[test]
fn test_remove_document_cleans_up_sources() {
    let mut engine = Engine::new();

    let doc = r#"
doc test
fact x = 10
"#;

    engine.add_lemma_code(doc, "file1.lemma").unwrap();
    assert!(engine.list_documents().contains(&"test".to_string()));

    engine.remove_document("test");
    assert!(!engine.list_documents().contains(&"test".to_string()));

    engine.add_lemma_code(doc, "file2.lemma").unwrap();
    assert!(engine.list_documents().contains(&"test".to_string()));
}

