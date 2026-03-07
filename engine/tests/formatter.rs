use lemma::{format_source, parse, ResourceLimits};

fn format_and_extract_rule_expr(source: &str) -> String {
    let formatted = format_source(source, "test.lemma").unwrap();
    let lines: Vec<&str> = formatted.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("rule x: ") {
            return rest.to_string();
        }
        if trimmed == "rule x:" {
            let next = lines.get(i + 1).map(|s| s.trim()).unwrap_or("");
            if !next.is_empty() {
                return next.to_string();
            }
        }
    }
    panic!(
        "Could not find 'rule x: ...' in formatted output:\n{}",
        formatted
    );
}

// =============================================================================
// Expression precedence tests
// =============================================================================

#[test]
fn precedence_add_inside_multiply_preserves_parens() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nrule x: (a + b) * c";
    assert_eq!(format_and_extract_rule_expr(src), "(a + b) * c");
}

#[test]
fn precedence_multiply_inside_add_omits_parens() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nrule x: a + b * c";
    assert_eq!(format_and_extract_rule_expr(src), "a + b * c");
}

#[test]
fn precedence_add_right_of_multiply_preserves_parens() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nrule x: a * (b + c)";
    assert_eq!(format_and_extract_rule_expr(src), "a * (b + c)");
}

#[test]
fn precedence_same_level_add_no_extra_parens() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nrule x: (a + b) + c";
    assert_eq!(format_and_extract_rule_expr(src), "a + b + c");
}

#[test]
fn precedence_same_level_multiply_no_extra_parens() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nrule x: (a * b) * c";
    assert_eq!(format_and_extract_rule_expr(src), "a * b * c");
}

#[test]
fn precedence_not_binds_tighter_than_and() {
    let src = "doc test\nfact a: true\nfact b: true\nrule x: not a and b";
    assert_eq!(format_and_extract_rule_expr(src), "not a and b");
}

#[test]
fn precedence_not_over_and_preserves_parens() {
    let src = "doc test\nfact a: true\nfact b: true\nrule x: not (a and b)";
    assert_eq!(format_and_extract_rule_expr(src), "not (a and b)");
}

#[test]
fn precedence_subtract_inside_multiply_preserves_parens() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nrule x: (a - b) * c";
    assert_eq!(format_and_extract_rule_expr(src), "(a - b) * c");
}

#[test]
fn precedence_multiply_inside_subtract_omits_parens() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nrule x: a - b * c";
    assert_eq!(format_and_extract_rule_expr(src), "a - b * c");
}

#[test]
fn precedence_nested_arithmetic_mixed() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nfact d: 4\nrule x: (a + b) * (c - d)";
    assert_eq!(format_and_extract_rule_expr(src), "(a + b) * (c - d)");
}

#[test]
fn precedence_comparison_lower_than_arithmetic() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nrule x: a + b > c";
    assert_eq!(format_and_extract_rule_expr(src), "a + b > c");
}

#[test]
fn precedence_deeply_nested() {
    let src = "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nfact d: 4\nrule x: a + b * c + d";
    assert_eq!(format_and_extract_rule_expr(src), "a + b * c + d");
}

// =============================================================================
// Round-trip tests against example .lemma files
// =============================================================================

const EXAMPLE_FILES: &[(&str, &str)] = &[
    (
        "01_coffee_order.lemma",
        include_str!("../../documentation/examples/01_coffee_order.lemma"),
    ),
    (
        "02_library_fees.lemma",
        include_str!("../../documentation/examples/02_library_fees.lemma"),
    ),
    (
        "03_recipe_scaling.lemma",
        include_str!("../../documentation/examples/03_recipe_scaling.lemma"),
    ),
    (
        "04_membership_benefits.lemma",
        include_str!("../../documentation/examples/04_membership_benefits.lemma"),
    ),
    (
        "05_weather_clothing.lemma",
        include_str!("../../documentation/examples/05_weather_clothing.lemma"),
    ),
];

