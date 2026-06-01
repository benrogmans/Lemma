//! Lemma source code formatting.
//!
//! Formats parsed specs into canonical Lemma source text. Uses `AsLemmaSource`
//! and `Expression::Display` for syntax; this module handles layout only.
//! Canonical source includes ASCII-lowercase logical identifier names.

use crate::parsing::ast::{
    expression_precedence, AsLemmaSource, Constraint, DataValue, Expression, ExpressionKind,
    LemmaData, LemmaRule, LemmaSpec,
};
use crate::{parse, Error, ParseResult, ResourceLimits};

/// Soft line length limit. Longer lines may be wrapped (unless clauses, expressions).
/// Data and other constructs are not broken if they exceed this.
/// 56 has been chosen to fit on an average mobile screen with an 11pt font.
pub const MAX_COLS: usize = 56;

// =============================================================================
// Public entry points
// =============================================================================

/// Format a sequence of parsed specs into canonical Lemma source.
///
/// specs are separated by two blank lines.
/// The result ends with a single newline.
#[must_use]
pub fn format_specs(specs: &[LemmaSpec]) -> String {
    let refs: Vec<&LemmaSpec> = specs.iter().collect();
    format_spec_refs(&refs)
}

/// Like [`format_specs`] for borrowed specs (e.g. from [`Arc<LemmaSpec>`](crate::parsing::ast::LemmaSpec)).
#[must_use]
pub fn format_spec_refs(specs: &[&LemmaSpec]) -> String {
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

/// Format a [`ParseResult`] (repository groups + specs) into canonical Lemma source.
#[must_use]
pub fn format_parse_result(result: &ParseResult) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for (repo, specs) in &result.repositories {
        let mut prefix = String::new();
        if let Some(name) = repo.name.as_deref() {
            prefix.push_str("repo ");
            prefix.push_str(name);
            prefix.push_str("\n\n");
        }
        if specs.is_empty() {
            if !prefix.is_empty() {
                blocks.push(prefix);
            }
            continue;
        }
        let body = format_specs(specs.as_slice());
        if prefix.is_empty() {
            blocks.push(body);
        } else {
            prefix.push_str(&body);
            blocks.push(prefix);
        }
    }
    let mut out = blocks.join("\n\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Parse a source string and format it to canonical Lemma source.
///
/// Returns an error if the source does not parse.
pub fn format_source(
    source: &str,
    source_type: crate::parsing::source::SourceType,
) -> Result<String, Error> {
    let limits = ResourceLimits::default();
    let result = parse(source, source_type, &limits)?;
    Ok(format_parse_result(&result))
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
        format_sorted_data(&spec.data, &mut out, "");
    }

    if !spec.rules.is_empty() {
        out.push('\n');
        for (index, rule) in spec.rules.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            let rule_text = format_rule(rule, max_cols);
            for line in rule_text.lines() {
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    out
}

// =============================================================================
// Data
// =============================================================================

/// Two spaces after `line_prefix` for each `-> ...` constraint line under `data ...: ...`.
const DATA_CONSTRAINT_INDENT: &str = "  ";

fn data_constraints_nonempty(constraints: &Option<Vec<Constraint>>) -> bool {
    constraints.as_ref().is_some_and(|v| !v.is_empty())
}

fn data_value_has_arrow_constraints(value: &DataValue) -> bool {
    match value {
        DataValue::Definition { constraints, .. } => data_constraints_nonempty(constraints),
        DataValue::With(_) => false,
        _ => false,
    }
}

fn data_value_rhs_for_spec_body(value: &DataValue, continuation_prefix: &str) -> String {
    match value {
        DataValue::Definition {
            base,
            constraints,
            value,
        } if data_constraints_nonempty(constraints) => {
            let cs = constraints
                .as_ref()
                .expect("BUG: constraints checked above");
            let head: String = if base.is_none() {
                match value {
                    Some(v) => format!("{}", AsLemmaSource(v)),
                    None => String::new(),
                }
            } else {
                match base.as_ref() {
                    Some(b) => format!("{}", b),
                    None => String::new(),
                }
            };
            let mut out = head;
            for (cmd, args) in cs {
                out.push('\n');
                out.push_str(continuation_prefix);
                out.push_str("-> ");
                out.push_str(&crate::parsing::ast::format_constraint_as_source(cmd, args));
            }
            out
        }
        DataValue::With(crate::parsing::ast::WithRhs::Reference { target }) => target.to_string(),
        _ => format!("{}", AsLemmaSource(value)),
    }
}

fn data_declaration_keyword(data: &LemmaData) -> &'static str {
    match &data.value {
        DataValue::Import(_) => unreachable!("BUG: format_data called on Import row"),
        DataValue::With(_) => "with",
        DataValue::Definition { .. } => "data",
    }
}

fn format_data(data: &LemmaData, line_prefix: &str) -> String {
    let kw = data_declaration_keyword(data);
    let ref_str = format!("{}", data.reference);
    let continuation = format!("{line_prefix}{DATA_CONSTRAINT_INDENT}");
    let rhs = data_value_rhs_for_spec_body(&data.value, &continuation);
    if let Some((first, rest)) = rhs.split_once('\n') {
        format!("{kw} {}: {}\n{}", ref_str, first, rest)
    } else {
        format!("{kw} {}: {}", ref_str, rhs)
    }
}

/// Byte length from start of `data ` or `with ` through the single space after `:` (same layout as [`format_data`]).
fn data_line_prefix_len_before_rhs(keyword: &str, ref_str: &str) -> usize {
    keyword.len() + 1 + ref_str.len() + 2
}

fn data_is_simple_single_line(data: &LemmaData, line_prefix: &str) -> bool {
    if data_value_has_arrow_constraints(&data.value) {
        return false;
    }
    let continuation = format!("{line_prefix}{DATA_CONSTRAINT_INDENT}");
    let rhs = data_value_rhs_for_spec_body(&data.value, &continuation);
    !rhs.contains('\n')
}

fn push_formatted_simple_data_line_padded(
    out: &mut String,
    data: &LemmaData,
    line_prefix: &str,
    target_prefix_len_before_rhs: usize,
) {
    let kw = data_declaration_keyword(data);
    let ref_str = format!("{}", data.reference);
    let continuation = format!("{line_prefix}{DATA_CONSTRAINT_INDENT}");
    let rhs = data_value_rhs_for_spec_body(&data.value, &continuation);
    let base = data_line_prefix_len_before_rhs(kw, &ref_str);
    let gap = 1 + target_prefix_len_before_rhs.saturating_sub(base);
    out.push_str(line_prefix);
    out.push_str(kw);
    out.push(' ');
    out.push_str(&ref_str);
    out.push(':');
    out.push_str(&" ".repeat(gap));
    out.push_str(&rhs);
}

fn emit_data_row_group(rows: &[&LemmaData], line_prefix: &str, out: &mut String) {
    let mut i = 0;
    while i < rows.len() {
        if data_is_simple_single_line(rows[i], line_prefix) {
            let run_start = i;
            i += 1;
            while i < rows.len() && data_is_simple_single_line(rows[i], line_prefix) {
                i += 1;
            }
            let run_end = i;
            let target = (run_start..run_end)
                .map(|k| {
                    let row = rows[k];
                    let kw = data_declaration_keyword(row);
                    let ref_str = format!("{}", row.reference);
                    data_line_prefix_len_before_rhs(kw, &ref_str)
                })
                .max()
                .expect("BUG: non-empty run");
            for row in rows[run_start..run_end].iter().copied() {
                push_formatted_simple_data_line_padded(out, row, line_prefix, target);
                out.push('\n');
            }
        } else {
            let row = rows[i];
            out.push_str(line_prefix);
            out.push_str(&format_data(row, line_prefix));
            out.push('\n');
            if data_value_has_arrow_constraints(&row.value) && i + 1 < rows.len() {
                out.push('\n');
            }
            i += 1;
        }
    }
}

fn format_import_row(data: &LemmaData) -> String {
    let alias = &data.reference.name;
    if let DataValue::Import(spec_ref) = &data.value {
        let spec_name = &spec_ref.name;
        let last_segment = spec_name.rsplit('/').next().unwrap_or(spec_name);
        if alias == last_segment {
            format!("uses {}", spec_ref)
        } else {
            format!("uses {}: {}", alias, spec_ref)
        }
    } else {
        unreachable!("BUG: format_import_row called on non-Import data")
    }
}

/// Group data into sections separated by blank lines:
///
/// 1. Imports (`uses`), each followed by their literal bindings — original order within this block
/// 2. Regular data (literals, type declarations, references) — original order
/// 3. Qualified overrides that did not attach to any import — original order
fn format_sorted_data(data: &[LemmaData], out: &mut String, line_prefix: &str) {
    let mut regular: Vec<&LemmaData> = Vec::new();
    let mut imports: Vec<&LemmaData> = Vec::new();
    let mut overrides: Vec<&LemmaData> = Vec::new();

    for data in data {
        if !data.reference.is_local() {
            overrides.push(data);
        } else if matches!(&data.value, DataValue::Import(_)) {
            imports.push(data);
        } else {
            regular.push(data);
        }
    }

    let emit_group =
        |rows: &[&LemmaData], out: &mut String| emit_data_row_group(rows, line_prefix, out);

    if !imports.is_empty() {
        out.push('\n');

        for (i, row) in imports.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(line_prefix);
            out.push_str(&format_import_row(row));
            out.push('\n');
            let ref_name = &row.reference.name;
            let binding_overrides: Vec<&LemmaData> = overrides
                .iter()
                .filter(|o| {
                    o.reference.segments.first().map(|s| s.as_str()) == Some(ref_name.as_str())
                })
                .copied()
                .collect();
            if !binding_overrides.is_empty() {
                emit_data_row_group(&binding_overrides, line_prefix, out);
            }
        }
    }

    if !regular.is_empty() {
        out.push('\n');
        emit_group(&regular, out);
    }

    let matched_prefixes: Vec<&str> = imports.iter().map(|f| f.reference.name.as_str()).collect();
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

const UNLESS_LINE_PREFIX: &str = "  unless ";

/// Logical line length for `max_cols` checks (no extra spec-level indent).
#[inline]
fn spec_line_len(line: &str) -> usize {
    line.len()
}

/// Default expression stays on the `rule name:` line when it fits under `max_cols`.
///
/// Single-line `unless … then …` clauses align `then` when every such line still fits under
/// `max_cols` after alignment. Any clause that splits across lines (expression wraps, or one line
/// would exceed `max_cols`) uses a fixed `then` indent — no column alignment with shorter sisters.
fn format_rule(rule: &LemmaRule, max_cols: usize) -> String {
    let expr_indent = "  ";
    let body = format_expr_wrapped(&rule.expression, max_cols, expr_indent, 10);
    let mut out = String::new();
    out.push_str("rule ");
    out.push_str(&rule.name);
    let body_single_line = !body.contains('\n');
    let header_fits_on_one_line =
        body_single_line && spec_line_len(&format!("rule {}: {}", rule.name, body)) <= max_cols;
    if header_fits_on_one_line {
        out.push_str(": ");
        out.push_str(&body);
    } else {
        out.push_str(":\n");
        out.push_str(expr_indent);
        out.push_str(&body);
    }

    let pl = UNLESS_LINE_PREFIX.len();
    let naive_single_len = |cond: &str, res: &str| pl + cond.len() + 6 + res.len();
    let aligned_single_len = |res: &str, max_end: usize| max_end + 6 + res.len();

    let mut clauses: Vec<(String, String, bool)> = Vec::new();
    for unless_clause in &rule.unless_clauses {
        let condition = format_expr_wrapped(&unless_clause.condition, max_cols, "    ", 10);
        let result = format_expr_wrapped(&unless_clause.result, max_cols, "    ", 10);
        let multiline = condition.contains('\n') || result.contains('\n');
        clauses.push((condition, result, multiline));
    }

    let mut singles: Vec<usize> = clauses
        .iter()
        .enumerate()
        .filter(|(_, (c, r, m))| !*m && naive_single_len(c, r) <= max_cols)
        .map(|(i, _)| i)
        .collect();

    loop {
        if singles.is_empty() {
            break;
        }
        let max_end = singles
            .iter()
            .map(|&i| pl + clauses[i].0.len())
            .max()
            .expect("BUG: singles non-empty");
        let before = singles.len();
        singles.retain(|&i| aligned_single_len(&clauses[i].1, max_end) <= max_cols);
        if singles.len() == before {
            break;
        }
    }

    let align_max_end = singles.iter().map(|&i| pl + clauses[i].0.len()).max();
    const SPLIT_THEN_INDENT_SPACES: usize = 4;

    for (i, (condition, result, multiline)) in clauses.iter().enumerate() {
        if *multiline {
            out.push_str("\n  unless ");
            out.push_str(condition);
            out.push('\n');
            out.push_str(&" ".repeat(SPLIT_THEN_INDENT_SPACES));
            out.push_str("then ");
            out.push_str(result);
            continue;
        }
        if singles.contains(&i) {
            let max_end = align_max_end.expect("BUG: singles.contains but align_max_end empty");
            let gap = 1 + max_end.saturating_sub(pl + condition.len());
            out.push('\n');
            out.push_str(UNLESS_LINE_PREFIX);
            out.push_str(condition);
            out.push_str(&" ".repeat(gap));
            out.push_str("then ");
            out.push_str(result);
            continue;
        }
        out.push_str("\n  unless ");
        out.push_str(condition);
        out.push('\n');
        out.push_str(&" ".repeat(SPLIT_THEN_INDENT_SPACES));
        out.push_str("then ");
        out.push_str(result);
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
        AsLemmaSource, BooleanValue, DateTimeValue, TimeValue, TimezoneValue, Value,
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
    fn test_format_value_quantity() {
        let v = Value::NumberWithUnit(Decimal::from_str("99.50").unwrap(), "eur".to_string());
        assert_eq!(fmt_value(&v), "99.50 eur");
    }

    #[test]
    fn test_format_value_duration_as_quantity() {
        let v = Value::NumberWithUnit(Decimal::from(40), "hours".to_string());
        assert_eq!(fmt_value(&v), "40 hours");
    }

    #[test]
    fn test_format_value_calendar() {
        let v = Value::NumberWithUnit(Decimal::from(6), "month".to_string());
        assert_eq!(fmt_value(&v), "6 month");
    }

    #[test]
    fn test_format_value_ratio_percent() {
        let v = Value::NumberWithUnit(Decimal::from_str("10").unwrap(), "percent".to_string());
        assert_eq!(fmt_value(&v), "10%");
    }

    #[test]
    fn test_format_value_ratio_permille() {
        let v = Value::NumberWithUnit(Decimal::from_str("5").unwrap(), "permille".to_string());
        assert_eq!(fmt_value(&v), "5%%");
    }

    #[test]
    fn test_format_value_number_with_unit_named() {
        let v = Value::NumberWithUnit(
            Decimal::from_str("500").unwrap(),
            "basis_points".to_string(),
        );
        assert_eq!(fmt_value(&v), "500 basis_points");
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
            microsecond: 0,
            timezone: None,
        });
        assert_eq!(fmt_value(&v), "14:30:45");
    }

    #[test]
    fn test_format_source_lowercases_logical_identifiers() {
        let source = r#"spec Test
data Price: number -> default 1
rule Total: price
"#;
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        assert!(formatted.contains("spec test"), "got: {formatted}");
        assert!(formatted.contains("data price"), "got: {formatted}");
        assert!(formatted.contains("rule total"), "got: {formatted}");
    }

    #[test]
    fn test_format_source_round_trips_text() {
        let source = r#"spec test

data name: "Alice"

rule greeting: "hello"
"#;
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        assert!(formatted.contains("\"Alice\""), "data text must be quoted");
        assert!(formatted.contains("\"hello\""), "rule text must be quoted");
    }

    #[test]
    fn test_format_source_preserves_percent() {
        let source = r#"spec test

data rate: 10 percent

rule tax: rate * 21%
"#;
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
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
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        let data_section = formatted
            .split("rule total")
            .next()
            .unwrap()
            .split("spec test\n")
            .nth(1)
            .unwrap();
        let lines: Vec<&str> = data_section.lines().filter(|l| !l.is_empty()).collect();
        // Constrained rows: one blank line after each when more `data` follows.
        assert_eq!(lines[0], "data income: number");
        assert_eq!(lines[1], "  -> minimum 0");
        assert_eq!(lines[2], "data filing_status: filing_status_type");
        assert_eq!(lines[3], "  -> default \"single\"");
        assert_eq!(lines[4], "data country: \"NL\"");
        assert_eq!(lines[5], "data deductions: number");
        assert_eq!(lines[6], "  -> minimum 0");
        assert_eq!(lines[7], "data name: text");
    }

    #[test]
    fn test_format_groups_spec_refs_with_overrides() {
        let source = r#"spec test

with retail.quantity: 5
uses order wholesale
uses order retail
with wholesale.quantity: 100
data base_price: 50

rule total: base_price
"#;
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        let data_section = formatted
            .split("rule total")
            .next()
            .unwrap()
            .split("spec test\n")
            .nth(1)
            .unwrap();
        let lines: Vec<&str> = data_section.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines[0], "uses order wholesale");
        assert_eq!(lines[1], "with wholesale.quantity: 100");
        assert_eq!(lines[2], "uses order retail");
        assert_eq!(lines[3], "with retail.quantity: 5");
        assert_eq!(lines[4], "data base_price: 50");
    }

    #[test]
    fn test_format_groups_with_literals_under_each_uses() {
        let source = r#"spec test

uses x
uses y

with x.name: "Ben"
with y.age: 15

rule r: 1
"#;
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        let data_section = formatted
            .split("rule r")
            .next()
            .unwrap()
            .split("spec test\n")
            .nth(1)
            .unwrap();
        let lines: Vec<&str> = data_section.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines[0], "uses x");
        assert_eq!(lines[1], "with x.name: \"Ben\"");
        assert_eq!(lines[2], "uses y");
        assert_eq!(lines[3], "with y.age: 15");
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
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
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
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
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
        let reparsed = format_source(&formatted, crate::parsing::source::SourceType::Volatile);
        assert!(reparsed.is_ok(), "formatted output should re-parse");
    }

    #[test]
    fn test_format_help_round_trips() {
        let source = r#"spec test
data quantity: number -> help "Number of items to order"
rule total: quantity
"#;
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        assert!(
            formatted.contains("help \"Number of items to order\""),
            "help must be quoted, got: {}",
            formatted
        );
        // Round-trip
        let reparsed = format_source(&formatted, crate::parsing::source::SourceType::Volatile);
        assert!(reparsed.is_ok(), "formatted output should re-parse");
    }

    #[test]
    fn test_format_quantity_type_def_round_trips() {
        let source = r#"spec test

data money: quantity
  -> unit eur 1.00
  -> unit usd 0.91
  -> decimals 2
  -> minimum 0

data price: money

rule total: price
"#;
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        assert!(
            formatted.contains("unit eur 1.00"),
            "quantity unit should not be quoted, got: {}",
            formatted
        );
        // Round-trip
        let reparsed = format_source(&formatted, crate::parsing::source::SourceType::Volatile);
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
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        let again =
            format_source(&formatted, crate::parsing::source::SourceType::Volatile).unwrap();
        assert_eq!(
            formatted, again,
            "AST Display-based format must be idempotent under parse/format"
        );
    }

    #[test]
    fn test_format_rule_default_on_same_line_when_fits() {
        let source = "spec test\nrule r: 1\n";
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        assert!(
            formatted.contains("rule r: 1\n"),
            "default expr should stay on rule line when under MAX_COLS, got:\n{formatted}"
        );
    }

    #[test]
    fn test_format_rule_unless_single_line_when_short() {
        let source = r#"spec test
data a: number
data b: boolean

rule r: no
  unless a < 1 then yes
  unless b then yes
"#;
        let formatted =
            format_source(source, crate::parsing::source::SourceType::Volatile).unwrap();
        assert!(
            formatted.contains("unless a < 1 then yes")
                && formatted.contains("unless b     then yes"),
            "unless stays on one line when under MAX_COLS, got:\n{formatted}"
        );
    }
}
