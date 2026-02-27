use super::ast::Span;
use super::Rule;
use crate::error::LemmaError;
use crate::limits::MAX_VERSION_TAG_LENGTH;
use crate::parsing::ast::*;
use crate::parsing::types;
use crate::Source;
use pest::iterators::Pair;
use std::sync::Arc;

pub(crate) fn parse_fact_definition(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<LemmaFact, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let attribute_str = attribute;
    let doc_name_str = doc_name;
    let mut fact_name = None;
    let mut fact_value = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::fact_reference_segment => fact_name = Some(inner_pair.as_str().to_string()),
            Rule::fact_value => {
                fact_value = Some(parse_fact_value(
                    inner_pair,
                    attribute_str,
                    doc_name_str,
                    source_text.clone(),
                )?)
            }
            _ => {}
        }
    }

    let name =
        fact_name.expect("BUG: grammar guarantees fact_definition has fact_reference_segment");
    let value = fact_value.expect("BUG: grammar guarantees fact_definition has fact_value");

    let fact = LemmaFact::new(
        FactReference::local(name),
        value,
        Source::new(
            attribute.to_string(),
            span,
            doc_name.to_string(),
            source_text.clone(),
        ),
    );
    Ok(fact)
}

pub(crate) fn parse_fact_binding(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<LemmaFact, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let attribute_str = attribute;
    let doc_name_str = doc_name;
    let mut fact_reference_path = None;
    let mut fact_value = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::fact_reference => {
                fact_reference_path = Some(parse_fact_reference_path(
                    inner_pair,
                    attribute_str,
                    doc_name_str,
                    source_text.clone(),
                )?)
            }
            Rule::fact_value => {
                fact_value = Some(parse_fact_value(
                    inner_pair,
                    attribute_str,
                    doc_name_str,
                    source_text.clone(),
                )?)
            }
            _ => {}
        }
    }

    let binding_ref_path =
        fact_reference_path.expect("BUG: grammar guarantees fact_binding has fact_reference");
    let value = fact_value.expect("BUG: grammar guarantees fact_binding has fact_value");

    let binding_ref = FactReference::from_path(binding_ref_path);
    let fact = LemmaFact::new(
        binding_ref,
        value,
        Source::new(
            attribute.to_string(),
            span,
            doc_name.to_string(),
            source_text.clone(),
        ),
    );
    Ok(fact)
}

fn parse_fact_reference_path(
    pair: Pair<Rule>,
    _attribute: &str,
    _doc_name: &str,
    _source_text: Arc<str>,
) -> Result<Vec<String>, LemmaError> {
    let text = pair.as_str();
    let parts: Vec<String> = text.split('.').map(|s| s.to_string()).collect();
    assert!(
        !parts.is_empty(),
        "BUG: grammar guarantees fact_reference has segments"
    );
    Ok(parts)
}

fn parse_fact_value(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<FactValue, LemmaError> {
    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::type_declaration => {
                return parse_type_declaration(inner_pair, attribute, doc_name, source_text.clone())
            }
            Rule::inline_type_definition => {
                return parse_inline_type_definition(
                    inner_pair,
                    attribute,
                    doc_name,
                    source_text.clone(),
                )
            }
            Rule::doc_reference => {
                return parse_fact_document_reference(
                    inner_pair,
                    attribute,
                    doc_name,
                    source_text.clone(),
                )
            }
            Rule::literal => {
                return parse_fact_literal(inner_pair, attribute, doc_name, source_text.clone())
            }
            _ => {}
        }
    }
    unreachable!("BUG: grammar guarantees fact_value contains literal, type_declaration, inline_type_definition, or doc_reference")
}

/// Parse a type declaration: `[type_name]` - a reference to a named type
///
/// This handles cases like `fact price = [money]` where `money` is a named type.
/// No resolution happens during parsing - that's deferred to the planning phase.
fn parse_type_declaration(
    pair: Pair<Rule>,
    _attribute: &str,
    _doc_name: &str,
    _source_text: Arc<str>,
) -> Result<FactValue, LemmaError> {
    let type_name_def = pair
        .into_inner()
        .next()
        .expect("BUG: grammar guarantees type_declaration has type_name_def");

    let type_name = type_name_def.as_str().to_string();

    Ok(FactValue::TypeDeclaration {
        base: type_name,
        constraints: None,
        from: None,
    })
}

