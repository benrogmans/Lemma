use lemma::{Lexer, TokenKind};
use tower_lsp::lsp_types::*;

/// Legend indices — must stay in sync with TOKEN_TYPES order and monaco.js SEMANTIC_TOKEN_TYPES.
const IDX_NAMESPACE: u32 = 0; // repo keyword + all qualifier tokens
const IDX_CLASS: u32 = 1; // spec keyword + spec name tokens
const IDX_PROPERTY: u32 = 2; // data keyword + field path tokens (before colon)
const IDX_FUNCTION: u32 = 3; // rule keyword + rule name token (colon excluded)
const IDX_VALUE: u32 = 4; // every value: literals, booleans, duration units, identifiers in body
const IDX_COMMENT: u32 = 5;
const IDX_KEYWORD: u32 = 6; // type/constraint words (muted; business users don't need them)
const IDX_OPERATOR: u32 = 7;
const IDX_CONTROL: u32 = 8; // unless, then, uses, and, not, from, in, veto, …
const IDX_DATA_BODY: u32 = 9; // data block after the colon
const IDX_PUNCTUATION: u32 = 10; // colons after data field path and rule name
const IDX_REFERENCE: u32 = 11; // identifiers in rule/spec body (paths, aliases, …)

/// Custom LSP token types not in the standard set.
pub const CONTROL_KEYWORD: SemanticTokenType = SemanticTokenType::new("controlKeyword");
pub const DATA_BODY: SemanticTokenType = SemanticTokenType::new("dataBody");
pub const PUNCTUATION: SemanticTokenType = SemanticTokenType::new("punctuation");
pub const REFERENCE: SemanticTokenType = SemanticTokenType::new("reference");

/// Ordered legend. Index positions are the `IDX_*` constants above.
pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE, // 0
    SemanticTokenType::CLASS,     // 1
    SemanticTokenType::PROPERTY,  // 2
    SemanticTokenType::FUNCTION,  // 3
    SemanticTokenType::STRING,    // 4 — the "value" bucket
    SemanticTokenType::COMMENT,   // 5
    SemanticTokenType::KEYWORD,   // 6
    SemanticTokenType::OPERATOR,  // 7
    CONTROL_KEYWORD,              // 8
    DATA_BODY,                    // 9
    PUNCTUATION,                  // 10
    REFERENCE,                    // 11
];

pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[];

/// Tracks whether the scan is currently consuming a declaration header.
#[derive(Clone, Copy, PartialEq)]
enum HeaderState {
    /// Not inside a declaration header — standard body context.
    None,
    /// After `repo`: consume qualifier tokens (At, Identifier, separators).
    Repo,
    /// After `spec`: consume spec name tokens.
    Spec,
    /// After `data`: consume field-path tokens until Colon.
    Data,
    /// After `data … :` — type annotation + constraint arrows, all IDX_DATA_BODY.
    DataBody,
    /// After `rule`: consume the single rule-name token then `RuleColon`.
    Rule,
    /// After rule name: expect Colon as IDX_PUNCTUATION, then body.
    RuleColon,
}

