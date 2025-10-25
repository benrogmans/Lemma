use lemma::Engine;

#[test]
fn test_empty_string() {
    let mut engine = Engine::new();
    let result = engine.add_lemma_code("", "test");
    println!("Empty string result: {:?}", result);
    assert!(result.is_ok());
}

#[test]
fn test_whitespace_only() {
    let mut engine = Engine::new();
    let result = engine.add_lemma_code("   \n\t  ", "test");
    println!("Whitespace result: {:?}", result);
    assert!(result.is_ok());
}
