//! Content hashing for LemmaDoc. Knows how to canonically serialize a doc
//! into bytes and produce an 8-char hex hash. Does NOT traverse graphs.

use crate::parsing::ast::{
    ArithmeticComputation, BooleanValue, CalendarUnit, CommandArg, ComparisonComputation,
    ConversionTarget, DateCalendarKind, DateRelativeKind, DurationUnit, ExpressionKind, FactValue,
    LemmaDoc, LemmaFact, LemmaRule, MathematicalComputation, MetaField, MetaValue, NegationType,
    Reference, TypeDef, UnlessClause, Value,
};
use sha2::{Digest, Sha256};
use std::io::Write;

const HASH_HEX_LEN: usize = 8;

/// Case-insensitive equality of two hash strings.
pub fn content_hash_matches(requested: &str, computed: &str) -> bool {
    requested.eq_ignore_ascii_case(computed)
}

/// First 32 bits of SHA-256 of `bytes`, as 8 lowercase hex chars.
pub fn hash_bytes(bytes: &[u8]) -> String {
    finalize_to_hex(&Sha256::digest(bytes))
}

/// Hash of `primary` bytes concatenated with `dep_hashes`. 8 lowercase hex chars.
pub fn hash_with_deps(primary: &[u8], dep_hashes: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(primary);
    for h in dep_hashes {
        hasher.update(h.as_bytes());
    }
    finalize_to_hex(&hasher.finalize())
}

/// Hash a LemmaDoc with its resolved dependency hashes.
///
/// Serializes the doc's semantic content (name, commentary, meta, types, facts,
/// rules) in sorted, deterministic order. Excludes editor metadata (attribute,
/// start_line, source_location) and hash pins (resolution instructions, not content).
/// Dependency content flows through `dep_hashes`, not inline serialization.
pub fn hash_doc(doc: &LemmaDoc, dep_hashes: &[String]) -> String {
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    write_doc_canonical(&mut buf, doc);
    hash_with_deps(&buf, dep_hashes)
}

fn finalize_to_hex(digest: &[u8]) -> String {
    let n = (u32::from(digest[0]) << 24)
        | (u32::from(digest[1]) << 16)
        | (u32::from(digest[2]) << 8)
        | u32::from(digest[3]);
    format!("{:0width$x}", n, width = HASH_HEX_LEN)
}

// ---------------------------------------------------------------------------
// Canonical serialization — deterministic byte representation of a LemmaDoc
// ---------------------------------------------------------------------------

fn w(buf: &mut Vec<u8>, s: &str) {
    let _ = buf.write_all(s.as_bytes());
}

fn write_doc_canonical(buf: &mut Vec<u8>, doc: &LemmaDoc) {
    w(buf, "doc:");
    w(buf, &doc.name);

    if let Some(ref c) = doc.commentary {
        w(buf, "\ncommentary:");
        w(buf, c);
    }

    // Meta fields sorted by key for determinism
    let mut metas: Vec<&MetaField> = doc.meta_fields.iter().collect();
    metas.sort_by(|a, b| a.key.cmp(&b.key));
    for m in metas {
        w(buf, "\nmeta:");
        w(buf, &m.key);
        w(buf, "=");
        write_meta_value(buf, &m.value);
    }

    // Types sorted by name
    let mut types: Vec<&TypeDef> = doc.types.iter().collect();
    types.sort_by(|a, b| a.name().cmp(b.name()));
    for t in types {
        w(buf, "\ntype:");
        write_type_def(buf, t);
    }

    // Facts sorted by reference display
    let mut facts: Vec<&LemmaFact> = doc.facts.iter().collect();
    facts.sort_by(|a, b| ref_sort_key(&a.reference).cmp(&ref_sort_key(&b.reference)));
    for f in facts {
        w(buf, "\nfact:");
        write_reference(buf, &f.reference);
        w(buf, "=");
        write_fact_value(buf, &f.value);
    }

    // Rules sorted by name
    let mut rules: Vec<&LemmaRule> = doc.rules.iter().collect();
    rules.sort_by(|a, b| a.name.cmp(&b.name));
    for r in rules {
        w(buf, "\nrule:");
        w(buf, &r.name);
        w(buf, "=");
        write_expression(buf, &r.expression.kind);
        for uc in &r.unless_clauses {
            write_unless_clause(buf, uc);
        }
    }
}

fn write_meta_value(buf: &mut Vec<u8>, v: &MetaValue) {
    match v {
        MetaValue::Literal(val) => write_value(buf, val),
        MetaValue::Unquoted(s) => w(buf, s),
    }
}

