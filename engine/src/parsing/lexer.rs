use crate::error::Error;
use crate::parsing::ast::Span;
use crate::parsing::source::Source;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Spec,
    Fact,
    Rule,
    Unless,
    Then,
    Not,
    And,
    In,
    Type,
    From,
    With,
    Meta,
    Veto,
    Now,
    Calendar,
    Past,
    Future,

    // Boolean keywords
    True,
    False,
    Yes,
    No,
    Accept,
    Reject,

    // Type keywords
    ScaleKw,
    NumberKw,
    TextKw,
    DateKw,
    TimeKw,
    DurationKw,
    BooleanKw,
    PercentKw,
    RatioKw,

    // Math function keywords
    Sqrt,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Log,
    Exp,
    Abs,
    Floor,
    Ceil,
    Round,

    // Duration unit keywords
    Years,
    Year,
    Months,
    Month,
    Weeks,
    Week,
    Days,
    Day,
    Hours,
    Hour,
    Minutes,
    Minute,
    Seconds,
    Second,
    Milliseconds,
    Millisecond,
    Microseconds,
    Microsecond,
    Permille,

    // Comparison keyword operators
    Is,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PercentPercent,
    Caret,
    Gt,
    Lt,
    Gte,
    Lte,
    EqEq,
    BangEq,

    // Punctuation
    Colon,
    Arrow,
    Tilde,
    Dot,
    At,
    LParen,
    RParen,
    LBracket,
    RBracket,

    // Literals
    NumberLit,
    StringLit,

    // Commentary (raw text between """ delimiters)
    Commentary,

    // Identifiers
    Identifier,

    // End of file
    Eof,
}

