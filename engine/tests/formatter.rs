use lemma::format_parse_result;
use lemma::{format_source, parse, ResourceLimits};

fn format_and_extract_rule_expr(source: &str) -> String {
    let formatted = format_source(source, lemma::SourceType::Volatile).unwrap();
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
        "Could not find 'rule x: ...' in formatted output: {}",
        formatted
    );
}

// =============================================================================
// Expression precedence tests
// =============================================================================

#[test]
fn precedence_add_inside_multiply_preserves_parens() {
    let src = "spec test data a: 1 data b: 2 data c: 3 rule x: (a + b) * c";
    assert_eq!(format_and_extract_rule_expr(src), "(a + b) * c");
}

#[test]
fn precedence_multiply_inside_add_omits_parens() {
    let src = "spec test data a: 1 data b: 2 data c: 3 rule x: a + b * c";
    assert_eq!(format_and_extract_rule_expr(src), "a + b * c");
}

#[test]
fn precedence_add_right_of_multiply_preserves_parens() {
    let src = "spec test data a: 1 data b: 2 data c: 3 rule x: a * (b + c)";
    assert_eq!(format_and_extract_rule_expr(src), "a * (b + c)");
}

#[test]
fn precedence_same_level_add_no_extra_parens() {
    let src = "spec test data a: 1 data b: 2 data c: 3 rule x: (a + b) + c";
    assert_eq!(format_and_extract_rule_expr(src), "a + b + c");
}

#[test]
fn precedence_same_level_multiply_no_extra_parens() {
    let src = "spec test data a: 1 data b: 2 data c: 3 rule x: (a * b) * c";
    assert_eq!(format_and_extract_rule_expr(src), "a * b * c");
}

#[test]
fn precedence_not_binds_tighter_than_and() {
    let src = "spec test data a: true data b: true rule x: not a and b";
    assert_eq!(format_and_extract_rule_expr(src), "not a and b");
}

#[test]
fn precedence_not_over_and_preserves_parens() {
    let src = "spec test data a: true data b: true rule x: not (a and b)";
    assert_eq!(format_and_extract_rule_expr(src), "not (a and b)");
}

#[test]
fn precedence_subtract_inside_multiply_preserves_parens() {
    let src = "spec test data a: 1 data b: 2 data c: 3 rule x: (a - b) * c";
    assert_eq!(format_and_extract_rule_expr(src), "(a - b) * c");
}

#[test]
fn precedence_multiply_inside_subtract_omits_parens() {
    let src = "spec test data a: 1 data b: 2 data c: 3 rule x: a - b * c";
    assert_eq!(format_and_extract_rule_expr(src), "a - b * c");
}

#[test]
fn precedence_nested_arithmetic_mixed() {
    let src = "spec test data a: 1 data b: 2 data c: 3 data d: 4 rule x: (a + b) * (c - d)";
    assert_eq!(format_and_extract_rule_expr(src), "(a + b) * (c - d)");
}

#[test]
fn precedence_comparison_lower_than_arithmetic() {
    let src = "spec test data a: 1 data b: 2 data c: 3 rule x: a + b > c";
    assert_eq!(format_and_extract_rule_expr(src), "a + b > c");
}

