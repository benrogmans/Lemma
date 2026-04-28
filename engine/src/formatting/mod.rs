//! Lemma source code formatting.
//!
//! Formats parsed specs into canonical Lemma source text. Uses `AsLemmaSource`
//! and `Expression::Display` for syntax; this module handles layout only.

use crate::parsing::ast::{
    expression_precedence, AsLemmaSource, DataValue, Expression, ExpressionKind, LemmaData,
    LemmaRule, LemmaSpec,
};
use crate::{parse, Error, ResourceLimits};

/// Soft line length limit. Longer lines may be wrapped (unless clauses, expressions).
/// Data and other constructs are not broken if they exceed this.
pub const MAX_COLS: usize = 60;

// =============================================================================
// Public entry points
// =============================================================================

/// Format a sequence of parsed specs into canonical Lemma source.
///
/// Specs are separated by two blank lines.
/// The result ends with a single newline.
#[must_use]
pub fn format_specs(specs: &[LemmaSpec]) -> String {
    let mut out = String::new();
    for (index, spec) in specs.iter().enumerate() {
        if index > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&format_spec(spec, MAX_COLS));
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Parse a source string and format it to canonical Lemma source.
///
/// Returns an error if the source does not parse.
pub fn format_source(source: &str, attribute: &str) -> Result<String, Error> {
    let limits = ResourceLimits::default();
    let result = parse(source, attribute, &limits)?;
    Ok(format_specs(&result.specs))
}

// =============================================================================
// Spec
// =============================================================================

pub(crate) fn format_spec(spec: &LemmaSpec, max_cols: usize) -> String {
    let mut out = String::new();
    out.push_str("spec ");
    out.push_str(&spec.name);
    if let crate::parsing::ast::EffectiveDate::DateTimeValue(ref af) = spec.effective_from {
        out.push(' ');
        out.push_str(&af.to_string());
    }
    out.push('\n');

    if let Some(ref commentary) = spec.commentary {
        out.push_str("\"\"\"\n");
        out.push_str(commentary);
        out.push_str("\n\"\"\"\n");
    }

    for meta in &spec.meta_fields {
        out.push_str(&format!(
            "meta {}: {}\n",
            meta.key,
            AsLemmaSource(&meta.value)
        ));
    }

    if !spec.data.is_empty() {
        format_sorted_data(&spec.data, &mut out);
    }

    if !spec.rules.is_empty() {
        out.push('\n');
        for (index, rule) in spec.rules.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            out.push_str(&format_rule(rule, max_cols));
        }
    }

    out
}

// =============================================================================
// Data
// =============================================================================

/// Format a data, optionally with the reference name padded to `align_width` characters
/// for column-aligned `=` signs within a group.
/// When `align_width` is 0 or less than the reference length, no padding is added.
fn format_data(data: &LemmaData, align_width: usize) -> String {
    let ref_str = format!("{}", data.reference);
    let padding = if align_width > ref_str.len() {
        " ".repeat(align_width - ref_str.len())
    } else {
        String::new()
    };
    match &data.value {
        DataValue::TypeDeclaration {
            base,
            constraints,
            from,
        } if from.is_some() && constraints.is_none() => {
            format!(
                "data {}{} from {}",
                ref_str,
                padding,
                from.as_ref().unwrap()
            )
        }
        _ => {
            format!(
                "data {}{} : {}",
                ref_str,
                padding,
                AsLemmaSource(&data.value)
            )
        }
    }
}

/// Compute the maximum data reference width across a slice of data.
fn max_ref_width(data: &[&LemmaData]) -> usize {
    data.iter()
        .map(|f| format!("{}", f.reference).len())
        .max()
        .unwrap_or(0)
}

fn format_with_statement(data: &LemmaData) -> String {
    let alias = &data.reference.name;
    if let DataValue::SpecReference(spec_ref) = &data.value {
        let spec_name = &spec_ref.name;
        let last_segment = spec_name.rsplit('/').next().unwrap_or(spec_name);
        if alias == last_segment {
            format!("with {}", spec_ref)
        } else {
            format!("with {}: {}", alias, spec_ref)
        }
    } else {
        unreachable!("BUG: format_with_statement called on non-SpecReference data")
    }
}

