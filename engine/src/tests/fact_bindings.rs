use crate::parsing::parse;
use crate::FactValue;

#[test]
fn test_parse_simple_spec_reference() {
    let input = r#"spec person
fact name: "John"
fact contract: spec employment_contract"#;
    let result = parse(input, "test.lemma", &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].facts.len(), 2);

    if let FactValue::SpecReference(spec_ref) = &result[0].facts[1].value {
        assert_eq!(spec_ref.name, "employment_contract");
        assert!(!spec_ref.is_registry);
    } else {
        panic!("Expected SpecReference");
    }
}

#[test]
fn test_parse_fact_bindings() {
    let input = r#"spec person
fact contract: spec employment_contract
fact contract.start_date: 2024-02-01
fact contract.end_date: [date]
fact contract.employment_type: "contractor"
fact contract.base: spec base_contract
fact contract.base.rate: 100"#;
    let result = parse(input, "test.lemma", &crate::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].facts.len(), 6);

    assert_eq!(
        result[0].facts[0].reference,
        crate::parsing::ast::Reference::from_path(vec!["contract".to_string()])
    );
    if let FactValue::SpecReference(spec_ref) = &result[0].facts[0].value {
        assert_eq!(spec_ref.name, "employment_contract");
        assert!(!spec_ref.is_registry);
    } else {
        panic!("Expected SpecReference");
    }

    assert_eq!(
        result[0].facts[1].reference,
        crate::parsing::ast::Reference::from_path(vec![
            "contract".to_string(),
            "start_date".to_string()
        ])
    );
    match &result[0].facts[1].value {
        FactValue::Literal(lit) => {
            assert!(
                matches!(lit, crate::Value::Date(_)),
                "Expected Date literal"
            );
        }
        _ => panic!("Expected Date literal"),
    }

    assert_eq!(
        result[0].facts[2].reference,
        crate::parsing::ast::Reference::from_path(vec![
            "contract".to_string(),
            "end_date".to_string()
        ])
    );
    assert!(
        matches!(&result[0].facts[2].value, FactValue::TypeDeclaration { .. }),
        "Expected TypeDeclaration"
    );

    assert_eq!(
        result[0].facts[3].reference,
        crate::parsing::ast::Reference::from_path(vec![
            "contract".to_string(),
            "employment_type".to_string()
        ])
    );
    if let FactValue::Literal(lit) = &result[0].facts[3].value {
        if let crate::Value::Text(s) = lit {
            assert_eq!(s, "contractor");
        } else {
            panic!("Expected Text literal");
        }
    } else {
        panic!("Expected Literal fact");
    }

    assert_eq!(
        result[0].facts[4].reference,
        crate::parsing::ast::Reference::from_path(vec!["contract".to_string(), "base".to_string()])
    );
    if let FactValue::SpecReference(spec_ref) = &result[0].facts[4].value {
        assert_eq!(spec_ref.name, "base_contract");
        assert!(!spec_ref.is_registry);
    } else {
        panic!("Expected SpecReference");
    }

    assert_eq!(
        result[0].facts[5].reference,
        crate::parsing::ast::Reference::from_path(vec![
            "contract".to_string(),
            "base".to_string(),
            "rate".to_string()
        ])
    );
    if let FactValue::Literal(lit) = &result[0].facts[5].value {
        if let crate::Value::Number(n) = lit {
            assert_eq!(n, &rust_decimal::Decimal::new(100, 0));
        } else {
            panic!("Expected Number literal");
        }
    } else {
        panic!("Expected Literal fact");
    }
}
