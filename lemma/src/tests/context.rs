use crate::evaluator::context::build_fact_map;
use crate::{
    FactReference, FactType, FactValue, LemmaDoc, LemmaFact, LemmaType, LiteralValue,
    TypeAnnotation,
};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn test_build_fact_map_basic() {
    let facts = vec![LemmaFact::new(
        FactType::Local("price".to_string()),
        FactValue::Literal(LiteralValue::Number(Decimal::from(100))),
    )];

    let doc = LemmaDoc::new("test".to_string());
    let documents = HashMap::new();
    let fact_map = build_fact_map(&doc, &facts, &[], &documents).unwrap();

    assert_eq!(fact_map.len(), 1);
    assert!(fact_map.contains_key(&FactReference {
        reference: vec!["price".to_string()]
    }));
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

    let mut doc = LemmaDoc::new("test".to_string());
    doc.facts = doc_facts.clone();
    let documents = HashMap::new();
    let fact_map = build_fact_map(&doc, &doc_facts, &overrides, &documents).unwrap();

    assert_eq!(fact_map.len(), 1);
    // Override should replace original
    if let Some(LiteralValue::Number(val)) = fact_map.get(&FactReference {
        reference: vec!["price".to_string()],
    }) {
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

    let doc = LemmaDoc::new("test".to_string());
    let documents = HashMap::new();
    let fact_map = build_fact_map(&doc, &facts, &[], &documents).unwrap();

    // Only price should be in the map, not quantity
    assert_eq!(fact_map.len(), 1);
    assert!(fact_map.contains_key(&FactReference {
        reference: vec!["price".to_string()]
    }));
    assert!(!fact_map.contains_key(&FactReference {
        reference: vec!["quantity".to_string()]
    }));
}

#[test]
fn test_build_fact_map_type_validation_success() {
    // Document declares price as money type
    let doc_facts = vec![LemmaFact::new(
        FactType::Local("price".to_string()),
        FactValue::TypeAnnotation(TypeAnnotation::LemmaType(LemmaType::Money)),
    )];

    // Override with correct money type
    let overrides = vec![LemmaFact::new(
        FactType::Local("price".to_string()),
        FactValue::Literal(LiteralValue::Unit(crate::NumericUnit::Money(
            Decimal::from(100),
            crate::MoneyUnit::Usd,
        ))),
    )];

    let mut doc = LemmaDoc::new("test".to_string());
    doc.facts = doc_facts.clone();
    let documents = HashMap::new();
    let fact_map = build_fact_map(&doc, &doc_facts, &overrides, &documents);

    assert!(fact_map.is_ok(), "Should accept correct money type");
    let fact_map = fact_map.unwrap();
    assert_eq!(fact_map.len(), 1);
    assert!(fact_map.contains_key(&FactReference {
        reference: vec!["price".to_string()]
    }));
}

#[test]
fn test_build_fact_map_type_validation_failure() {
    // Document declares price as money type
    let doc_facts = vec![LemmaFact::new(
        FactType::Local("price".to_string()),
        FactValue::TypeAnnotation(TypeAnnotation::LemmaType(LemmaType::Money)),
    )];

    // Override with wrong number type
    let overrides = vec![LemmaFact::new(
        FactType::Local("price".to_string()),
        FactValue::Literal(LiteralValue::Number(Decimal::from(100))),
    )];

    let mut doc = LemmaDoc::new("test".to_string());
    doc.facts = doc_facts.clone();
    let documents = HashMap::new();
    let result = build_fact_map(&doc, &doc_facts, &overrides, &documents);

    assert!(result.is_err(), "Should reject number type for money fact");
    let error = result.unwrap_err().to_string();
    assert!(error.contains("Type mismatch for fact 'price'"));
    assert!(error.contains("expected money"));
    assert!(error.contains("got number"));
}