impl std::fmt::Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenKind::Spec => write!(f, "'spec'"),
            TokenKind::Fact => write!(f, "'fact'"),
            TokenKind::Rule => write!(f, "'rule'"),
            TokenKind::Unless => write!(f, "'unless'"),
            TokenKind::Then => write!(f, "'then'"),
            TokenKind::Not => write!(f, "'not'"),
            TokenKind::And => write!(f, "'and'"),
            TokenKind::In => write!(f, "'in'"),
            TokenKind::Type => write!(f, "'type'"),
            TokenKind::From => write!(f, "'from'"),
            TokenKind::With => write!(f, "'with'"),
            TokenKind::Meta => write!(f, "'meta'"),
            TokenKind::Veto => write!(f, "'veto'"),
            TokenKind::Now => write!(f, "'now'"),
            TokenKind::Calendar => write!(f, "'calendar'"),
            TokenKind::Past => write!(f, "'past'"),
            TokenKind::Future => write!(f, "'future'"),
            TokenKind::True => write!(f, "'true'"),
            TokenKind::False => write!(f, "'false'"),
            TokenKind::Yes => write!(f, "'yes'"),
            TokenKind::No => write!(f, "'no'"),
            TokenKind::Accept => write!(f, "'accept'"),
            TokenKind::Reject => write!(f, "'reject'"),
            TokenKind::ScaleKw => write!(f, "'scale'"),
            TokenKind::NumberKw => write!(f, "'number'"),
            TokenKind::TextKw => write!(f, "'text'"),
            TokenKind::DateKw => write!(f, "'date'"),
            TokenKind::TimeKw => write!(f, "'time'"),
            TokenKind::DurationKw => write!(f, "'duration'"),
            TokenKind::BooleanKw => write!(f, "'boolean'"),
            TokenKind::PercentKw => write!(f, "'percent'"),
            TokenKind::RatioKw => write!(f, "'ratio'"),
            TokenKind::Sqrt => write!(f, "'sqrt'"),
            TokenKind::Sin => write!(f, "'sin'"),
            TokenKind::Cos => write!(f, "'cos'"),
            TokenKind::Tan => write!(f, "'tan'"),
            TokenKind::Asin => write!(f, "'asin'"),
            TokenKind::Acos => write!(f, "'acos'"),
            TokenKind::Atan => write!(f, "'atan'"),
            TokenKind::Log => write!(f, "'log'"),
            TokenKind::Exp => write!(f, "'exp'"),
            TokenKind::Abs => write!(f, "'abs'"),
            TokenKind::Floor => write!(f, "'floor'"),
            TokenKind::Ceil => write!(f, "'ceil'"),
            TokenKind::Round => write!(f, "'round'"),
            TokenKind::Years => write!(f, "'years'"),
            TokenKind::Year => write!(f, "'year'"),
            TokenKind::Months => write!(f, "'months'"),
            TokenKind::Month => write!(f, "'month'"),
            TokenKind::Weeks => write!(f, "'weeks'"),
            TokenKind::Week => write!(f, "'week'"),
            TokenKind::Days => write!(f, "'days'"),
            TokenKind::Day => write!(f, "'day'"),
            TokenKind::Hours => write!(f, "'hours'"),
            TokenKind::Hour => write!(f, "'hour'"),
            TokenKind::Minutes => write!(f, "'minutes'"),
            TokenKind::Minute => write!(f, "'minute'"),
            TokenKind::Seconds => write!(f, "'seconds'"),
            TokenKind::Second => write!(f, "'second'"),
            TokenKind::Milliseconds => write!(f, "'milliseconds'"),
            TokenKind::Millisecond => write!(f, "'millisecond'"),
            TokenKind::Microseconds => write!(f, "'microseconds'"),
            TokenKind::Microsecond => write!(f, "'microsecond'"),
            TokenKind::Permille => write!(f, "'permille'"),
            TokenKind::Is => write!(f, "'is'"),
            TokenKind::Plus => write!(f, "'+'"),
            TokenKind::Minus => write!(f, "'-'"),
            TokenKind::Star => write!(f, "'*'"),
            TokenKind::Slash => write!(f, "'/'"),
            TokenKind::Percent => write!(f, "'%'"),
            TokenKind::PercentPercent => write!(f, "'%%'"),
            TokenKind::Caret => write!(f, "'^'"),
            TokenKind::Gt => write!(f, "'>'"),
            TokenKind::Lt => write!(f, "'<'"),
            TokenKind::Gte => write!(f, "'>='"),
            TokenKind::Lte => write!(f, "'<='"),
            TokenKind::EqEq => write!(f, "'=='"),
            TokenKind::BangEq => write!(f, "'!='"),
            TokenKind::Colon => write!(f, "':'"),
            TokenKind::Arrow => write!(f, "'->'"),
            TokenKind::Tilde => write!(f, "'~'"),
            TokenKind::Dot => write!(f, "'.'"),
            TokenKind::At => write!(f, "'@'"),
            TokenKind::LParen => write!(f, "'('"),
            TokenKind::RParen => write!(f, "')'"),
            TokenKind::LBracket => write!(f, "'['"),
            TokenKind::RBracket => write!(f, "']'"),
            TokenKind::NumberLit => write!(f, "a number"),
            TokenKind::StringLit => write!(f, "a string"),
            TokenKind::Commentary => write!(f, "commentary block"),
            TokenKind::Identifier => write!(f, "an identifier"),
            TokenKind::Eof => write!(f, "end of file"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub text: String,
}

impl Token {
    pub fn eof(offset: usize, line: usize, col: usize) -> Self {
        Token {
            kind: TokenKind::Eof,
            span: Span {
                start: offset,
                end: offset,
                line,
                col,
            },
            text: String::new(),
        }
    }
}

pub struct Lexer {
    source: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    byte_offset: usize,
    attribute: String,
    source_text: Arc<str>,
    peeked: Option<Token>,
    peeked2: Option<Token>,
}

impl Lexer {
    pub fn new(input: &str, attribute: &str) -> Self {
        let source_text: Arc<str> = Arc::from(input);
        Lexer {
            source: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            byte_offset: 0,
            attribute: attribute.to_string(),
            source_text,
            peeked: None,
            peeked2: None,
        }
    }

    pub fn source_text(&self) -> Arc<str> {
        self.source_text.clone()
    }

    pub fn attribute(&self) -> &str {
        &self.attribute
    }

    pub fn peek(&mut self) -> Result<&Token, Error> {
        if self.peeked.is_none() {
            let token = self.lex_token()?;
            self.peeked = Some(token);
        }
        Ok(self.peeked.as_ref().expect("just assigned"))
    }

