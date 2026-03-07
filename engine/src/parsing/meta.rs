use crate::error::Error;
use crate::parsing::ast::{MetaField, MetaValue, Span};
use crate::parsing::literals::parse_literal;
use crate::parsing::source::Source;
use crate::parsing::Rule;
use pest::iterators::Pair;
use std::sync::Arc;

pub fn parse_meta_definition(
    pair: Pair<Rule>,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<MetaField, Error> {
    debug_assert_eq!(pair.as_rule(), Rule::meta_definition);

    let span = Span::from_pest_span(pair.as_span());
    let source_location = Source::new(attribute, span, spec_name, source_text.clone());

    let mut key = String::new();
    let mut value = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::meta_key => {
                key = inner.as_str().to_string();
            }
            Rule::meta_value => {
                let inner_val = inner.into_inner().next().unwrap();
                match inner_val.as_rule() {
                    Rule::literal => {
                        let specific_literal = inner_val.into_inner().next().unwrap();
                        let val = parse_literal(
                            specific_literal,
                            attribute,
                            spec_name,
                            source_text.clone(),
                        )?;
                        value = Some(MetaValue::Literal(val));
                    }
                    Rule::meta_identifier => {
                        value = Some(MetaValue::Unquoted(inner_val.as_str().to_string()));
                    }
                    _ => unreachable!("meta_value should be literal or meta_identifier"),
                }
            }
            _ => {}
        }
    }

    Ok(MetaField {
        key,
        value: value.expect("grammar guarantees meta_value"),
        source_location,
    })
}
