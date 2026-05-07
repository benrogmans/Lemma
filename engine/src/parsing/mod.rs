use crate::error::Error;
use crate::limits::ResourceLimits;

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod source;

pub use ast::{DepthTracker, Span};
pub use source::Source;

pub use ast::*;
pub use parser::ParseResult;

pub fn parse(
    content: &str,
    source_type: source::SourceType,
    limits: &ResourceLimits,
) -> Result<ParseResult, Error> {
    parser::parse(content, source_type, limits)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::formatting::format_parse_result;
    use crate::Error;
    use crate::ResourceLimits;

    #[test]
    fn parse_empty_input_returns_no_specs() {
        let result = parse(
            "",
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn parse_workspace_file_yields_expected_spec_datas_and_rules() {
        let input = r#"spec person
data name: "John Doe"
rule adult: true"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[0].data.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].name, "adult");
    }

    #[test]
    fn mixing_data_and_rules_is_collected_into_spec() {
        let input = r#"spec test
data name: "John"
rule is_adult: age >= 18
data age: 25
rule can_drink: age >= 21
data status: "active"
rule is_eligible: is_adult and status is "active""#;

        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].data.len(), 3);
        assert_eq!(result[0].rules.len(), 3);
    }

    #[test]
    fn parse_simple_spec_collects_data() {
        let input = r#"spec person
data name: "John"
data age: 25"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[0].data.len(), 2);
    }

    #[test]
    fn parse_dotted_spec_name() {
        let input = r#"spec contracts.employment.jack
data name: "Jack""#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "contracts.employment.jack");
    }

    #[test]
    fn parse_slashed_spec_name() {
        let input = "spec contracts/employment/jack\ndata x: 1";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "contracts/employment/jack");
    }

    #[test]
    fn parse_spec_name_no_version_tag() {
        let input = "spec myspec\nrule x: 1";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "myspec");
        assert_eq!(result[0].effective_from(), None);
    }

    #[test]
    fn parse_commentary_block_is_attached_to_spec() {
        let input = r#"spec person
"""
This is a markdown comment
uses **bold** text
"""
data name: "John""#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 1);
        assert!(result[0].commentary.is_some());
        assert!(result[0].commentary.as_ref().unwrap().contains("**bold**"));
    }

    #[test]
    fn parse_spec_with_rule_collects_rule() {
        let input = r#"spec person
rule is_adult: age >= 18"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rules.len(), 1);
        assert_eq!(result[0].rules[0].name, "is_adult");
    }

    #[test]
    fn parse_multiple_specs_returns_all_specs() {
        let input = r#"spec person
data name: "John"

spec company
data name: "Acme Corp""#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[1].name, "company");
    }

    #[test]
    fn parse_allows_duplicate_data_names() {
        let input = r#"spec person
data name: "John"
data name: "Jane""#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        );
        assert!(
            result.is_ok(),
            "Parser should succeed even with duplicate data"
        );
    }

    #[test]
    fn parse_allows_duplicate_rule_names() {
        let input = r#"spec person
rule is_adult: age >= 18
rule is_adult: age >= 21"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        );
        assert!(
            result.is_ok(),
            "Parser should succeed even with duplicate rules"
        );
    }

    #[test]
    fn parse_rejects_malformed_input() {
        let input = "invalid syntax here";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn parse_handles_whitespace_variants_in_expressions() {
        let test_cases = vec![
            ("spec test\nrule test: 2+3", "no spaces in arithmetic"),
            ("spec test\nrule test: age>=18", "no spaces in comparison"),
            (
                "spec test\nrule test: age >= 18 and salary>50000",
                "spaces around and keyword",
            ),
            (
                "spec test\nrule test: age  >=  18  and  salary  >  50000",
                "extra spaces",
            ),
            (
                "spec test\nrule test: \n  age >= 18 \n  and \n  salary > 50000",
                "newlines in expression",
            ),
        ];

        for (input, description) in test_cases {
            let result = parse(
                input,
                crate::parsing::source::SourceType::Volatile,
                &ResourceLimits::default(),
            );
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
    fn parse_error_cases_are_rejected() {
        let error_cases = vec![
            (
                "spec test\ndata name: \"unclosed string",
                "unclosed string literal",
            ),
            ("spec test\nrule test: (2 + 3", "unclosed parenthesis"),
            ("spec test\nrule test: 2 + 3)", "extra closing paren"),
            ("spec test\ndata spec: 123", "reserved keyword as data name"),
            (
                "spec test\nrule rule: true",
                "reserved keyword as rule name",
            ),
        ];

        for (input, description) in error_cases {
            let result = parse(
                input,
                crate::parsing::source::SourceType::Volatile,
                &ResourceLimits::default(),
            );
            assert!(
                result.is_err(),
                "Expected error for {} but got success",
                description
            );
        }
    }

    #[test]
    fn parse_duration_literals_in_rules() {
        let test_cases = vec![
            ("2 years", "years"),
            ("6 months", "months"),
            ("52 weeks", "weeks"),
            ("365 days", "days"),
            ("24 hours", "hours"),
            ("60 minutes", "minutes"),
            ("3600 seconds", "seconds"),
            ("1000 milliseconds", "milliseconds"),
            ("500000 microseconds", "microseconds"),
            ("50 percent", "percent"),
        ];

        for (expr, description) in test_cases {
            let input = format!("spec test\nrule test: {}", expr);
            let result = parse(
                &input,
                crate::parsing::source::SourceType::Volatile,
                &ResourceLimits::default(),
            );
            assert!(
                result.is_ok(),
                "Failed to parse literal {} ({}): {:?}",
                expr,
                description,
                result.err()
            );
        }
    }

    #[test]
    fn parse_comparisons_with_duration_unit_conversions() {
        let test_cases = vec![
            (
                "(duration in hours) > 2",
                "duration conversion in comparison with parens",
            ),
            (
                "(meeting_time in minutes) >= 30",
                "duration conversion with gte",
            ),
            (
                "(project_length in days) < 100",
                "duration conversion with lt",
            ),
            (
                "(delay in seconds) is 60",
                "duration conversion with equality",
            ),
            (
                "(1 hours) > (30 minutes)",
                "duration conversions on both sides",
            ),
            (
                "duration in hours > 2",
                "duration conversion without parens",
            ),
            (
                "meeting_time in seconds > 3600",
                "variable duration conversion in comparison",
            ),
            (
                "project_length in days > deadline_days",
                "two variables with duration conversion",
            ),
            (
                "duration in hours >= 1 and duration in hours <= 8",
                "multiple duration comparisons",
            ),
        ];

        for (expr, description) in test_cases {
            let input = format!("spec test\nrule test: {}", expr);
            let result = parse(
                &input,
                crate::parsing::source::SourceType::Volatile,
                &ResourceLimits::default(),
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
    fn parse_error_includes_attribute_and_parse_error_spec_name() {
        let result = parse(
            r#"
spec test
data name: "Unclosed string
data age: 25
"#,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        );

        match result {
            Err(Error::Parsing(details)) => {
                let src = details
                    .source
                    .as_ref()
                    .expect("BUG: parsing errors always have source");
                assert_eq!(
                    src.source_type,
                    crate::parsing::source::SourceType::Volatile
                );
            }
            Err(e) => panic!("Expected Parse error, got: {e:?}"),
            Ok(_) => panic!("Expected parse error for unclosed string"),
        }
    }

    #[test]
    fn parse_single_spec_file() {
        let input = r#"spec somespec
data name: "Alice""#;
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let specs = parsed.flatten_specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "somespec");
    }

    #[test]
    fn parse_uses_registry_spec_explicit_alias() {
        let input = r#"spec example
uses external: @user/workspace somespec"#;
        let specs = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].data.len(), 1);
        match &specs[0].data[0].value {
            crate::parsing::ast::DataValue::Import(spec_ref) => {
                assert_eq!(spec_ref.name, "somespec");
                let repository_hdr = spec_ref
                    .repository
                    .as_ref()
                    .expect("expected repository qualifier");
                assert_eq!(repository_hdr.name, "@user/workspace");
            }
            other => panic!("Expected Import, got: {:?}", other),
        }
    }

    #[test]
    fn parse_multiple_specs_cross_reference_in_file() {
        let input = r#"spec spec_a
data x: 10

spec spec_b
data y: 20
uses a: spec_a"#;
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let specs = parsed.flatten_specs();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "spec_a");
        assert_eq!(specs[1].name, "spec_b");
    }

    #[test]
    fn parse_uses_registry_spec_default_alias() {
        let input = "spec example\nuses @owner/repo somespec";
        let specs = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        match &specs[0].data[0].value {
            crate::parsing::ast::DataValue::Import(spec_ref) => {
                assert_eq!(spec_ref.name, "somespec");
                let repository_hdr = spec_ref
                    .repository
                    .as_ref()
                    .expect("expected repository qualifier");
                assert_eq!(repository_hdr.name, "@owner/repo");
            }
            other => panic!("Expected Import, got: {:?}", other),
        }
    }

    #[test]
    fn parse_uses_local_spec_default_alias() {
        let input = "spec example\nuses myspec";
        let specs = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        match &specs[0].data[0].value {
            crate::parsing::ast::DataValue::Import(spec_ref) => {
                assert_eq!(spec_ref.name, "myspec");
                assert!(
                    spec_ref.repository.is_none(),
                    "same-repository reference must omit repository qualifier"
                );
            }
            other => panic!("Expected Import, got: {:?}", other),
        }
    }

    #[test]
    fn parse_spec_name_with_trailing_dot_is_error() {
        let input = "spec myspec.\ndata x: 1";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        );
        assert!(
            result.is_err(),
            "Trailing dot after spec name should be a parse error"
        );
    }

    #[test]
    fn parse_multiple_specs_in_same_file() {
        let input = "spec myspec_a\nrule x: 1\n\nspec myspec_b\nrule x: 2";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "myspec_a");
        assert_eq!(result[1].name, "myspec_b");
    }

    #[test]
    fn parse_uses_accepts_name_only() {
        let input = "spec consumer\nuses other";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        );
        assert!(result.is_ok(), "uses name should parse");
        let specs = result.unwrap().into_flattened_specs();
        let spec_ref = match &specs[0].data[0].value {
            crate::parsing::ast::DataValue::Import(r) => r,
            _ => panic!("expected Import"),
        };
        assert_eq!(spec_ref.name, "other");
    }

    #[test]
    fn parse_uses_bare_year_effective() {
        let input = "spec consumer\nuses other 2026";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let specs = result.into_flattened_specs();
        let spec_ref = match &specs[0].data[0].value {
            crate::parsing::ast::DataValue::Import(r) => r,
            _ => panic!("expected Import"),
        };
        assert_eq!(spec_ref.name, "other");
        let eff = spec_ref.effective.as_ref().expect("effective");
        assert_eq!(eff.year, 2026);
        assert_eq!(eff.month, 1);
        assert_eq!(eff.day, 1);
    }

    #[test]
    fn parse_uses_comma_separated_bare() {
        let input = "spec consumer\nuses a, b, c";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let data = &result.flatten_specs()[0].data;
        assert_eq!(data.len(), 3);
        for (i, expected) in ["a", "b", "c"].iter().enumerate() {
            let sr = match &data[i].value {
                crate::parsing::ast::DataValue::Import(r) => r,
                _ => panic!("expected Import for item {i}"),
            };
            assert_eq!(sr.name, *expected);
            assert_eq!(data[i].reference.name, *expected);
            assert!(sr.effective.is_none());
        }
    }

    #[test]
    fn parse_uses_comma_separated_cross_repository() {
        let input = "spec consumer\nuses pricing retail, pricing wholesale";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let data = &result.flatten_specs()[0].data;
        assert_eq!(data.len(), 2);
        let sr0 = match &data[0].value {
            crate::parsing::ast::DataValue::Import(r) => r,
            _ => panic!("expected Import"),
        };
        assert_eq!(sr0.name, "retail");
        let repository_hdr0 = sr0
            .repository
            .as_ref()
            .expect("expected repository qualifier");
        assert_eq!(repository_hdr0.name, "pricing");
        assert_eq!(data[0].reference.name, "retail");
        let sr1 = match &data[1].value {
            crate::parsing::ast::DataValue::Import(r) => r,
            _ => panic!("expected Import"),
        };
        assert_eq!(sr1.name, "wholesale");
        let repository_hdr1 = sr1
            .repository
            .as_ref()
            .expect("expected repository qualifier");
        assert_eq!(repository_hdr1.name, "pricing");
        assert_eq!(data[1].reference.name, "wholesale");
    }

    #[test]
    fn parse_uses_comma_separated_registry() {
        let input = "spec consumer\nuses @org/repo spec_a, @org/repo spec_b";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let data = &result.flatten_specs()[0].data;
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].reference.name, "spec_a");
        assert_eq!(data[1].reference.name, "spec_b");
        for sr in [&data[0].value, &data[1].value] {
            let r = match sr {
                crate::parsing::ast::DataValue::Import(r) => r,
                _ => panic!("expected Import"),
            };
            let repository_hdr = r
                .repository
                .as_ref()
                .expect("expected repository qualifier");
            assert_eq!(repository_hdr.name, "@org/repo");
        }
    }

    #[test]
    fn parse_uses_registry_spec_ref_records_repository_and_target_spans() {
        let input = "spec consumer\nuses @lemma/std finance 2026";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let spec = &result.flatten_specs()[0];
        let sr = match &spec.data[0].value {
            crate::parsing::ast::DataValue::Import(r) => r,
            _ => panic!("expected Import"),
        };
        let rs = sr
            .repository_span
            .as_ref()
            .expect("repository_span should be set for @-qualified uses");
        let ts = sr
            .target_span
            .as_ref()
            .expect("target_span should cover spec name and effective");
        assert_eq!(&input[rs.start..rs.end], "@lemma/std");
        assert_eq!(&input[ts.start..ts.end], "finance 2026");
    }

    #[test]
    fn parse_uses_alias_no_comma_continuation() {
        let input = "spec consumer\nuses alias: pricing retail\ndata x: 1";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let data = &result.flatten_specs()[0].data;
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].reference.name, "alias");
        let sr = match &data[0].value {
            crate::parsing::ast::DataValue::Import(r) => r,
            _ => panic!("expected Import"),
        };
        assert_eq!(sr.name, "retail");
        let repository_hdr = sr
            .repository
            .as_ref()
            .expect("expected repository qualifier");
        assert_eq!(repository_hdr.name, "pricing");
    }

    #[test]
    fn parse_data_import_with_effective_and_repository_qualifier() {
        let input =
            "spec consumer\ndata price: number from @lemma/std finance 2026-06-01 -> minimum 0";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        match &result[0].data[0].value {
            crate::parsing::ast::DataValue::Definition {
                base,
                constraints,
                from,
                value,
            } => {
                assert!(value.is_none());
                assert_eq!(
                    base.as_ref().expect("expected base"),
                    &crate::parsing::ast::ParentType::Primitive {
                        primitive: crate::parsing::ast::PrimitiveKind::Number
                    }
                );
                let spec_ref = from.as_ref().expect("expected from clause");
                assert_eq!(spec_ref.name, "finance");

                let eff = spec_ref
                    .effective
                    .as_ref()
                    .expect("expected effective datetime");
                assert_eq!(eff.year, 2026);
                assert_eq!(eff.month, 6);

                let qualifier = spec_ref
                    .repository
                    .as_ref()
                    .expect("expected repository qualifier");
                assert_eq!(qualifier.name, "@lemma/std");

                let cs = constraints
                    .as_ref()
                    .expect("expected trailing constraint chain");
                assert_eq!(cs.len(), 1);
            }
            other => panic!("expected Definition, got: {:?}", other),
        }
    }

    #[test]
    fn parse_error_is_returned_for_garbage_input() {
        let result = parse(
            r#"
spec test
this is not valid lemma syntax @#$%
"#,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        );

        assert!(result.is_err(), "Should fail on malformed input");
        match result {
            Err(Error::Parsing { .. }) => {
                // Expected
            }
            Err(e) => panic!("Expected Parse error, got: {e:?}"),
            Ok(_) => panic!("Expected parse error"),
        }
    }

    // ─── Parser-level pins for DataValue variants ────────────────────

    /// `data x: a.b` (local LHS, dotted RHS) must be parsed as Reference.
    /// This is the value-copy reference form for local references.
    #[test]
    fn parse_data_with_dotted_rhs_is_reference() {
        let input = r#"spec s
data a: number -> default 1
data x: a.something"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let x_value = &result[0]
            .data
            .iter()
            .find(|d| d.reference.name == "x")
            .expect("data x not found")
            .value;
        assert!(
            matches!(x_value, crate::parsing::ast::DataValue::Reference { .. }),
            "dotted RHS must yield DataValue::Reference, got: {:?}",
            x_value
        );
    }

    /// `data x: a.b.c.d` (3+ segment RHS) must parse and preserve segments.
    #[test]
    fn parse_data_with_multi_segment_reference_rhs() {
        let input = r#"spec s
data x: alpha.beta.gamma.delta"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let value = &result[0].data[0].value;
        match value {
            crate::parsing::ast::DataValue::Reference { target, .. } => {
                assert_eq!(target.segments, vec!["alpha", "beta", "gamma"]);
                assert_eq!(target.name, "delta");
            }
            other => panic!("expected Reference, got: {:?}", other),
        }
    }

    /// `data x: a.b -> minimum 5` must parse as Reference WITH the
    /// trailing constraint chain captured in `constraints`.
    #[test]
    fn parse_reference_with_trailing_constraint_captures_constraints() {
        let input = r#"spec s
data x: foo.bar -> minimum 5"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let value = &result[0].data[0].value;
        match value {
            crate::parsing::ast::DataValue::Reference { constraints, .. } => {
                let c = constraints.as_ref().expect("constraints expected");
                assert_eq!(c.len(), 1, "exactly one constraint expected, got: {:?}", c);
            }
            other => panic!("expected Reference, got: {:?}", other),
        }
    }

    /// `data x: notdotted` (local LHS, non-dotted RHS) MUST stay a
    /// Definition with an explicit custom parent — not silently reinterpreted as a Reference.
    #[test]
    fn parse_local_non_dotted_rhs_stays_definition_with_custom_base() {
        let input = r#"spec s
data x: myothertype"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let value = &result[0].data[0].value;
        assert!(
            matches!(
                value,
                crate::parsing::ast::DataValue::Definition {
                    base: Some(crate::parsing::ast::ParentType::Custom { .. }),
                    ..
                }
            ),
            "non-dotted local RHS must stay Definition with custom base, got: {:?}",
            value
        );
    }

    /// `data x.y: notdotted` (binding LHS, non-dotted RHS) IS parsed as
    /// Reference per the current implementation — even though the AST doc
    /// comment claims otherwise. Pin the real behavior.
    #[test]
    fn parse_binding_non_dotted_rhs_is_reference() {
        let input = r#"spec s
data child.slot: somename"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let value = &result[0].data[0].value;
        assert!(
            matches!(value, crate::parsing::ast::DataValue::Reference { .. }),
            "non-dotted RHS in binding context must yield Reference; got: {:?}",
            value
        );
    }

    /// Legacy shorthand `data x: spec …` after `spec` keyword introduced; rejected.
    #[test]
    fn parse_legacy_data_colon_spec_rhs_is_rejected() {
        let result = parse(
            r#"
spec s
data x: spec other
"#,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        );
        match result {
            Ok(_) => panic!("legacy `data x: spec other` must fail to parse"),
            Err(err) => {
                let msg = err.to_string();
                assert!(
                    msg.contains("spec") && (msg.contains("removed") || msg.contains("syntax")),
                    "error must indicate the legacy syntax was removed, got: {msg}"
                );
            }
        }
    }

    /// `data x.y: z.w` (binding LHS, dotted RHS) → Reference with two LHS
    /// segments and two RHS segments.
    #[test]
    fn parse_binding_with_dotted_rhs_preserves_both_sides() {
        let input = r#"spec s
data outer.inner: target.field"#;
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let datum = &result[0].data[0];
        assert_eq!(datum.reference.segments, vec!["outer"]);
        assert_eq!(datum.reference.name, "inner");
        match &datum.value {
            crate::parsing::ast::DataValue::Reference {
                target,
                constraints,
                ..
            } => {
                assert_eq!(target.segments, vec!["target"]);
                assert_eq!(target.name, "field");
                assert!(constraints.is_none(), "no trailing constraints expected");
            }
            other => panic!("expected Reference, got: {:?}", other),
        }
    }

    #[test]
    fn parse_bare_file_yields_single_anonymous_repository_group() {
        let input = "spec a\ndata x: 1\nspec b\ndata y: 2";
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.repositories.len(), 1);
        let (repo, specs) = parsed.repositories.iter().next().unwrap();
        assert!(repo.name.is_none());
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "a");
        assert_eq!(specs[1].name, "b");
    }

    #[test]
    fn parse_repo_sections_preserve_order_and_names() {
        let input = r#"repo r1

spec a
data x: 1

repo r2

spec b
data y: 2"#;
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.repositories.len(), 2);
        let keys: Vec<_> = parsed.repositories.keys().collect();
        assert_eq!(keys[0].name.as_deref(), Some("r1"));
        assert_eq!(keys[1].name.as_deref(), Some("r2"));
    }

    #[test]
    fn parse_duplicate_repo_name_merges_spec_lists() {
        let input = r#"repo dup

spec a
data x: 1

repo dup

spec b
data y: 2"#;
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.repositories.len(), 1);
        assert_eq!(parsed.flatten_specs().len(), 2);
    }

    #[test]
    fn parse_repo_with_no_specs_then_eof_yields_empty_spec_vec_for_that_repo() {
        let input = "repo empty";
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.repositories.len(), 1);
        let (_repo, specs) = parsed.repositories.iter().next().unwrap();
        assert_eq!(specs.len(), 0);
    }

    #[test]
    fn parse_repo_followed_by_repo_without_specs_first_repo_empty_second_has_spec() {
        let input = "repo a\n\nrepo b\n\nspec s\ndata x: 1";
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.repositories.len(), 2);
        let names: Vec<_> = parsed
            .repositories
            .keys()
            .map(|r| r.name.as_deref())
            .collect();
        assert_eq!(names, vec![Some("a"), Some("b")]);
        assert!(parsed.repositories.values().next().unwrap().is_empty());
        assert_eq!(parsed.repositories.values().nth(1).unwrap().len(), 1);
    }

    #[test]
    fn parse_spec_named_repo_keyword_should_be_rejected() {
        assert!(
            parse(
                "spec repo\ndata x: 1",
                crate::parsing::source::SourceType::Volatile,
                &ResourceLimits::default(),
            )
            .is_err(),
            "spec must not be allowed to use reserved keyword `repo` as its name"
        );
    }

    #[test]
    fn parse_repo_declaration_cannot_use_spec_keyword_as_repository_name() {
        assert!(
            parse(
                "repo spec\n\nspec z\ndata q: 1\nrule r: q",
                crate::parsing::source::SourceType::Volatile,
                &ResourceLimits::default(),
            )
            .is_err(),
            "repository name cannot be the token `spec`"
        );
    }

    #[test]
    fn parse_repo_declaration_cannot_use_data_keyword_as_repository_name() {
        assert!(
            parse(
                "repo data\n\nspec z\ndata q: 1\nrule r: q",
                crate::parsing::source::SourceType::Volatile,
                &ResourceLimits::default(),
            )
            .is_err(),
            "repository name cannot be the token `data`"
        );
    }

    #[test]
    fn parse_repo_declaration_cannot_use_rule_keyword_as_repository_name() {
        assert!(
            parse(
                "repo rule\n\nspec z\ndata q: 1\nrule r: q",
                crate::parsing::source::SourceType::Volatile,
                &ResourceLimits::default(),
            )
            .is_err(),
            "repository name cannot be the token `rule`"
        );
    }

    #[test]
    fn parse_data_named_repo_keyword_is_rejected() {
        let err = parse(
            "spec s\ndata repo: 1",
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("repo"),
            "data named repo should not parse: {}",
            err
        );
    }

    #[test]
    fn parse_rule_named_repo_keyword_is_rejected() {
        let err = parse(
            "spec s\ndata x: 1\nrule repo: x",
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("repo") || msg.contains("reserved"),
            "rule named repo should not parse: {msg}"
        );
    }

    #[test]
    fn parse_repo_declaration_accepts_non_keyword_repository_identifier() {
        let parsed = parse(
            "repo warehouse\n\nspec z\ndata q: 1\nrule r: q",
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.repositories.len(), 1);
        assert_eq!(
            parsed.repositories.keys().next().unwrap().name.as_deref(),
            Some("warehouse")
        );
    }

    #[test]
    fn parse_repo_name_case_distinctness_two_repositories_not_merged() {
        let input = "repo Foo\n\nspec a\ndata x: 1\n\nrepo foo\n\nspec b\ndata y: 2";
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(
            parsed.repositories.len(),
            2,
            "Foo and foo must be distinct repository identities"
        );
    }

    #[test]
    fn parse_repo_empty_name_errors() {
        let err = parse(
            "repo \nspec a\ndata x: 1",
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap_err();
        assert!(
            !err.to_string().is_empty(),
            "empty repo name should not parse quietly: {err}"
        );
    }

    #[test]
    fn parse_repo_numeric_name_behavior() {
        let input = "repo 123\n\nspec a\ndata x: 1";
        let result = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        );
        match result {
            Ok(parsed) => {
                assert_eq!(
                    parsed.repositories.keys().next().unwrap().name.as_deref(),
                    Some("123"),
                    "if numeric repo names parse, identity must be stable"
                );
            }
            Err(e) => {
                assert!(
                    !e.to_string().is_empty(),
                    "rejecting numeric repo name is ok if explicit: {e}"
                );
            }
        }
    }

    #[test]
    fn parse_duplicate_repo_three_sections_preserves_spec_order_abc() {
        let input = r#"repo dup

spec a
data x: 1

repo dup

spec b
data y: 2

repo dup

spec c
data z: 3"#;
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(parsed.repositories.len(), 1);
        let specs = parsed.repositories.values().next().unwrap();
        assert_eq!(
            specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn parse_repo_single_section_roundtrips_through_formatter() {
        let input = "repo r\n\nspec a\ndata x: 1";
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let formatted = format_parse_result(&parsed);
        let again = parse(
            &formatted,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(again.repositories.len(), parsed.repositories.len());
        assert_eq!(again.flatten_specs().len(), parsed.flatten_specs().len());
        assert_eq!(
            again.flatten_specs()[0].name,
            parsed.flatten_specs()[0].name
        );
    }

    #[test]
    fn parse_repo_two_sections_roundtrips_through_formatter() {
        let input = "repo r1\n\nspec a\ndata x: 1\n\nrepo r2\n\nspec b\ndata y: 2";
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let formatted = format_parse_result(&parsed);
        let again = parse(
            &formatted,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(again.repositories.len(), 2);
        assert_eq!(again.flatten_specs().len(), 2);
    }

    #[test]
    fn parse_repo_duplicate_merge_formatter_emits_single_repo_block_or_equivalent_parse() {
        let input = r#"repo dup

spec a
data x: 1

repo dup

spec b
data y: 2"#;
        let parsed = parse(
            input,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        let formatted = format_parse_result(&parsed);
        let again = parse(
            &formatted,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap();
        assert_eq!(
            again.repositories.len(),
            1,
            "formatted duplicate-repo file must still merge to one logical repo"
        );
        assert_eq!(again.flatten_specs().len(), 2);
    }
}