    pub fn peek_second(&mut self) -> Result<&Token, Error> {
        self.peek()?;
        if self.peeked2.is_none() {
            let token = self.lex_token()?;
            self.peeked2 = Some(token);
        }
        Ok(self.peeked2.as_ref().expect("just assigned"))
    }

    /// Current raw position as a Span. Does not trigger tokenization.
    pub fn current_span(&self) -> Span {
        Span {
            start: self.byte_offset,
            end: self.byte_offset,
            line: self.line,
            col: self.col,
        }
    }

    /// Scan a contiguous run of alphanumeric characters as a raw string,
    /// bypassing normal tokenization. Used for content hashes after `~`
    /// where sequences like `7e20848b` must not be split by scientific
    /// notation scanning.
    pub fn scan_raw_alphanumeric(&mut self) -> Result<String, Error> {
        self.peeked = None;
        self.peeked2 = None;
        self.skip_whitespace();
        let mut result = String::new();
        while let Some(ch) = self.current_char() {
            if ch.is_ascii_alphanumeric() {
                result.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        Ok(result)
    }

    pub fn next_token(&mut self) -> Result<Token, Error> {
        if let Some(token) = self.peeked.take() {
            self.peeked = self.peeked2.take();
            return Ok(token);
        }
        self.lex_token()
    }

    fn current_char(&self) -> Option<char> {
        self.source.get(self.pos).copied()
    }

    fn peek_char(&self) -> Option<char> {
        self.source.get(self.pos + 1).copied()
    }

    fn peek_char_at(&self, offset: usize) -> Option<char> {
        self.source.get(self.pos + offset).copied()
    }

    fn advance(&mut self) {
        if let Some(ch) = self.current_char() {
            self.byte_offset += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.pos += 1;
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.current_char() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn make_span(&self, start_byte: usize, start_line: usize, start_col: usize) -> Span {
        Span {
            start: start_byte,
            end: self.byte_offset,
            line: start_line,
            col: start_col,
        }
    }

    fn make_error(&self, message: impl Into<String>, span: Span) -> Error {
        Error::parsing(
            message,
            Source::new(&self.attribute, span, self.source_text.clone()),
            None::<String>,
        )
    }

    fn lex_token(&mut self) -> Result<Token, Error> {
        self.skip_whitespace();

        let start_byte = self.byte_offset;
        let start_line = self.line;
        let start_col = self.col;

        let Some(ch) = self.current_char() else {
            return Ok(Token::eof(start_byte, start_line, start_col));
        };

        // Triple-quote commentary
        if ch == '"' && self.peek_char() == Some('"') && self.peek_char_at(2) == Some('"') {
            return self.scan_triple_quote(start_byte, start_line, start_col);
        }

        // String literal
        if ch == '"' {
            return self.scan_string(start_byte, start_line, start_col);
        }

        // Number literal (sign handled by parser, not lexer)
        if ch.is_ascii_digit() {
            return self.scan_number(start_byte, start_line, start_col);
        }

        // Two-character operators (check before single-char)
        if let Some(token) = self.try_two_char_operator(start_byte, start_line, start_col) {
            return Ok(token);
        }

        // Single-character operators/punctuation
        if let Some(kind) = self.single_char_token(ch) {
            self.advance();
            let span = self.make_span(start_byte, start_line, start_col);
            let text = ch.to_string();
            return Ok(Token { kind, span, text });
        }

        // Identifier or keyword (starts with letter or @)
        if ch.is_ascii_alphabetic() || ch == '_' {
            return Ok(self.scan_identifier(start_byte, start_line, start_col));
        }

        // @ prefix for registry references
        if ch == '@' {
            self.advance();
            let span = self.make_span(start_byte, start_line, start_col);
            return Ok(Token {
                kind: TokenKind::At,
                span,
                text: "@".to_string(),
            });
        }

        // Unknown character
        self.advance();
        let span = self.make_span(start_byte, start_line, start_col);
        Err(self.make_error(format!("Unexpected character '{}'", ch), span))
    }

    fn scan_triple_quote(
        &mut self,
        start_byte: usize,
        start_line: usize,
        start_col: usize,
    ) -> Result<Token, Error> {
        self.advance(); // "
        self.advance(); // "
        self.advance(); // "

        let content_start = self.byte_offset;
        loop {
            match self.current_char() {
                None => {
                    let span = self.make_span(start_byte, start_line, start_col);
                    return Err(self.make_error(
                        "Unterminated commentary block: expected closing \"\"\"",
                        span,
                    ));
                }
                Some('"')
                    if self.source.get(self.pos + 1) == Some(&'"')
                        && self.source.get(self.pos + 2) == Some(&'"') =>
                {
                    let content_end = self.byte_offset;
                    self.advance(); // "
                    self.advance(); // "
                    self.advance(); // "
                    let raw: String = self.source_text[content_start..content_end].to_string();
                    let span = self.make_span(start_byte, start_line, start_col);
                    return Ok(Token {
                        kind: TokenKind::Commentary,
                        span,
                        text: raw,
                    });
                }
                Some(_) => {
                    self.advance();
                }
            }
        }
    }

    fn scan_string(
        &mut self,
        start_byte: usize,
        start_line: usize,
        start_col: usize,
    ) -> Result<Token, Error> {
        self.advance(); // consume opening "
        let mut content = String::new();
        loop {
            match self.current_char() {
                None => {
                    let span = self.make_span(start_byte, start_line, start_col);
                    return Err(self.make_error("String starting here was never closed", span));
                }
                Some('"') => {
                    self.advance(); // consume closing "
                    break;
                }
                Some(ch) => {
                    content.push(ch);
                    self.advance();
                }
            }
        }
        let span = self.make_span(start_byte, start_line, start_col);
        // Store the full text including quotes for span accuracy,
        // but content without quotes for the parser to use.
        let full_text = format!("\"{}\"", content);
        Ok(Token {
            kind: TokenKind::StringLit,
            span,
            text: full_text,
        })
    }

    fn scan_number(
        &mut self,
        start_byte: usize,
        start_line: usize,
        start_col: usize,
    ) -> Result<Token, Error> {
        let mut text = String::new();

        // Integer part: digits with optional _ or , separators
        while let Some(ch) = self.current_char() {
            if ch.is_ascii_digit() || ch == '_' || ch == ',' {
                text.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Decimal part
        if self.current_char() == Some('.') {
            // Check if next char after dot is a digit (not a method call or dotted reference)
            if let Some(next) = self.peek_char() {
                if next.is_ascii_digit() {
                    text.push('.');
                    self.advance(); // consume .
                    while let Some(ch) = self.current_char() {
                        if ch.is_ascii_digit() {
                            text.push(ch);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        // Scientific notation: e or E followed by optional +/- and digits
        if let Some(ch) = self.current_char() {
            if ch == 'e' || ch == 'E' {
                let mut sci_text = String::new();
                sci_text.push(ch);
                let save_pos = self.pos;
                let save_byte = self.byte_offset;
                let save_line = self.line;
                let save_col = self.col;
                self.advance(); // consume e/E

                if let Some(sign) = self.current_char() {
                    if sign == '+' || sign == '-' {
                        sci_text.push(sign);
                        self.advance();
                    }
                }

                if let Some(d) = self.current_char() {
                    if d.is_ascii_digit() {
                        while let Some(ch) = self.current_char() {
                            if ch.is_ascii_digit() {
                                sci_text.push(ch);
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        text.push_str(&sci_text);
                    } else {
                        // Not actually scientific notation, backtrack
                        self.pos = save_pos;
                        self.byte_offset = save_byte;
                        self.line = save_line;
                        self.col = save_col;
                    }
                } else {
                    self.pos = save_pos;
                    self.byte_offset = save_byte;
                    self.line = save_line;
                    self.col = save_col;
                }
            }
        }

        let span = self.make_span(start_byte, start_line, start_col);
        Ok(Token {
            kind: TokenKind::NumberLit,
            span,
            text,
        })
    }

    fn try_two_char_operator(
        &mut self,
        start_byte: usize,
        start_line: usize,
        start_col: usize,
    ) -> Option<Token> {
        let ch = self.current_char()?;
        let next = self.peek_char();

        let kind = match (ch, next) {
            ('-', Some('>')) => TokenKind::Arrow,
            ('>', Some('=')) => TokenKind::Gte,
            ('<', Some('=')) => TokenKind::Lte,
            ('=', Some('=')) => TokenKind::EqEq,
            ('!', Some('=')) => TokenKind::BangEq,
            ('%', Some('%')) => {
                // Check that it's not followed by a digit (invalid permille like 10%%5)
                TokenKind::PercentPercent
            }
            _ => return None,
        };

        self.advance();
        self.advance();
        let span = self.make_span(start_byte, start_line, start_col);
        let text: String = self.source_text[span.start..span.end].to_string();
        Some(Token { kind, span, text })
    }

    fn single_char_token(&self, ch: char) -> Option<TokenKind> {
        match ch {
            '+' => Some(TokenKind::Plus),
            '*' => Some(TokenKind::Star),
            '/' => Some(TokenKind::Slash),
            '^' => Some(TokenKind::Caret),
            ':' => Some(TokenKind::Colon),
            '~' => Some(TokenKind::Tilde),
            '.' => Some(TokenKind::Dot),
            '(' => Some(TokenKind::LParen),
            ')' => Some(TokenKind::RParen),
            '[' => Some(TokenKind::LBracket),
            ']' => Some(TokenKind::RBracket),
            '>' => Some(TokenKind::Gt),
            '<' => Some(TokenKind::Lt),
            '%' => Some(TokenKind::Percent),
            '-' => Some(TokenKind::Minus),
            _ => None,
        }
    }

    fn scan_identifier(&mut self, start_byte: usize, start_line: usize, start_col: usize) -> Token {
        let mut text = String::new();
        while let Some(ch) = self.current_char() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                text.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let kind = keyword_from_identifier(&text);
        let span = self.make_span(start_byte, start_line, start_col);
        Token { kind, span, text }
    }
}

fn keyword_from_identifier(text: &str) -> TokenKind {
    match text.to_lowercase().as_str() {
        "spec" => TokenKind::Spec,
        "fact" => TokenKind::Fact,
        "rule" => TokenKind::Rule,
        "unless" => TokenKind::Unless,
        "then" => TokenKind::Then,
        "not" => TokenKind::Not,
        "and" => TokenKind::And,
        "in" => TokenKind::In,
        "type" => TokenKind::Type,
        "from" => TokenKind::From,
        "with" => TokenKind::With,
        "meta" => TokenKind::Meta,
        "veto" => TokenKind::Veto,
        "now" => TokenKind::Now,
        "calendar" => TokenKind::Calendar,
        "past" => TokenKind::Past,
        "future" => TokenKind::Future,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "yes" => TokenKind::Yes,
        "no" => TokenKind::No,
        "accept" => TokenKind::Accept,
        "reject" => TokenKind::Reject,
        "scale" => TokenKind::ScaleKw,
        "number" => TokenKind::NumberKw,
        "text" => TokenKind::TextKw,
        "date" => TokenKind::DateKw,
        "time" => TokenKind::TimeKw,
        "duration" => TokenKind::DurationKw,
        "boolean" => TokenKind::BooleanKw,
        "percent" => TokenKind::PercentKw,
        "ratio" => TokenKind::RatioKw,
        "sqrt" => TokenKind::Sqrt,
        "sin" => TokenKind::Sin,
        "cos" => TokenKind::Cos,
        "tan" => TokenKind::Tan,
        "asin" => TokenKind::Asin,
        "acos" => TokenKind::Acos,
        "atan" => TokenKind::Atan,
        "log" => TokenKind::Log,
        "exp" => TokenKind::Exp,
        "abs" => TokenKind::Abs,
        "floor" => TokenKind::Floor,
        "ceil" => TokenKind::Ceil,
        "round" => TokenKind::Round,
        "is" => TokenKind::Is,
        "years" => TokenKind::Years,
        "year" => TokenKind::Year,
        "months" => TokenKind::Months,
        "month" => TokenKind::Month,
        "weeks" => TokenKind::Weeks,
        "week" => TokenKind::Week,
        "days" => TokenKind::Days,
        "day" => TokenKind::Day,
        "hours" => TokenKind::Hours,
        "hour" => TokenKind::Hour,
        "minutes" => TokenKind::Minutes,
        "minute" => TokenKind::Minute,
        "seconds" => TokenKind::Seconds,
        "second" => TokenKind::Second,
        "milliseconds" => TokenKind::Milliseconds,
        "millisecond" => TokenKind::Millisecond,
        "microseconds" => TokenKind::Microseconds,
        "microsecond" => TokenKind::Microsecond,
        "permille" => TokenKind::Permille,
        _ => TokenKind::Identifier,
    }
}

/// Structural keywords can never be used as identifiers (fact/rule names).
/// Type keywords (scale, number, text, date, time, duration, boolean, percent, ratio)
/// CAN be used as names because `reference_segment` accepts them
/// via the `type_standard` alternative.
pub fn is_structural_keyword(kind: &TokenKind) -> bool {
    matches!(
        kind,
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
            | TokenKind::Sqrt
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
            | TokenKind::Round
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Yes
            | TokenKind::No
            | TokenKind::Accept
            | TokenKind::Reject
    )
}

/// Returns true if the given token kind represents a type keyword
/// (used for type declarations and inline type annotations).
pub fn is_type_keyword(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::BooleanKw
            | TokenKind::ScaleKw
            | TokenKind::NumberKw
            | TokenKind::PercentKw
            | TokenKind::RatioKw
            | TokenKind::TextKw
            | TokenKind::DateKw
            | TokenKind::TimeKw
            | TokenKind::DurationKw
    )
}

/// Returns true if the token kind represents a boolean literal keyword.
pub fn is_boolean_keyword(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::True
            | TokenKind::False
            | TokenKind::Yes
            | TokenKind::No
            | TokenKind::Accept
            | TokenKind::Reject
    )
}

/// Returns true if the token kind represents a duration unit keyword.
pub fn is_duration_unit(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Years
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
            | TokenKind::PercentKw
    )
}

/// Returns true if the token kind represents a math function keyword.
pub fn is_math_function(kind: &TokenKind) -> bool {
    matches!(
        kind,
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
            | TokenKind::Round
    )
}

/// Returns true if the token kind can start the body of a spec
/// (fact, rule, type, or meta definition).
pub fn is_spec_body_keyword(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Fact | TokenKind::Rule | TokenKind::Type | TokenKind::Meta
    )
}

/// Returns true if the token can be used as a label/identifier
/// (i.e. it is a non-reserved keyword or an identifier).
/// Some keywords like duration units, calendar units, etc. are allowed
/// as identifiers in certain contexts (e.g. unit names, type names).
pub fn can_be_label(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::Calendar
            | TokenKind::Past
            | TokenKind::Future
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
            | TokenKind::Permille
            | TokenKind::Is
    )
}

/// Returns true if the token kind can be used as a reference segment
/// (identifier, type keyword, or non-reserved contextual keyword).
pub fn can_be_reference_segment(kind: &TokenKind) -> bool {
    can_be_label(kind) || is_type_keyword(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_all(input: &str) -> Result<Vec<Token>, Error> {
        let mut lexer = Lexer::new(input, "test.lemma");
        let mut tokens = Vec::new();
        loop {
            let token = lexer.next_token()?;
            if token.kind == TokenKind::Eof {
                tokens.push(token);
                break;
            }
            tokens.push(token);
        }
        Ok(tokens)
    }

    fn lex_kinds(input: &str) -> Result<Vec<TokenKind>, Error> {
        Ok(lex_all(input)?.into_iter().map(|t| t.kind).collect())
    }

    #[test]
    fn lex_empty_input() {
        let tokens = lex_all("").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Eof);
    }

    #[test]
    fn lex_spec_declaration() {
        let kinds = lex_kinds("spec person").unwrap();
        assert_eq!(
            kinds,
            vec![TokenKind::Spec, TokenKind::Identifier, TokenKind::Eof]
        );
    }

    #[test]
    fn lex_fact_definition() {
        let kinds = lex_kinds("fact age: 25").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Fact,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::NumberLit,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lex_rule_with_comparison() {
        let kinds = lex_kinds("rule is_adult: age >= 18").unwrap();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Rule,
                TokenKind::Identifier,
                TokenKind::Colon,
                TokenKind::Identifier,
                TokenKind::Gte,
                TokenKind::NumberLit,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lex_string_literal() {
        let tokens = lex_all(r#""hello world""#).unwrap();
        assert_eq!(tokens[0].kind, TokenKind::StringLit);
        assert_eq!(tokens[0].text, "\"hello world\"");
    }

    #[test]
    fn lex_unterminated_string() {
        let result = lex_all(r#""hello"#);
        assert!(result.is_err());
    }

    #[test]
    fn lex_number_with_decimal() {
        let tokens = lex_all("3.14").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::NumberLit);
        assert_eq!(tokens[0].text, "3.14");
    }

    #[test]
    fn lex_number_with_underscores() {
        let tokens = lex_all("1_000_000").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::NumberLit);
        assert_eq!(tokens[0].text, "1_000_000");
    }

    #[test]
    fn lex_scientific_notation() {
        let tokens = lex_all("1.5e+10").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::NumberLit);
        assert_eq!(tokens[0].text, "1.5e+10");
    }

    #[test]
    fn lex_all_operators() {
        let kinds = lex_kinds("+ - * / % ^ > < >= <= == != -> %%").unwrap();
        assert_eq!(
            &kinds[..14],
            &[
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Percent,
                TokenKind::Caret,
                TokenKind::Gt,
                TokenKind::Lt,
                TokenKind::Gte,
                TokenKind::Lte,
                TokenKind::EqEq,
                TokenKind::BangEq,
                TokenKind::Arrow,
                TokenKind::PercentPercent,
            ]
        );
    }

    #[test]
    fn lex_keywords() {
        let kinds = lex_kinds("spec fact rule unless then not and in type from with meta veto now")
            .unwrap();
        assert_eq!(
            &kinds[..14],
            &[
                TokenKind::Spec,
                TokenKind::Fact,
                TokenKind::Rule,
                TokenKind::Unless,
                TokenKind::Then,
                TokenKind::Not,
                TokenKind::And,
                TokenKind::In,
                TokenKind::Type,
                TokenKind::From,
                TokenKind::With,
                TokenKind::Meta,
                TokenKind::Veto,
                TokenKind::Now,
            ]
        );
    }

    #[test]
    fn lex_boolean_keywords() {
        let kinds = lex_kinds("true false yes no accept reject").unwrap();
        assert_eq!(
            &kinds[..6],
            &[
                TokenKind::True,
                TokenKind::False,
                TokenKind::Yes,
                TokenKind::No,
                TokenKind::Accept,
                TokenKind::Reject,
            ]
        );
    }

    #[test]
    fn lex_duration_keywords() {
        let kinds = lex_kinds("years months weeks days hours minutes seconds").unwrap();
        assert_eq!(
            &kinds[..7],
            &[
                TokenKind::Years,
                TokenKind::Months,
                TokenKind::Weeks,
                TokenKind::Days,
                TokenKind::Hours,
                TokenKind::Minutes,
                TokenKind::Seconds,
            ]
        );
    }

    #[test]
    fn lex_commentary() {
        let tokens = lex_all(r#""""hello world""""#).unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Commentary);
        assert_eq!(tokens[0].text, "hello world");
    }

    #[test]
    fn lex_at_sign() {
        let kinds = lex_kinds("@user").unwrap();
        assert_eq!(kinds[0], TokenKind::At);
        assert_eq!(kinds[1], TokenKind::Identifier);
    }

    #[test]
    fn lex_tilde() {
        let kinds = lex_kinds("~").unwrap();
        assert_eq!(kinds[0], TokenKind::Tilde);
    }

    #[test]
    fn lex_brackets() {
        let kinds = lex_kinds("[number]").unwrap();
        assert_eq!(
            &kinds[..3],
            &[
                TokenKind::LBracket,
                TokenKind::NumberKw,
                TokenKind::RBracket
            ]
        );
    }

    #[test]
    fn lex_parentheses() {
        let kinds = lex_kinds("(x + 1)").unwrap();
        assert_eq!(
            &kinds[..5],
            &[
                TokenKind::LParen,
                TokenKind::Identifier,
                TokenKind::Plus,
                TokenKind::NumberLit,
                TokenKind::RParen,
            ]
        );
    }

    #[test]
    fn lex_dot_for_references() {
        let kinds = lex_kinds("employee.salary").unwrap();
        assert_eq!(
            &kinds[..3],
            &[TokenKind::Identifier, TokenKind::Dot, TokenKind::Identifier]
        );
    }

    #[test]
    fn lex_spec_name_with_slashes() {
        let tokens = lex_all("spec contracts/employment/jack").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Spec);
        // The lexer will see "contracts" as identifier, then "/" as Slash
        // The parser will handle assembling the spec name.
        assert_eq!(tokens[1].kind, TokenKind::Identifier);
    }

    #[test]
    fn lex_number_not_followed_by_e_identifier() {
        // "42 eur" should be number then identifier, not scientific notation
        let tokens = lex_all("42 eur").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::NumberLit);
        assert_eq!(tokens[0].text, "42");
        assert_eq!(tokens[1].kind, TokenKind::Identifier);
        assert_eq!(tokens[1].text, "eur");
    }

    #[test]
    fn lex_unknown_character() {
        let result = lex_all("§");
        assert!(result.is_err());
    }

    #[test]
    fn lex_peek_does_not_consume() {
        let mut lexer = Lexer::new("spec test", "test.lemma");
        let peeked_kind = lexer.peek().unwrap().kind.clone();
        assert_eq!(peeked_kind, TokenKind::Spec);
        let next = lexer.next_token().unwrap();
        assert_eq!(next.kind, TokenKind::Spec);
    }

    #[test]
    fn lex_span_byte_offsets() {
        let tokens = lex_all("spec test").unwrap();
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 4);
        assert_eq!(tokens[0].span.line, 1);
        assert_eq!(tokens[0].span.col, 1);

        assert_eq!(tokens[1].span.start, 5);
        assert_eq!(tokens[1].span.end, 9);
        assert_eq!(tokens[1].span.line, 1);
        assert_eq!(tokens[1].span.col, 6);
    }

    #[test]
    fn lex_multiline_span_tracking() {
        let tokens = lex_all("spec test\nfact x: 1").unwrap();
        // "fact" should be on line 2
        let fact_token = &tokens[2]; // spec, test, fact
        assert_eq!(fact_token.kind, TokenKind::Fact);
        assert_eq!(fact_token.span.line, 2);
        assert_eq!(fact_token.span.col, 1);
    }

    #[test]
    fn lex_case_insensitive_keywords() {
        // Lemma keywords are case-insensitive
        let kinds = lex_kinds("SPEC Fact RULE").unwrap();
        assert_eq!(kinds[0], TokenKind::Spec);
        assert_eq!(kinds[1], TokenKind::Fact);
        assert_eq!(kinds[2], TokenKind::Rule);
    }

    #[test]
    fn lex_math_function_keywords() {
        let kinds =
            lex_kinds("sqrt sin cos tan asin acos atan log exp abs floor ceil round").unwrap();
        assert_eq!(
            &kinds[..13],
            &[
                TokenKind::Sqrt,
                TokenKind::Sin,
                TokenKind::Cos,
                TokenKind::Tan,
                TokenKind::Asin,
                TokenKind::Acos,
                TokenKind::Atan,
                TokenKind::Log,
                TokenKind::Exp,
                TokenKind::Abs,
                TokenKind::Floor,
                TokenKind::Ceil,
                TokenKind::Round,
            ]
        );
    }

    #[test]
    fn lex_is_keyword() {
        let kinds = lex_kinds("status is \"active\"").unwrap();
        assert_eq!(kinds[0], TokenKind::Identifier);
        assert_eq!(kinds[1], TokenKind::Is);
        assert_eq!(kinds[2], TokenKind::StringLit);
    }

    #[test]
    fn lex_percent_not_followed_by_digit() {
        // "50%" should be number then percent
        let kinds = lex_kinds("50%").unwrap();
        assert_eq!(kinds[0], TokenKind::NumberLit);
        assert_eq!(kinds[1], TokenKind::Percent);
    }

    #[test]
    fn lex_number_with_commas() {
        let tokens = lex_all("1,000,000").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::NumberLit);
        assert_eq!(tokens[0].text, "1,000,000");
    }

    #[test]
    fn lex_arrow_chain() {
        let kinds = lex_kinds("-> unit eur 1.00 -> decimals 2").unwrap();
        assert_eq!(kinds[0], TokenKind::Arrow);
        assert_eq!(kinds[1], TokenKind::Identifier);
        assert_eq!(kinds[2], TokenKind::Identifier);
        assert_eq!(kinds[3], TokenKind::NumberLit);
        assert_eq!(kinds[4], TokenKind::Arrow);
    }
}
