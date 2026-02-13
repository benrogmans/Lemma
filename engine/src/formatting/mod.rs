//! Lemma source code formatting.
//!
//! Formats parsed documents into canonical Lemma source text.
//! Produces valid, parseable Lemma — does NOT use `Display` impls (those are for
//! human-readable output in evaluation/errors, not for round-trippable source).

use crate::parsing::ast::{
    ArithmeticComputation, BooleanValue, ComparisonComputation, ConversionTarget, DocRef,
    Expression, ExpressionKind, FactReference, FactValue, LemmaDoc, LemmaFact, LemmaRule,
    MathematicalComputation, RuleReference, TypeDef, Value,
};
use crate::{parse, LemmaError, ResourceLimits};
use rust_decimal::Decimal;

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
pub fn format_source(source: &str, attribute: &str) -> Result<String, LemmaError> {
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
    out.push('\n');

    if let Some(ref commentary) = doc.commentary {
        out.push_str("\"\"\"\n");
        out.push_str(commentary);
        out.push_str("\n\"\"\"\n");
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
            out.push_str(&format_type_def(type_def));
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
// Type definitions
// =============================================================================

fn format_type_def(td: &TypeDef) -> String {
    match td {
        TypeDef::Regular {
            name,
            parent,
            constraints,
            ..
        } => {
            let mut out = format!("type {} = {}", name, parent);
            if let Some(constraints) = constraints {
                for (cmd, args) in constraints {
                    out.push_str("\n  -> ");
                    out.push_str(cmd);
                    for arg in args {
                        out.push(' ');
                        out.push_str(arg);
                    }
                }
            }
            out
        }
        TypeDef::Import {
            name,
            from,
            constraints,
            ..
        } => {
            let mut out = format!("type {} from {}", name, format_doc_ref(from));
            if let Some(constraints) = constraints {
                for (cmd, args) in constraints {
                    out.push_str(" -> ");
                    out.push_str(cmd);
                    for arg in args {
                        out.push(' ');
                        out.push_str(arg);
                    }
                }
            }
            out
        }
        TypeDef::Inline { .. } => String::new(),
    }
}

// =============================================================================
// Facts
// =============================================================================

/// Format a fact, optionally with the reference name padded to `align_width` characters
/// for column-aligned `=` signs within a group.
/// When `align_width` is 0 or less than the reference length, no padding is added.
fn format_fact(fact: &LemmaFact, align_width: usize) -> String {
    let ref_str = format_fact_reference(&fact.reference);
    let padded = if align_width > ref_str.len() {
        format!("{:width$}", ref_str, width = align_width)
    } else {
        ref_str
    };
    format!("fact {} = {}", padded, format_fact_value(&fact.value))
}

/// Compute the maximum fact reference width across a slice of facts.
fn max_ref_width(facts: &[&LemmaFact]) -> usize {
    facts
        .iter()
        .map(|f| format_fact_reference(&f.reference).len())
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
            let ref_name = &doc_fact.reference.fact;
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
    let matched_prefixes: Vec<&str> = doc_refs.iter().map(|f| f.reference.fact.as_str()).collect();
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

fn format_fact_reference(r: &FactReference) -> String {
    let mut out = String::new();
    for segment in &r.segments {
        out.push_str(segment);
        out.push('.');
    }
    out.push_str(&r.fact);
    out
}

fn format_fact_value(fv: &FactValue) -> String {
    match fv {
        FactValue::Literal(v) => format_value(v),
        FactValue::DocumentReference(doc_ref) => {
            format!("doc {}", format_doc_ref(doc_ref))
        }
        FactValue::TypeDeclaration {
            base,
            constraints,
            from,
        } => {
            let base_str = if let Some(from_doc) = from {
                format!("{} from {}", base, format_doc_ref(from_doc))
            } else {
                base.clone()
            };
            if let Some(ref constraints_vec) = constraints {
                let constraint_str = constraints_vec
                    .iter()
                    .map(|(cmd, args)| {
                        let args_str = args.join(" ");
                        if args_str.is_empty() {
                            cmd.clone()
                        } else {
                            format!("{} {}", cmd, args_str)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" -> ");
                format!("[{} -> {}]", base_str, constraint_str)
            } else {
                format!("[{}]", base_str)
            }
        }
    }
}

fn format_doc_ref(dr: &DocRef) -> String {
    if dr.is_registry {
        format!("@{}", dr.name)
    } else {
        dr.name.clone()
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
    out.push_str(" = ");
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
// Values — produce valid Lemma source (NOT Display)
// =============================================================================

fn format_value(v: &Value) -> String {
    match v {
        Value::Number(n) => format_decimal(n),
        Value::Text(s) => {
            // Text literals must be quoted in Lemma source.
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{}\"", escaped)
        }
        Value::Date(dt) => {
            let is_date_only =
                dt.hour == 0 && dt.minute == 0 && dt.second == 0 && dt.timezone.is_none();
            if is_date_only {
                format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day)
            } else {
                let mut s = format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                    dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
                );
                if let Some(tz) = &dt.timezone {
                    if tz.offset_hours == 0 && tz.offset_minutes == 0 {
                        s.push('Z');
                    } else {
                        let sign = if tz.offset_hours >= 0 { "+" } else { "-" };
                        let hours = tz.offset_hours.abs();
                        s.push_str(&format!("{}{:02}:{:02}", sign, hours, tz.offset_minutes));
                    }
                }
                s
            }
        }
        Value::Time(t) => {
            let mut s = format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second);
            if let Some(tz) = &t.timezone {
                if tz.offset_hours == 0 && tz.offset_minutes == 0 {
                    s.push('Z');
                } else {
                    let sign = if tz.offset_hours >= 0 { "+" } else { "-" };
                    let hours = tz.offset_hours.abs();
                    s.push_str(&format!("{}{:02}:{:02}", sign, hours, tz.offset_minutes));
                }
            }
            s
        }
        Value::Boolean(b) => format_boolean(b),
        Value::Scale(n, u) => format!("{} {}", format_decimal(n), u),
        Value::Duration(n, u) => format!("{} {}", format_decimal(n), format_duration_unit(u)),
        Value::Ratio(n, unit) => format_ratio(n, unit.as_deref()),
    }
}

fn format_boolean(b: &BooleanValue) -> String {
    match b {
        BooleanValue::True => "true",
        BooleanValue::False => "false",
        BooleanValue::Yes => "yes",
        BooleanValue::No => "no",
        BooleanValue::Accept => "accept",
        BooleanValue::Reject => "reject",
    }
    .to_string()
}

fn format_duration_unit(u: &crate::parsing::ast::DurationUnit) -> &'static str {
    use crate::parsing::ast::DurationUnit;
    match u {
        DurationUnit::Year => "years",
        DurationUnit::Month => "months",
        DurationUnit::Week => "weeks",
        DurationUnit::Day => "days",
        DurationUnit::Hour => "hours",
        DurationUnit::Minute => "minutes",
        DurationUnit::Second => "seconds",
        DurationUnit::Millisecond => "milliseconds",
        DurationUnit::Microsecond => "microseconds",
    }
}

/// Format a ratio value as valid Lemma source.
/// Percent uses `N%`; permille uses `N%%`; other units use `N unit`;
/// bare ratios (no unit) are just the number.
fn format_ratio(n: &Decimal, unit: Option<&str>) -> String {
    match unit {
        Some("percent") => {
            let display_value = *n * Decimal::from(100);
            format!("{}%", format_decimal(&display_value))
        }
        Some("permille") => {
            let display_value = *n * Decimal::from(1000);
            format!("{}%%", format_decimal(&display_value))
        }
        Some(unit_name) => {
            format!("{} {}", format_decimal(n), unit_name)
        }
        None => format_decimal(n),
    }
}

/// Format a Decimal, removing trailing zeros and unnecessary fractional parts.
fn format_decimal(n: &Decimal) -> String {
    let norm = n.normalize();
    if norm.fract().is_zero() {
        norm.trunc().to_string()
    } else {
        norm.to_string()
    }
}

// =============================================================================
// Expressions — produce valid Lemma source with precedence-based parens
// =============================================================================

/// Precedence levels (must match parsing/ast.rs).
fn expression_precedence(kind: &ExpressionKind) -> u8 {
    match kind {
        ExpressionKind::LogicalOr(..) => 1,
        ExpressionKind::LogicalAnd(..) => 2,
        ExpressionKind::LogicalNegation(..) => 3,
        ExpressionKind::Comparison(..) => 4,
        ExpressionKind::UnitConversion(..) => 4,
        ExpressionKind::Arithmetic(_, op, _) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => 5,
            ArithmeticComputation::Multiply
            | ArithmeticComputation::Divide
            | ArithmeticComputation::Modulo => 6,
            ArithmeticComputation::Power => 7,
        },
        ExpressionKind::MathematicalComputation(..) => 8,
        ExpressionKind::Literal(..)
        | ExpressionKind::FactReference(..)
        | ExpressionKind::RuleReference(..)
        | ExpressionKind::UnresolvedUnitLiteral(..)
        | ExpressionKind::Veto(..) => 10,
    }
}

/// Format an expression as valid Lemma source (flat, no wrapping).
fn format_expr(expr: &Expression, parent_prec: u8) -> String {
    let my_prec = expression_precedence(&expr.kind);

    let needs_parens = parent_prec < 10 && my_prec < parent_prec;

    let inner = match &expr.kind {
        ExpressionKind::Literal(lit) => format_value(lit),
        ExpressionKind::FactReference(r) => format_fact_reference(r),
        ExpressionKind::RuleReference(rr) => format_rule_reference(rr),
        ExpressionKind::UnresolvedUnitLiteral(number, unit_name) => {
            format!("{} {}", format_decimal(number), unit_name)
        }
        ExpressionKind::Arithmetic(left, op, right) => {
            let left_str = format_expr(left, my_prec);
            let right_str = format_expr(right, my_prec);
            format!("{} {} {}", left_str, op.symbol(), right_str)
        }
        ExpressionKind::Comparison(left, op, right) => {
            let left_str = format_expr(left, my_prec);
            let right_str = format_expr(right, my_prec);
            format!("{} {} {}", left_str, format_comparison_op(op), right_str)
        }
        ExpressionKind::UnitConversion(value, target) => {
            let value_str = format_expr(value, my_prec);
            format!("{} in {}", value_str, format_conversion_target(target))
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
        ExpressionKind::LogicalOr(left, right) => {
            let left_str = format_expr(left, my_prec);
            let right_str = format_expr(right, my_prec);
            format!("{} or {}", left_str, right_str)
        }
        ExpressionKind::MathematicalComputation(op, operand) => {
            let op_name = format_math_op(op);
            let operand_str = format_expr(operand, my_prec);
            format!("{} {}", op_name, operand_str)
        }
        ExpressionKind::Veto(veto) => match &veto.message {
            Some(msg) => {
                let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
                format!("veto \"{}\"", escaped)
            }
            None => "veto".to_string(),
        },
    };

    if needs_parens {
        format!("({})", inner)
    } else {
        inner
    }
}

fn format_rule_reference(rr: &RuleReference) -> String {
    if rr.segments.is_empty() {
        format!("{}?", rr.rule)
    } else {
        format!("{}.{}?", rr.segments.join("."), rr.rule)
    }
}

fn format_comparison_op(op: &ComparisonComputation) -> &'static str {
    match op {
        ComparisonComputation::GreaterThan => ">",
        ComparisonComputation::LessThan => "<",
        ComparisonComputation::GreaterThanOrEqual => ">=",
        ComparisonComputation::LessThanOrEqual => "<=",
        ComparisonComputation::Equal => "==",
        ComparisonComputation::NotEqual => "!=",
        ComparisonComputation::Is => "is",
        ComparisonComputation::IsNot => "is not",
    }
}

fn format_conversion_target(ct: &ConversionTarget) -> String {
    match ct {
        ConversionTarget::Duration(unit) => format_duration_unit(unit).to_string(),
        ConversionTarget::Unit(unit) => unit.clone(),
    }
}

fn format_math_op(op: &MathematicalComputation) -> &'static str {
    match op {
        MathematicalComputation::Sqrt => "sqrt",
        MathematicalComputation::Sin => "sin",
        MathematicalComputation::Cos => "cos",
        MathematicalComputation::Tan => "tan",
        MathematicalComputation::Asin => "asin",
        MathematicalComputation::Acos => "acos",
        MathematicalComputation::Atan => "atan",
        MathematicalComputation::Log => "log",
        MathematicalComputation::Exp => "exp",
        MathematicalComputation::Abs => "abs",
        MathematicalComputation::Floor => "floor",
        MathematicalComputation::Ceil => "ceil",
        MathematicalComputation::Round => "round",
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
    use crate::parsing::ast::{DateTimeValue, DurationUnit, TimeValue, TimezoneValue};
    use rust_decimal::prelude::FromStr;

    #[test]
    fn test_format_value_text_is_quoted() {
        let v = Value::Text("light".to_string());
        assert_eq!(format_value(&v), "\"light\"");
    }

    #[test]
    fn test_format_value_text_escapes_quotes() {
        let v = Value::Text("say \"hello\"".to_string());
        assert_eq!(format_value(&v), "\"say \\\"hello\\\"\"");
    }

    #[test]
    fn test_format_value_number() {
        let v = Value::Number(Decimal::from_str("42.50").unwrap());
        assert_eq!(format_value(&v), "42.5");
    }

    #[test]
    fn test_format_value_number_integer() {
        let v = Value::Number(Decimal::from_str("100.00").unwrap());
        assert_eq!(format_value(&v), "100");
    }

    #[test]
    fn test_format_value_boolean() {
        assert_eq!(format_value(&Value::Boolean(BooleanValue::True)), "true");
        assert_eq!(format_value(&Value::Boolean(BooleanValue::Yes)), "yes");
        assert_eq!(format_value(&Value::Boolean(BooleanValue::No)), "no");
        assert_eq!(
            format_value(&Value::Boolean(BooleanValue::Accept)),
            "accept"
        );
        assert_eq!(
            format_value(&Value::Boolean(BooleanValue::Reject)),
            "reject"
        );
    }

    #[test]
    fn test_format_value_scale() {
        let v = Value::Scale(Decimal::from_str("99.50").unwrap(), "eur".to_string());
        assert_eq!(format_value(&v), "99.5 eur");
    }

    #[test]
    fn test_format_value_duration() {
        let v = Value::Duration(Decimal::from(40), DurationUnit::Hour);
        assert_eq!(format_value(&v), "40 hours");
    }

    #[test]
    fn test_format_value_ratio_percent() {
        let v = Value::Ratio(
            Decimal::from_str("0.10").unwrap(),
            Some("percent".to_string()),
        );
        assert_eq!(format_value(&v), "10%");
    }

    #[test]
    fn test_format_value_ratio_permille() {
        let v = Value::Ratio(
            Decimal::from_str("0.005").unwrap(),
            Some("permille".to_string()),
        );
        assert_eq!(format_value(&v), "5%%");
    }

    #[test]
    fn test_format_value_ratio_bare() {
        let v = Value::Ratio(Decimal::from_str("0.25").unwrap(), None);
        assert_eq!(format_value(&v), "0.25");
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
        assert_eq!(format_value(&v), "2024-01-15");
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
        assert_eq!(format_value(&v), "2024-01-15T14:30:00Z");
    }

    #[test]
    fn test_format_value_time() {
        let v = Value::Time(TimeValue {
            hour: 14,
            minute: 30,
            second: 45,
            timezone: None,
        });
        assert_eq!(format_value(&v), "14:30:45");
    }

    #[test]
    fn test_format_source_round_trips_text() {
        let source = r#"doc test

fact name = "Alice"

rule greeting = "hello"
"#;
        let formatted = format_source(source, "test.lemma").unwrap();
        assert!(formatted.contains("\"Alice\""), "fact text must be quoted");
        assert!(formatted.contains("\"hello\""), "rule text must be quoted");
    }

    #[test]
    fn test_format_source_preserves_percent() {
        let source = r#"doc test

fact rate = 10 percent

rule tax = rate * 21%
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

fact income = [number -> minimum 0]
fact filing_status = [filing_status_type -> default "single"]
fact country = "NL"
fact deductions = [number -> minimum 0]
fact name = [text]

rule total = income
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
        assert_eq!(lines[0], "fact income        = [number -> minimum 0]");
        assert_eq!(
            lines[1],
            "fact filing_status = [filing_status_type -> default \"single\"]"
        );
        assert_eq!(lines[2], "fact country       = \"NL\"");
        assert_eq!(lines[3], "fact deductions    = [number -> minimum 0]");
        assert_eq!(lines[4], "fact name          = [text]");
    }

    #[test]
    fn test_format_groups_doc_refs_with_overrides() {
        let source = r#"doc test

fact retail.quantity = 5
fact wholesale = doc order/wholesale
fact retail = doc order/retail
fact wholesale.quantity = 100
fact base_price = 50

rule total = base_price
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
        assert_eq!(lines[0], "fact base_price = 50");
        // Group 4: Doc refs in original order, each with its overrides, aligned
        assert_eq!(lines[1], "fact wholesale          = doc order/wholesale");
        assert_eq!(lines[2], "fact wholesale.quantity = 100");
        assert_eq!(lines[3], "fact retail          = doc order/retail");
        assert_eq!(lines[4], "fact retail.quantity = 5");
    }

    #[test]
    fn test_format_source_weather_clothing_text_quoted() {
        let source = r#"doc weather_clothing

type clothing_style = text
  -> option "light"
  -> option "warm"

fact temperature = [number]

rule clothing_layer = "light"
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
}