/// Group data into two sections separated by a blank line:
///
/// 1. Regular data (literals, type declarations) — original order, aligned
/// 2. With statements (spec refs), each followed by their literal bindings — original order
fn format_sorted_data(data: &[LemmaData], out: &mut String) {
    let mut regular: Vec<&LemmaData> = Vec::new();
    let mut spec_refs: Vec<&LemmaData> = Vec::new();
    let mut overrides: Vec<&LemmaData> = Vec::new();

    for data in data {
        if !data.reference.is_local() {
            overrides.push(data);
        } else if matches!(&data.value, DataValue::SpecReference(_)) {
            spec_refs.push(data);
        } else {
            regular.push(data);
        }
    }

    let emit_group = |data: &[&LemmaData], out: &mut String| {
        let width = max_ref_width(data);
        for data in data {
            out.push_str(&format_data(data, width));
            out.push('\n');
        }
    };

    if !regular.is_empty() {
        out.push('\n');
        emit_group(&regular, out);
    }

    if !spec_refs.is_empty() {
        out.push('\n');

        let has_overrides = |spec_data: &LemmaData| -> bool {
            let ref_name = &spec_data.reference.name;
            overrides.iter().any(|o| {
                o.reference.segments.first().map(|s| s.as_str()) == Some(ref_name.as_str())
            })
        };

        let is_bare = |spec_data: &LemmaData| -> bool {
            if let DataValue::SpecReference(sr) = &spec_data.value {
                let last = sr.name.rsplit('/').next().unwrap_or(&sr.name);
                spec_data.reference.name == last
                    && sr.effective.is_none()
                    && !has_overrides(spec_data)
            } else {
                false
            }
        };

        let mut i = 0;
        while i < spec_refs.len() {
            if i > 0 {
                out.push('\n');
            }
            if is_bare(spec_refs[i]) {
                // Collect consecutive bare refs into a comma-separated line
                let mut group_names = Vec::new();
                while i < spec_refs.len() && is_bare(spec_refs[i]) {
                    if let DataValue::SpecReference(sr) = &spec_refs[i].value {
                        group_names.push(sr.to_string());
                    }
                    i += 1;
                }
                if group_names.len() == 1 {
                    out.push_str(&format!("with {}", group_names[0]));
                } else {
                    out.push_str(&format!("with {}", group_names.join(", ")));
                }
                out.push('\n');
            } else {
                let spec_data = spec_refs[i];
                out.push_str(&format_with_statement(spec_data));
                out.push('\n');
                let ref_name = &spec_data.reference.name;
                let binding_overrides: Vec<&LemmaData> = overrides
                    .iter()
                    .filter(|o| {
                        o.reference.segments.first().map(|s| s.as_str()) == Some(ref_name.as_str())
                    })
                    .copied()
                    .collect();
                if !binding_overrides.is_empty() {
                    let width = max_ref_width(&binding_overrides);
                    for ovr in &binding_overrides {
                        out.push_str(&format_data(ovr, width));
                        out.push('\n');
                    }
                }
                i += 1;
            }
        }
    }

    let matched_prefixes: Vec<&str> = spec_refs
        .iter()
        .map(|f| f.reference.name.as_str())
        .collect();
    let unmatched: Vec<&LemmaData> = overrides
        .iter()
        .filter(|o| {
            o.reference
                .segments
                .first()
                .map(|s| !matched_prefixes.contains(&s.as_str()))
                .unwrap_or(true)
        })
        .copied()
        .collect();
    if !unmatched.is_empty() {
        out.push('\n');
        emit_group(&unmatched, out);
    }
}

// =============================================================================
// Rules
// =============================================================================

