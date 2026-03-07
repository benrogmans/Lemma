//! Lemma source code formatting.
//!
//! Formats parsed documents into canonical Lemma source text.
//! Value and constraint formatting is delegated to [`AsLemmaSource`] (in `parsing::ast`),
//! which emits valid, round-trippable Lemma syntax. The regular `Display` impls on AST
//! types are for human-readable output (error messages, evaluation); they are **not** used
//! here. This module handles layout: alignment, line wrapping, and section ordering.

use crate::parsing::ast::{
    expression_precedence, AsLemmaSource, Expression, ExpressionKind, FactValue, LemmaDoc,
    LemmaFact, LemmaRule, TypeDef,
};
use crate::{parse, Error, ResourceLimits};

/// Soft line length limit. Longer lines may be wrapped (unless clauses, expressions).
/// Facts and other constructs are not broken if they exceed this.
pub const MAX_COLS: usize = 60;

// =============================================================================
// Public entry points
// =============================================================================

/// Format a sequence of parsed documents into canonical Lemma source.
///
/// Documents are separated by two blank lines.
/// The result ends with a single newline.
#[must_use]
pub fn format_docs(docs: &[LemmaDoc]) -> String {
    let mut out = String::new();
    for (index, doc) in docs.iter().enumerate() {
        if index > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&format_document(doc, MAX_COLS));
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
    let docs = parse(source, attribute, &limits)?;
    Ok(format_docs(&docs))
}

// =============================================================================
// Document
// =============================================================================

fn format_document(doc: &LemmaDoc, max_cols: usize) -> String {
    let mut out = String::new();
    out.push_str("doc ");
    out.push_str(&doc.name);
    if let Some(ref af) = doc.effective_from {
        out.push(' ');
        out.push_str(&af.to_string());
    }
    out.push('\n');

    if let Some(ref commentary) = doc.commentary {
        out.push_str("\"\"\"\n");
        out.push_str(commentary);
        out.push_str("\n\"\"\"\n");
    }

    for meta in &doc.meta_fields {
        out.push_str(&format!(
            "meta {}: {}\n",
            meta.key,
            AsLemmaSource(&meta.value)
        ));
    }

    let named_types: Vec<_> = doc
        .types
        .iter()
        .filter(|t| !matches!(t, TypeDef::Inline { .. }))
        .collect();
    if !named_types.is_empty() {
        out.push('\n');
        for (index, type_def) in named_types.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            out.push_str(&format!("{}", AsLemmaSource(*type_def)));
            out.push('\n');
        }
    }

    if !doc.facts.is_empty() {
        format_sorted_facts(&doc.facts, &mut out);
    }

    if !doc.rules.is_empty() {
        out.push('\n');
        for (index, rule) in doc.rules.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            out.push_str(&format_rule(rule, max_cols));
        }
    }

    out
}

// =============================================================================
// Type definitions — delegated to AsLemmaSource<TypeDef>
// =============================================================================

// =============================================================================
// Facts
// =============================================================================

/// Format a fact, optionally with the reference name padded to `align_width` characters
/// for column-aligned `=` signs within a group.
/// When `align_width` is 0 or less than the reference length, no padding is added.
fn format_fact(fact: &LemmaFact, align_width: usize) -> String {
    let ref_str = format!("{}", fact.reference);
    let padding = if align_width > ref_str.len() {
        " ".repeat(align_width - ref_str.len())
    } else {
        String::new()
    };
    format!(
        "fact {}{} : {}",
        ref_str,
        padding,
        AsLemmaSource(&fact.value)
    )
}

/// Compute the maximum fact reference width across a slice of facts.
fn max_ref_width(facts: &[&LemmaFact]) -> usize {
    facts
        .iter()
        .map(|f| format!("{}", f.reference).len())
        .max()
        .unwrap_or(0)
}

