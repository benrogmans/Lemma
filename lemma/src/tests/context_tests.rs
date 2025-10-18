use crate::evaluator::context::build_fact_map;
use crate::{FactType, FactValue, LemmaFact, LemmaType, LiteralValue, TypeAnnotation};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn test_build_fact_map_basic() {
    let facts = vec![LemmaFact::new(
        FactType::Local("price".to_string()),
        FactValue::Literal(LiteralValue::Number(Decimal::from(100))),
    )];

    let documents = HashMap::new();
    let fact_map = build_fact_map(&facts, &[], &documents);

    assert_eq!(fact_map.len(), 1);
    assert!(fact_map.contains_key("price"));
}

#[test]
fn test_build_fact_map_with_overrides() {
    let doc_facts = vec![LemmaFact::new(
        FactType::Local("price".to_string()),
        FactValue::Literal(LiteralValue::Number(Decimal::from(100))),
    )];

    let overrides = vec![LemmaFact::new(
        FactType::Local("price".to_string()),
        FactValue::Literal(LiteralValue::Number(Decimal::from(200))),
    )];

    let documents = HashMap::new();
    let fact_map = build_fact_map(&doc_facts, &overrides, &documents);

    assert_eq!(fact_map.len(), 1);
    // Override should replace original
    if let Some(LiteralValue::Number(val)) = fact_map.get("price") {
        assert_eq!(*val, Decimal::from(200));
    } else {
        panic!("Expected number value");
    }
}

#[test]
fn test_build_fact_map_skips_type_annotations() {
    let facts = vec![
        LemmaFact::new(
            FactType::Local("price".to_string()),
            FactValue::Literal(LiteralValue::Number(Decimal::from(100))),
        ),
        LemmaFact::new(
            FactType::Local("quantity".to_string()),
            FactValue::TypeAnnotation(TypeAnnotation::LemmaType(LemmaType::Number)),
        ),
    ];

    let documents = HashMap::new();
    let fact_map = build_fact_map(&facts, &[], &documents);

    // Only price should be in the map, not quantity
    assert_eq!(fact_map.len(), 1);
    assert!(fact_map.contains_key("price"));
    assert!(!fact_map.contains_key("quantity"));
}