fn write_type_def(buf: &mut Vec<u8>, td: &TypeDef) {
    match td {
        TypeDef::Regular {
            name,
            parent,
            constraints,
            ..
        } => {
            w(buf, name);
            w(buf, ":");
            w(buf, parent);
            if let Some(cs) = constraints {
                write_constraints(buf, cs);
            }
        }
        TypeDef::Import {
            name,
            source_type,
            from,
            constraints,
            ..
        } => {
            w(buf, name);
            w(buf, ":from:");
            w(buf, &from.name);
            if name != source_type {
                w(buf, ":as:");
                w(buf, source_type);
            }
            if let Some(cs) = constraints {
                write_constraints(buf, cs);
            }
        }
        TypeDef::Inline {
            parent,
            constraints,
            fact_ref,
            from,
            ..
        } => {
            w(buf, "inline:");
            write_reference(buf, fact_ref);
            w(buf, ":");
            w(buf, parent);
            if let Some(doc_ref) = from {
                w(buf, ":from:");
                w(buf, &doc_ref.name);
            }
            if let Some(cs) = constraints {
                write_constraints(buf, cs);
            }
        }
    }
}

fn write_constraints(buf: &mut Vec<u8>, constraints: &[(String, Vec<CommandArg>)]) {
    for (name, args) in constraints {
        w(buf, "->");
        w(buf, name);
        for arg in args {
            w(buf, " ");
            write_command_arg(buf, arg);
        }
    }
}

fn write_command_arg(buf: &mut Vec<u8>, arg: &CommandArg) {
    match arg {
        CommandArg::Number(s) => {
            w(buf, "n:");
            w(buf, s);
        }
        CommandArg::Boolean(s) => {
            w(buf, "b:");
            w(buf, s);
        }
        CommandArg::Text(s) => {
            w(buf, "t:");
            w(buf, s);
        }
        CommandArg::Label(s) => {
            w(buf, "l:");
            w(buf, s);
        }
    }
}

fn write_fact_value(buf: &mut Vec<u8>, fv: &FactValue) {
    match fv {
        FactValue::Literal(val) => write_value(buf, val),
        FactValue::DocumentReference(doc_ref) => {
            // Hash pin excluded: it's a resolution instruction, not content
            w(buf, "docref:");
            w(buf, &doc_ref.name);
        }
        FactValue::TypeDeclaration {
            base,
            constraints,
            from,
        } => {
            w(buf, "typedecl:");
            w(buf, base);
            if let Some(doc_ref) = from {
                w(buf, ":from:");
                w(buf, &doc_ref.name);
            }
            if let Some(cs) = constraints {
                write_constraints(buf, cs);
            }
        }
    }
}

fn write_value(buf: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Number(d) => {
            w(buf, "num:");
            w(buf, &d.to_string());
        }
        Value::Scale(d, unit) => {
            w(buf, "scale:");
            w(buf, &d.to_string());
            w(buf, ":");
            w(buf, unit);
        }
        Value::Text(s) => {
            w(buf, "text:");
            w(buf, s);
        }
        Value::Date(dt) => {
            w(buf, "date:");
            w(
                buf,
                &format!(
                    "{}-{:02}-{:02}T{:02}:{:02}:{:02}",
                    dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
                ),
            );
            if let Some(ref tz) = dt.timezone {
                w(
                    buf,
                    &format!("{:+03}:{:02}", tz.offset_hours, tz.offset_minutes),
                );
            }
        }
        Value::Time(t) => {
            w(buf, "time:");
            w(
                buf,
                &format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second),
            );
            if let Some(ref tz) = t.timezone {
                w(
                    buf,
                    &format!("{:+03}:{:02}", tz.offset_hours, tz.offset_minutes),
                );
            }
        }
        Value::Boolean(b) => {
            w(buf, "bool:");
            w(
                buf,
                match b {
                    BooleanValue::True => "true",
                    BooleanValue::False => "false",
                    BooleanValue::Yes => "yes",
                    BooleanValue::No => "no",
                    BooleanValue::Accept => "accept",
                    BooleanValue::Reject => "reject",
                },
            );
        }
        Value::Duration(d, unit) => {
            w(buf, "dur:");
            w(buf, &d.to_string());
            w(buf, ":");
            write_duration_unit(buf, unit);
        }
        Value::Ratio(d, label) => {
            w(buf, "ratio:");
            w(buf, &d.to_string());
            if let Some(l) = label {
                w(buf, ":");
                w(buf, l);
            }
        }
    }
}

