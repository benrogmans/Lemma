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
    attribute: &str,
    limits: &ResourceLimits,
) -> Result<ParseResult, Error> {
    parser::parse(content, attribute, limits)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::Error;
    use crate::ResourceLimits;

    #[test]
    fn parse_empty_input_returns_no_specs() {
        let result = parse("", "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn parse_workspace_file_yields_expected_spec_datas_and_rules() {
        let input = r#"spec person
data name: "John Doe"
rule adult: true"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
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

        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].data.len(), 3);
        assert_eq!(result[0].rules.len(), 3);
    }

    #[test]
    fn parse_simple_spec_collects_data() {
        let input = r#"spec person
data name: "John"
data age: 25"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[0].data.len(), 2);
    }

    #[test]
    fn parse_spec_name_with_slashes_is_preserved() {
        let input = r#"spec contracts/employment/jack
data name: "Jack""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "contracts/employment/jack");
    }

    #[test]
    fn parse_spec_name_no_version_tag() {
        let input = "spec myspec\nrule x: 1";
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "myspec");
        assert_eq!(result[0].effective_from(), None);
    }

    #[test]
    fn parse_commentary_block_is_attached_to_spec() {
        let input = r#"spec person
"""
This is a markdown comment
with **bold** text
"""
data name: "John""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 1);
        assert!(result[0].commentary.is_some());
        assert!(result[0].commentary.as_ref().unwrap().contains("**bold**"));
    }

    #[test]
    fn parse_spec_with_rule_collects_rule() {
        let input = r#"spec person
rule is_adult: age >= 18"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
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
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "person");
        assert_eq!(result[1].name, "company");
    }

    #[test]
    fn parse_allows_duplicate_data_names() {
        let input = r#"spec person
data name: "John"
data name: "Jane""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default());
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
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(
            result.is_ok(),
            "Parser should succeed even with duplicate rules"
        );
    }

    #[test]
    fn parse_rejects_malformed_input() {
        let input = "invalid syntax here";
        let result = parse(input, "test.lemma", &ResourceLimits::default());
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
            let result = parse(input, "test.lemma", &ResourceLimits::default());
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
            let result = parse(input, "test.lemma", &ResourceLimits::default());
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
            let result = parse(&input, "test.lemma", &ResourceLimits::default());
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
            let result = parse(&input, "test.lemma", &ResourceLimits::default());
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
            "test.lemma",
            &ResourceLimits::default(),
        );

        match result {
            Err(Error::Parsing(details)) => {
                let src = details
                    .source
                    .as_ref()
                    .expect("BUG: parsing errors always have source");
                assert_eq!(src.attribute, "test.lemma");
            }
            Err(e) => panic!("Expected Parse error, got: {e:?}"),
            Ok(_) => panic!("Expected parse error for unclosed string"),
        }
    }

    #[test]
    fn parse_registry_style_spec_name() {
        let input = r#"spec user/workspace/somespec
data name: "Alice""#;
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "user/workspace/somespec");
    }

    #[test]
    fn parse_with_registry_spec_explicit_alias() {
        let input = r#"spec example
with external: @user/workspace/somespec"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].data.len(), 1);
        match &result[0].data[0].value {
            crate::parsing::ast::DataValue::SpecReference(spec_ref) => {
                assert_eq!(spec_ref.name, "@user/workspace/somespec");
                assert!(spec_ref.from_registry, "expected registry reference");
            }
            other => panic!("Expected SpecReference, got: {:?}", other),
        }
    }

    #[test]
    fn parse_multiple_registry_specs_in_same_file() {
        let input = r#"spec user/workspace/spec_a
data x: 10

spec user/workspace/spec_b
data y: 20
with a: @user/workspace/spec_a"#;
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "user/workspace/spec_a");
        assert_eq!(result[1].name, "user/workspace/spec_b");
    }

    #[test]
    fn parse_with_registry_spec_default_alias() {
        let input = "spec example\nwith @owner/repo/somespec";
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        match &result[0].data[0].value {
            crate::parsing::ast::DataValue::SpecReference(spec_ref) => {
                assert_eq!(spec_ref.name, "@owner/repo/somespec");
                assert!(spec_ref.from_registry);
            }
            other => panic!("Expected SpecReference, got: {:?}", other),
        }
    }

    #[test]
    fn parse_with_local_spec_default_alias() {
        let input = "spec example\nwith myspec";
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        match &result[0].data[0].value {
            crate::parsing::ast::DataValue::SpecReference(spec_ref) => {
                assert_eq!(spec_ref.name, "myspec");
                assert!(!spec_ref.from_registry);
            }
            other => panic!("Expected SpecReference, got: {:?}", other),
        }
    }

    #[test]
    fn parse_spec_name_with_trailing_dot_is_error() {
        let input = "spec myspec.\ndata x: 1";
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(
            result.is_err(),
            "Trailing dot after spec name should be a parse error"
        );
    }

    #[test]
    fn parse_multiple_specs_in_same_file() {
        let input = "spec myspec_a\nrule x: 1\n\nspec myspec_b\nrule x: 2";
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "myspec_a");
        assert_eq!(result[1].name, "myspec_b");
    }

    #[test]
    fn parse_with_accepts_name_only() {
        let input = "spec consumer\nwith other";
        let result = parse(input, "test.lemma", &ResourceLimits::default());
        assert!(result.is_ok(), "with name should parse");
        let spec_ref = match &result.as_ref().unwrap().specs[0].data[0].value {
            crate::parsing::ast::DataValue::SpecReference(r) => r,
            _ => panic!("expected SpecReference"),
        };
        assert_eq!(spec_ref.name, "other");
    }

    #[test]
    fn parse_with_bare_year_effective() {
        let input = "spec consumer\nwith other 2026";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let spec_ref = match &result.specs[0].data[0].value {
            crate::parsing::ast::DataValue::SpecReference(r) => r,
            _ => panic!("expected SpecReference"),
        };
        assert_eq!(spec_ref.name, "other");
        let eff = spec_ref.effective.as_ref().expect("effective");
        assert_eq!(eff.year, 2026);
        assert_eq!(eff.month, 1);
        assert_eq!(eff.day, 1);
    }

    #[test]
    fn parse_with_comma_separated_bare() {
        let input = "spec consumer\nwith a, b, c";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let data = &result.specs[0].data;
        assert_eq!(data.len(), 3);
        for (i, expected) in ["a", "b", "c"].iter().enumerate() {
            let sr = match &data[i].value {
                crate::parsing::ast::DataValue::SpecReference(r) => r,
                _ => panic!("expected SpecReference for item {i}"),
            };
            assert_eq!(sr.name, *expected);
            assert_eq!(data[i].reference.name, *expected);
            assert!(sr.effective.is_none());
        }
    }

    #[test]
    fn parse_with_comma_separated_paths() {
        let input = "spec consumer\nwith pricing/retail, pricing/wholesale";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let data = &result.specs[0].data;
        assert_eq!(data.len(), 2);
        let sr0 = match &data[0].value {
            crate::parsing::ast::DataValue::SpecReference(r) => r,
            _ => panic!("expected SpecReference"),
        };
        assert_eq!(sr0.name, "pricing/retail");
        assert_eq!(data[0].reference.name, "retail");
        let sr1 = match &data[1].value {
            crate::parsing::ast::DataValue::SpecReference(r) => r,
            _ => panic!("expected SpecReference"),
        };
        assert_eq!(sr1.name, "pricing/wholesale");
        assert_eq!(data[1].reference.name, "wholesale");
    }

    #[test]
    fn parse_with_comma_separated_registry() {
        let input = "spec consumer\nwith @org/repo/spec_a, @org/repo/spec_b";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let data = &result.specs[0].data;
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].reference.name, "spec_a");
        assert_eq!(data[1].reference.name, "spec_b");
    }

    #[test]
    fn parse_with_alias_no_comma_continuation() {
        let input = "spec consumer\nwith alias: pricing/retail\ndata x: 1";
        let result = parse(input, "test.lemma", &ResourceLimits::default()).unwrap();
        let data = &result.specs[0].data;
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].reference.name, "alias");
        let sr = match &data[0].value {
            crate::parsing::ast::DataValue::SpecReference(r) => r,
            _ => panic!("expected SpecReference"),
        };
        assert_eq!(sr.name, "pricing/retail");
    }

    #[test]
    fn parse_inline_type_from_with_effective() {
        let input = "spec consumer\ndata price: money from finance 2026-06-01 -> minimum 0";
        let result = parse(input, "test.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        match &result[0].data[0].value {
            crate::parsing::ast::DataValue::TypeDeclaration { from, .. } => {
                let spec_ref = from.as_ref().expect("expected from spec ref");
                assert_eq!(spec_ref.name, "finance");
                let eff = spec_ref
                    .effective
                    .as_ref()
                    .expect("expected effective datetime");
                assert_eq!(eff.year, 2026);
                assert_eq!(eff.month, 6);
            }
            other => panic!("expected TypeDeclaration, got: {:?}", other),
        }
    }

    #[test]
    fn parse_error_is_returned_for_garbage_input() {
        let result = parse(
            r#"
spec test
this is not valid lemma syntax @#$%
"#,
            "test.lemma",
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
        let result = parse(input, "t.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
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
        let result = parse(input, "t.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
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
        let result = parse(input, "t.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
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
    /// TypeDeclaration — not silently reinterpreted as a Reference. Pin the
    /// parser behavior so future refactors cannot change the shape without
    /// the test flipping.
    #[test]
    fn parse_local_non_dotted_rhs_stays_type_declaration() {
        let input = r#"spec s
data x: myothertype"#;
        let result = parse(input, "t.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let value = &result[0].data[0].value;
        assert!(
            matches!(
                value,
                crate::parsing::ast::DataValue::TypeDeclaration { .. }
            ),
            "non-dotted local RHS must stay TypeDeclaration, got: {:?}",
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
        let result = parse(input, "t.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let value = &result[0].data[0].value;
        assert!(
            matches!(value, crate::parsing::ast::DataValue::Reference { .. }),
            "non-dotted RHS in binding context must yield Reference; got: {:?}",
            value
        );
    }

    /// Legacy syntax `data x: spec other` was removed; must be rejected.
    #[test]
    fn parse_legacy_data_colon_spec_is_rejected() {
        let result = parse(
            r#"
spec s
data x: spec other
"#,
            "t.lemma",
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
        let result = parse(input, "t.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let datum = &result[0].data[0];
        assert_eq!(datum.reference.segments, vec!["outer"]);
        assert_eq!(datum.reference.name, "inner");
        match &datum.value {
            crate::parsing::ast::DataValue::Reference {
                target,
                constraints,
            } => {
                assert_eq!(target.segments, vec!["target"]);
                assert_eq!(target.name, "field");
                assert!(constraints.is_none(), "no trailing constraints expected");
            }
            other => panic!("expected Reference, got: {:?}", other),
        }
    }
}