/// Classify a token that appears in body (non-header) context.
fn type_in_body(kind: &TokenKind) -> Option<u32> {
    match kind {
        // Control / flow surface — subdued but distinct
        TokenKind::Unless
        | TokenKind::Then
        | TokenKind::Uses
        | TokenKind::Not
        | TokenKind::And
        | TokenKind::In
        | TokenKind::Type
        | TokenKind::Meta
        | TokenKind::Veto
        | TokenKind::Now
        | TokenKind::Past
        | TokenKind::Future
        // Structural `repo` in body/stray position (not `repo` declaration or qualifier segment)
        | TokenKind::Repo => Some(IDX_CONTROL),

        // Type system / constraint keywords — muted; business users skip past these
        TokenKind::QuantityKw
        | TokenKind::NumberKw
        | TokenKind::TextKw
        | TokenKind::DateKw
        | TokenKind::TimeKw
        | TokenKind::BooleanKw
        | TokenKind::PercentKw
        | TokenKind::RatioKw => Some(IDX_KEYWORD),

        // Math function names — muted; treat as type-system noise
        TokenKind::Sqrt
        | TokenKind::Sin
        | TokenKind::Cos
        | TokenKind::Tan
        | TokenKind::Asin
        | TokenKind::Acos
        | TokenKind::Atan
        | TokenKind::Log
        | TokenKind::Exp
        | TokenKind::Abs
        | TokenKind::Floor
        | TokenKind::Ceil
        | TokenKind::Round => Some(IDX_KEYWORD),

        // Operators
        TokenKind::Plus
        | TokenKind::Minus
        | TokenKind::Star
        | TokenKind::Slash
        | TokenKind::Percent
        | TokenKind::PercentPercent
        | TokenKind::Caret
        | TokenKind::Gt
        | TokenKind::Lt
        | TokenKind::Gte
        | TokenKind::Lte
        | TokenKind::Arrow
        | TokenKind::Is => Some(IDX_OPERATOR),

        // Comment
        TokenKind::Commentary => Some(IDX_COMMENT),

        // Value-like: literals, booleans, @ in body (identifiers
        // use [`expression_semantic_type`] → IDX_REFERENCE).
        TokenKind::At
        | TokenKind::StringLit
        | TokenKind::NumberLit
        | TokenKind::True
        | TokenKind::False
        | TokenKind::Yes
        | TokenKind::No
        | TokenKind::Accept
        | TokenKind::Reject
        | TokenKind::Permille => Some(IDX_VALUE),

        // Punctuation (Colon, Dot, Comma, LParen, RParen, …) — transparent
        _ => None,
    }
}

/// Semantic type in spec/rule/repo body: literals and operators from
/// [`type_in_body`], plus [`TokenKind::Identifier`] as [`IDX_REFERENCE`].
fn expression_semantic_type(kind: &TokenKind) -> Option<u32> {
    type_in_body(kind).or(if matches!(kind, TokenKind::Identifier) {
        Some(IDX_REFERENCE)
    } else {
        None
    })
}

/// Returns true for token kinds that can legally appear as a rule name.
/// Type keywords are also legal identifiers in that position in the grammar.
fn is_name_token(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::QuantityKw
            | TokenKind::NumberKw
            | TokenKind::TextKw
            | TokenKind::DateKw
            | TokenKind::TimeKw
            | TokenKind::BooleanKw
            | TokenKind::PercentKw
            | TokenKind::RatioKw
    )
}