fn write_duration_unit(buf: &mut Vec<u8>, u: &DurationUnit) {
    w(
        buf,
        match u {
            DurationUnit::Year => "year",
            DurationUnit::Month => "month",
            DurationUnit::Week => "week",
            DurationUnit::Day => "day",
            DurationUnit::Hour => "hour",
            DurationUnit::Minute => "minute",
            DurationUnit::Second => "second",
            DurationUnit::Millisecond => "millisecond",
            DurationUnit::Microsecond => "microsecond",
        },
    );
}

fn write_reference(buf: &mut Vec<u8>, r: &Reference) {
    for seg in &r.segments {
        w(buf, seg);
        w(buf, ".");
    }
    w(buf, &r.name);
}

fn ref_sort_key(r: &Reference) -> String {
    let mut s = String::new();
    for seg in &r.segments {
        s.push_str(seg);
        s.push('.');
    }
    s.push_str(&r.name);
    s
}

fn write_expression(buf: &mut Vec<u8>, kind: &ExpressionKind) {
    match kind {
        ExpressionKind::Literal(val) => write_value(buf, val),
        ExpressionKind::Reference(r) => {
            w(buf, "ref:");
            write_reference(buf, r);
        }
        ExpressionKind::UnresolvedUnitLiteral(d, unit) => {
            w(buf, "ulit:");
            w(buf, &d.to_string());
            w(buf, ":");
            w(buf, unit);
        }
        ExpressionKind::LogicalAnd(l, r) => {
            w(buf, "(");
            write_expression(buf, &l.kind);
            w(buf, " and ");
            write_expression(buf, &r.kind);
            w(buf, ")");
        }
        ExpressionKind::Arithmetic(l, op, r) => {
            w(buf, "(");
            write_expression(buf, &l.kind);
            w(
                buf,
                match op {
                    ArithmeticComputation::Add => "+",
                    ArithmeticComputation::Subtract => "-",
                    ArithmeticComputation::Multiply => "*",
                    ArithmeticComputation::Divide => "/",
                    ArithmeticComputation::Modulo => "%",
                    ArithmeticComputation::Power => "^",
                },
            );
            write_expression(buf, &r.kind);
            w(buf, ")");
        }
        ExpressionKind::Comparison(l, op, r) => {
            w(buf, "(");
            write_expression(buf, &l.kind);
            w(
                buf,
                match op {
                    ComparisonComputation::GreaterThan => ">",
                    ComparisonComputation::LessThan => "<",
                    ComparisonComputation::GreaterThanOrEqual => ">=",
                    ComparisonComputation::LessThanOrEqual => "<=",
                    ComparisonComputation::Equal => "==",
                    ComparisonComputation::NotEqual => "!=",
                    ComparisonComputation::Is => " is ",
                    ComparisonComputation::IsNot => " is not ",
                },
            );
            write_expression(buf, &r.kind);
            w(buf, ")");
        }
        ExpressionKind::UnitConversion(expr, target) => {
            w(buf, "(");
            write_expression(buf, &expr.kind);
            w(buf, " in ");
            match target {
                ConversionTarget::Duration(du) => write_duration_unit(buf, du),
                ConversionTarget::Unit(u) => w(buf, u),
            }
            w(buf, ")");
        }
        ExpressionKind::LogicalNegation(expr, neg) => {
            match neg {
                NegationType::Not => w(buf, "not "),
            }
            write_expression(buf, &expr.kind);
        }
        ExpressionKind::MathematicalComputation(op, expr) => {
            w(
                buf,
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
                },
            );
            w(buf, " ");
            write_expression(buf, &expr.kind);
        }
        ExpressionKind::Veto(ve) => {
            w(buf, "veto");
            if let Some(ref msg) = ve.message {
                w(buf, ":");
                w(buf, msg);
            }
        }
        ExpressionKind::Now => {
            w(buf, "now");
        }
        ExpressionKind::DateRelative(kind, date_expr, tolerance) => {
            w(buf, "(");
            write_expression(buf, &date_expr.kind);
            match kind {
                DateRelativeKind::InPast => w(buf, " in past"),
                DateRelativeKind::InFuture => w(buf, " in future"),
            }
            if let Some(tol) = tolerance {
                w(buf, " ");
                write_expression(buf, &tol.kind);
            }
            w(buf, ")");
        }
        ExpressionKind::DateCalendar(kind, unit, date_expr) => {
            w(buf, "(");
            write_expression(buf, &date_expr.kind);
            match kind {
                DateCalendarKind::Current => w(buf, " in calendar"),
                DateCalendarKind::Past => w(buf, " in past calendar"),
                DateCalendarKind::Future => w(buf, " in future calendar"),
                DateCalendarKind::NotIn => w(buf, " not in calendar"),
            }
            match unit {
                CalendarUnit::Year => w(buf, " year"),
                CalendarUnit::Month => w(buf, " month"),
                CalendarUnit::Week => w(buf, " week"),
            }
            w(buf, ")");
        }
    }
}