/// Format a rule with optional line wrapping: long unless lines get "then" on
/// the next line; long expressions break at arithmetic operators.
fn format_rule(rule: &LemmaRule, max_cols: usize) -> String {
    let expr_indent = "  ";
    let body = format_expr_wrapped(&rule.expression, max_cols, expr_indent, 10);
    let mut out = String::new();
    out.push_str("rule ");
    out.push_str(&rule.name);
    out.push_str(":\n");
    out.push_str(expr_indent);
    out.push_str(&body);

    for unless_clause in &rule.unless_clauses {
        let condition_str = format_expr_wrapped(&unless_clause.condition, max_cols, "    ", 10);
        let result_str = format_expr_wrapped(&unless_clause.result, max_cols, "    ", 10);
        let line = format!("  unless {} then {}", condition_str, result_str);
        if line.len() > max_cols {
            out.push_str("\n  unless ");
            out.push_str(&condition_str);
            out.push_str("\n    then ");
            out.push_str(&result_str);
        } else {
            out.push_str("\n  unless ");
            out.push_str(&condition_str);
            out.push_str(" then ");
            out.push_str(&result_str);
        }
    }
    out.push('\n');
    out
}

// =============================================================================
// Expression wrapping (soft line breaking at max_cols)
// =============================================================================