#[test]
fn precedence_deeply_nested() {
    let src = "spec test data a: 1 data b: 2 data c: 3 data d: 4 rule x: a + b * c + d";
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

/// Verify that formatting preserves the spec structure:
/// parse(source) and parse(format(source)) must have the same specs,
/// with the same names, data references, rule names, and unless-clause counts.
fn round_trip_example(filename: &str, source: &str) {
    let st = lemma::SourceType::Volatile;
    let formatted = format_source(source, st.clone())
        .unwrap_or_else(|e| panic!("[{}] format_source failed: {:?}", filename, e));

    let limits = ResourceLimits::default();
    let original_specs = parse(source, st.clone(), &limits)
        .unwrap_or_else(|e| panic!("[{}] initial parse failed: {:?}", filename, e))
        .into_flattened_specs();
    let reformatted_specs = parse(&formatted, st, &limits)
        .unwrap_or_else(|e| {
            panic!(
                "[{}] re-parse of formatted output failed: {:?} Formatted output: {}",
                filename, e, formatted
            )
        })
        .into_flattened_specs();

    assert_eq!(
        original_specs.len(),
        reformatted_specs.len(),
        "[{}] spec count mismatch after round-trip",
        filename
    );

    for (orig, refmt) in original_specs.iter().zip(reformatted_specs.iter()) {
        assert_eq!(orig.name, refmt.name, "[{}] spec name mismatch", filename);

        assert_eq!(
            orig.commentary, refmt.commentary,
            "[{}] spec '{}' commentary mismatch",
            filename, orig.name
        );

        assert_eq!(
            orig.data.len(),
            refmt.data.len(),
            "[{}] spec '{}' data count mismatch",
            filename,
            orig.name
        );

        let mut orig_data_refs: Vec<_> = orig.data.iter().map(|f| &f.reference).collect();
        let mut refmt_data_refs: Vec<_> = refmt.data.iter().map(|f| &f.reference).collect();
        orig_data_refs.sort_by_key(|r| r.to_string());
        refmt_data_refs.sort_by_key(|r| r.to_string());
        assert_eq!(
            orig_data_refs, refmt_data_refs,
            "[{}] spec '{}' data references mismatch",
            filename, orig.name
        );

        assert_eq!(
            orig.rules.len(),
            refmt.rules.len(),
            "[{}] spec '{}' rule count mismatch",
            filename,
            orig.name
        );

        for (orig_rule, refmt_rule) in orig.rules.iter().zip(refmt.rules.iter()) {
            assert_eq!(
                orig_rule.name, refmt_rule.name,
                "[{}] spec '{}' rule name mismatch",
                filename, orig.name
            );
            assert_eq!(
                orig_rule.expression, refmt_rule.expression,
                "[{}] spec '{}' rule '{}' expression mismatch",
                filename, orig.name, orig_rule.name
            );
            assert_eq!(
                orig_rule.unless_clauses.len(),
                refmt_rule.unless_clauses.len(),
                "[{}] spec '{}' rule '{}' unless-clause count mismatch",
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
                    "[{}] spec '{}' rule '{}' unless[{}] condition mismatch",
                    filename, orig.name, orig_rule.name, i
                );
                assert_eq!(
                    orig_uc.result, refmt_uc.result,
                    "[{}] spec '{}' rule '{}' unless[{}] result mismatch",
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
// Idempotency (synthetic expressions)
// =============================================================================

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
            "spec test data a: 1 data b: 2 data c: 3 data d: 4 rule x: {}",
            expr
        );
        let output1 = format_source(&src, lemma::SourceType::Volatile)
            .unwrap_or_else(|e| panic!("first format failed for '{}': {:?}", expr, e));
        let output2 = format_source(&output1, lemma::SourceType::Volatile).unwrap_or_else(|e| {
            panic!(
                "second format failed for '{}': {:?} First output: {}",
                expr, e, output1
            )
        });
        assert_eq!(
            output1, output2,
            "formatter is not idempotent for expression '{}'. First: {} Second: {}",
            expr, output1, output2
        );
    }
}

// =============================================================================
// `uses` / qualified-parent round-trip tests
// =============================================================================

fn import_with_data_lines(formatted: &str, spec_name: &str) -> Vec<String> {
    let marker = format!("spec {spec_name}\n");
    formatted
        .split("rule ")
        .next()
        .unwrap_or(formatted)
        .split(&marker)
        .nth(1)
        .unwrap_or("")
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

const GROUPED_USES_WITH_LINES: &[&str] =
    &["uses x", "with x.name: \"Ben\"", "uses y", "with y.age: 15"];

#[test]
fn format_groups_with_under_each_uses() {
    let source = r#"spec test

uses x
uses y

with x.name: "Ben"
with y.age: 15

rule r: 1
"#;
    let formatted = format_source(source, lemma::SourceType::Volatile).unwrap();
    let lines = import_with_data_lines(&formatted, "test");
    assert_eq!(
        lines, GROUPED_USES_WITH_LINES,
        "expected with under matching uses, got:\n{formatted}"
    );
    let reformatted = format_source(&formatted, lemma::SourceType::Volatile).unwrap();
    assert_eq!(
        formatted, reformatted,
        "grouped uses/with layout is not idempotent"
    );
}

#[test]
fn format_groups_with_under_each_uses_scrambled_source() {
    let source = r#"spec test

with x.name: "Ben"
with y.age: 15
uses x
uses y

rule r: 1
"#;
    let formatted = format_source(source, lemma::SourceType::Volatile).unwrap();
    let lines = import_with_data_lines(&formatted, "test");
    assert_eq!(
        lines, GROUPED_USES_WITH_LINES,
        "scrambled source must canonicalize to grouped layout, got:\n{formatted}"
    );
}

#[test]
fn round_trip_multiple_bare_uses_one_per_line() {
    let source = "spec consumer\nuses a\nuses b\nuses c";
    let formatted = format_source(source, lemma::SourceType::Volatile).unwrap();
    assert!(
        formatted.contains("uses a\n")
            && formatted.contains("uses b\n")
            && formatted.contains("uses c\n"),
        "expected one uses line per import, got: {}",
        formatted
    );
    assert!(
        !formatted.contains("uses a, b") && !formatted.contains(", b,"),
        "must not comma-join imports, got: {}",
        formatted
    );
    let reformatted = format_source(&formatted, lemma::SourceType::Volatile).unwrap();
    assert_eq!(
        formatted, reformatted,
        "multiple bare uses one per line is not idempotent"
    );
}

#[test]
fn round_trip_qualified_type_import_with_effective_on_uses() {
    let source = "spec consumer uses finance 2026-01-15 data money: finance.money data p: money";
    let formatted = format_source(source, lemma::SourceType::Volatile).unwrap();
    assert!(
        formatted.contains("finance.money") && formatted.contains("uses"),
        "expected uses + qualified parent type in formatted output: {}",
        formatted
    );
    let reformatted = format_source(&formatted, lemma::SourceType::Volatile).unwrap();
    assert_eq!(
        formatted, reformatted,
        "qualified type import with effective on uses is not idempotent"
    );
}

#[test]
fn round_trip_qualified_type_import_registry_with_effective_on_uses() {
    let source =
        "spec consumer uses @iso/countries alpha2 2026-01-15 data country: alpha2.code data c: country";
    let formatted = format_source(source, lemma::SourceType::Volatile).unwrap();
    assert!(
        formatted.contains("alpha2.code") && formatted.contains("@iso/countries"),
        "expected registry uses + qualified type in formatted output: {}",
        formatted
    );
    let reformatted = format_source(&formatted, lemma::SourceType::Volatile).unwrap();
    assert_eq!(
        formatted, reformatted,
        "registry qualified type import with effective is not idempotent"
    );
}

// =============================================================================
// `repo` declaration formatting
// =============================================================================

#[test]
fn format_repo_block_preserves_repository_header_in_output() {
    let src = "repo pack\n\nspec a\ndata x: 1";
    let parsed = parse(src, lemma::SourceType::Volatile, &ResourceLimits::default()).unwrap();
    let out = format_parse_result(&parsed);
    assert!(
        out.contains("repo pack"),
        "formatted output must retain repo header:\n{out}"
    );
}

#[test]
fn format_two_repo_blocks_emit_two_headers() {
    let src = "repo p1\n\nspec a\ndata x: 1\n\nrepo p2\n\nspec b\ndata y: 2";
    let parsed = parse(src, lemma::SourceType::Volatile, &ResourceLimits::default()).unwrap();
    let out = format_parse_result(&parsed);
    assert!(out.contains("repo p1") && out.contains("repo p2"), "{out}");
}

#[test]
fn format_repo_sections_idempotent_under_format_parse_result_roundtrip() {
    let src = "repo q\n\nspec z\ndata n: 7";
    let parsed = parse(src, lemma::SourceType::Volatile, &ResourceLimits::default()).unwrap();
    let once = format_parse_result(&parsed);
    let again = parse(
        &once,
        lemma::SourceType::Volatile,
        &ResourceLimits::default(),
    )
    .unwrap();
    let twice = format_parse_result(&again);
    assert_eq!(
        once, twice,
        "format_parse_result must be stable when reapplied"
    );
}

#[test]
fn format_compound_unit_metre_per_second_idempotent() {
    let source = r#"spec test
data velocity: quantity
  -> unit mps metre/second
"#;
    let st = lemma::SourceType::Volatile;
    let once = format_source(source, st.clone()).expect("format");
    assert!(
        once.contains("metre/second"),
        "formatted unit must use slash notation, got:\n{once}"
    );
    assert!(
        !once.contains("second^-1"),
        "must not format denominator as negative exponent, got:\n{once}"
    );
    let twice = format_source(&once, st).expect("reformat");
    assert_eq!(once, twice, "compound unit format must be idempotent");
}