/// Verify that formatting preserves the document structure:
/// parse(source) and parse(format(source)) must have the same documents,
/// with the same names, fact references, rule names, and unless-clause counts.
fn round_trip_example(filename: &str, source: &str) {
    let formatted = format_source(source, filename)
        .unwrap_or_else(|e| panic!("[{}] format_source failed: {:?}", filename, e));

    let limits = ResourceLimits::default();
    let original_docs = parse(source, filename, &limits)
        .unwrap_or_else(|e| panic!("[{}] initial parse failed: {:?}", filename, e));
    let reformatted_docs = parse(&formatted, filename, &limits).unwrap_or_else(|e| {
        panic!(
            "[{}] re-parse of formatted output failed: {:?}\nFormatted output:\n{}",
            filename, e, formatted
        )
    });

    assert_eq!(
        original_docs.len(),
        reformatted_docs.len(),
        "[{}] document count mismatch after round-trip",
        filename
    );

    for (orig, refmt) in original_docs.iter().zip(reformatted_docs.iter()) {
        assert_eq!(
            orig.name, refmt.name,
            "[{}] document name mismatch",
            filename
        );

        assert_eq!(
            orig.commentary, refmt.commentary,
            "[{}] doc '{}' commentary mismatch",
            filename, orig.name
        );

        assert_eq!(
            orig.types.len(),
            refmt.types.len(),
            "[{}] doc '{}' type count mismatch",
            filename,
            orig.name
        );

        assert_eq!(
            orig.facts.len(),
            refmt.facts.len(),
            "[{}] doc '{}' fact count mismatch",
            filename,
            orig.name
        );

        let orig_fact_refs: Vec<_> = orig.facts.iter().map(|f| &f.reference).collect();
        let refmt_fact_refs: Vec<_> = refmt.facts.iter().map(|f| &f.reference).collect();
        assert_eq!(
            orig_fact_refs, refmt_fact_refs,
            "[{}] doc '{}' fact references mismatch",
            filename, orig.name
        );

        assert_eq!(
            orig.rules.len(),
            refmt.rules.len(),
            "[{}] doc '{}' rule count mismatch",
            filename,
            orig.name
        );

        for (orig_rule, refmt_rule) in orig.rules.iter().zip(refmt.rules.iter()) {
            assert_eq!(
                orig_rule.name, refmt_rule.name,
                "[{}] doc '{}' rule name mismatch",
                filename, orig.name
            );
            assert_eq!(
                orig_rule.expression, refmt_rule.expression,
                "[{}] doc '{}' rule '{}' expression mismatch",
                filename, orig.name, orig_rule.name
            );
            assert_eq!(
                orig_rule.unless_clauses.len(),
                refmt_rule.unless_clauses.len(),
                "[{}] doc '{}' rule '{}' unless-clause count mismatch",
                filename,
                orig.name,
                orig_rule.name
            );
            for (i, (orig_uc, refmt_uc)) in orig_rule
                .unless_clauses
                .iter()
                .zip(refmt_rule.unless_clauses.iter())
                .enumerate()
            {
                assert_eq!(
                    orig_uc.condition, refmt_uc.condition,
                    "[{}] doc '{}' rule '{}' unless[{}] condition mismatch",
                    filename, orig.name, orig_rule.name, i
                );
                assert_eq!(
                    orig_uc.result, refmt_uc.result,
                    "[{}] doc '{}' rule '{}' unless[{}] result mismatch",
                    filename, orig.name, orig_rule.name, i
                );
            }
        }
    }
}

#[test]
fn round_trip_01_coffee_order() {
    round_trip_example(EXAMPLE_FILES[0].0, EXAMPLE_FILES[0].1);
}

#[test]
fn round_trip_02_library_fees() {
    round_trip_example(EXAMPLE_FILES[1].0, EXAMPLE_FILES[1].1);
}

#[test]
fn round_trip_03_recipe_scaling() {
    round_trip_example(EXAMPLE_FILES[2].0, EXAMPLE_FILES[2].1);
}

#[test]
fn round_trip_04_membership_benefits() {
    round_trip_example(EXAMPLE_FILES[3].0, EXAMPLE_FILES[3].1);
}

#[test]
fn round_trip_05_weather_clothing() {
    round_trip_example(EXAMPLE_FILES[4].0, EXAMPLE_FILES[4].1);
}

// =============================================================================
// Idempotency tests
// =============================================================================

fn idempotency_example(filename: &str, source: &str) {
    let output1 = format_source(source, filename)
        .unwrap_or_else(|e| panic!("[{}] first format failed: {:?}", filename, e));
    let output2 = format_source(&output1, filename).unwrap_or_else(|e| {
        panic!(
            "[{}] second format failed: {:?}\nFirst format output:\n{}",
            filename, e, output1
        )
    });

    assert_eq!(
        output1, output2,
        "[{}] formatter is not idempotent.\nFirst format:\n{}\nSecond format:\n{}",
        filename, output1, output2
    );
}

#[test]
fn idempotency_01_coffee_order() {
    idempotency_example(EXAMPLE_FILES[0].0, EXAMPLE_FILES[0].1);
}

#[test]
fn idempotency_02_library_fees() {
    idempotency_example(EXAMPLE_FILES[1].0, EXAMPLE_FILES[1].1);
}

#[test]
fn idempotency_03_recipe_scaling() {
    idempotency_example(EXAMPLE_FILES[2].0, EXAMPLE_FILES[2].1);
}

#[test]
fn idempotency_04_membership_benefits() {
    idempotency_example(EXAMPLE_FILES[3].0, EXAMPLE_FILES[3].1);
}

#[test]
fn idempotency_05_weather_clothing() {
    idempotency_example(EXAMPLE_FILES[4].0, EXAMPLE_FILES[4].1);
}

#[test]
fn idempotency_precedence_expressions() {
    let expressions = [
        "(a + b) * c",
        "a + b * c",
        "a * (b + c)",
        "(a + b) + c",
        "not a and b",
        "not (a and b)",
        "(a + b) * (c - d)",
    ];
    for expr in expressions {
        let src = format!(
            "doc test\nfact a: 1\nfact b: 2\nfact c: 3\nfact d: 4\nrule x: {}",
            expr
        );
        let output1 = format_source(&src, "test.lemma")
            .unwrap_or_else(|e| panic!("first format failed for '{}': {:?}", expr, e));
        let output2 = format_source(&output1, "test.lemma").unwrap_or_else(|e| {
            panic!(
                "second format failed for '{}':\n{:?}\nFirst output:\n{}",
                expr, e, output1
            )
        });
        assert_eq!(
            output1, output2,
            "formatter is not idempotent for expression '{}'.\nFirst:\n{}\nSecond:\n{}",
            expr, output1, output2
        );
    }
}
