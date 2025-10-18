use crate::parser::parse;

#[test]
fn test_literal_expressions_comprehensive() {
    let test_cases = vec![
        ("42", "integer"),
        ("3.14", "decimal"),
        ("0", "zero"),
        ("-5", "negative integer"),
        ("-3.14", "negative decimal"),
        ("1.0", "decimal with zero"),
        ("\"hello\"", "simple string"),
        ("\"\"", "empty string"),
        ("\"Hello, World!\"", "string with punctuation"),
        ("\"user@example.com\"", "string with special chars"),
        ("\"123\"", "numeric string"),
        ("true", "true boolean"),
        ("false", "false boolean"),
        ("yes", "yes boolean"),
        ("no", "no boolean"),
        ("0.0", "zero decimal"),
        ("-0", "negative zero"),
        ("\"true\"", "string true"),
        ("\"false\"", "string false"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
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
fn test_percentage_literals() {
    let test_cases = vec![
        ("5%", "simple percentage"),
        ("3.14%", "decimal percentage"),
        ("100%", "full percentage"),
        ("0.5%", "fractional percentage"),
        ("0%", "zero percentage"),
        ("99.99%", "precise percentage"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nfact discount = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} in fact ({}): {:?}",
            expr,
            description,
            result.err()
        );

        let input = format!("doc test\nrule discount = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} in rule ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}

#[test]
fn test_regex_literals() {
    let test_cases = vec![
        ("/[a-z]+/", "simple regex"),
        ("/\\d{3}-\\d{4}/", "phone pattern"),
        ("/^[A-Z]/", "starts with capital"),
        ("/test/", "literal text"),
        ("/[0-9]+\\.[0-9]+/", "decimal pattern"),
        ("/hello\\/world/", "escaped slash"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nfact pattern = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} in fact ({}): {:?}",
            expr,
            description,
            result.err()
        );

        let input = format!("doc test\nrule pattern = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} in rule ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}

#[test]
fn test_date_literals_comprehensive() {
    let test_cases = vec![
        ("2024-01-15", "simple date"),
        ("2024-12-31", "end of year"),
        ("2024-01-01", "start of year"),
        ("2024-02-29", "leap year date"),
        ("2024-01-15T14:30:00Z", "datetime with UTC"),
        ("2024-01-15T14:30:00+01:00", "datetime with timezone offset"),
        ("2024-01-15T14:30:00-05:00", "datetime with negative offset"),
        ("2024-01-15T00:00:00Z", "midnight UTC"),
        ("2024-01-15T23:59:59Z", "end of day"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nfact birth_date = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} in fact ({}): {:?}",
            expr,
            description,
            result.err()
        );

        let input = format!("doc test\nrule date_value = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} in rule ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}

#[test]
fn test_scientific_notation() {
    let test_cases = vec![
        ("1.23e+5", "positive exponent"),
        ("5.67E-3", "negative exponent uppercase"),
        ("1e10", "integer with exponent"),
        ("2.5e0", "zero exponent"),
        ("9.81e+2", "standard form"),
        ("1.602e-19", "very small number"),
        ("6.022e23", "large number"),
        ("-1.5e-10", "negative scientific"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
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
fn test_number_with_underscores() {
    let test_cases = vec![
        ("1_000", "thousands separator"),
        ("1_000_000", "millions"),
        ("123_456_789", "large number"),
        ("1_234.56", "decimal with underscore"),
        ("1_000_000_000", "billions"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
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
fn test_accept_reject_literals() {
    let test_cases = vec![
        ("accept", "accept literal"),
        ("reject", "reject literal"),
        ("accept and not reject", "accept with not reject"),
        ("is_valid == accept", "comparison with accept"),
        ("result != reject", "comparison with reject"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
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
fn test_edge_cases_comprehensive() {
    let test_cases = vec![
        ("2+3", "no spaces"),
        ("2 + 3", "normal spaces"),
        ("2  +  3", "extra spaces"),
        ("2\t+\t3", "tabs"),
        ("2\n+\n3", "newlines"),
        ("0", "zero"),
        ("1", "one"),
        ("true", "boolean true"),
        ("false", "boolean false"),
        ("\"\"", "empty string"),
        ("\"a\"", "single character string"),
        ("\"Hello, World!\"", "punctuation"),
        ("\"user@example.com\"", "email format"),
        ("\"123-456-7890\"", "phone format"),
        ("\"$1,234.56\"", "currency format"),
        ("\"café\"", "unicode string"),
        ("\"naïve\"", "unicode with diacritics"),
        ("\"测试\"", "non-latin characters"),
        ("-2147483648", "minimum 32-bit integer"),
        ("2147483647", "maximum 32-bit integer"),
        ("0.000001", "very small decimal"),
        ("999999.999999", "large decimal"),
    ];

    for (expr, description) in test_cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {} ({}): {:?}",
            expr,
            description,
            result.err()
        );
    }
}