/// Parse an inline type definition: `[type_arrow_chain]` - an inline type with commands
///
/// This handles cases like `fact price = [number -> minimum 0]` or `fact buyin = [money -> minimal 100]`.
/// No resolution happens during parsing - that's deferred to the planning phase.
fn parse_inline_type_definition(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<FactValue, LemmaError> {
    let type_arrow_chain = pair
        .into_inner()
        .next()
        .expect("BUG: grammar guarantees inline_type_definition has type_arrow_chain");

    let (parent_name, inline_constraints, from_doc) = types::parse_type_arrow_chain_with_commands(
        type_arrow_chain,
        attribute,
        doc_name,
        source_text.clone(),
    )?;

    Ok(FactValue::TypeDeclaration {
        base: parent_name,
        constraints: inline_constraints,
        from: from_doc,
    })
}

fn parse_fact_document_reference(
    pair: Pair<Rule>,
    _attribute: &str,
    _doc_name: &str,
    _source_text: Arc<str>,
) -> Result<FactValue, LemmaError> {
    let doc_name_pair = pair
        .into_inner()
        .next()
        .expect("BUG: grammar guarantees doc_reference has doc_name");

    Ok(FactValue::DocumentReference(parse_doc_name_pair(
        doc_name_pair,
    )?))
}

/// Extract a `DocRef` from a `doc_name` grammar pair by reading its named inner pairs.
///
/// Returns `Err` if the version tag exceeds [`MAX_VERSION_TAG_LENGTH`] characters.
pub(crate) fn parse_doc_name_pair(pair: Pair<Rule>) -> Result<DocRef, LemmaError> {
    let mut is_registry = false;
    let mut name = String::new();
    let mut version = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::doc_name_at => {
                is_registry = true;
            }
            Rule::doc_name_base => {
                name = inner.as_str().to_string();
            }
            Rule::doc_version_tag => {
                let tag = inner.as_str();
                if tag.len() > MAX_VERSION_TAG_LENGTH {
                    return Err(LemmaError::parse(
                        format!(
                            "Version tag '{}' exceeds maximum length of {} characters",
                            tag, MAX_VERSION_TAG_LENGTH
                        ),
                        None,
                        None::<String>,
                    ));
                }
                version = Some(tag.to_string());
            }
            _ => {}
        }
    }

    Ok(DocRef {
        name,
        version,
        is_registry,
    })
}

fn parse_fact_literal(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
    source_text: Arc<str>,
) -> Result<FactValue, LemmaError> {
    let mut inner = pair.into_inner();
    let literal_pair = inner
        .next()
        .expect("BUG: grammar guarantees literal has inner value");

    let literal_value = crate::parsing::literals::parse_literal(
        literal_pair,
        attribute,
        doc_name,
        source_text.clone(),
    )?;
    Ok(FactValue::Literal(literal_value))
}

#[cfg(test)]
mod tests {
    use crate::parsing::parse;
    use crate::FactValue;

    #[test]
    fn test_parse_simple_document_reference() {
        let input = r#"doc person
fact name = "John"
fact contract = doc employment_contract"#;
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 2);

        if let FactValue::DocumentReference(doc_ref) = &result[0].facts[1].value {
            assert_eq!(doc_ref.name, "employment_contract");
            assert!(!doc_ref.is_registry);
        } else {
            panic!("Expected DocumentReference");
        }
    }

    #[test]
    fn test_parse_fact_bindings() {
        let input = r#"doc person
fact contract = doc employment_contract
fact contract.start_date = 2024-02-01
fact contract.end_date = [date]
fact contract.employment_type = "contractor"
fact contract.base = doc base_contract
fact contract.base.rate = 100"#;
        let result = parse(input, "test.lemma", &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 6);

        assert_eq!(
            result[0].facts[0].reference,
            crate::FactReference::from_path(vec!["contract".to_string()])
        );
        if let FactValue::DocumentReference(doc_ref) = &result[0].facts[0].value {
            assert_eq!(doc_ref.name, "employment_contract");
            assert!(!doc_ref.is_registry);
        } else {
            panic!("Expected DocumentReference");
        }

        assert_eq!(
            result[0].facts[1].reference,
            crate::FactReference::from_path(vec!["contract".to_string(), "start_date".to_string()])
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
            crate::FactReference::from_path(vec!["contract".to_string(), "end_date".to_string()])
        );
        assert!(
            matches!(&result[0].facts[2].value, FactValue::TypeDeclaration { .. }),
            "Expected TypeDeclaration"
        );

        assert_eq!(
            result[0].facts[3].reference,
            crate::FactReference::from_path(vec![
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
            crate::FactReference::from_path(vec!["contract".to_string(), "base".to_string()])
        );
        if let FactValue::DocumentReference(doc_ref) = &result[0].facts[4].value {
            assert_eq!(doc_ref.name, "base_contract");
            assert!(!doc_ref.is_registry);
        } else {
            panic!("Expected DocumentReference");
        }

        assert_eq!(
            result[0].facts[5].reference,
            crate::FactReference::from_path(vec![
                "contract".to_string(),
                "base".to_string(),
                "rate".to_string()
            ])
        );
        if let FactValue::Literal(lit) = &result[0].facts[5].value {
            if let crate::Value::Number(n) = lit {
                assert_eq!(*n, rust_decimal::Decimal::new(100, 0));
            } else {
                panic!("Expected Number literal");
            }
        } else {
            panic!("Expected Literal fact");
        }
    }

    #[test]
    fn parse_type_annotations_in_facts_collects_all_facts() {
        let input = r#"doc test
fact name = [text]
fact age = [number]
fact birth_date = [date]
fact is_active = [boolean]
fact discount = [percent]
fact duration = [duration]"#;

        let result = parse(input, "test.lemma", &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 6);
    }

    #[test]
    fn parse_primitive_type_annotations_in_facts_collects_all_facts() {
        let input = r#"doc test
fact duration = [duration]
fact number = [number]
fact text = [text]
fact date = [date]
fact boolean = [boolean]
fact percentage = [percent]"#;

        let result = parse(input, "test.lemma", &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 6);
    }

    /// Fact value "1 eur" (number_unit_literal) should parse and resolve via document's scale type.
    #[test]
    fn parse_fact_with_number_unit_literal_resolves_unit() {
        let input = r#"doc pricing
type money = scale
  -> unit eur 1
  -> unit usd 1.19

fact zz = 1 eur"#;

        let result = parse(input, "test.lemma", &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 1);
        assert_eq!(result[0].facts[0].reference.fact, "zz".to_string())
    }
}
