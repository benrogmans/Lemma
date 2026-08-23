//! Round-trip format tests against shipped documentation examples.

use lemma::{format_source, parse, ResourceLimits};

const EXAMPLE_FILES: &[(&str, &str)] = &[
    (
        "01_coffee_order.lemma",
        include_str!("../../../engine/documentation/examples/01_coffee_order.lemma"),
    ),
    (
        "02_library_fees.lemma",
        include_str!("../../../engine/documentation/examples/02_library_fees.lemma"),
    ),
    (
        "03_recipe_scaling.lemma",
        include_str!("../../../engine/documentation/examples/03_recipe_scaling.lemma"),
    ),
    (
        "04_membership_benefits.lemma",
        include_str!("../../../engine/documentation/examples/04_membership_benefits.lemma"),
    ),
    (
        "05_weather_clothing.lemma",
        include_str!("../../../engine/documentation/examples/05_weather_clothing.lemma"),
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
