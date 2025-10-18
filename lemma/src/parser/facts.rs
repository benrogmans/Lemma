use crate::error::LemmaError;
use crate::parser::Rule;
use crate::semantic::*;
use pest::iterators::Pair;

pub(crate) fn parse_fact_definition(pair: Pair<Rule>) -> Result<LemmaFact, LemmaError> {
    let span = crate::ast::Span::from_pest_span(pair.as_span());
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

    Ok(LemmaFact::new(crate::FactType::Local(name), value).with_span(span))
}

pub(crate) fn parse_fact_override(pair: Pair<Rule>) -> Result<LemmaFact, LemmaError> {
    let span = crate::ast::Span::from_pest_span(pair.as_span());
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

    let override_ref = fact_override_name.ok_or_else(|| {
        LemmaError::Engine("Grammar error: fact_override missing fact_override_name".to_string())
    })?;
    let value = fact_value.ok_or_else(|| {
        LemmaError::Engine("Grammar error: fact_override missing fact_value".to_string())
    })?;

    Ok(LemmaFact::new(crate::FactType::Foreign(override_ref), value).with_span(span))
}

fn parse_fact_override_name(pair: Pair<Rule>) -> Result<crate::ForeignFact, LemmaError> {
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
    Ok(crate::ForeignFact { reference })
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
                    Rule::data_size_type => LemmaType::DataSize,
                    Rule::money_type => LemmaType::Money,
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
        crate::parser::literals::parse_literal(pair.into_inner().next().ok_or_else(|| {
            LemmaError::Engine("Grammar error: literal must contain a literal value".to_string())
        })?)?;
    Ok(FactValue::Literal(literal_value))
}
