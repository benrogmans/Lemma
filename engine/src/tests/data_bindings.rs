use crate::parsing::ast::{DataValue, FillRhs};
use crate::parsing::parse;

#[test]
fn test_parse_with_spec_reference() {
    let input = r#"spec person
data name: "John"
uses contract: employment_contract"#;
    let result = parse(
        input,
        crate::parsing::source::SourceType::Volatile,
        &crate::ResourceLimits::default(),
    )
    .unwrap()
    .into_flattened_specs();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].data.len(), 2);

    assert_eq!(
        result[0].data[1].reference,
        crate::parsing::ast::Reference::local("contract".to_string())
    );
    if let DataValue::Import(spec_ref) = &result[0].data[1].value {
        assert_eq!(spec_ref.name, "employment_contract");
        assert!(spec_ref.repository.is_none());
    } else {
        panic!("Expected Import");
    }
}

#[test]
fn test_parse_with_and_data_bindings() {
    let input = r#"spec person
uses contract: employment_contract
fill contract.start_date: 2024-02-01
data declaration_probe: date
fill contract.employment_type: "contractor"
uses base: base_contract"#;
    let result = parse(
        input,
        crate::parsing::source::SourceType::Volatile,
        &crate::ResourceLimits::default(),
    )
    .unwrap()
    .into_flattened_specs();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].data.len(), 5);

    assert_eq!(
        result[0].data[0].reference,
        crate::parsing::ast::Reference::local("contract".to_string())
    );
    if let DataValue::Import(spec_ref) = &result[0].data[0].value {
        assert_eq!(spec_ref.name, "employment_contract");
        assert!(spec_ref.repository.is_none());
    } else {
        panic!("Expected Import");
    }

    assert_eq!(
        result[0].data[1].reference,
        crate::parsing::ast::Reference::from_path(vec![
            "contract".to_string(),
            "start_date".to_string()
        ])
    );
    match &result[0].data[1].value {
        DataValue::Fill(FillRhs::Literal(lit)) => {
            assert!(
                matches!(lit, crate::parsing::ast::Value::Date(_)),
                "Expected Date literal in fill"
            );
        }
        other => panic!("Expected fill with date literal, got {:?}", other),
    }

    assert_eq!(
        result[0].data[2].reference,
        crate::parsing::ast::Reference::local("declaration_probe".to_string())
    );
    assert!(
        matches!(
            &result[0].data[2].value,
            DataValue::Definition {
                base: Some(crate::parsing::ast::ParentType::Primitive {
                    primitive: crate::parsing::ast::PrimitiveKind::Date,
                }),
                value: None,
                ..
            }
        ),
        "Expected Definition with date primitive base"
    );

    assert_eq!(
        result[0].data[3].reference,
        crate::parsing::ast::Reference::from_path(vec![
            "contract".to_string(),
            "employment_type".to_string()
        ])
    );
    match &result[0].data[3].value {
        DataValue::Fill(FillRhs::Literal(crate::parsing::ast::Value::Text(s))) => {
            assert_eq!(s, "contractor");
        }
        other => panic!("Expected fill with text literal, got {:?}", other),
    }

    assert_eq!(
        result[0].data[4].reference,
        crate::parsing::ast::Reference::local("base".to_string())
    );
    if let DataValue::Import(spec_ref) = &result[0].data[4].value {
        assert_eq!(spec_ref.name, "base_contract");
        assert!(spec_ref.repository.is_none());
    } else {
        panic!("Expected Import");
    }
}

#[test]
fn test_data_spec_shorthand_syntax_is_rejected() {
    let input = r#"spec person
data contract: spec employment_contract"#;
    let result = parse(
        input,
        crate::parsing::source::SourceType::Volatile,
        &crate::ResourceLimits::default(),
    );
    assert!(
        result.is_err(),
        "`data ... : spec ...` must be rejected; use `uses` to import specs"
    );
}
