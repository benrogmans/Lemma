use crate::parser::parse;

#[test]
fn test_simple_number() {
    let input = r#"doc test
rule number = 42"#;
    let result = parse(input, None);
    assert!(
        result.is_ok(),
        "Failed to parse simple number: {:?}",
        result.err()
    );
}

#[test]
fn test_fact_reference_parsing() {
    let input = r#"doc test
rule simple_ref = age"#;
    let result = parse(input, None);
    assert!(
        result.is_ok(),
        "Failed to parse fact reference: {:?}",
        result.err()
    );

    let input = r#"doc test
rule nested_ref = employee.salary"#;
    let result = parse(input, None);
    assert!(
        result.is_ok(),
        "Failed to parse nested fact reference: {:?}",
        result.err()
    );
}

#[test]
fn test_arithmetic_operations_work() {
    let cases = vec![
        "2 + 3", "2+1", "5 * 6", "5* 6", "7 % 3", "3%2", "2 ^ 3", "2^3",
    ];
    for expr in cases {
        let input = format!("doc test\nrule test = {}", expr);
        let result = parse(&input, None);
        assert!(
            result.is_ok(),
            "Failed to parse {}: {:?}",
            expr,
            result.err()
        );
    }
}

#[test]
fn test_arithmetic_expressions_comprehensive() {
    let test_cases = vec![
        ("2 + 3", "addition"),
        ("10 - 4", "subtraction"),
        ("6 * 7", "multiplication"),
        ("15 / 3", "division"),
        ("17 % 5", "modulo"),
        ("2 ^ 8", "exponentiation"),
        ("2 + 3 * 4", "operator precedence"),
        ("(2 + 3) * 4", "parentheses"),
        ("2 * 3 + 4 * 5", "multiple operations"),
        ("(2 + 3) * (4 + 5)", "nested parentheses"),
        ("-5", "unary minus"),
        ("+10", "unary plus"),
        ("-(2 + 3)", "unary minus with parentheses"),
        ("+(-5)", "nested unary operators"),
        ("age + 5", "variable addition"),
        ("salary * 1.1", "variable multiplication"),
        ("-age", "unary minus on variable"),
        ("0", "zero"),
        ("1", "one"),
        ("-0", "negative zero"),
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
fn test_comparison_expressions_comprehensive() {
    let test_cases = vec![
        ("age > 18", "greater than"),
        ("age < 65", "less than"),
        ("age >= 18", "greater than or equal"),
        ("age <= 65", "less than or equal"),
        ("age == 25", "equality"),
        ("age != 30", "inequality"),
        ("name == \"John\"", "string equality"),
        ("name != \"Jane\"", "string inequality"),
        ("status == \"active\"", "status comparison"),
        ("is_active == true", "boolean equality"),
        ("is_active != false", "boolean inequality"),
        ("is_active is true", "is operator"),
        ("is_active is not false", "is not operator"),
        ("age >= 18 and age <= 65", "range check"),
        (
            "salary > 50000 and status == \"active\"",
            "multiple conditions",
        ),
        ("(age + 5) > 21", "arithmetic in comparison"),
        ("age == 0", "zero comparison"),
        ("name == \"\"", "empty string"),
        ("is_active == false", "false comparison"),
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
fn test_logical_expressions_comprehensive() {
    let test_cases = vec![
        ("is_active and is_verified", "simple and"),
        ("is_student or is_employee", "simple or"),
        ("not is_blocked", "simple not"),
        ("is_active and not is_blocked", "and with not"),
        (
            "(is_student or is_employee) and is_verified",
            "parentheses with and/or",
        ),
        ("not (is_blocked or is_suspended)", "not with parentheses"),
        ("have license", "have operator"),
        ("have not license", "have not operator"),
        ("not have license", "not have operator"),
        ("sqrt(16)", "square root"),
        ("sin(0)", "sine function"),
        ("cos(0)", "cosine function"),
        ("tan(0)", "tangent function"),
        ("log(10)", "logarithm"),
        ("exp(1)", "exponential"),
        (
            "service_started? and not service_ended?",
            "fact references with logical ops",
        ),
        (
            "age >= 18 and (have license or is_employee)",
            "comparison with logical ops",
        ),
        (
            "sqrt(age * age + salary * salary) > 1000",
            "math function with arithmetic",
        ),
        ("true and false", "boolean literals"),
        ("not true", "not with boolean"),
        ("have age", "have with fact reference"),
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
fn test_fact_reference_expressions_comprehensive() {
    let test_cases = vec![
        ("age", "simple fact"),
        ("name", "string fact"),
        ("is_active", "boolean fact"),
        ("salary", "numeric fact"),
        ("service_started?", "fact with question mark"),
        ("has_license?", "has fact with question mark"),
        ("is_verified?", "is fact with question mark"),
        ("employee.salary", "nested fact reference"),
        ("person.address.street", "deeply nested fact"),
        ("company.employee.name", "multiple levels"),
        ("user.profile.settings.theme", "deep nesting"),
        ("order.customer.address.zip_code", "real-world example"),
        ("a", "single character"),
        ("very_long_fact_name_with_underscores", "long name"),
        ("fact123", "fact with numbers"),
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
fn test_nested_expressions_comprehensive() {
    let test_cases = vec![
        ("(2 + 3) * (4 + 5)", "nested arithmetic"),
        ("((2 + 3) * 4) + 5", "deeply nested arithmetic"),
        ("2 * (3 + (4 * 5))", "mixed nesting"),
        ("(age + 5) > (salary / 12)", "arithmetic in comparison"),
        ("((age >= 18) and (age <= 65))", "nested comparisons"),
        (
            "(is_active and is_verified) or (is_admin and is_trusted)",
            "nested logical",
        ),
        (
            "not (is_blocked or (is_suspended and not is_appealed))",
            "complex nested logical",
        ),
        (
            "(age >= 18) and ((salary > 50000) or (have degree))",
            "comparison and logical nesting",
        ),
        (
            "sqrt((x * x) + (y * y)) > 100",
            "math function with nested arithmetic",
        ),
        (
            "(service_started? and not service_ended?) or (is_manual and is_verified)",
            "fact refs with nesting",
        ),
        ("((((5))))", "deeply nested parentheses"),
        ("(true)", "boolean in parentheses"),
        ("(\"hello\")", "string in parentheses"),
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
fn test_operator_precedence_comprehensive() {
    let test_cases = vec![
        ("2 + 3 * 4", "multiplication before addition"),
        ("2 * 3 + 4 * 5", "multiple operations"),
        ("2 ^ 3 * 4", "exponentiation before multiplication"),
        ("2 * 3 ^ 4", "exponentiation after multiplication"),
        ("2 + 3 * 4 ^ 5", "all arithmetic operators"),
        ("true and false or true", "and before or"),
        ("not true and false", "not before and"),
        ("true or false and true", "and before or"),
        (
            "age >= 18 and salary > 50000 or have degree",
            "comparison and logical",
        ),
        ("2 + 3 > 4 and 5 * 6 < 40", "arithmetic and comparison"),
        ("(2 + 3) * 4", "parentheses override arithmetic"),
        ("true and (false or true)", "parentheses override logical"),
        (
            "(age >= 18) and (salary > 50000)",
            "parentheses in comparisons",
        ),
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
fn test_parenthesized_expression_edge_cases() {
    let test_cases = vec![
        ("(32 / 7) + 67", "division then addition"),
        ("(2 + 3) * (4 - 1)", "multiple paren groups"),
        ("(10 - 5) / 2 + 3", "paren then mixed ops"),
        ("5 + (3 * 2) - 1", "paren in middle"),
        ("(32 / 7) in kilograms", "paren with unit conversion"),
        ("(100 + 50) in meters", "addition with unit"),
        ("(temperature - 32) * 5 / 9 in celsius", "complex with unit"),
        ("(a + b) > (c + d)", "paren on both sides of comparison"),
        ("(salary * 12) >= 60000", "paren in comparison"),
        ("(x in meters) > 100", "unit conversion in comparison"),
        ("((((5))))", "deeply nested value"),
        ("(((2 + 3) * 4) - 1)", "deeply nested operations"),
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
fn test_rule_references_comprehensive() {
    let test_cases = vec![
        ("is_adult?", "simple rule reference"),
        ("service_started?", "service rule reference"),
        ("is_valid? and is_active?", "multiple rule references"),
        ("not is_blocked?", "not with rule reference"),
        ("have license and is_verified?", "have with rule reference"),
        ("is_employee? or is_contractor?", "or with rule references"),
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
fn test_complex_real_world_expressions() {
    let test_cases = vec![
        ("age >= 18 and (have license or is_employee)", "age verification with alternatives"),
        ("salary > 50000 and status == \"active\" and not is_on_probation", "employee eligibility"),
        ("(order_total > 100) and (payment_status == \"completed\") and (shipping_address != \"\")", "order validation"),
        ("(cpu_usage < 80) and (memory_usage < 90) and (disk_space > 1024)", "system health check"),
        ("(response_time < 500) and (error_rate < 0.01) and (uptime > 0.99)", "service monitoring"),
        ("sqrt((x - center_x)^2 + (y - center_y)^2) <= radius", "point in circle"),
        ("(a^2 + b^2) == c^2", "Pythagorean theorem check"),
        ("(temperature - 32) * 5 / 9 in celsius", "Fahrenheit to Celsius"),
        ("((user.age >= 18) and (user.verified == true)) or ((user.is_employee == true) and (user.manager_approved == true))", "access control"),
        ("(order.items_count > 0) and ((order.total > 50) or (order.customer.is_vip == true)) and (order.payment.method != \"pending\")", "order processing"),
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
