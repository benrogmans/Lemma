use lemma::Engine;

#[test]
fn test_money_minus_percentage() {
    let mut engine = Engine::new();

    let code = r#"
doc test_money_minus_percentage

fact base_price = 200
fact discount_rate = 25%

rule price_after_discount = base_price - discount_rate
rule expected = 150

rule test_passes = price_after_discount? == expected?
"#;

    engine.add_lemma_code(code, "test").unwrap();
    let response = engine
        .evaluate("test_money_minus_percentage", vec![])
        .unwrap();

    let price_after_discount = response
        .results
        .iter()
        .find(|r| r.rule_name == "price_after_discount")
        .unwrap();
    assert_eq!(
        price_after_discount.result.as_ref().unwrap().to_string(),
        "150"
    );

    let test_passes = response
        .results
        .iter()
        .find(|r| r.rule_name == "test_passes")
        .unwrap();
    assert_eq!(test_passes.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_money_plus_percentage() {
    let mut engine = Engine::new();

    let code = r#"
doc test_money_plus_percentage

fact base = 100
fact markup = 10%

rule price_with_markup = base + markup
rule expected = 110

rule test_passes = price_with_markup? == expected?
"#;

    engine.add_lemma_code(code, "test").unwrap();
    let response = engine
        .evaluate("test_money_plus_percentage", vec![])
        .unwrap();

    let price_with_markup = response
        .results
        .iter()
        .find(|r| r.rule_name == "price_with_markup")
        .unwrap();
    assert_eq!(
        price_with_markup.result.as_ref().unwrap().to_string(),
        "110"
    );

    let test_passes = response
        .results
        .iter()
        .find(|r| r.rule_name == "test_passes")
        .unwrap();
    assert_eq!(test_passes.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_number_times_percentage() {
    let mut engine = Engine::new();

    let code = r#"
doc test_number_times_percentage

fact amount = 1000
fact rate = 15%

rule result = amount * rate
rule expected = 150

rule test_passes = result? == expected?
"#;

    engine.add_lemma_code(code, "test").unwrap();
    let response = engine
        .evaluate("test_number_times_percentage", vec![])
        .unwrap();

    let result = response
        .results
        .iter()
        .find(|r| r.rule_name == "result")
        .unwrap();
    assert_eq!(result.result.as_ref().unwrap().to_string(), "150");

    let test_passes = response
        .results
        .iter()
        .find(|r| r.rule_name == "test_passes")
        .unwrap();
    assert_eq!(test_passes.result.as_ref().unwrap().to_string(), "true");
}

#[test]
fn test_money_minus_percentage_with_rule_reference() {
    let mut engine = Engine::new();

    let code = r#"
doc test_with_rule_reference

fact base_price = 200
fact discount_rate = 25%

rule discount_amount = base_price * discount_rate
rule final_price = base_price - discount_amount?
rule expected = 150

rule test_passes = final_price? == expected?
"#;

    engine.add_lemma_code(code, "test").unwrap();
    let response = engine.evaluate("test_with_rule_reference", vec![]).unwrap();

    let discount_amount = response
        .results
        .iter()
        .find(|r| r.rule_name == "discount_amount")
        .unwrap();
    assert_eq!(discount_amount.result.as_ref().unwrap().to_string(), "50");

    let final_price = response
        .results
        .iter()
        .find(|r| r.rule_name == "final_price")
        .unwrap();
    assert_eq!(final_price.result.as_ref().unwrap().to_string(), "150");
}

#[test]
fn test_chained_percentage_operations() {
    let mut engine = Engine::new();

    let code = r#"
doc test_chained_percentages

fact original_price = 100
fact first_discount = 20%
fact second_discount = 10%

rule after_first = original_price - first_discount
rule after_second = after_first? - second_discount

rule expected = 72

rule test_passes = after_second? == expected?
"#;

    engine.add_lemma_code(code, "test").unwrap();
    let response = engine.evaluate("test_chained_percentages", vec![]).unwrap();

    let after_first = response
        .results
        .iter()
        .find(|r| r.rule_name == "after_first")
        .unwrap();
    assert_eq!(after_first.result.as_ref().unwrap().to_string(), "80");

    let after_second = response
        .results
        .iter()
        .find(|r| r.rule_name == "after_second")
        .unwrap();
    assert_eq!(after_second.result.as_ref().unwrap().to_string(), "72");
}