/// Group facts into two sections separated by a blank line:
///
/// 1. Regular facts (literals, type declarations) — original order, aligned
/// 2. Document references, each followed by its cross-doc overrides — original order, aligned per sub-group
fn format_sorted_facts(facts: &[LemmaFact], out: &mut String) {
    let mut regular: Vec<&LemmaFact> = Vec::new();
    let mut doc_refs: Vec<&LemmaFact> = Vec::new();
    let mut overrides: Vec<&LemmaFact> = Vec::new();

    for fact in facts {
        if !fact.reference.is_local() {
            overrides.push(fact);
        } else if matches!(&fact.value, FactValue::DocumentReference(_)) {
            doc_refs.push(fact);
        } else {
            regular.push(fact);
        }
    }

    // Helper: emit an aligned group of facts
    let emit_group = |facts: &[&LemmaFact], out: &mut String| {
        let width = max_ref_width(facts);
        for fact in facts {
            out.push_str(&format_fact(fact, width));
            out.push('\n');
        }
    };

    // Group 1: Regular facts (literals + type declarations), original order, aligned
    if !regular.is_empty() {
        out.push('\n');
        emit_group(&regular, out);
    }

    // Group 2: Doc references, each followed by its overrides
    if !doc_refs.is_empty() {
        out.push('\n');
        for (i, doc_fact) in doc_refs.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let ref_name = &doc_fact.reference.name;
            let mut sub_group: Vec<&LemmaFact> = vec![doc_fact];
            for ovr in &overrides {
                if ovr.reference.segments.first().map(|s| s.as_str()) == Some(ref_name.as_str()) {
                    sub_group.push(ovr);
                }
            }
            emit_group(&sub_group, out);
        }
    }

    // Any overrides that didn't match a doc ref (shouldn't happen in valid Lemma, but be safe)
    let matched_prefixes: Vec<&str> = doc_refs.iter().map(|f| f.reference.name.as_str()).collect();
    let unmatched: Vec<&LemmaFact> = overrides
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
// Expressions — produce valid Lemma source with precedence-based parens
// =============================================================================

