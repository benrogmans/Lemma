use crate::error::LemmaError;
use crate::limits::ResourceLimits;
use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use std::sync::Arc;

pub mod ast;
pub mod expressions;
pub mod facts;
pub mod literals;
pub mod rules;
pub mod source;
pub mod units;

pub use ast::{ExpressionDepthTracker, Span};
pub use source::Source;

// Re-export semantic types for convenience (semantic is now at lib level)
pub use crate::semantic::*;

#[derive(Parser)]
#[grammar = "src/parsing/lemma.pest"]
pub struct LemmaParser;

pub fn parse(
    content: &str,
    filename: Option<String>,
    limits: &ResourceLimits,
) -> Result<Vec<LemmaDoc>, LemmaError> {
    // Check file size limit
    if content.len() > limits.max_file_size_bytes {
        return Err(LemmaError::ResourceLimitExceeded {
            limit_name: "max_file_size_bytes".to_string(),
            limit_value: format!(
                "{} bytes ({} MB)",
                limits.max_file_size_bytes,
                limits.max_file_size_bytes / (1024 * 1024)
            ),
            actual_value: format!(
                "{} bytes ({:.2} MB)",
                content.len(),
                content.len() as f64 / (1024.0 * 1024.0)
            ),
            suggestion: "Reduce file size or split into multiple documents".to_string(),
        });
    }

    let mut depth_tracker = ExpressionDepthTracker::with_max_depth(limits.max_expression_depth);
    let filename_str = filename.as_deref().unwrap_or("");

    match LemmaParser::parse(Rule::lemma_file, content) {
        Ok(pairs) => {
            let mut docs = Vec::new();
            for pair in pairs {
                if pair.as_rule() == Rule::lemma_file {
                    for inner_pair in pair.into_inner() {
                        if inner_pair.as_rule() == Rule::doc {
                            docs.push(parse_doc(
                                inner_pair,
                                filename_str,
                                content,
                                &mut depth_tracker,
                            )?);
                        }
                    }
                }
            }
            Ok(docs)
        }
        Err(e) => {
            let pest_span = match e.line_col {
                pest::error::LineColLocation::Pos((line, col)) => Span {
                    start: 0,
                    end: 0,
                    line,
                    col,
                },
                pest::error::LineColLocation::Span((start_line, start_col), (_, _)) => Span {
                    start: 0,
                    end: 0,
                    line: start_line,
                    col: start_col,
                },
            };

            Err(LemmaError::parse(
                format!("Parse error: {}", e.variant),
                pest_span,
                filename_str,
                Arc::from(content),
                "<parse-error>",
                1,
            ))
        }
    }
}

pub fn parse_facts(fact_strings: &[&str]) -> Result<Vec<LemmaFact>, LemmaError> {
    let mut facts = Vec::new();

    for fact_str in fact_strings {
        let fact_input = format!("fact {}", fact_str);
        let pairs = LemmaParser::parse(Rule::fact, &fact_input).map_err(|e| {
            LemmaError::Engine(format!("Failed to parse fact '{}': {}", fact_str, e))
        })?;

        let fact_pair = pairs.into_iter().next().ok_or_else(|| {
            LemmaError::Engine(format!("No parse result for fact '{}'", fact_str))
        })?;

        let inner_pair = fact_pair
            .into_inner()
            .next()
            .ok_or_else(|| LemmaError::Engine(format!("No inner rule for fact '{}'", fact_str)))?;

        let fact = match inner_pair.as_rule() {
            Rule::fact_definition => {
                crate::parsing::facts::parse_fact_definition(inner_pair, None, None)?
            }
            Rule::fact_override => {
                crate::parsing::facts::parse_fact_override(inner_pair, None, None)?
            }
            _ => {
                return Err(LemmaError::Engine(format!(
                    "Unexpected rule type for fact '{}'",
                    fact_str
                )))
            }
        };

        facts.push(fact);
    }

    Ok(facts)
}

