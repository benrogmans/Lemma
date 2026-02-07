use super::ast::Span;
use super::Rule;
use crate::error::LemmaError;
use crate::parsing::ast::TypeDef;
use pest::iterators::Pair;
use std::sync::Arc;

pub(crate) fn parse_type_definition(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<TypeDef, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let pair_str = pair.as_str();
    let source_location = crate::Source::new(attribute, span.clone(), doc_name);
    let mut type_name = None;
    let mut type_arrow_chain = None;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::type_name_def => {
                type_name = Some(inner_pair.as_str().to_string());
            }
            Rule::type_arrow_chain => {
                type_arrow_chain = Some(inner_pair);
            }
            _ => {}
        }
    }

    let type_name_str = type_name.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: type_definition missing type_name_def",
            crate::Source::new(attribute, span.clone(), doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;

    let arrow_chain_pair = type_arrow_chain.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: type_definition missing type_arrow_chain",
            crate::Source::new(attribute, span, doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;

    let (parent, constraints, _from) =
        parse_type_arrow_chain_with_commands(arrow_chain_pair, attribute, doc_name)?;
    // Regular types don't support 'from' - it's only for imports and inline types

    Ok(TypeDef::Regular {
        source_location,
        name: type_name_str,
        parent,
        constraints,
    })
}

pub(crate) fn parse_type_import(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<TypeDef, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let pair_str = pair.as_str();
    let source_location = crate::Source::new(attribute, span.clone(), doc_name);
    // The pair is type_import, which contains type_import_def
    let type_import_def = pair.into_inner().next().ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: type_import must contain type_import_def",
            crate::Source::new(attribute, span.clone(), doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;

    let mut type_names = Vec::new();
    let mut imported_doc_name = None;

    for inner_pair in type_import_def.into_inner() {
        match inner_pair.as_rule() {
            Rule::type_name_def => {
                type_names.push(inner_pair.as_str().to_string());
            }
            Rule::doc_name => {
                imported_doc_name = Some(super::ast::DocRef::parse(inner_pair.as_str()));
            }
            _ => {}
        }
    }

    let imported_doc_name = imported_doc_name.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: type_import missing doc_name",
            crate::Source::new(attribute, span.clone(), doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;

    if type_names.is_empty() {
        return Err(LemmaError::engine(
            "Grammar error: type_import missing type_name_def",
            crate::Source::new(attribute, span, doc_name),
            Arc::from(pair_str),
            None::<String>,
        ));
    }

    let source_type_name = if type_names.len() == 1 {
        type_names[0].clone()
    } else {
        type_names[1].clone()
    };

    let final_type_name = type_names[0].clone();

    Ok(TypeDef::Import {
        source_location,
        name: final_type_name,
        source_type: source_type_name,
        from: imported_doc_name,
        constraints: None,
    })
}

type TypeArrowChainResult = (
    String,
    Option<Vec<(String, Vec<String>)>>,
    Option<super::ast::DocRef>,
);

pub(crate) fn parse_type_arrow_chain_with_commands(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<TypeArrowChainResult, LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let pair_str = pair.as_str();
    let mut inner = pair.into_inner();
    let first = inner.next().ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: type_arrow_chain cannot be empty",
            crate::Source::new(attribute, span.clone(), doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;

    // Store the remaining items for command parsing (after the first element)
    let remaining_items: Vec<_> = inner.collect();

    fn parse_type_name_def_pair(
        pair: &Pair<Rule>,
        attribute: &str,
        doc_name: &str,
    ) -> Result<String, LemmaError> {
        let mut inner = pair.clone().into_inner();
        match inner.next() {
            Some(child) => match child.as_rule() {
                Rule::type_standard => Ok(pair.as_str().to_lowercase()),
                Rule::type_custom => Ok(pair.as_str().to_string()),
                _ => Err(LemmaError::engine(
                    format!("Unexpected rule in type_name_def: {:?}", child.as_rule()),
                    crate::Source::new(attribute, Span::from_pest_span(child.as_span()), doc_name),
                    Arc::from(pair.as_str()),
                    None::<String>,
                )),
            },
            None => Err(LemmaError::engine(
                "Grammar error: type_name_def must contain type_custom or type_standard",
                crate::Source::new(attribute, Span::from_pest_span(pair.as_span()), doc_name),
                Arc::from(pair.as_str()),
                None::<String>,
            )),
        }
    }

    let (parent_name, from_doc) = match first.as_rule() {
        Rule::type_name_def => (parse_type_name_def_pair(&first, attribute, doc_name)?, None),
        Rule::type_import_def => {
            // Parse: type_name_def ~ "from" ~ doc_name
            let inner = first.clone().into_inner();
            let mut type_name_def = None;
            let mut imported_doc_name = None;

            for item in inner {
                match item.as_rule() {
                    Rule::type_name_def => {
                        type_name_def = Some(parse_type_name_def_pair(&item, attribute, doc_name)?);
                    }
                    Rule::doc_name => {
                        imported_doc_name = Some(super::ast::DocRef::parse(item.as_str()));
                    }
                    _ => {}
                }
            }

            let first_span = Span::from_pest_span(first.as_span());
            let source_type = type_name_def.ok_or_else(|| {
                LemmaError::engine(
                    "Grammar error: type_import_def missing type_name_def",
                    crate::Source::new(attribute, first_span.clone(), doc_name),
                    Arc::from(first.as_str()),
                    None::<String>,
                )
            })?;

            let from = imported_doc_name.ok_or_else(|| {
                LemmaError::engine(
                    "Grammar error: type_import_def missing doc_name",
                    crate::Source::new(attribute, first_span, doc_name),
                    Arc::from(first.as_str()),
                    None::<String>,
                )
            })?;

            (source_type, Some(from))
        }
        _ => {
            return Err(LemmaError::engine(
                format!("Unexpected rule in type_arrow_chain: {:?}", first.as_rule()),
                crate::Source::new(attribute, span.clone(), doc_name),
                Arc::from(pair_str),
                None::<String>,
            ));
        }
    };

    let mut commands = Vec::new();
    let mut expecting_command = false;

    for item in remaining_items {
        match item.as_rule() {
            Rule::arrow_symbol => {
                expecting_command = true;
            }
            Rule::command => {
                if !expecting_command {
                    let item_span = Span::from_pest_span(item.as_span());
                    return Err(LemmaError::engine(
                        "Grammar error: command must follow arrow_symbol",
                        crate::Source::new(attribute, item_span, doc_name),
                        Arc::from(item.as_str()),
                        None::<String>,
                    ));
                }
                let (command_name, args) = parse_command(item, attribute, doc_name)?;
                commands.push((command_name, args));
                expecting_command = false;
            }
            _ => {
                let item_span = Span::from_pest_span(item.as_span());
                return Err(LemmaError::engine(
                    format!("Unexpected rule in type_arrow_chain: {:?}", item.as_rule()),
                    crate::Source::new(attribute, item_span, doc_name),
                    Arc::from(item.as_str()),
                    None::<String>,
                ));
            }
        }
    }

    if expecting_command {
        return Err(LemmaError::engine(
            "Grammar error: arrow_symbol must be followed by command",
            crate::Source::new(attribute, span.clone(), doc_name),
            Arc::from(pair_str),
            None::<String>,
        ));
    }

    let constraints = if commands.is_empty() {
        None
    } else {
        Some(commands)
    };

    Ok((parent_name, constraints, from_doc))
}

fn parse_command(
    pair: Pair<Rule>,
    attribute: &str,
    doc_name: &str,
) -> Result<(String, Vec<String>), LemmaError> {
    let span = Span::from_pest_span(pair.as_span());
    let pair_str = pair.as_str();
    let mut command_name = None;
    let mut command_args = Vec::new();

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::command_name => {
                command_name = Some(inner_pair.as_str().to_string());
            }
            Rule::command_arg => {
                command_args.push(inner_pair.as_str().to_string());
            }
            _ => {}
        }
    }

    let name = command_name.ok_or_else(|| {
        LemmaError::engine(
            "Grammar error: command must contain command_name",
            crate::Source::new(attribute, span, doc_name),
            Arc::from(pair_str),
            None::<String>,
        )
    })?;

    Ok((name, command_args))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::{parse, ResourceLimits};

    #[test]
    fn type_definition_parsing_produces_regular_typedef_with_constraints() {
        let code = r#"doc test
type dice = number -> minimum 0 -> maximum 6"#;

        let docs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(docs.len(), 1);

        let doc = &docs[0];
        assert_eq!(doc.name, "test");
        assert_eq!(doc.types.len(), 1);

        let type_def = &doc.types[0];
        match type_def {
            crate::TypeDef::Regular {
                name,
                parent,
                constraints,
                ..
            } => {
                assert_eq!(name, "dice");
                assert_eq!(parent, "number");
                assert!(constraints.is_some());

                let constraints = constraints.as_ref().unwrap();
                assert_eq!(constraints.len(), 2);
                assert_eq!(constraints[0].0, "minimum");
                assert_eq!(constraints[0].1, vec!["0"]);
                assert_eq!(constraints[1].0, "maximum");
                assert_eq!(constraints[1].1, vec!["6"]);
            }
            other => panic!("Expected Regular type definition, got {:?}", other),
        }
    }
}
