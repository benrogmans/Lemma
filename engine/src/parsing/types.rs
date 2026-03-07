use super::ast::Span;
use super::Rule;
use crate::error::Error;
use crate::parsing::ast::{CommandArg, TypeDef};
use pest::iterators::Pair;
use std::sync::Arc;

pub(crate) fn parse_type_definition(
    pair: Pair<Rule>,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<TypeDef, Error> {
    let span = Span::from_pest_span(pair.as_span());
    let source_location =
        crate::Source::new(attribute, span.clone(), spec_name, source_text.clone());
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

    let type_name_str =
        type_name.expect("BUG: grammar guarantees type_definition has type_name_def");

    let arrow_chain_pair =
        type_arrow_chain.expect("BUG: grammar guarantees type_definition has type_arrow_chain");

    let (parent, constraints, _from) = parse_type_arrow_chain_with_commands(
        arrow_chain_pair,
        attribute,
        spec_name,
        source_text.clone(),
    )?;
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
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<TypeDef, Error> {
    let span = Span::from_pest_span(pair.as_span());
    let source_location =
        crate::Source::new(attribute, span.clone(), spec_name, source_text.clone());
    // The pair is type_import, which contains type_import_def
    let type_import_def = pair
        .into_inner()
        .next()
        .expect("BUG: grammar guarantees type_import contains type_import_def");

    let mut type_names = Vec::new();
    let mut imported_spec_ref = None;
    let mut hash: Option<String> = None;

    for inner_pair in type_import_def.into_inner() {
        match inner_pair.as_rule() {
            Rule::type_name_def => {
                type_names.push(inner_pair.as_str().to_string());
            }
            Rule::spec_name => {
                imported_spec_ref = Some(super::facts::parse_spec_name_pair(inner_pair)?);
            }
            Rule::spec_ref_hash => {
                hash = Some(inner_pair.as_str().to_string());
            }
            _ => {}
        }
    }

    let mut imported_spec_ref =
        imported_spec_ref.expect("BUG: grammar guarantees type_import has spec_name");
    imported_spec_ref.hash_pin = hash;

    assert!(
        !type_names.is_empty(),
        "BUG: grammar guarantees type_import has type_name_def"
    );

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
        from: imported_spec_ref,
        constraints: None,
    })
}

type TypeArrowChainResult = (
    String,
    Option<Vec<(String, Vec<CommandArg>)>>,
    Option<super::ast::SpecRef>,
);

pub(crate) fn parse_type_arrow_chain_with_commands(
    pair: Pair<Rule>,
    attribute: &str,
    spec_name: &str,
    source_text: Arc<str>,
) -> Result<TypeArrowChainResult, Error> {
    let mut inner = pair.into_inner();
    let first = inner
        .next()
        .expect("BUG: grammar guarantees type_arrow_chain is non-empty");

    // Store the remaining items for command parsing (after the first element)
    let remaining_items: Vec<_> = inner.collect();

    fn parse_type_name_def_pair(
        pair: &Pair<Rule>,
        _attribute: &str,
        _spec_name: &str,
        _source_text: Arc<str>,
    ) -> Result<String, Error> {
        let mut inner = pair.clone().into_inner();
        let child = inner
            .next()
            .expect("BUG: grammar guarantees type_name_def has inner rule");
        match child.as_rule() {
            Rule::type_standard => Ok(pair.as_str().to_lowercase()),
            Rule::type_custom => Ok(pair.as_str().to_string()),
            _ => unreachable!(
                "BUG: unexpected rule in type_name_def: {:?}",
                child.as_rule()
            ),
        }
    }

    let (parent_name, from_spec) = match first.as_rule() {
        Rule::type_name_def => (
            parse_type_name_def_pair(&first, attribute, spec_name, source_text.clone())?,
            None,
        ),
        Rule::type_import_def => {
            let inner = first.clone().into_inner();
            let mut type_name_def = None;
            let mut imported_spec_ref = None;
            let mut hash: Option<String> = None;

            for item in inner {
                match item.as_rule() {
                    Rule::type_name_def => {
                        type_name_def = Some(parse_type_name_def_pair(
                            &item,
                            attribute,
                            spec_name,
                            source_text.clone(),
                        )?);
                    }
                    Rule::spec_name => {
                        imported_spec_ref = Some(super::facts::parse_spec_name_pair(item)?);
                    }
                    Rule::spec_ref_hash => {
                        hash = Some(item.as_str().to_string());
                    }
                    _ => {}
                }
            }

            let source_type =
                type_name_def.expect("BUG: grammar guarantees type_import_def has type_name_def");

            let mut from =
                imported_spec_ref.expect("BUG: grammar guarantees type_import_def has spec_name");
            from.hash_pin = hash;

            (source_type, Some(from))
        }
        _ => {
            unreachable!(
                "BUG: unexpected rule in type_arrow_chain: {:?}",
                first.as_rule()
            )
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
                assert!(
                    expecting_command,
                    "BUG: grammar guarantees command follows arrow_symbol"
                );
                let (command_name, args) =
                    parse_command(item, attribute, spec_name, source_text.clone())?;
                commands.push((command_name, args));
                expecting_command = false;
            }
            _ => {
                unreachable!(
                    "BUG: unexpected rule in type_arrow_chain: {:?}",
                    item.as_rule()
                )
            }
        }
    }

    assert!(
        !expecting_command,
        "BUG: grammar guarantees arrow_symbol is followed by command"
    );

    let constraints = if commands.is_empty() {
        None
    } else {
        Some(commands)
    };

    Ok((parent_name, constraints, from_spec))
}

