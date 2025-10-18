use crate::parser::parse;
use crate::{FactType, FactValue, LiteralValue};

#[test]
fn test_parse_simple_document_reference() {
    let input = r#"doc person
fact name = "John"
fact contract = doc employment_contract"#;
    let result = parse(input, None).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].facts.len(), 2);

    if let FactValue::DocumentReference(doc_name) = &result[0].facts[1].value {
        assert_eq!(doc_name, "employment_contract");
    } else {
        panic!("Expected DocumentReference");
    }
}

#[test]
fn test_parse_fact_overrides() {
    let input = r#"doc person
fact contract = doc employment_contract
fact contract.start_date = 2024-02-01
fact contract.end_date = [date]
fact contract.employment_type = "contractor"
fact contract.base = doc base_contract
fact contract.base.rate = 100"#;
    let result = parse(input, None).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].facts.len(), 6);

    assert_eq!(
        result[0].facts[0].fact_type,
        FactType::Local("contract".to_string())
    );
    if let FactValue::DocumentReference(doc_name) = &result[0].facts[0].value {
        assert_eq!(doc_name, "employment_contract");
    } else {
        panic!("Expected DocumentReference");
    }

    assert_eq!(
        result[0].facts[1].fact_type,
        FactType::Foreign(crate::ForeignFact {
            reference: vec!["contract".to_string(), "start_date".to_string()]
        })
    );
    if let FactValue::Literal(LiteralValue::Date(_)) = &result[0].facts[1].value {
    } else {
        panic!("Expected Date literal");
    }

    assert_eq!(
        result[0].facts[2].fact_type,
        FactType::Foreign(crate::ForeignFact {
            reference: vec!["contract".to_string(), "end_date".to_string()]
        })
    );
    if let FactValue::TypeAnnotation(_) = &result[0].facts[2].value {
    } else {
        panic!("Expected TypeAnnotation");
    }

    assert_eq!(
        result[0].facts[3].fact_type,
        FactType::Foreign(crate::ForeignFact {
            reference: vec!["contract".to_string(), "employment_type".to_string()]
        })
    );
    if let FactValue::Literal(LiteralValue::Text(s)) = &result[0].facts[3].value {
        assert_eq!(s, "contractor");
    } else {
        panic!("Expected Text literal");
    }

    assert_eq!(
        result[0].facts[4].fact_type,
        FactType::Foreign(crate::ForeignFact {
            reference: vec!["contract".to_string(), "base".to_string()]
        })
    );
    if let FactValue::DocumentReference(doc_name) = &result[0].facts[4].value {
        assert_eq!(doc_name, "base_contract");
    } else {
        panic!("Expected DocumentReference");
    }

    assert_eq!(
        result[0].facts[5].fact_type,
        FactType::Foreign(crate::ForeignFact {
            reference: vec![
                "contract".to_string(),
                "base".to_string(),
                "rate".to_string()
            ]
        })
    );
    if let FactValue::Literal(LiteralValue::Number(n)) = &result[0].facts[5].value {
        assert_eq!(*n, rust_decimal::Decimal::new(100, 0));
    } else {
        panic!("Expected Number literal");
    }
}
