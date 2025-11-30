//! Integration tests for the parsing module
//!
//! Tests the parsing module end-to-end, including document parsing,
//! fact parsing, rule parsing, and error handling.

use lemma::{parse, ExpressionKind, LiteralValue};

#[test]
fn test_parse_simple_document() {
    let input = r#"doc person
fact name = "John"
fact age = 25"#;
    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].name, "person");
    assert_eq!(result[0].facts.len(), 2);
}

#[test]
fn test_parse_document_with_inheritance() {
    let input = r#"doc contracts/employment/jack
fact name = "Jack""#;
    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
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
    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].commentary.is_some());
    assert!(result[0].commentary.as_ref().unwrap().contains("**bold**"));
}

#[test]
fn test_parse_document_with_rule() {
    let input = r#"doc person
rule is_adult = age >= 18"#;
    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
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
    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].name, "person");
    assert_eq!(result[1].name, "company");
}

#[test]
fn test_parse_error_duplicate_fact_names() {
    let input = r#"doc person
fact name = "John"
fact name = "Jane""#;
    let result = parse(input, None, &lemma::ResourceLimits::default());
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
    let result = parse(input, None, &lemma::ResourceLimits::default());
    assert!(
        result.is_ok(),
        "Parser should succeed even with duplicate rules"
    );
}

#[test]
fn test_parse_error_malformed_input() {
    let input = "invalid syntax here";
    let result = parse(input, None, &lemma::ResourceLimits::default());
    assert!(result.is_err());
}

#[test]
fn test_parse_empty_input() {
    let input = "";
    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 0);
}

#[test]
fn test_parse_document_with_unless_clause() {
    let input = r#"doc person
rule is_active = service_started? and not service_ended?
unless maintenance_mode then false"#;
    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rules.len(), 1);
    assert_eq!(result[0].rules[0].unless_clauses.len(), 1);
}

#[test]
fn test_parse_workspace_file() {
    let input = r#"doc person
fact name = "John Doe"
rule adult = true"#;
    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
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

    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
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

    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
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

    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
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

    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
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

    let result = parse(input, None, &lemma::ResourceLimits::default()).unwrap();
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
        let result = parse(input, None, &lemma::ResourceLimits::default());
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
        &lemma::ResourceLimits::default(),
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
        ExpressionKind::Veto(veto) => {
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
        &lemma::ResourceLimits::default(),
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
        ExpressionKind::Veto(veto) => {
            assert_eq!(veto.message, Some("Age cannot be over 150".to_string()));
        }
        _ => panic!("Expected veto expression"),
    }

    match &rule.unless_clauses[1].result.kind {
        ExpressionKind::Veto(veto) => {
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
        &lemma::ResourceLimits::default(),
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
        ExpressionKind::Veto(veto) => {
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
        &lemma::ResourceLimits::default(),
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
        ExpressionKind::Veto(veto) => {
            assert_eq!(veto.message, Some("Invalid age".to_string()));
        }
        _ => panic!("Expected veto expression"),
    }

    match &rule.unless_clauses[1].result.kind {
        ExpressionKind::Literal(LiteralValue::Number(n)) => {
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
            &lemma::ResourceLimits::default(),
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
            &lemma::ResourceLimits::default(),
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
            &lemma::ResourceLimits::default(),
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
            &lemma::ResourceLimits::default(),
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
            &lemma::ResourceLimits::default(),
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