/// Returns a typed `CommandArg` preserving which grammar alternative matched.
///
/// - `text_literal` → `CommandArg::Text` (content between quotes, no surrounding quotes)
/// - `number_literal` → `CommandArg::Number` (raw token text)
/// - `boolean_literal` → `CommandArg::Boolean` (raw token text)
/// - `label` → `CommandArg::Label` (raw token text)
///
/// Note: `label` is a silent rule in the Pest grammar (`_{ ... }`), so when
/// `command_arg` matches via `label`, `.into_inner()` yields no children.
/// We capture the raw text first to handle that case.
fn command_arg_value(pair: Pair<Rule>) -> CommandArg {
    let raw = pair.as_str().to_string();
    let mut inner = pair.into_inner();
    let Some(child) = inner.next() else {
        // Matched via `label` (silent rule) — use the raw token text.
        return CommandArg::Label(raw);
    };
    match child.as_rule() {
        Rule::text_literal => {
            let s = child.as_str();
            let content = if s.len() >= 2 {
                s[1..s.len() - 1].to_string()
            } else {
                s.to_string()
            };
            CommandArg::Text(content)
        }
        Rule::number_literal => CommandArg::Number(child.as_str().to_string()),
        Rule::boolean_literal => CommandArg::Boolean(child.as_str().to_string()),
        // Any other rule is treated as a label-like token.
        _ => CommandArg::Label(child.as_str().to_string()),
    }
}

