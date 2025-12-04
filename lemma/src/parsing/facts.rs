use super::ast::Span;
use super::Rule;
use crate::error::LemmaError;
use crate::semantic::*;
use crate::Source;
use pest::iterators::Pair;

pub(crate) fn parse_fact_definition(
    pair: Pair<Rule>,
    source_id: Option<&str>,
    doc_name: Option<&str>,
) -> Result<LemmaFact, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let mut fact_name = None;
    let mut fact_value = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::fact_name => fact_name = Some(inner_pair.as_str().to_string()),
            Rule::fact_value => fact_value = Some(parse_fact_value(inner_pair)?),
            _ => {}
        }
    }

    let name = fact_name.ok_or_else(|| {
        LemmaError::Engine("Grammar error: fact_definition missing fact_name".to_string())
    })?;
    let value = fact_value.ok_or_else(|| {
        LemmaError::Engine("Grammar error: fact_definition missing fact_value".to_string())
    })?;

    let mut fact = LemmaFact::new(FactReference::local(name), value);
    if let (Some(source_id), Some(doc_name)) = (source_id, doc_name) {
        fact = fact.with_source(Source::new(
            source_id.to_string(),
            span,
            doc_name.to_string(),
        ));
    }
    Ok(fact)
}

pub(crate) fn parse_fact_override(
    pair: Pair<Rule>,
    source_id: Option<&str>,
    doc_name: Option<&str>,
) -> Result<LemmaFact, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let mut fact_override_name = None;
    let mut fact_value = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::fact_override_name => {
                fact_override_name = Some(parse_fact_override_name(inner_pair)?)
            }
            Rule::fact_value => fact_value = Some(parse_fact_value(inner_pair)?),
            _ => {}
        }
    }

    let override_ref_path = fact_override_name.ok_or_else(|| {
        LemmaError::Engine("Grammar error: fact_override missing fact_override_name".to_string())
    })?;
    let value = fact_value.ok_or_else(|| {
        LemmaError::Engine("Grammar error: fact_override missing fact_value".to_string())
    })?;

    let override_ref = FactReference::from_path(override_ref_path);
    let mut fact = LemmaFact::new(override_ref, value);
    if let (Some(source_id), Some(doc_name)) = (source_id, doc_name) {
        fact = fact.with_source(Source::new(
            source_id.to_string(),
            span,
            doc_name.to_string(),
        ));
    }
    Ok(fact)
}

fn parse_fact_override_name(pair: Pair<Rule>) -> Result<Vec<String>, LemmaError> {
    let mut reference = Vec::new();
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::label {
            reference.push(inner_pair.as_str().to_string());
        }
    }
    if reference.is_empty() {
        return Err(LemmaError::Engine(
            "Grammar error: fact_override_name has no labels".to_string(),
        ));
    }
    Ok(reference)
}

fn parse_fact_value(pair: Pair<Rule>) -> Result<FactValue, LemmaError> {
    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::type_annotation => return parse_fact_type_annotation(inner_pair),
            Rule::document_reference => return parse_fact_document_reference(inner_pair),
            Rule::literal => return parse_fact_literal(inner_pair),
            _ => {}
        }
    }
    Err(LemmaError::Engine(
        "Grammar error: fact_value must contain literal, type_annotation, or document_reference"
            .to_string(),
    ))
}