fn write_unless_clause(buf: &mut Vec<u8>, uc: &UnlessClause) {
    w(buf, "\nunless:");
    write_expression(buf, &uc.condition.kind);
    w(buf, "=>");
    write_expression(buf, &uc.result.kind);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::ast::Span;
    use crate::parsing::ast::{Expression, LemmaFact, LemmaRule, Reference, Value};
    use crate::Source;
    use rust_decimal::Decimal;

    fn src() -> Source {
        Source::new(
            "test",
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 1,
            },
            "test",
            "".into(),
        )
    }

    #[test]
    fn hash_bytes_is_exactly_eight_lowercase_hex_chars() {
        let s = hash_bytes(b"any bytes");
        assert_eq!(s.len(), HASH_HEX_LEN);
        assert!(s
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn content_hash_matches_case_insensitive() {
        assert!(content_hash_matches("a1b2c3d4", "a1b2c3d4"));
        assert!(content_hash_matches("A1B2C3D4", "a1b2c3d4"));
        assert!(!content_hash_matches("a1b2c3d4", "a1b2c3d5"));
    }

    #[test]
    fn hash_doc_is_deterministic() {
        let doc = make_test_doc();
        let h1 = hash_doc(&doc, &[]);
        let h2 = hash_doc(&doc, &[]);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), HASH_HEX_LEN);
    }

    #[test]
    fn hash_doc_changes_with_name() {
        let d1 = make_test_doc();
        let mut d2 = make_test_doc();
        d2.name = "other".to_string();
        assert_ne!(hash_doc(&d1, &[]), hash_doc(&d2, &[]));
    }

    #[test]
    fn hash_doc_changes_with_dep_hashes() {
        let doc = make_test_doc();
        let h1 = hash_doc(&doc, &[]);
        let h2 = hash_doc(&doc, &["abcdef01".to_string()]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_doc_fact_order_irrelevant() {
        let mut d1 = make_test_doc();
        d1.facts = vec![
            make_fact("alpha", Value::Number(Decimal::new(1, 0))),
            make_fact("beta", Value::Number(Decimal::new(2, 0))),
        ];
        let mut d2 = make_test_doc();
        d2.facts = vec![
            make_fact("beta", Value::Number(Decimal::new(2, 0))),
            make_fact("alpha", Value::Number(Decimal::new(1, 0))),
        ];
        assert_eq!(hash_doc(&d1, &[]), hash_doc(&d2, &[]));
    }

    #[test]
    fn hash_doc_rule_order_irrelevant() {
        let mut d1 = make_test_doc();
        d1.rules = vec![make_rule("x", 10), make_rule("y", 20)];
        let mut d2 = make_test_doc();
        d2.rules = vec![make_rule("y", 20), make_rule("x", 10)];
        assert_eq!(hash_doc(&d1, &[]), hash_doc(&d2, &[]));
    }

    #[test]
    fn hash_doc_different_values_differ() {
        let mut d1 = make_test_doc();
        d1.facts = vec![make_fact("x", Value::Number(Decimal::new(1, 0)))];
        let mut d2 = make_test_doc();
        d2.facts = vec![make_fact("x", Value::Number(Decimal::new(2, 0)))];
        assert_ne!(hash_doc(&d1, &[]), hash_doc(&d2, &[]));
    }

    #[test]
    fn hash_doc_commentary_affects_hash() {
        let mut d1 = make_test_doc();
        d1.commentary = None;
        let mut d2 = make_test_doc();
        d2.commentary = Some("important note".to_string());
        assert_ne!(hash_doc(&d1, &[]), hash_doc(&d2, &[]));
    }

    fn make_test_doc() -> LemmaDoc {
        let mut doc = LemmaDoc::new("test".to_string());
        doc.facts = vec![make_fact("price", Value::Number(Decimal::new(100, 0)))];
        doc.rules = vec![make_rule("total", 100)];
        doc
    }

    fn make_fact(name: &str, value: Value) -> LemmaFact {
        LemmaFact {
            reference: Reference::local(name.to_string()),
            value: FactValue::Literal(value),
            source_location: src(),
        }
    }

    fn make_rule(name: &str, value: i64) -> LemmaRule {
        LemmaRule {
            name: name.to_string(),
            expression: Expression {
                kind: ExpressionKind::Literal(Value::Number(Decimal::new(value, 0))),
                source_location: Some(src()),
            },
            unless_clauses: vec![],
            source_location: src(),
        }
    }
}
