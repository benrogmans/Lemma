use crate::parsing::ast::{DataValue, WithRhs};

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
    if let DataValue::Import { spec_ref, bindings } = &result[0].data[1].value {
        assert_eq!(spec_ref.name, "employment_contract");
        assert!(spec_ref.repository.is_none());
        assert!(bindings.is_empty());
    } else {
        panic!("Expected Import");
    }
}

#[test]
fn test_parse_with_and_data_bindings() {
    let input = r#"spec person
uses contract: employment_contract
  -> with start_date: 2024-02-01
  -> with employment_type: "contractor"
data declaration_probe: date
uses base: base_contract"#;
    let result = parse(
        input,
        crate::parsing::source::SourceType::Volatile,
        &crate::ResourceLimits::default(),
    )
    .unwrap()
    .into_flattened_specs();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].data.len(), 3);

    assert_eq!(
        result[0].data[0].reference,
        crate::parsing::ast::Reference::local("contract".to_string())
    );
    if let DataValue::Import { spec_ref, bindings } = &result[0].data[0].value {
        assert_eq!(spec_ref.name, "employment_contract");
        assert!(spec_ref.repository.is_none());
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].path.name, "start_date");
        match &bindings[0].rhs {
            WithRhs::Literal(lit) => {
                assert!(
                    matches!(lit, crate::parsing::ast::Value::Date(_)),
                    "Expected Date literal in binding"
                );
            }
            other => panic!("Expected literal rhs, got {:?}", other),
        }
        assert_eq!(bindings[1].path.name, "employment_type");
        match &bindings[1].rhs {
            WithRhs::Literal(crate::parsing::ast::Value::Text(s)) => {
                assert_eq!(s, "contractor");
            }
            other => panic!("Expected text literal, got {:?}", other),
        }
    } else {
        panic!("Expected Import");
    }

    assert_eq!(
        result[0].data[1].reference,
        crate::parsing::ast::Reference::local("declaration_probe".to_string())
    );
    assert!(
        matches!(
            &result[0].data[1].value,
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
        result[0].data[2].reference,
        crate::parsing::ast::Reference::local("base".to_string())
    );
    if let DataValue::Import { spec_ref, bindings } = &result[0].data[2].value {
        assert_eq!(spec_ref.name, "base_contract");
        assert!(spec_ref.repository.is_none());
        assert!(bindings.is_empty());
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