fn parse_doc(
    pair: Pair<Rule>,
    filename: &str,
    _source: &str,
    depth_tracker: &mut ExpressionDepthTracker,
) -> Result<LemmaDoc, LemmaError> {
    let doc_start_line = pair.as_span().start_pos().line_col().0;

    let mut doc_name: Option<String> = None;
    let mut commentary: Option<String> = None;
    let mut facts = Vec::new();
    let mut rules = Vec::new();

    for inner_pair in pair.clone().into_inner() {
        if inner_pair.as_rule() == Rule::doc_declaration {
            for decl_inner in inner_pair.into_inner() {
                if decl_inner.as_rule() == Rule::doc_name {
                    doc_name = Some(parse_doc_name(decl_inner)?);
                    break;
                }
            }
        }
    }

    let name = doc_name.ok_or_else(|| {
        LemmaError::Engine("Grammar error: doc missing doc_declaration".to_string())
    })?;

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::commentary_content => {
                commentary = Some(inner_pair.as_str().trim().to_string());
            }
            Rule::fact_definition => {
                let fact = crate::parsing::facts::parse_fact_definition(
                    inner_pair,
                    Some(filename),
                    Some(&name),
                )?;
                facts.push(fact);
            }
            Rule::fact_override => {
                let fact = crate::parsing::facts::parse_fact_override(
                    inner_pair,
                    Some(filename),
                    Some(&name),
                )?;
                facts.push(fact);
            }
            Rule::rule_definition => {
                let rule = crate::parsing::rules::parse_rule_definition(
                    inner_pair,
                    depth_tracker,
                    filename,
                    &name,
                )?;
                rules.push(rule);
            }
            _ => {}
        }
    }
    let mut doc = LemmaDoc::new(name)
        .with_source_text(filename.to_string())
        .with_start_line(doc_start_line);

    if let Some(commentary_text) = commentary {
        doc = doc.set_commentary(commentary_text);
    }

    for fact in facts {
        doc = doc.add_fact(fact);
    }
    for rule in rules {
        doc = doc.add_rule(rule);
    }

    Ok(doc)
}

fn parse_doc_name(pair: Pair<Rule>) -> Result<String, LemmaError> {
    Ok(pair.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;

    #[test]
    fn test_empty_string() {
        let mut engine = Engine::new();
        let result = engine.add_lemma_code("", "test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_whitespace_only() {
        let mut engine = Engine::new();
        let result = engine.add_lemma_code("   \n\t  ", "test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_simple_document() {
        let input = r#"doc person
fact name = "John"
fact age = 25"#;
        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[0].facts.len(), 2);
    }

    #[test]
    fn test_parse_document_with_inheritance() {
        let input = r#"doc contracts/employment/jack
fact name = "Jack""#;
        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "contracts/employment/jack");
    }

    #[test]
    fn test_parse_document_with_commentary() {
        let input = r#"doc person
"""
This is a markdown comment
with **bold** text
"""
fact name = "John""#;
        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].commentary.is_some());
        assert!(result[0].commentary.as_ref().unwrap().contains("**bold**"));
    }

    #[test]
    fn test_parse_document_with_rule() {
        let input = r#"doc person
rule is_adult = age >= 18"#;
        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].name, "is_adult");
    }

    #[test]
    fn test_parse_multiple_documents() {
        let input = r#"doc person
fact name = "John"

doc company
fact name = "Acme Corp""#;
        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[1].name, "company");
    }

    #[test]
    fn test_parse_error_duplicate_fact_names() {
        let input = r#"doc person
fact name = "John"
fact name = "Jane""#;
        let result = parse(input, None, &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Parser should succeed even with duplicate facts"
        );
    }

    #[test]
    fn test_parse_error_duplicate_rule_names() {
        let input = r#"doc person
rule is_adult = age >= 18
rule is_adult = age >= 21"#;
        let result = parse(input, None, &crate::ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Parser should succeed even with duplicate rules"
        );
    }

    #[test]
    fn test_parse_error_malformed_input() {
        let input = "invalid syntax here";
        let result = parse(input, None, &crate::ResourceLimits::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_document_with_unless_clause() {
        let input = r#"doc person
rule is_active = service_started? and not service_ended?
unless maintenance_mode then false"#;
        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].unless_clauses.len(), 1);
    }

    #[test]
    fn test_parse_workspace_file() {
        let input = r#"doc person
fact name = "John Doe"
rule adult = true"#;
        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[0].facts.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].name, "adult");
    }

    #[test]
    fn test_multiple_unless_clauses() {
        let input = r#"doc test
rule is_eligible = age >= 18 and has_license
unless emergency_mode then true
unless system_override then accept"#;

        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].unless_clauses.len(), 2);
    }

    #[test]
    fn test_multiple_rules_in_document() {
        let input = r#"doc test
rule is_adult = age >= 18
rule is_senior = age >= 65
rule is_minor = age < 18
rule can_vote = age >= 18 and is_citizen"#;

        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 4);
        assert_eq!(result[0].rules[0].name, "is_adult");
        assert_eq!(result[0].rules[1].name, "is_senior");
        assert_eq!(result[0].rules[2].name, "is_minor");
        assert_eq!(result[0].rules[3].name, "can_vote");
    }

    #[test]
    fn test_mixing_facts_and_rules() {
        let input = r#"doc test
fact name = "John"
rule is_adult = age >= 18
fact age = 25
rule can_drink = age >= 21
fact status = "active"
rule is_eligible = is_adult and status == "active""#;

        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 3);
        assert_eq!(result[0].rules.len(), 3);
    }

    #[test]
    fn test_type_annotations_in_facts() {
        let input = r#"doc test
fact name = [text]
fact age = [number]
fact birth_date = [date]
fact is_active = [boolean]
fact pattern = [regex]
fact discount = [percentage]
fact weight = [weight]
fact height = [length]"#;

        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 8);
    }

    #[test]
    fn test_complex_unit_type_annotations() {
        let input = r#"doc test
fact volume = [volume]
fact duration = [duration]
fact temp = [temperature]
fact power = [power]
fact energy = [energy]
fact force = [force]
fact pressure = [pressure]
fact freq = [frequency]
fact data = [data_size]"#;

        let result = parse(input, None, &crate::ResourceLimits::default()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].facts.len(), 9);
    }

    #[test]
    fn test_whitespace_handling_comprehensive() {
        let test_cases = vec![
            ("doc test\nrule test = 2+3", "no spaces in arithmetic"),
            ("doc test\nrule test = age>=18", "no spaces in comparison"),
            (
                "doc test\nrule test = age >= 18 and salary>50000",
                "spaces around and keyword",
            ),
            (
                "doc test\nrule test = age  >=  18  and  salary  >  50000",
                "extra spaces",
            ),
            (
                "doc test\nrule test = \n  age >= 18 \n  and \n  salary > 50000",
                "newlines in expression",
            ),
        ];

        for (input, description) in test_cases {
            let result = parse(input, None, &crate::ResourceLimits::default());
            assert!(
                result.is_ok(),
                "Failed to parse {} ({}): {:?}",
                input,
                description,
                result.err()
            );
        }
    }

    #[test]
    fn test_veto_in_unless_clauses() {
        let input = r#"doc test
rule is_adult = age >= 18 unless age < 0 then veto "Age must be 0 or higher""#;
        let result = parse(
            input,
            Some("test.lemma".to_string()),
            &crate::ResourceLimits::default(),
        );
        assert!(
            result.is_ok(),
            "Failed to parse single veto: {:?}",
            result.err()
        );

        let docs = result.unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].rules.len(), 1);

        let rule = &docs[0].rules[0];
        assert_eq!(rule.name, "is_adult");
        assert_eq!(rule.unless_clauses.len(), 1);

        match &rule.unless_clauses[0].result.kind {
            crate::ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, Some("Age must be 0 or higher".to_string()));
            }
            _ => panic!(
                "Expected veto expression, got {:?}",
                rule.unless_clauses[0].result
            ),
        }

        let input = r#"doc test