/// Produce LSP delta-encoded semantic tokens for `text` using a stateful
/// single-pass scan over the lexer token stream.
///
/// Declaration headers (`repo`, `spec`, `data` keyword + field path, `rule` +
/// name) use dedicated buckets; `dataBody` covers the data block after the
/// colon; `punctuation` covers declaration colons. Rule bodies use reference,
/// value, operator, keyword, and control buckets.
pub fn tokenize(text: &str) -> Vec<SemanticToken> {
    let mut lexer = Lexer::new(text, &lemma::SourceType::Volatile);
    let mut tokens = Vec::new();
    let mut prev_line: u32 = 0;
    let mut prev_col: u32 = 0;
    let mut state = HeaderState::None;

    while let Ok(tok) = lexer.next_token() {
        if tok.kind == TokenKind::Eof {
            break;
        }

        // Declaration keywords always (re-)start a header state from any context.
        // `repo` is handled inside [`HeaderState::None`] and [`HeaderState::Repo`] so
        // `spec repo` does not reinterpret the keyword as a new repository declaration.
        // Returns (type_index, modifier_bits).
        let token_info: Option<(u32, u32)> = match tok.kind {
            TokenKind::Spec => {
                state = HeaderState::Spec;
                Some((IDX_CLASS, 0))
            }
            TokenKind::Data => {
                state = HeaderState::Data;
                Some((IDX_PROPERTY, 0))
            }
            TokenKind::With => {
                state = HeaderState::Data;
                Some((IDX_PROPERTY, 0))
            }
            TokenKind::Rule => {
                state = HeaderState::Rule;
                Some((IDX_FUNCTION, 0))
            }

            _ => match state {
                HeaderState::Repo => match tok.kind {
                    // Qualifier segments (keyword text may appear in paths, e.g. `@org/repo`)
                    TokenKind::At | TokenKind::Identifier | TokenKind::Repo => {
                        Some((IDX_NAMESPACE, 0))
                    }
                    // Separators within a qualifier (/, ., -) — transparent but stay in state
                    TokenKind::Slash | TokenKind::Dot | TokenKind::Minus => None,
                    // Anything else terminates the header; reprocess in body context
                    _ => {
                        state = HeaderState::None;
                        expression_semantic_type(&tok.kind).map(|idx| (idx, 0))
                    }
                },

                HeaderState::Spec => match tok.kind {
                    TokenKind::Identifier => Some((IDX_CLASS, 0)),
                    // NumberLit covers the effective-from year/date (e.g. `spec foo 2025`
                    // or `spec foo 2026-03-04`). Minus is already transparent so date
                    // separators stay invisible between the coloured number segments.
                    TokenKind::NumberLit => Some((IDX_CLASS, 0)),
                    TokenKind::Slash | TokenKind::Dot | TokenKind::Minus => None,
                    _ => {
                        state = HeaderState::None;
                        expression_semantic_type(&tok.kind).map(|idx| (idx, 0))
                    }
                },

                HeaderState::Data => match tok.kind {
                    TokenKind::Identifier => Some((IDX_PROPERTY, 0)),
                    TokenKind::Dot => None,
                    TokenKind::Colon => {
                        state = HeaderState::DataBody;
                        Some((IDX_PUNCTUATION, 0))
                    }
                    _ => {
                        state = HeaderState::DataBody;
                        Some((IDX_DATA_BODY, 0))
                    }
                },

                HeaderState::DataBody => {
                    if tok.kind == TokenKind::Commentary {
                        Some((IDX_COMMENT, 0))
                    } else if type_in_body(&tok.kind) == Some(IDX_CONTROL) {
                        state = HeaderState::None;
                        Some((IDX_CONTROL, 0))
                    } else {
                        Some((IDX_DATA_BODY, 0))
                    }
                }

                HeaderState::Rule => {
                    if is_name_token(&tok.kind) {
                        state = HeaderState::RuleColon;
                        Some((IDX_FUNCTION, 0))
                    } else {
                        state = HeaderState::None;
                        expression_semantic_type(&tok.kind).map(|idx| (idx, 0))
                    }
                }

                HeaderState::RuleColon => {
                    state = HeaderState::None;
                    if tok.kind == TokenKind::Colon {
                        Some((IDX_PUNCTUATION, 0))
                    } else {
                        expression_semantic_type(&tok.kind).map(|idx| (idx, 0))
                    }
                }

                HeaderState::None => {
                    if tok.kind == TokenKind::Repo {
                        state = HeaderState::Repo;
                        Some((IDX_NAMESPACE, 0))
                    } else {
                        expression_semantic_type(&tok.kind).map(|idx| (idx, 0))
                    }
                }
            },
        };

        let (type_idx, modifier_bits) = match token_info {
            Some(info) => info,
            None => continue,
        };

        let start_line = (tok.span.line as u32).saturating_sub(1);
        let start_col = (tok.span.col as u32).saturating_sub(1);

        // Commentary text excludes the `"""` delimiters but the span covers
        // them. Reconstruct the full visual text so the delimiters get colored.
        let full_commentary;
        let display_text = if tok.kind == TokenKind::Commentary {
            full_commentary = format!("\"\"\"{}\"\"\"", tok.text);
            &full_commentary
        } else {
            &tok.text
        };

        // A single lexer token can span multiple lines (e.g. block comments).
        // Emit one SemanticToken per visual line segment.
        let lines: Vec<&str> = display_text.split('\n').collect();
        for (i, segment) in lines.iter().enumerate() {
            let seg_len = segment.chars().count() as u32;
            if seg_len == 0 {
                continue;
            }

            let line = start_line + i as u32;
            let col = if i == 0 { start_col } else { 0 };

            let delta_line = line - prev_line;
            let delta_start = if delta_line == 0 { col - prev_col } else { col };

            tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length: seg_len,
                token_type: type_idx,
                token_modifiers_bitset: modifier_bits,
            });

            prev_line = line;
            prev_col = col;
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_types(text: &str) -> Vec<u32> {
        tokenize(text).iter().map(|t| t.token_type).collect()
    }

    #[test]
    fn repo_keyword_and_qualifier_same_colour() {
        // repo → NAMESPACE, @lemma → NAMESPACE (At + Identifier), / transparent, std → NAMESPACE
        assert_eq!(
            token_types("repo @lemma/std"),
            vec![IDX_NAMESPACE, IDX_NAMESPACE, IDX_NAMESPACE, IDX_NAMESPACE]
        );
    }

    #[test]
    fn simple_repo_name_no_qualifier() {
        assert_eq!(
            token_types("repo local"),
            vec![IDX_NAMESPACE, IDX_NAMESPACE]
        );
    }

    #[test]
    fn spec_keyword_and_name_same_colour() {
        assert_eq!(
            token_types("spec weather_clothing"),
            vec![IDX_CLASS, IDX_CLASS]
        );
    }

    /// `repo` after `spec` is invalid Lemma; highlight as control, not as a `repo` declaration.
    #[test]
    fn spec_followed_by_repo_keyword_is_control_not_namespace() {
        assert_eq!(token_types("spec repo"), vec![IDX_CLASS, IDX_CONTROL]);
    }

    #[test]
    fn data_keyword_field_type_and_colon() {
        // data → PROPERTY, temperature → PROPERTY, : PUNCTUATION, number → DATA_BODY
        assert_eq!(
            token_types("data temperature: number"),
            vec![IDX_PROPERTY, IDX_PROPERTY, IDX_PUNCTUATION, IDX_DATA_BODY]
        );
    }

    #[test]
    fn data_body_constraints_all_data_body_after_header() {
        let text = "data temperature: quantity\n  -> unit celsius 1.0\n  -> minimum -70 celsius";
        let types = token_types(text);
        assert_eq!(&types[..3], &[IDX_PROPERTY, IDX_PROPERTY, IDX_PUNCTUATION]);
        assert!(types.iter().skip(3).all(|&t| t == IDX_DATA_BODY));
    }

    #[test]
    fn data_body_ends_at_next_declaration() {
        let types = token_types("data x: number\nrule y: 5");
        assert_eq!(
            types,
            vec![
                IDX_PROPERTY,
                IDX_PROPERTY,
                IDX_PUNCTUATION,
                IDX_DATA_BODY,
                IDX_FUNCTION,
                IDX_FUNCTION,
                IDX_PUNCTUATION,
                IDX_VALUE,
            ]
        );
    }

    #[test]
    fn with_dotted_path_colon_punctuation() {
        // with → PROPERTY, employee → PROPERTY, . transparent, name → PROPERTY, : PUNCTUATION
        assert_eq!(
            token_types("with employee.name:"),
            vec![IDX_PROPERTY, IDX_PROPERTY, IDX_PROPERTY, IDX_PUNCTUATION]
        );
    }

    #[test]
    fn rule_keyword_name_and_colon() {
        // rule → FUNCTION, needs_umbrella → FUNCTION, : PUNCTUATION, 42 → VALUE
        assert_eq!(
            token_types("rule needs_umbrella: 42"),
            vec![IDX_FUNCTION, IDX_FUNCTION, IDX_PUNCTUATION, IDX_VALUE]
        );
    }

    #[test]
    fn unless_then_are_control() {
        // rule → FUNCTION, x → FUNCTION, : PUNCTUATION, yes → VALUE
        // unless → CONTROL, a → REFERENCE, then → CONTROL, no → VALUE
        assert_eq!(
            token_types("rule x: yes\n  unless a then no"),
            vec![
                IDX_FUNCTION,
                IDX_FUNCTION,
                IDX_PUNCTUATION,
                IDX_VALUE,
                IDX_CONTROL,
                IDX_REFERENCE,
                IDX_CONTROL,
                IDX_VALUE,
            ]
        );
    }

    #[test]
    fn uses_is_control() {
        // spec → CLASS, s → CLASS, uses → CONTROL, alias → REFERENCE
        assert_eq!(
            token_types("spec s\nuses alias"),
            vec![IDX_CLASS, IDX_CLASS, IDX_CONTROL, IDX_REFERENCE]
        );
    }

    #[test]
    fn rule_body_identifiers_are_reference() {
        assert_eq!(
            token_types("rule r: x"),
            vec![IDX_FUNCTION, IDX_FUNCTION, IDX_PUNCTUATION, IDX_REFERENCE,]
        );
    }

    #[test]
    fn condition_references_and_literals() {
        // rule → FUNCTION, x → FUNCTION, : PUNCTUATION, 1 → VALUE
        // unless → CONTROL, temperature → REFERENCE, < → OPERATOR, 5 → VALUE, then → CONTROL, 2 → VALUE
        assert_eq!(
            token_types("rule x: 1\n  unless temperature < 5 then 2"),
            vec![
                IDX_FUNCTION,
                IDX_FUNCTION,
                IDX_PUNCTUATION,
                IDX_VALUE,
                IDX_CONTROL,
                IDX_REFERENCE,
                IDX_OPERATOR,
                IDX_VALUE,
                IDX_CONTROL,
                IDX_VALUE,
            ]
        );
    }

    #[test]
    fn string_and_number_and_bool_all_value() {
        assert_eq!(
            token_types("rule a: \"hello\"\nrule b: 42\nrule c: yes"),
            vec![
                IDX_FUNCTION,
                IDX_FUNCTION,
                IDX_PUNCTUATION,
                IDX_VALUE,
                IDX_FUNCTION,
                IDX_FUNCTION,
                IDX_PUNCTUATION,
                IDX_VALUE,
                IDX_FUNCTION,
                IDX_FUNCTION,
                IDX_PUNCTUATION,
                IDX_VALUE,
            ]
        );
    }

    #[test]
    fn spec_effective_year_same_colour_as_name() {
        // spec → CLASS, weather_clothing → CLASS, 2025 → CLASS
        assert_eq!(
            token_types("spec weather_clothing 2025"),
            vec![IDX_CLASS, IDX_CLASS, IDX_CLASS]
        );
    }

    #[test]
    fn spec_effective_full_date_same_colour_as_name() {
        // spec → CLASS, foo → CLASS, 2026 → CLASS, - transparent, 03 → CLASS, - transparent, 04 → CLASS
        assert_eq!(
            token_types("spec foo 2026-03-04"),
            vec![IDX_CLASS, IDX_CLASS, IDX_CLASS, IDX_CLASS, IDX_CLASS]
        );
    }

    #[test]
    fn commentary_delimiters_colored_as_comment() {
        let toks = tokenize("\"\"\"hello\"\"\"");
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].token_type, IDX_COMMENT);
        // length covers """hello""" = 11 chars
        assert_eq!(toks[0].length, 11);
    }

    #[test]
    fn multiline_commentary_delimiters_colored() {
        let toks = tokenize("\"\"\"\nHello\n\"\"\"");
        // Three segments: """ (line 0), Hello (line 1), """ (line 2)
        assert_eq!(toks.len(), 3);
        assert!(toks.iter().all(|t| t.token_type == IDX_COMMENT));
        assert_eq!(toks[0].length, 3); // opening """
        assert_eq!(toks[1].length, 5); // Hello
        assert_eq!(toks[2].length, 3); // closing """
    }

    #[test]
    fn declaration_keywords_restart_state_from_any_context() {
        // spec → CLASS, a → CLASS, data → PROPERTY (restarts from Spec state), x → PROPERTY, : PUNCTUATION
        assert_eq!(
            token_types("spec a\ndata x:"),
            vec![
                IDX_CLASS,
                IDX_CLASS,
                IDX_PROPERTY,
                IDX_PROPERTY,
                IDX_PUNCTUATION
            ]
        );
    }
}