fn parse_fact_type_annotation(pair: Pair<Rule>) -> Result<FactValue, LemmaError> {
    for inner_pair in pair.into_inner() {
        if inner_pair.as_rule() == Rule::type_name {
            if let Some(type_inner) = inner_pair.into_inner().next() {
                let lemma_type = match type_inner.as_rule() {
                    Rule::text_type => LemmaType::Text,
                    Rule::number_type => LemmaType::Number,
                    Rule::date_type => LemmaType::Date,
                    Rule::boolean_type => LemmaType::Boolean,
                    Rule::regex_type => LemmaType::Regex,
                    Rule::percentage_type => LemmaType::Percentage,
                    Rule::weight_type => LemmaType::Mass,
                    Rule::length_type => LemmaType::Length,
                    Rule::volume_type => LemmaType::Volume,
                    Rule::duration_type => LemmaType::Duration,
                    Rule::temperature_type => LemmaType::Temperature,
                    Rule::power_type => LemmaType::Power,
                    Rule::energy_type => LemmaType::Energy,
                    Rule::force_type => LemmaType::Force,
                    Rule::pressure_type => LemmaType::Pressure,
                    Rule::frequency_type => LemmaType::Frequency,
                    Rule::data_size_type => LemmaType::Data,
                    _ => {
                        return Err(LemmaError::Engine(format!(
                            "Unknown type rule: {:?}",
                            type_inner.as_rule()
                        )))
                    }
                };
                return Ok(FactValue::TypeAnnotation(TypeAnnotation::LemmaType(
                    lemma_type,
                )));
            }
        }
    }
    Err(LemmaError::Engine(
        "Grammar error: type_annotation must contain type_name".to_string(),
    ))
}

fn parse_fact_document_reference(pair: Pair<Rule>) -> Result<FactValue, LemmaError> {
    let doc_name = pair
        .into_inner()
        .next()
        .ok_or_else(|| {
            LemmaError::Engine("Grammar error: document_reference must contain label".to_string())
        })?
        .as_str()
        .to_string();

    Ok(FactValue::DocumentReference(doc_name))
}

fn parse_fact_literal(pair: Pair<Rule>) -> Result<FactValue, LemmaError> {
    let literal_value =
        crate::parsing::literals::parse_literal(pair.into_inner().next().ok_or_else(|| {
            LemmaError::Engine("Grammar error: literal must contain a literal value".to_string())
        })?)?;
    Ok(FactValue::Literal(literal_value))
}

#[cfg(test)]
mod tests {
    use crate::parsing::parse;
    use crate::{FactValue, LiteralValue};

    #[test]
    fn test_parse_simple_document_reference() {
        let input = r#"doc person
fact name = "John"
fact contract = doc employment_contract"#;
        let result = parse(
            input,
            Some("test.lemma".to_string()),
            &crate::ResourceLimits::default(),
        )
        .unwrap();
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
        let result = parse(
            input,
            Some("test.lemma".to_string()),
            &crate::ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 6);

        assert_eq!(
            result[0].facts[0].reference,
            crate::FactReference::from_path(vec!["contract".to_string()])
        );
        if let FactValue::DocumentReference(doc_name) = &result[0].facts[0].value {
            assert_eq!(doc_name, "employment_contract");
        } else {
            panic!("Expected DocumentReference");
        }

        assert_eq!(
            result[0].facts[1].reference,
            crate::FactReference::from_path(vec!["contract".to_string(), "start_date".to_string()])
        );
        assert!(
            matches!(
                &result[0].facts[1].value,
                FactValue::Literal(LiteralValue::Date(_))
            ),
            "Expected Date literal"
        );

        assert_eq!(
            result[0].facts[2].reference,
            crate::FactReference::from_path(vec!["contract".to_string(), "end_date".to_string()])
        );
        assert!(
            matches!(&result[0].facts[2].value, FactValue::TypeAnnotation(_)),
            "Expected TypeAnnotation"
        );

        assert_eq!(
            result[0].facts[3].reference,
            crate::FactReference::from_path(vec![
                "contract".to_string(),
                "employment_type".to_string()
            ])
        );
        if let FactValue::Literal(LiteralValue::Text(s)) = &result[0].facts[3].value {
            assert_eq!(s, "contractor");
        } else {
            panic!("Expected Text literal");
        }

        assert_eq!(
            result[0].facts[4].reference,
            crate::FactReference::from_path(vec!["contract".to_string(), "base".to_string()])
        );
        if let FactValue::DocumentReference(doc_name) = &result[0].facts[4].value {
            assert_eq!(doc_name, "base_contract");
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
        if let FactValue::Literal(LiteralValue::Number(n)) = &result[0].facts[5].value {
            assert_eq!(*n, rust_decimal::Decimal::new(100, 0));
        } else {
            panic!("Expected Number literal");
        }
    }
}