fn parse_command(
    pair: Pair<Rule>,
    _attribute: &str,
    _spec_name: &str,
    _source_text: Arc<str>,
) -> Result<(String, Vec<CommandArg>), Error> {
    let mut command_name = None;
    let mut command_args = Vec::new();

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::command_name => {
                command_name = Some(inner_pair.as_str().to_string());
            }
            Rule::command_arg => {
                let arg_value = command_arg_value(inner_pair);
                command_args.push(arg_value);
            }
            _ => {}
        }
    }

    let name = command_name.expect("BUG: grammar guarantees command has command_name");

    Ok((name, command_args))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::parsing::ast::{CommandArg, FactValue};
    use crate::{parse, ResourceLimits};

    #[test]
    fn type_definition_parsing_produces_regular_typedef_with_constraints() {
        let code = r#"spec test
type dice: number -> minimum 0 -> maximum 6"#;

        let specs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        assert_eq!(specs.len(), 1);

        let spec = &specs[0];
        assert_eq!(spec.name, "test");
        assert_eq!(spec.types.len(), 1);

        let type_def = &spec.types[0];
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
                assert_eq!(constraints[0].1, vec![CommandArg::Number("0".to_string())]);
                assert_eq!(constraints[1].0, "maximum");
                assert_eq!(constraints[1].1, vec![CommandArg::Number("6".to_string())]);
            }
            other => panic!("Expected Regular type definition, got {:?}", other),
        }
    }

    #[test]
    fn parser_produces_command_arg_number_for_number_literals() {
        let code = "spec test\nfact x: [number -> minimum 5 -> maximum 100 -> default 42]";
        let specs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let fact = &specs[0].facts[0];
        match &fact.value {
            FactValue::TypeDeclaration { constraints, .. } => {
                let constraints = constraints.as_ref().unwrap();
                assert_eq!(constraints[0].0, "minimum");
                assert_eq!(constraints[0].1, vec![CommandArg::Number("5".to_string())]);
                assert_eq!(constraints[1].0, "maximum");
                assert_eq!(
                    constraints[1].1,
                    vec![CommandArg::Number("100".to_string())]
                );
                assert_eq!(constraints[2].0, "default");
                assert_eq!(constraints[2].1, vec![CommandArg::Number("42".to_string())]);
            }
            other => panic!("Expected TypeDeclaration, got {:?}", other),
        }
    }

    #[test]
    fn parser_produces_command_arg_text_for_text_literals() {
        let code = r#"spec test
fact x: [text -> help "Enter your name" -> default "Alice"]"#;
        let specs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let fact = &specs[0].facts[0];
        match &fact.value {
            FactValue::TypeDeclaration { constraints, .. } => {
                let constraints = constraints.as_ref().unwrap();
                assert_eq!(constraints[0].0, "help");
                assert_eq!(
                    constraints[0].1,
                    vec![CommandArg::Text("Enter your name".to_string())]
                );
                assert_eq!(constraints[1].0, "default");
                assert_eq!(
                    constraints[1].1,
                    vec![CommandArg::Text("Alice".to_string())]
                );
            }
            other => panic!("Expected TypeDeclaration, got {:?}", other),
        }
    }

    #[test]
    fn parser_produces_command_arg_boolean_for_boolean_literals() {
        let code = "spec test\nfact x: [boolean -> default true]";
        let specs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let fact = &specs[0].facts[0];
        match &fact.value {
            FactValue::TypeDeclaration { constraints, .. } => {
                let constraints = constraints.as_ref().unwrap();
                assert_eq!(constraints[0].0, "default");
                assert_eq!(
                    constraints[0].1,
                    vec![CommandArg::Boolean("true".to_string())]
                );
            }
            other => panic!("Expected TypeDeclaration, got {:?}", other),
        }
    }

    #[test]
    fn parser_produces_command_arg_label_for_unit_names() {
        let code = "spec test\ntype money: scale -> unit eur 1.00 -> unit usd 1.10";
        let specs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let type_def = &specs[0].types[0];
        match type_def {
            crate::TypeDef::Regular { constraints, .. } => {
                let constraints = constraints.as_ref().unwrap();
                assert_eq!(constraints[0].0, "unit");
                assert_eq!(
                    constraints[0].1,
                    vec![
                        CommandArg::Label("eur".to_string()),
                        CommandArg::Number("1.00".to_string()),
                    ]
                );
                assert_eq!(constraints[1].0, "unit");
                assert_eq!(
                    constraints[1].1,
                    vec![
                        CommandArg::Label("usd".to_string()),
                        CommandArg::Number("1.10".to_string()),
                    ]
                );
            }
            other => panic!("Expected Regular type definition, got {:?}", other),
        }
    }

    #[test]
    fn parser_produces_command_arg_text_for_option_values() {
        let code = r#"spec test
type status: text -> option "active" -> option "inactive""#;
        let specs = parse(code, "test.lemma", &ResourceLimits::default()).unwrap();
        let type_def = &specs[0].types[0];
        match type_def {
            crate::TypeDef::Regular { constraints, .. } => {
                let constraints = constraints.as_ref().unwrap();
                assert_eq!(constraints[0].0, "option");
                assert_eq!(
                    constraints[0].1,
                    vec![CommandArg::Text("active".to_string())]
                );
                assert_eq!(constraints[1].0, "option");
                assert_eq!(
                    constraints[1].1,
                    vec![CommandArg::Text("inactive".to_string())]
                );
            }
            other => panic!("Expected Regular type definition, got {:?}", other),
        }
    }

    #[test]
    fn parser_distinguishes_text_literal_from_number_literal() {
        // "10" is a text_literal; 10 is a number_literal.
        // The parser must produce different CommandArg variants.
        let code_text = r#"spec test
fact x: [number -> default "10"]"#;
        let specs_text = parse(code_text, "test.lemma", &ResourceLimits::default()).unwrap();
        let fact_text = &specs_text[0].facts[0];
        match &fact_text.value {
            FactValue::TypeDeclaration { constraints, .. } => {
                let constraints = constraints.as_ref().unwrap();
                assert_eq!(constraints[0].1, vec![CommandArg::Text("10".to_string())]);
            }
            other => panic!("Expected TypeDeclaration, got {:?}", other),
        }

        let code_number = "spec test\nfact x: [number -> default 10]";
        let specs_number = parse(code_number, "test.lemma", &ResourceLimits::default()).unwrap();
        let fact_number = &specs_number[0].facts[0];
        match &fact_number.value {
            FactValue::TypeDeclaration { constraints, .. } => {
                let constraints = constraints.as_ref().unwrap();
                assert_eq!(constraints[0].1, vec![CommandArg::Number("10".to_string())]);
            }
            other => panic!("Expected TypeDeclaration, got {:?}", other),
        }
    }
}