/// Format an expression as valid Lemma source (flat, no wrapping).
///
/// Uses `AsLemmaSource<Value>` for literals and the AST types' `Display` impls
/// for operators, rule references, etc. (those `Display` impls already emit
/// valid Lemma syntax for these simple tokens).
fn format_expr(expr: &Expression, parent_prec: u8) -> String {
    let my_prec = expression_precedence(&expr.kind);

    let needs_parens = parent_prec < 10 && my_prec < parent_prec;

    let inner = match &expr.kind {
        ExpressionKind::Literal(lit) => format!("{}", AsLemmaSource(lit)),
        ExpressionKind::Reference(r) => format!("{}", r),
        ExpressionKind::UnresolvedUnitLiteral(..) => {
            // Expression::Display already normalizes the decimal.
            format!("{}", expr)
        }
        ExpressionKind::Arithmetic(left, op, right) => {
            let left_str = format_expr(left, my_prec);
            let right_str = format_expr(right, my_prec);
            format!("{} {} {}", left_str, op.symbol(), right_str)
        }
        ExpressionKind::Comparison(left, op, right) => {
            let left_str = format_expr(left, my_prec);
            let right_str = format_expr(right, my_prec);
            format!("{} {} {}", left_str, op.symbol(), right_str)
        }
        ExpressionKind::UnitConversion(value, target) => {
            let value_str = format_expr(value, my_prec);
            format!("{} in {}", value_str, target)
        }
        ExpressionKind::LogicalNegation(inner_expr, _) => {
            let inner_str = format_expr(inner_expr, my_prec);
            format!("not {}", inner_str)
        }
        ExpressionKind::LogicalAnd(left, right) => {
            let left_str = format_expr(left, my_prec);
            let right_str = format_expr(right, my_prec);
            format!("{} and {}", left_str, right_str)
        }
        ExpressionKind::MathematicalComputation(op, operand) => {
            let operand_str = format_expr(operand, my_prec);
            format!("{} {}", op, operand_str)
        }
        ExpressionKind::Veto(veto) => match &veto.message {
            Some(msg) => format!("veto {}", crate::parsing::ast::quote_lemma_text(msg)),
            None => "veto".to_string(),
        },
    };

    if needs_parens {
        format!("({})", inner)
    } else {
        inner
    }
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
            let op_symbol = op.symbol();
            let single_line = format!("{} {} {}", left_str, op_symbol, right_str);
            if single_line.len() <= max_cols && !single_line.contains('\n') {
                return wrap_in_parens(single_line);
            }
            let continued_right = indent_after_first_line(&right_str, indent);
            let continuation = format!("{}{} {}", indent, op_symbol, continued_right);
            let multi_line = format!("{}\n{}", left_str, continuation);
            wrap_in_parens(multi_line)
        }
        _ => {
            let s = format_expr(expr, parent_prec);
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
        assert_eq!(fmt_value(&v), "42.5");
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
        assert_eq!(fmt_value(&v), "99.5 eur");
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
        let source = r#"doc test

fact name: "Alice"

rule greeting: "hello"
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        assert!(formatted.contains("\"Alice\""), "fact text must be quoted");
        assert!(formatted.contains("\"hello\""), "rule text must be quoted");
    }

    #[test]
    fn test_format_source_preserves_percent() {
        let source = r#"doc test

fact rate: 10 percent

rule tax: rate * 21%
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        assert!(
            formatted.contains("10%"),
            "fact percent must use shorthand %, got: {}",
            formatted
        );
    }

    #[test]
    fn test_format_groups_facts_preserving_order() {
        // Facts are deliberately mixed: the formatter keeps all regular facts together
        // in original order, aligned
        let source = r#"doc test

fact income: [number -> minimum 0]
fact filing_status: [filing_status_type -> default "single"]
fact country: "NL"
fact deductions: [number -> minimum 0]
fact name: [text]

rule total: income
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        let fact_section = formatted
            .split("rule total")
            .next()
            .unwrap()
            .split("doc test\n")
            .nth(1)
            .unwrap();
        let lines: Vec<&str> = fact_section.lines().filter(|l| !l.is_empty()).collect();
        // All regular facts in one group, original order, aligned
        assert_eq!(lines[0], "fact income        : [number -> minimum 0]");
        assert_eq!(
            lines[1],
            "fact filing_status : [filing_status_type -> default \"single\"]"
        );
        assert_eq!(lines[2], "fact country       : \"NL\"");
        assert_eq!(lines[3], "fact deductions    : [number -> minimum 0]");
        assert_eq!(lines[4], "fact name          : [text]");
    }

    #[test]
    fn test_format_groups_doc_refs_with_overrides() {
        let source = r#"doc test

fact retail.quantity: 5
fact wholesale: doc order/wholesale
fact retail: doc order/retail
fact wholesale.quantity: 100
fact base_price: 50

rule total: base_price
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        let fact_section = formatted
            .split("rule total")
            .next()
            .unwrap()
            .split("doc test\n")
            .nth(1)
            .unwrap();
        let lines: Vec<&str> = fact_section.lines().filter(|l| !l.is_empty()).collect();
        // Group 1: Literals
        assert_eq!(lines[0], "fact base_price : 50");
        // Group 4: Doc refs in original order, each with its overrides, aligned
        assert_eq!(lines[1], "fact wholesale          : doc order/wholesale");
        assert_eq!(lines[2], "fact wholesale.quantity : 100");
        assert_eq!(lines[3], "fact retail          : doc order/retail");
        assert_eq!(lines[4], "fact retail.quantity : 5");
    }

    #[test]
    fn test_format_source_weather_clothing_text_quoted() {
        let source = r#"doc weather_clothing

type clothing_style: text
  -> option "light"
  -> option "warm"

fact temperature: [number]

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
        let source = r#"doc test

type status: text
  -> option "active"
  -> option "inactive"

fact s: [status]

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
        let source = r#"doc test
fact quantity: [number -> help "Number of items to order"]
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
        let source = r#"doc test

type money: scale
  -> unit eur 1.00
  -> unit usd 1.10
  -> decimals 2
  -> minimum 0

fact price: [money]

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
}
