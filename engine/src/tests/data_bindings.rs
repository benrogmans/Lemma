use crate::parsing::ast::DataValue;
use crate::parsing::parse;

#[test]
fn test_parse_with_spec_reference() {
    let input = r#"spec person
data name: "John"
with contract: employment_contract"#;
    let result = parse(input, "test.lemma", &crate::ResourceLimits::default())
        .unwrap()
        .specs;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].data.len(), 2);

    assert_eq!(
        result[0].data[1].reference,
        crate::parsing::ast::Reference::local("contract".to_string())
    );
    if let DataValue::SpecReference(spec_ref) = &result[0].data[1].value {
        assert_eq!(spec_ref.name, "employment_contract");
        assert!(!spec_ref.from_registry);
    } else {
        panic!("Expected SpecReference");
    }
}

#[test]
fn test_parse_with_and_data_bindings() {
    let input = r#"spec person
with contract: employment_contract
data contract.start_date: 2024-02-01
data contract.end_date: date
data contract.employment_type: "contractor"
with base: base_contract"#;
    let result = parse(input, "test.lemma", &crate::ResourceLimits::default())
        .unwrap()
        .specs;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].data.len(), 5);

    assert_eq!(
        result[0].data[0].reference,
        crate::parsing::ast::Reference::local("contract".to_string())
    );
    if let DataValue::SpecReference(spec_ref) = &result[0].data[0].value {
        assert_eq!(spec_ref.name, "employment_contract");
        assert!(!spec_ref.from_registry);
    } else {
        panic!("Expected SpecReference");
    }

    assert_eq!(
        result[0].data[1].reference,
        crate::parsing::ast::Reference::from_path(vec![
            "contract".to_string(),
            "start_date".to_string()
        ])
    );
    match &result[0].data[1].value {
        DataValue::Literal(lit) => {
            assert!(
                matches!(lit, crate::parsing::ast::Value::Date(_)),
                "Expected Date literal"
            );
        }
        _ => panic!("Expected Date literal"),
    }

    assert_eq!(
        result[0].data[2].reference,
        crate::parsing::ast::Reference::from_path(vec![
            "contract".to_string(),
            "end_date".to_string()
        ])
    );
    assert!(
        matches!(&result[0].data[2].value, DataValue::TypeDeclaration { .. }),
        "Expected TypeDeclaration"
    );

    assert_eq!(
        result[0].data[3].reference,
        crate::parsing::ast::Reference::from_path(vec![
            "contract".to_string(),
            "employment_type".to_string()
        ])
    );
    if let DataValue::Literal(lit) = &result[0].data[3].value {
        if let crate::parsing::ast::Value::Text(s) = lit {
            assert_eq!(s, "contractor");
        } else {
            panic!("Expected Text literal");
        }
    } else {
        panic!("Expected Literal data");
    }

    assert_eq!(
        result[0].data[4].reference,
        crate::parsing::ast::Reference::local("base".to_string())
    );
    if let DataValue::SpecReference(spec_ref) = &result[0].data[4].value {
        assert_eq!(spec_ref.name, "base_contract");
        assert!(!spec_ref.from_registry);
    } else {
        panic!("Expected SpecReference");
    }
}

#[test]
fn test_data_spec_syntax_is_rejected() {
    let input = r#"spec person
data contract: spec employment_contract"#;
    let result = parse(input, "test.lemma", &crate::ResourceLimits::default());
    assert!(
        result.is_err(),
        "'data ... : spec ...' syntax should be rejected"
    );
}