/// Indent every line after the first by `indent`.
fn indent_after_first_line(s: &str, indent: &str) -> String {
    let mut first = true;
    let mut out = String::new();
    for line in s.lines() {
        if first {
            first = false;
            out.push_str(line);
        } else {
            out.push('\n');
            out.push_str(indent);
            out.push_str(line);
        }
    }
    if s.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Format an expression with optional wrapping at arithmetic operators when over max_cols.
/// `parent_prec` is used to add parentheses when needed (pass 10 for top level).
fn format_expr_wrapped(
    expr: &Expression,
    max_cols: usize,
    indent: &str,
    parent_prec: u8,
) -> String {
    let my_prec = expression_precedence(&expr.kind);

    let wrap_in_parens = |s: String| {
        if parent_prec < 10 && my_prec < parent_prec {
            format!("({})", s)
        } else {
            s
        }
    };

    match &expr.kind {
        ExpressionKind::Arithmetic(left, op, right) => {
            let left_str = format_expr_wrapped(left.as_ref(), max_cols, indent, my_prec);
            let right_str = format_expr_wrapped(right.as_ref(), max_cols, indent, my_prec);
            let single_line = format!("{} {} {}", left_str, op, right_str);
            if single_line.len() <= max_cols && !single_line.contains('\n') {
                return wrap_in_parens(single_line);
            }
            let continued_right = indent_after_first_line(&right_str, indent);
            let continuation = format!("{}{} {}", indent, op, continued_right);
            let multi_line = format!("{}\n{}", left_str, continuation);
            wrap_in_parens(multi_line)
        }
        _ => {
            let s = expr.to_string();
            wrap_in_parens(s)
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::{
        AsLemmaSource, BooleanValue, DateTimeValue, DurationUnit, TimeValue, TimezoneValue, Value,
    };
    use rust_decimal::prelude::FromStr;
    use rust_decimal::Decimal;

    /// Helper: format a Value as canonical Lemma source via AsLemmaSource.
    fn fmt_value(v: &Value) -> String {
        format!("{}", AsLemmaSource(v))
    }

    #[test]
    fn test_format_value_text_is_quoted() {
        let v = Value::Text("light".to_string());
        assert_eq!(fmt_value(&v), "\"light\"");
    }

    #[test]
    fn test_format_value_text_escapes_quotes() {
        let v = Value::Text("say \"hello\"".to_string());
        assert_eq!(fmt_value(&v), "\"say \\\"hello\\\"\"");
    }

    #[test]
    fn test_format_value_number() {
        let v = Value::Number(Decimal::from_str("42.50").unwrap());
        assert_eq!(fmt_value(&v), "42.50");
    }

    #[test]
    fn test_format_value_number_integer() {
        let v = Value::Number(Decimal::from_str("100.00").unwrap());
        assert_eq!(fmt_value(&v), "100");
    }

    #[test]
    fn test_format_value_boolean() {
        assert_eq!(fmt_value(&Value::Boolean(BooleanValue::True)), "true");
        assert_eq!(fmt_value(&Value::Boolean(BooleanValue::Yes)), "yes");
        assert_eq!(fmt_value(&Value::Boolean(BooleanValue::No)), "no");
        assert_eq!(fmt_value(&Value::Boolean(BooleanValue::Accept)), "accept");
        assert_eq!(fmt_value(&Value::Boolean(BooleanValue::Reject)), "reject");
    }

    #[test]
    fn test_format_value_scale() {
        let v = Value::Scale(Decimal::from_str("99.50").unwrap(), "eur".to_string());
        assert_eq!(fmt_value(&v), "99.50 eur");
    }

    #[test]
    fn test_format_value_duration() {
        let v = Value::Duration(Decimal::from(40), DurationUnit::Hour);
        assert_eq!(fmt_value(&v), "40 hours");
    }

    #[test]
    fn test_format_value_ratio_percent() {
        let v = Value::Ratio(
            Decimal::from_str("0.10").unwrap(),
            Some("percent".to_string()),
        );
        assert_eq!(fmt_value(&v), "10%");
    }

    #[test]
    fn test_format_value_ratio_permille() {
        let v = Value::Ratio(
            Decimal::from_str("0.005").unwrap(),
            Some("permille".to_string()),
        );
        assert_eq!(fmt_value(&v), "5%%");
    }

    #[test]
    fn test_format_value_ratio_bare() {
        let v = Value::Ratio(Decimal::from_str("0.25").unwrap(), None);
        assert_eq!(fmt_value(&v), "0.25");
    }

    #[test]
    fn test_format_value_date_only() {
        let v = Value::Date(DateTimeValue {
            year: 2024,
            month: 1,
            day: 15,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        });
        assert_eq!(fmt_value(&v), "2024-01-15");
    }

    #[test]
    fn test_format_value_datetime_with_tz() {
        let v = Value::Date(DateTimeValue {
            year: 2024,
            month: 1,
            day: 15,
            hour: 14,
            minute: 30,
            second: 0,
            microsecond: 0,
            timezone: Some(TimezoneValue {
                offset_hours: 0,
                offset_minutes: 0,
            }),
        });
        assert_eq!(fmt_value(&v), "2024-01-15T14:30:00Z");
    }

    #[test]
    fn test_format_value_time() {
        let v = Value::Time(TimeValue {
            hour: 14,
            minute: 30,
            second: 45,
            timezone: None,
        });
        assert_eq!(fmt_value(&v), "14:30:45");
    }

    #[test]
    fn test_format_source_round_trips_text() {
        let source = r#"spec test

data name: "Alice"

rule greeting: "hello"
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        assert!(formatted.contains("\"Alice\""), "data text must be quoted");
        assert!(formatted.contains("\"hello\""), "rule text must be quoted");
    }

    #[test]
    fn test_format_source_preserves_percent() {
        let source = r#"spec test

data rate: 10 percent

rule tax: rate * 21%
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        assert!(
            formatted.contains("10%"),
            "data percent must use shorthand %, got: {}",
            formatted
        );
    }

    #[test]
    fn test_format_groups_data_preserving_order() {
        // Data are deliberately mixed: the formatter keeps all regular data together
        // in original order, aligned
        let source = r#"spec test

data income: number -> minimum 0
data filing_status: filing_status_type -> default "single"
data country: "NL"
data deductions: number -> minimum 0
data name: text

rule total: income
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        let data_section = formatted
            .split("rule total")
            .next()
            .unwrap()
            .split("spec test\n")
            .nth(1)
            .unwrap();
        let lines: Vec<&str> = data_section.lines().filter(|l| !l.is_empty()).collect();
        // All regular data in one group, original order, aligned
        assert_eq!(lines[0], "data income        : number -> minimum 0");
        assert_eq!(
            lines[1],
            "data filing_status : filing_status_type -> default \"single\""
        );
        assert_eq!(lines[2], "data country       : \"NL\"");
        assert_eq!(lines[3], "data deductions    : number -> minimum 0");
        assert_eq!(lines[4], "data name          : text");
    }

    #[test]
    fn test_format_groups_spec_refs_with_overrides() {
        let source = r#"spec test

data retail.quantity: 5
with order/wholesale
with order/retail
data wholesale.quantity: 100
data base_price: 50

rule total: base_price
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        let data_section = formatted
            .split("rule total")
            .next()
            .unwrap()
            .split("spec test\n")
            .nth(1)
            .unwrap();
        let lines: Vec<&str> = data_section.lines().filter(|l| !l.is_empty()).collect();
        // Group 1: Literals
        assert_eq!(lines[0], "data base_price : 50");
        // Group 4: Spec refs in original order, each with its overrides, aligned
        assert_eq!(lines[1], "with order/wholesale");
        assert_eq!(lines[2], "data wholesale.quantity : 100");
        assert_eq!(lines[3], "with order/retail");
        assert_eq!(lines[4], "data retail.quantity : 5");
    }

    #[test]
    fn test_format_source_weather_clothing_text_quoted() {
        let source = r#"spec weather_clothing

data clothing_style: text
  -> option "light"
  -> option "warm"

data temperature: number

rule clothing_layer: "light"
  unless temperature < 5 then "warm"
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        assert!(
            formatted.contains("\"light\""),
            "text in rule must be quoted, got: {}",
            formatted
        );
        assert!(
            formatted.contains("\"warm\""),
            "text in unless must be quoted, got: {}",
            formatted
        );
    }

    // NOTE: Default value type validation (e.g. rejecting "10 $$" as a number
    // default) is tested at the planning level in engine.rs, not here. The
    // formatter only parses — it does not validate types. Planning catches
    // invalid defaults for both primitives and named types.

    #[test]
    fn test_format_text_option_round_trips() {
        let source = r#"spec test

data status: text
  -> option "active"
  -> option "inactive"

data s: status

rule out: s
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        assert!(
            formatted.contains("option \"active\""),
            "text option must be quoted, got: {}",
            formatted
        );
        assert!(
            formatted.contains("option \"inactive\""),
            "text option must be quoted, got: {}",
            formatted
        );
        // Round-trip
        let reparsed = format_source(&formatted, "test.lemma");
        assert!(reparsed.is_ok(), "formatted output should re-parse");
    }

    #[test]
    fn test_format_help_round_trips() {
        let source = r#"spec test
data quantity: number -> help "Number of items to order"
rule total: quantity
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        assert!(
            formatted.contains("help \"Number of items to order\""),
            "help must be quoted, got: {}",
            formatted
        );
        // Round-trip
        let reparsed = format_source(&formatted, "test.lemma");
        assert!(reparsed.is_ok(), "formatted output should re-parse");
    }

    #[test]
    fn test_format_scale_type_def_round_trips() {
        let source = r#"spec test

data money: scale
  -> unit eur 1.00
  -> unit usd 1.10
  -> decimals 2
  -> minimum 0

data price: money

rule total: price
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        assert!(
            formatted.contains("unit eur 1.00"),
            "scale unit should not be quoted, got: {}",
            formatted
        );
        // Round-trip
        let reparsed = format_source(&formatted, "test.lemma");
        assert!(
            reparsed.is_ok(),
            "formatted output should re-parse, got: {:?}",
            reparsed
        );
    }

    #[test]
    fn test_format_expression_display_stable_round_trip() {
        let source = r#"spec test
data a: 1.00
rule r: a + 2.00 * 3
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        let again = format_source(&formatted, "test.lemma").unwrap();
        assert_eq!(
            formatted, again,
            "AST Display-based format must be idempotent under parse/format"
        );
    }
}