rule is_adult = age >= 18
  unless age > 150 then veto "Age cannot be over 150"
  unless age < 0 then veto "Age must be 0 or higher""#;
        let result = parse(
            input,
            Some("test.lemma".to_string()),
            &crate::ResourceLimits::default(),
        );
        assert!(
            result.is_ok(),
            "Failed to parse multiple vetoes: {:?}",
            result.err()
        );

        let docs = result.unwrap();
        let rule = &docs[0].rules[0];
        assert_eq!(rule.unless_clauses.len(), 2);

        match &rule.unless_clauses[0].result.kind {
            crate::ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, Some("Age cannot be over 150".to_string()));
            }
            _ => panic!("Expected veto expression"),
        }

        match &rule.unless_clauses[1].result.kind {
            crate::ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, Some("Age must be 0 or higher".to_string()));
            }
            _ => panic!("Expected veto expression"),
        }
    }

    #[test]
    fn test_veto_without_message() {
        let input = r#"doc test
rule adult = age >= 18 unless age > 150 then veto"#;
        let result = parse(
            input,
            Some("test.lemma".to_string()),
            &crate::ResourceLimits::default(),
        );
        assert!(
            result.is_ok(),
            "Failed to parse veto without message: {:?}",
            result.err()
        );

        let docs = result.unwrap();
        let rule = &docs[0].rules[0];
        assert_eq!(rule.unless_clauses.len(), 1);

        match &rule.unless_clauses[0].result.kind {
            crate::ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, None);
            }
            _ => panic!("Expected veto expression"),
        }
    }

    #[test]
    fn test_mixed_veto_and_regular_unless() {
        let input = r#"doc test
rule adjusted_age = age + 1
  unless age < 0 then veto "Invalid age"
  unless age > 100 then 100"#;
        let result = parse(
            input,
            Some("test.lemma".to_string()),
            &crate::ResourceLimits::default(),
        );
        assert!(
            result.is_ok(),
            "Failed to parse mixed unless: {:?}",
            result.err()
        );

        let docs = result.unwrap();
        let rule = &docs[0].rules[0];
        assert_eq!(rule.unless_clauses.len(), 2);

        match &rule.unless_clauses[0].result.kind {
            crate::ExpressionKind::Veto(veto) => {
                assert_eq!(veto.message, Some("Invalid age".to_string()));
            }
            _ => panic!("Expected veto expression"),
        }

        match &rule.unless_clauses[1].result.kind {
            crate::ExpressionKind::Literal(crate::LiteralValue::Number(n)) => {
                assert_eq!(*n, rust_decimal::Decimal::new(100, 0));
            }
            _ => panic!("Expected literal number"),
        }
    }

    #[test]
    fn test_error_cases_comprehensive() {
        let error_cases = vec![
            (
                "doc test\nfact name = \"unclosed string",
                "unclosed string literal",
            ),
            ("doc test\nrule test = 2 + + 3", "double operator"),
            ("doc test\nrule test = (2 + 3", "unclosed parenthesis"),
            ("doc test\nrule test = 2 + 3)", "extra closing paren"),
            ("doc test\nrule test = 5 in invalidunit", "invalid unit"),
            ("doc test\nfact doc = 123", "reserved keyword as fact name"),
            (
                "doc test\nrule rule = true",
                "reserved keyword as rule name",
            ),
        ];

        for (input, description) in error_cases {
            let result = parse(
                input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
            assert!(
                result.is_err(),
                "Expected error for {} but got success",
                description
            );
        }
    }

    #[test]
    fn test_in_expressions_comprehensive() {
        let test_cases = vec![
            ("100 in meters", "length conversion"),
            ("5 in kilograms", "mass conversion"),
            ("2.5 in liters", "volume conversion"),
            ("3600 in seconds", "time conversion"),
            ("25 in celsius", "temperature conversion"),
            ("1000 in watts", "power conversion"),
            ("50 in newtons", "force conversion"),
            ("101325 in pascals", "pressure conversion"),
            ("1000 in joules", "energy conversion"),
            ("440 in hertz", "frequency conversion"),
            ("1024 in bytes", "data size conversion"),
            ("(100 + 50) in meters", "arithmetic with unit conversion"),
            ("(age * 365) in days", "complex arithmetic with conversion"),
            ("0 in meters", "zero with unit"),
            ("1 in meters", "one with unit"),
            ("-5 in celsius", "negative with unit"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
            assert!(
                result.is_ok(),
                "Failed to parse {} ({}): {:?}",
                expr,
                description,
                result.err()
            );
        }
    }

    #[test]
    fn test_all_unit_types_comprehensive() {
        let test_cases = vec![
            ("100 in liters", "liters"),
            ("50 in gallons", "gallons"),
            ("1000 in watts", "watts"),
            ("5 in kilowatts", "kilowatts"),
            ("2 in megawatts", "megawatts"),
            ("100 in horsepower", "horsepower"),
            ("50 in newtons", "newtons"),
            ("100 in kilonewtons", "kilonewtons"),
            ("75 in lbf", "pound-force"),
            ("101325 in pascals", "pascals"),
            ("100 in kilopascals", "kilopascals"),
            ("1 in megapascals", "megapascals"),
            ("1 in bar", "bar"),
            ("14.7 in psi", "psi"),
            ("1000 in joules", "joules"),
            ("5 in kilojoules", "kilojoules"),
            ("1 in megajoules", "megajoules"),
            ("1 in kilowatthour", "kilowatt-hour"),
            ("2000 in calorie", "calories"),
            ("500 in kilocalorie", "kilocalories"),
            ("440 in hertz", "hertz"),
            ("2.4 in gigahertz", "gigahertz"),
            ("100 in kilohertz", "kilohertz"),
            ("98.5 in megahertz", "megahertz"),
            ("1024 in bytes", "bytes"),
            ("1 in kilobytes", "kilobytes"),
            ("500 in megabytes", "megabytes"),
            ("100 in gigabytes", "gigabytes"),
            ("5 in terabytes", "terabytes"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
            assert!(
                result.is_ok(),
                "Failed to parse {} ({}): {:?}",
                expr,
                description,
                result.err()
            );
        }
    }

    #[test]
    fn test_unit_literals_in_rules() {
        let test_cases = vec![
            ("5 kilograms", "kilograms"),
            ("100 grams", "grams"),
            ("500 milligrams", "milligrams"),
            ("5 tons", "tons"),
            ("10 pounds", "pounds"),
            ("8 ounces", "ounces"),
            ("100 meters", "meters"),
            ("5 kilometers", "kilometers"),
            ("10 miles", "miles"),
            ("50 nautical_miles", "nautical miles"),
            ("25 decimeters", "decimeters"),
            ("180 centimeters", "centimeters"),
            ("50 millimeters", "millimeters"),
            ("10 yards", "yards"),
            ("6 feet", "feet"),
            ("72 inches", "inches"),
            ("5 cubic_meters", "cubic meters"),
            ("1000 cubic_centimeters", "cubic centimeters"),
            ("2.5 liters", "liters"),
            ("5 deciliters", "deciliters"),
            ("10 centiliters", "centiliters"),
            ("500 milliliters", "milliliters"),
            ("1 gallon", "gallons"),
            ("2 quarts", "quarts"),
            ("4 pints", "pints"),
            ("16 fluid_ounces", "fluid ounces"),
            ("-5 celsius", "celsius"),
            ("98.6 fahrenheit", "fahrenheit"),
            ("273 kelvin", "kelvin"),
            ("2 years", "years"),
            ("6 months", "months"),
            ("52 weeks", "weeks"),
            ("365 days", "days"),
            ("24 hours", "hours"),
            ("60 minutes", "minutes"),
            ("3600 seconds", "seconds"),
            ("1000 milliseconds", "milliseconds"),
            ("500000 microseconds", "microseconds"),
            ("1000 watts", "watts"),
            ("500 milliwatts", "milliwatts"),
            ("5 kilowatts", "kilowatts"),
            ("2 megawatts", "megawatts"),
            ("100 horsepower", "horsepower"),
            ("1000 joules", "joules"),
            ("5 kilojoules", "kilojoules"),
            ("2 megajoules", "megajoules"),
            ("1 kilowatthour", "kilowatt-hour"),
            ("500 watthours", "watt-hours"),
            ("2000 calories", "calories"),
            ("100 kilocalories", "kilocalories"),
            ("5000 btu", "BTU"),
            ("50 newtons", "newtons"),
            ("100 kilonewtons", "kilonewtons"),
            ("101325 pascals", "pascals"),
            ("100 kilopascals", "kilopascals"),
            ("5 megapascals", "megapascals"),
            ("1 atmosphere", "atmosphere"),
            ("1 bar", "bar"),
            ("14.7 psi", "psi"),
            ("760 torr", "torr"),
            ("760 mmhg", "mmHg"),
            ("440 hertz", "hertz"),
            ("2.4 gigahertz", "gigahertz"),
            ("1024 bytes", "bytes"),
            ("10 kilobytes", "kilobytes"),
            ("500 megabytes", "megabytes"),
            ("100 gigabytes", "gigabytes"),
            ("5 terabytes", "terabytes"),
            ("1 petabyte", "petabyte"),
            ("1024 kibibytes", "kibibytes"),
            ("512 mebibytes", "mebibytes"),
            ("8 gibibytes", "gibibytes"),
            ("2 tebibytes", "tebibytes"),
            ("50 percent", "percent"),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
            assert!(
                result.is_ok(),
                "Failed to parse unit literal {} ({}): {:?}",
                expr,
                description,
                result.err()
            );
        }
    }

    #[test]
    fn test_comparison_with_unit_conversions() {
        let test_cases = vec![
            (
                "(weight in kilograms) > 50",
                "unit conversion in comparison with parens",
            ),
            ("(height in meters) >= 1.8", "unit conversion with gte"),
            ("(distance in kilometers) < 100", "unit conversion with lt"),
            ("(temp in celsius) == 25", "unit conversion with equality"),
            (
                "(100 in meters) > (50 in feet)",
                "unit conversions on both sides",
            ),
            ("weight in kilograms > 50", "unit conversion without parens"),
            (
                "distance_km in miles > 50",
                "variable conversion in comparison",
            ),
            (
                "package_weight in pounds > weight_limit",
                "two variables with conversion",
            ),
            (
                "(x + 10 kilograms) in pounds > 50",
                "arithmetic with conversion in comparison",
            ),
            (
                "temp in fahrenheit >= 70 and temp in fahrenheit <= 90",
                "multiple comparisons",
            ),
        ];

        for (expr, description) in test_cases {
            let input = format!("doc test\nrule test = {}", expr);
            let result = parse(
                &input,
                Some("test.lemma".to_string()),
                &crate::ResourceLimits::default(),
            );
            assert!(
                result.is_ok(),
                "Failed to parse {} ({}): {:?}",
                expr,
                description,
                result.err()
            );
        }
    }
}
