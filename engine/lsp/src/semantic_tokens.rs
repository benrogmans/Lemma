use lemma::parsing::lexer::{Lexer, TokenKind};
use tower_lsp::lsp_types::*;

pub const TOKEN_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::KEYWORD,
    SemanticTokenType::TYPE,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::NUMBER,
    SemanticTokenType::STRING,
    SemanticTokenType::COMMENT,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::PROPERTY,
];

fn token_type_index(kind: &TokenKind) -> Option<u32> {
    match kind {
        // Keywords → 0
        TokenKind::Spec
        | TokenKind::Fact
        | TokenKind::Rule
        | TokenKind::Unless
        | TokenKind::Then
        | TokenKind::Not
        | TokenKind::And
        | TokenKind::In
        | TokenKind::Type
        | TokenKind::From
        | TokenKind::With
        | TokenKind::Meta
        | TokenKind::Veto
        | TokenKind::Now
        | TokenKind::Calendar
        | TokenKind::Past
        | TokenKind::Future => Some(0),

        // Type keywords → 1
        TokenKind::ScaleKw
        | TokenKind::NumberKw
        | TokenKind::TextKw
        | TokenKind::DateKw
        | TokenKind::TimeKw
        | TokenKind::DurationKw
        | TokenKind::BooleanKw
        | TokenKind::PercentKw
        | TokenKind::RatioKw => Some(1),

        // Math functions → 2
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
        | TokenKind::Round => Some(2),

        // Identifiers → 3
        TokenKind::Identifier => Some(3),

        // Numbers and duration units → 4
        TokenKind::NumberLit
        | TokenKind::Years
        | TokenKind::Year
        | TokenKind::Months
        | TokenKind::Month
        | TokenKind::Weeks
        | TokenKind::Week
        | TokenKind::Days
        | TokenKind::Day
        | TokenKind::Hours
        | TokenKind::Hour
        | TokenKind::Minutes
        | TokenKind::Minute
        | TokenKind::Seconds
        | TokenKind::Second
        | TokenKind::Milliseconds
        | TokenKind::Millisecond
        | TokenKind::Microseconds
        | TokenKind::Microsecond
        | TokenKind::Permille => Some(4),

        // Strings → 5
        TokenKind::StringLit => Some(5),

        // Comments → 6
        TokenKind::Commentary => Some(6),

        // Operators → 7
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
        | TokenKind::EqEq
        | TokenKind::BangEq
        | TokenKind::Arrow
        | TokenKind::Is => Some(7),

        // Boolean literals → 8
        TokenKind::True
        | TokenKind::False
        | TokenKind::Yes
        | TokenKind::No
        | TokenKind::Accept
        | TokenKind::Reject => Some(8),

        // Punctuation, EOF — not highlighted
        _ => None,
    }
}

pub fn tokenize(text: &str) -> Vec<SemanticToken> {
    let mut lexer = Lexer::new(text, "");
    let mut tokens = Vec::new();
    let mut prev_line: u32 = 0;
    let mut prev_col: u32 = 0;

    while let Ok(tok) = lexer.next_token() {
        if tok.kind == TokenKind::Eof {
            break;
        }

        let type_idx = match token_type_index(&tok.kind) {
            Some(idx) => idx,
            None => continue,
        };

        let start_line = (tok.span.line as u32).saturating_sub(1);
        let start_col = (tok.span.col as u32).saturating_sub(1);

        let lines: Vec<&str> = tok.text.split('\n').collect();
        for (i, segment) in lines.iter().enumerate() {
            let seg_len = segment.chars().count() as u32;
            if seg_len == 0 && i < lines.len() - 1 {
                continue;
            }
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
                token_modifiers_bitset: 0,
            });

            prev_line = line;
            prev_col = col;
        }
    }

    tokens
}
