use lemma::{Engine, FactReference, LiteralValue, OperationResult, Target, TargetOp};
use rust_decimal::Decimal;
use std::collections::HashMap;

fn money_eur(amount: i64) -> LiteralValue {
    LiteralValue::Unit(lemma::NumericUnit::Money(
        Decimal::from(amount),
        lemma::MoneyUnit::Eur,
    ))
}

#[test]
fn premium_greater_than_or_equal() {
    let code = r#"
        doc insurance
        fact age = [number]

        rule premium = 100 EUR
          unless age < 25 then veto "too young"
          unless age > 65 then veto "too old"
          unless (age >= 30 and age <= 40) then 80 EUR
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Find ages where premium >= 80 EUR
    let solutions = engine
        .invert(
            "insurance",
            "premium",
            Target::with_op(TargetOp::Gte, OperationResult::Value(money_eur(80))),
            HashMap::new(),
        )
        .expect("should invert");

    println!("\n=== Ages where premium >= 80 EUR ===");
    for (i, solution) in solutions.iter().enumerate() {
        println!("Solution {}:", i + 1);
        for (fact, domain) in solution.iter() {
            println!("  {}: {:?}", fact.reference.join("."), domain);
        }
    }

    // Should include:
    // - Ages 25-29 (premium = 100 EUR, which is >= 80)
    // - Ages 30-40 (premium = 80 EUR, which is >= 80)
    // - Ages 41-65 (premium = 100 EUR, which is >= 80)

    assert!(
        !solutions.is_empty(),
        "Expected solutions where premium >= 80 EUR"
    );
}

#[test]
fn discount_greater_than_threshold() {
    let code = r#"
        doc pricing
        fact quantity = [number]

        rule discount = 0%
          unless quantity >= 10 then 5%
          unless quantity >= 50 then 10%
          unless quantity >= 100 then 15%
          unless quantity < 0 then veto "invalid"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Find quantities where discount > 5%
    let solutions = engine
        .invert(
            "pricing",
            "discount",
            Target::with_op(
                TargetOp::Gt,
                OperationResult::Value(LiteralValue::Percentage(
                    Decimal::from_str_exact("5").unwrap(),
                )),
            ),
            HashMap::new(),
        )
        .expect("should invert");

    println!("\n=== Quantities where discount > 5% ===");
    let quantity_path = FactReference {
        reference: vec!["pricing".to_string(), "quantity".to_string()],
    };
    for (i, solution) in solutions.iter().enumerate() {
        if let Some(domain) = solution.get(&quantity_path) {
            println!("Solution {}: {:?}", i + 1, domain);
        }
    }

    // Should return solutions for:
    // - quantity >= 50 (discount = 10%)
    // - quantity >= 100 (discount = 15%)

    assert!(
        !solutions.is_empty(),
        "Expected solutions with discount > 5%"
    );
}

#[test]
fn price_less_than_budget() {
    let code = r#"
        doc shopping
        fact base_price = [money]
        fact quantity = [number]

        rule total = base_price * quantity
          unless quantity < 1 then veto "invalid quantity"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Find combinations where total < 100 EUR
    let solutions = engine
        .invert(
            "shopping",
            "total",
            Target::with_op(TargetOp::Lt, OperationResult::Value(money_eur(100))),
            HashMap::new(),
        )
        .expect("should invert");

    println!("\n=== Price/quantity combinations where total < 100 EUR ===");
    for (i, solution) in solutions.iter().enumerate() {
        println!("Solution {}:", i + 1);
        for (fact, domain) in solution.iter() {
            println!("  {}: {:?}", fact.reference.join("."), domain);
        }
    }

    // Relationship: base_price * quantity < 100 EUR (with quantity >= 1)
    assert!(!solutions.is_empty(), "Expected solutions");
}

#[test]
fn temperature_in_comfortable_range() {
    let code = r#"
        doc climate
        fact temp = [number]

        rule comfort_level = 0
          unless temp >= 18 then 1
          unless temp >= 22 then 2
          unless temp >= 26 then 1
          unless temp >= 30 then 0
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Find temps where comfort >= 2 (most comfortable)
    let solutions = engine
        .invert(
            "climate",
            "comfort_level",
            Target::with_op(
                TargetOp::Gte,
                OperationResult::Value(LiteralValue::Number(Decimal::from(2))),
            ),
            HashMap::new(),
        )
        .expect("should invert");

    println!("\n=== Temperatures where comfort_level >= 2 ===");
    let temp_path = FactReference {
        reference: vec!["climate".to_string(), "temp".to_string()],
    };
    for (i, solution) in solutions.iter().enumerate() {
        if let Some(domain) = solution.get(&temp_path) {
            println!("Solution {}: {:?}", i + 1, domain);
        }
    }

    // Should return temp range: 22 <= temp < 26
    assert!(
        !solutions.is_empty(),
        "Expected comfortable temperature range"
    );
}

#[test]
fn get_valid_domain_with_threshold() {
    // Use case: "What order sizes are eligible for free shipping?"
    let code = r#"
        doc shipping
        fact order_total = [money]

        rule shipping_cost = 5 EUR
          unless order_total >= 50 EUR then 0 EUR
          unless order_total < 0 EUR then veto "invalid"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // First, find when shipping_cost <= 0 EUR (free shipping)
    let solutions = engine
        .invert(
            "shipping",
            "shipping_cost",
            Target::with_op(TargetOp::Lte, OperationResult::Value(money_eur(0))),
            HashMap::new(),
        )
        .expect("should invert");

    println!("\n=== Order totals eligible for free shipping (cost <= 0) ===");
    let total_path = FactReference {
        reference: vec!["shipping".to_string(), "order_total".to_string()],
    };
    for (i, solution) in solutions.iter().enumerate() {
        if let Some(domain) = solution.get(&total_path) {
            println!("Solution {}: {:?}", i + 1, domain);
        }
    }

    // Should show: order_total >= 50 EUR
    assert!(
        !solutions.is_empty(),
        "Expected threshold for free shipping"
    );
}

#[test]
fn all_comparison_operators() {
    let code = r#"
        doc test
        fact x = [number]

        rule result = x * 2
          unless x < 0 then veto "negative"
    "#;

    let mut engine = Engine::new();
    engine.add_lemma_code(code, "test").unwrap();

    // Test all operators
    let test_cases = vec![
        ("Eq", TargetOp::Eq, "result = 10"),
        ("Gt", TargetOp::Gt, "result > 10"),
        ("Lt", TargetOp::Lt, "result < 10"),
        ("Gte", TargetOp::Gte, "result >= 10"),
        ("Lte", TargetOp::Lte, "result <= 10"),
    ];

    println!("\n=== Testing all comparison operators ===");
    for (name, op, description) in test_cases {
        let solutions = engine
            .invert(
                "test",
                "result",
                Target::with_op(
                    op,
                    OperationResult::Value(LiteralValue::Number(Decimal::from(10))),
                ),
                HashMap::new(),
            )
            .expect("should invert");

        println!(
            "{} ({}): {} solution(s)",
            name,
            description,
            solutions.len()
        );
    }
}
