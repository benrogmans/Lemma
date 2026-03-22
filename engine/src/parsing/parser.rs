use crate::error::Error;
use crate::limits::ResourceLimits;
use crate::parsing::ast::{try_parse_type_constraint_command, *};
use crate::parsing::lexer::{
    can_be_label, can_be_reference_segment, conversion_target_from_token, is_boolean_keyword,
    is_calendar_unit_token, is_duration_unit, is_math_function, is_spec_body_keyword,
    is_structural_keyword, is_type_keyword, token_kind_to_boolean_value,
    token_kind_to_calendar_unit, token_kind_to_duration_unit, token_kind_to_primitive, Lexer,
    Token, TokenKind,
};
use crate::parsing::source::Source;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;

type TypeArrowChain = (ParentType, Option<SpecRef>, Option<Vec<Constraint>>);

pub struct ParseResult {
    pub specs: Vec<LemmaSpec>,
    pub expression_count: usize,
}

pub fn parse(
    content: &str,
    attribute: &str,
    limits: &ResourceLimits,
) -> Result<ParseResult, Error> {
    if content.len() > limits.max_file_size_bytes {
        return Err(Error::resource_limit_exceeded(
            "max_file_size_bytes",
            format!(
                "{} bytes ({} MB)",
                limits.max_file_size_bytes,
                limits.max_file_size_bytes / (1024 * 1024)
            ),
            format!(
                "{} bytes ({:.2} MB)",
                content.len(),
                content.len() as f64 / (1024.0 * 1024.0)
            ),
            "Reduce file size or split into multiple specs",
            None,
            None,
            None,
        ));
    }

    let mut parser = Parser::new(content, attribute, limits);
    let specs = parser.parse_file()?;
    Ok(ParseResult {
        specs,
        expression_count: parser.expression_count,
    })
}

struct Parser {
    lexer: Lexer,
    depth_tracker: DepthTracker,
    expression_count: usize,
    max_expression_count: usize,
}

impl Parser {
    fn new(content: &str, attribute: &str, limits: &ResourceLimits) -> Self {
        Parser {
            lexer: Lexer::new(content, attribute),
            depth_tracker: DepthTracker::with_max_depth(limits.max_expression_depth),
            expression_count: 0,
            max_expression_count: limits.max_expression_count,
        }
    }

    fn attribute(&self) -> String {
        self.lexer.attribute().to_string()
    }

    fn peek(&mut self) -> Result<&Token, Error> {
        self.lexer.peek()
    }

    fn next(&mut self) -> Result<Token, Error> {
        self.lexer.next_token()
    }

    fn at(&mut self, kind: &TokenKind) -> Result<bool, Error> {
        Ok(&self.peek()?.kind == kind)
    }

    fn at_any(&mut self, kinds: &[TokenKind]) -> Result<bool, Error> {
        let current = &self.peek()?.kind;
        Ok(kinds.contains(current))
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<Token, Error> {
        let token = self.next()?;
        if &token.kind == kind {
            Ok(token)
        } else {
            Err(self.error_at_token(&token, format!("Expected {}, found {}", kind, token.kind)))
        }
    }

    fn error_at_token(&self, token: &Token, message: impl Into<String>) -> Error {
        Error::parsing(
            message,
            Source::new(self.lexer.attribute(), token.span.clone()),
            None::<String>,
        )
    }

    fn error_at_token_with_suggestion(
        &self,
        token: &Token,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Error {
        Error::parsing(
            message,
            Source::new(self.lexer.attribute(), token.span.clone()),
            Some(suggestion),
        )
    }

    /// Parse `~ HASH` where HASH is 8 alphanumeric chars. Optional — returns
    /// `Ok(None)` when the next token is not `~`. Uses raw scanning after the
    /// tilde to bypass tokenization (avoids scientific-notation mis-lexing of
    /// hashes like `7e20848b`).
    fn try_parse_hash_pin(&mut self) -> Result<Option<String>, Error> {
        if !self.at(&TokenKind::Tilde)? {
            return Ok(None);
        }
        let tilde_span = self.next()?.span;
        let hash = self.lexer.scan_raw_alphanumeric()?;
        if hash.len() != 8 {
            return Err(Error::parsing(
                format!(
                    "Expected an 8-character alphanumeric plan hash after '~', found '{}'",
                    hash
                ),
                self.make_source(tilde_span),
                None::<String>,
            ));
        }
        Ok(Some(hash))
    }

    fn make_source(&self, span: Span) -> Source {
        Source::new(self.lexer.attribute(), span)
    }

    fn span_from(&self, start: &Span) -> Span {
        // Create a span from start to the current lexer position.
        // We peek to get the current position.
        Span {
            start: start.start,
            end: start.end.max(start.start),
            line: start.line,
            col: start.col,
        }
    }

    fn span_covering(&self, start: &Span, end: &Span) -> Span {
        Span {
            start: start.start,
            end: end.end,
            line: start.line,
            col: start.col,
        }
    }

    // ========================================================================
    // Top-level: file and spec
    // ========================================================================

    fn parse_file(&mut self) -> Result<Vec<LemmaSpec>, Error> {
        let mut specs = Vec::new();
        loop {
            if self.at(&TokenKind::Eof)? {
                break;
            }
            if self.at(&TokenKind::Spec)? {
                specs.push(self.parse_spec()?);
            } else {
                let token = self.next()?;
                return Err(self.error_at_token_with_suggestion(
                    &token,
                    format!(
                        "Expected a spec declaration (e.g. 'spec my_spec'), found {}",
                        token.kind
                    ),
                    "A Lemma file must start with 'spec <name>'",
                ));
            }
        }
        Ok(specs)
    }

    fn parse_spec(&mut self) -> Result<LemmaSpec, Error> {
        let spec_token = self.expect(&TokenKind::Spec)?;
        let start_line = spec_token.span.line;

        let (name, _name_span) = self.parse_spec_name()?;

        let effective_from = self.try_parse_effective_from()?;

        let commentary = self.try_parse_commentary()?;

        let attribute = self.attribute();
        let mut spec = LemmaSpec::new(name.clone())
            .with_attribute(attribute)
            .with_start_line(start_line);
        spec.effective_from = effective_from;

        if let Some(commentary_text) = commentary {
            spec = spec.set_commentary(commentary_text);
        }

        // First pass: collect type definitions
        // We need to peek and handle type definitions first, but since we consume tokens
        // linearly, we'll collect all items in one pass.
        let mut facts = Vec::new();
        let mut rules = Vec::new();
        let mut types = Vec::new();
        let mut meta_fields = Vec::new();

        loop {
            let peek_kind = self.peek()?.kind.clone();
            match peek_kind {
                TokenKind::Fact => {
                    let fact = self.parse_fact()?;
                    facts.push(fact);
                }
                TokenKind::Rule => {
                    let rule = self.parse_rule()?;
                    rules.push(rule);
                }
                TokenKind::Type => {
                    let type_def = self.parse_type_def()?;
                    types.push(type_def);
                }
                TokenKind::Meta => {
                    let meta = self.parse_meta()?;
                    meta_fields.push(meta);
                }
                TokenKind::Spec | TokenKind::Eof => break,
                _ => {
                    let token = self.next()?;
                    return Err(self.error_at_token_with_suggestion(
                        &token,
                        format!(
                            "Expected 'fact', 'rule', 'type', 'meta', or a new 'spec', found '{}'",
                            token.text
                        ),
                        "Check the spelling or add the appropriate keyword",
                    ));
                }
            }
        }

        for type_def in types {
            spec = spec.add_type(type_def);
        }
        for fact in facts {
            spec = spec.add_fact(fact);
        }
        for rule in rules {
            spec = spec.add_rule(rule);
        }
        for meta in meta_fields {
            spec = spec.add_meta_field(meta);
        }

        Ok(spec)
    }

    /// Parse a spec name: optional @ prefix, then identifier segments separated by /
    /// Allows: "myspec", "contracts/employment/jack", "@user/workspace/spec"
    fn parse_spec_name(&mut self) -> Result<(String, Span), Error> {
        let mut name = String::new();
        let start_span;

        if self.at(&TokenKind::At)? {
            let at_tok = self.next()?;
            start_span = at_tok.span.clone();
            name.push('@');
        } else {
            start_span = self.peek()?.span.clone();
        }

        // First segment must be an identifier or a keyword that can serve as name
        let first = self.next()?;
        if !first.kind.is_identifier_like() {
            return Err(self.error_at_token(
                &first,
                format!("Expected a spec name, found {}", first.kind),
            ));
        }
        name.push_str(&first.text);
        let mut end_span = first.span.clone();

        // Continue consuming / identifier segments
        while self.at(&TokenKind::Slash)? {
            self.next()?; // consume /
            name.push('/');
            let seg = self.next()?;
            if !seg.kind.is_identifier_like() {
                return Err(self.error_at_token(
                    &seg,
                    format!(
                        "Expected identifier after '/' in spec name, found {}",
                        seg.kind
                    ),
                ));
            }
            name.push_str(&seg.text);
            end_span = seg.span.clone();
        }

        // Check for hyphen-containing spec names like "my-spec"
        while self.at(&TokenKind::Minus)? {
            // Only consume if the next token after minus is an identifier
            // (hyphenated names like "my-spec")
            let minus_span = self.peek()?.span.clone();
            self.next()?; // consume -
            if let Ok(peeked) = self.peek() {
                if peeked.kind.is_identifier_like() {
                    let seg = self.next()?;
                    name.push('-');
                    name.push_str(&seg.text);
                    end_span = seg.span.clone();
                    // Could be followed by /
                    while self.at(&TokenKind::Slash)? {
                        self.next()?; // consume /
                        name.push('/');
                        let seg2 = self.next()?;
                        if !seg2.kind.is_identifier_like() {
                            return Err(self.error_at_token(
                                &seg2,
                                format!(
                                    "Expected identifier after '/' in spec name, found {}",
                                    seg2.kind
                                ),
                            ));
                        }
                        name.push_str(&seg2.text);
                        end_span = seg2.span.clone();
                    }
                } else {
                    // The minus wasn't part of the name; this is an error
                    let span = self.span_covering(&start_span, &minus_span);
                    return Err(Error::parsing(
                        "Trailing '-' after spec name",
                        self.make_source(span),
                        None::<String>,
                    ));
                }
            }
        }

        let full_span = self.span_covering(&start_span, &end_span);
        Ok((name, full_span))
    }

    fn try_parse_effective_from(&mut self) -> Result<Option<DateTimeValue>, Error> {
        // effective_from is a date/time token right after the spec name.
        // It's tricky because it looks like a number (e.g. 2026-03-04).
        // In the old grammar it was a special atomic rule.
        // We'll check if the next token is a NumberLit that looks like a year.
        if !self.at(&TokenKind::NumberLit)? {
            return Ok(None);
        }

        let peeked = self.peek()?;
        let peeked_text = peeked.text.clone();
        let peeked_span = peeked.span.clone();

        // Check if it could be a date: 4-digit number followed by -
        if peeked_text.len() == 4 && peeked_text.chars().all(|c| c.is_ascii_digit()) {
            // Collect the full datetime string by consuming tokens
            let mut dt_str = String::new();
            let num_tok = self.next()?; // consume the year number
            dt_str.push_str(&num_tok.text);

            // Try to consume -MM-DD and optional T... parts
            while self.at(&TokenKind::Minus)? {
                self.next()?; // consume -
                dt_str.push('-');
                let part = self.next()?;
                dt_str.push_str(&part.text);
            }

            // Check for T (time part)
            if self.at(&TokenKind::Identifier)? {
                let peeked = self.peek()?;
                if peeked.text.starts_with('T') || peeked.text.starts_with('t') {
                    let time_part = self.next()?;
                    dt_str.push_str(&time_part.text);
                    // Consume any : separated parts
                    while self.at(&TokenKind::Colon)? {
                        self.next()?;
                        dt_str.push(':');
                        let part = self.next()?;
                        dt_str.push_str(&part.text);
                    }
                    // Check for timezone (+ or Z)
                    if self.at(&TokenKind::Plus)? {
                        self.next()?;
                        dt_str.push('+');
                        let tz_part = self.next()?;
                        dt_str.push_str(&tz_part.text);
                        if self.at(&TokenKind::Colon)? {
                            self.next()?;
                            dt_str.push(':');
                            let tz_min = self.next()?;
                            dt_str.push_str(&tz_min.text);
                        }
                    }
                }
            }

            // Try to parse as datetime
            if let Ok(dtv) = dt_str.parse::<DateTimeValue>() {
                return Ok(Some(dtv));
            }

            return Err(Error::parsing(
                format!("Invalid date/time in spec declaration: '{}'", dt_str),
                self.make_source(peeked_span),
                None::<String>,
            ));
        }

        Ok(None)
    }

    fn try_parse_commentary(&mut self) -> Result<Option<String>, Error> {
        if !self.at(&TokenKind::Commentary)? {
            return Ok(None);
        }
        let token = self.next()?;
        let trimmed = token.text.trim().to_string();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed))
        }
    }

    // ========================================================================
    // Fact parsing
    // ========================================================================

    fn parse_fact(&mut self) -> Result<LemmaFact, Error> {
        let fact_token = self.expect(&TokenKind::Fact)?;
        let start_span = fact_token.span.clone();

        // Parse fact reference (single segment = definition, multi-segment = binding)
        let reference = self.parse_reference()?;

        self.expect(&TokenKind::Colon)?;

        let value = self.parse_fact_value()?;

        let end_span = self.peek()?.span.clone();
        let span = self.span_covering(&start_span, &end_span);
        let source = self.make_source(span);

        Ok(LemmaFact::new(reference, value, source))
    }

    fn parse_reference(&mut self) -> Result<Reference, Error> {
        let mut segments = Vec::new();

        let first = self.next()?;
        // Structural keywords (spec, fact, rule, unless, ...) cannot be names.
        // Type keywords (duration, number, date, ...) CAN be names per the grammar.
        if is_structural_keyword(&first.kind) {
            return Err(self.error_at_token_with_suggestion(
                &first,
                format!(
                    "'{}' is a reserved keyword and cannot be used as a name",
                    first.text
                ),
                "Choose a different name that is not a reserved keyword",
            ));
        }

        if !can_be_reference_segment(&first.kind) {
            return Err(self.error_at_token(
                &first,
                format!("Expected an identifier, found {}", first.kind),
            ));
        }

        segments.push(first.text.clone());

        // Consume . separated segments
        while self.at(&TokenKind::Dot)? {
            self.next()?; // consume .
            let seg = self.next()?;
            if !can_be_reference_segment(&seg.kind) {
                return Err(self.error_at_token(
                    &seg,
                    format!("Expected an identifier after '.', found {}", seg.kind),
                ));
            }
            segments.push(seg.text.clone());
        }

        Ok(Reference::from_path(segments))
    }

    fn parse_fact_value(&mut self) -> Result<FactValue, Error> {
        // Check for type declaration: [type_name] or [type_arrow_chain]
        if self.at(&TokenKind::LBracket)? {
            return self.parse_type_declaration_or_inline();
        }

        // Check for spec reference: spec <name>
        if self.at(&TokenKind::Spec)? {
            return self.parse_fact_spec_reference();
        }

        // Otherwise, it's a literal value
        let value = self.parse_literal_value()?;
        Ok(FactValue::Literal(value))
    }

    fn parse_type_declaration_or_inline(&mut self) -> Result<FactValue, Error> {
        self.expect(&TokenKind::LBracket)?;

        // Parse the type name (could be a standard type or custom type)
        let (base, from_spec, constraints) = self.parse_type_arrow_chain()?;

        self.expect(&TokenKind::RBracket)?;

        Ok(FactValue::TypeDeclaration {
            base,
            constraints,
            from: from_spec,
        })
    }

    fn parse_fact_spec_reference(&mut self) -> Result<FactValue, Error> {
        self.expect(&TokenKind::Spec)?;

        let (name, _name_span) = self.parse_spec_name()?;
        let from_registry = name.starts_with('@');

        let hash_pin = self.try_parse_hash_pin()?;

        let mut effective = None;
        // Check for effective datetime after spec reference
        if self.at(&TokenKind::NumberLit)? {
            let peeked = self.peek()?;
            if peeked.text.len() == 4 && peeked.text.chars().all(|c| c.is_ascii_digit()) {
                // Could be a datetime effective
                effective = self.try_parse_effective_from()?;
            }
        }

        Ok(FactValue::SpecReference(SpecRef {
            name,
            from_registry,
            hash_pin,
            effective,
        }))
    }

    // ========================================================================
    // Rule parsing
    // ========================================================================

    fn parse_rule(&mut self) -> Result<LemmaRule, Error> {
        let rule_token = self.expect(&TokenKind::Rule)?;
        let start_span = rule_token.span.clone();

        let name_tok = self.next()?;
        if is_structural_keyword(&name_tok.kind) {
            return Err(self.error_at_token_with_suggestion(
                &name_tok,
                format!(
                    "'{}' is a reserved keyword and cannot be used as a rule name",
                    name_tok.text
                ),
                "Choose a different name that is not a reserved keyword",
            ));
        }
        if !can_be_label(&name_tok.kind) && !is_type_keyword(&name_tok.kind) {
            return Err(self.error_at_token(
                &name_tok,
                format!("Expected a rule name, found {}", name_tok.kind),
            ));
        }
        let rule_name = name_tok.text.clone();

        self.expect(&TokenKind::Colon)?;

        // Parse the base expression or veto
        let expression = if self.at(&TokenKind::Veto)? {
            self.parse_veto_expression()?
        } else {
            self.parse_expression()?
        };

        // Parse unless clauses
        let mut unless_clauses = Vec::new();
        while self.at(&TokenKind::Unless)? {
            unless_clauses.push(self.parse_unless_clause()?);
        }

        let end_span = if let Some(last_unless) = unless_clauses.last() {
            last_unless.source_location.span.clone()
        } else if let Some(ref loc) = expression.source_location {
            loc.span.clone()
        } else {
            start_span.clone()
        };

        let span = self.span_covering(&start_span, &end_span);
        Ok(LemmaRule {
            name: rule_name,
            expression,
            unless_clauses,
            source_location: self.make_source(span),
        })
    }

    fn parse_veto_expression(&mut self) -> Result<Expression, Error> {
        let veto_tok = self.expect(&TokenKind::Veto)?;
        let start_span = veto_tok.span.clone();

        let message = if self.at(&TokenKind::StringLit)? {
            let str_tok = self.next()?;
            let content = unquote_string(&str_tok.text);
            Some(content)
        } else {
            None
        };

        let span = self.span_from(&start_span);
        self.new_expression(
            ExpressionKind::Veto(VetoExpression { message }),
            self.make_source(span),
        )
    }

    fn parse_unless_clause(&mut self) -> Result<UnlessClause, Error> {
        let unless_tok = self.expect(&TokenKind::Unless)?;
        let start_span = unless_tok.span.clone();

        let condition = self.parse_expression()?;

        self.expect(&TokenKind::Then)?;

        let result = if self.at(&TokenKind::Veto)? {
            self.parse_veto_expression()?
        } else {
            self.parse_expression()?
        };

        let end_span = result
            .source_location
            .as_ref()
            .map(|s| s.span.clone())
            .unwrap_or_else(|| start_span.clone());
        let span = self.span_covering(&start_span, &end_span);

        Ok(UnlessClause {
            condition,
            result,
            source_location: self.make_source(span),
        })
    }

    // ========================================================================
    // Type definitions
    // ========================================================================

    fn parse_type_def(&mut self) -> Result<TypeDef, Error> {
        let type_tok = self.expect(&TokenKind::Type)?;
        let start_span = type_tok.span.clone();

        // Parse type name
        let name_tok = self.next()?;
        let type_name = name_tok.text.clone();

        // Check if this is a type import (type X from Y) or a type definition (type X: Y)
        if self.at(&TokenKind::From)? {
            return self.parse_type_import(type_name, start_span);
        }

        // Regular type definition: type X: Y -> ...
        if self.at(&TokenKind::Colon)? {
            self.next()?; // consume :
        } else {
            // Could also be an import without us seeing 'from' yet if there are two type names
            // e.g. "type money from other_spec"
            let peek = self.peek()?.clone();
            return Err(self.error_at_token(
                &peek,
                format!(
                    "Expected ':' or 'from' after type name '{}', found {}",
                    type_name, peek.kind
                ),
            ));
        }

        let (parent, _from, constraints) = self.parse_type_arrow_chain()?;

        let end_span = self.peek()?.span.clone();
        let span = self.span_covering(&start_span, &end_span);
        Ok(TypeDef::Regular {
            source_location: self.make_source(span),
            name: type_name,
            parent,
            constraints,
        })
    }

    fn parse_type_import(&mut self, type_name: String, start_span: Span) -> Result<TypeDef, Error> {
        self.expect(&TokenKind::From)?;

        let (from_name, _from_span) = self.parse_spec_name()?;
        let from_registry = from_name.starts_with('@');
        let hash_pin = self.try_parse_hash_pin()?;

        let from = SpecRef {
            name: from_name,
            from_registry,
            hash_pin,
            effective: None,
        };

        // Check for arrow chain constraints after import
        let constraints = if self.at(&TokenKind::Arrow)? {
            let (_, _, constraints) = self.parse_remaining_arrow_chain()?;
            constraints
        } else {
            None
        };

        let end_span = self.peek()?.span.clone();
        let span = self.span_covering(&start_span, &end_span);

        let source_type = type_name.clone();

        Ok(TypeDef::Import {
            source_location: self.make_source(span),
            name: type_name,
            source_type,
            from,
            constraints,
        })
    }

    /// Parse a type arrow chain: type_name (-> command)* or type_name from spec (-> command)*
    fn parse_type_arrow_chain(&mut self) -> Result<TypeArrowChain, Error> {
        let name_tok = self.next()?;
        let base = if let Some(kind) = token_kind_to_primitive(&name_tok.kind) {
            ParentType::Primitive(kind)
        } else if can_be_label(&name_tok.kind) {
            ParentType::Custom(name_tok.text.clone())
        } else {
            return Err(self.error_at_token(
                &name_tok,
                format!("Expected a type name, found {}", name_tok.kind),
            ));
        };

        // Check for 'from' (inline type import)
        let from_spec = if self.at(&TokenKind::From)? {
            self.next()?; // consume from
            let (from_name, _) = self.parse_spec_name()?;
            let from_registry = from_name.starts_with('@');
            let hash_pin = self.try_parse_hash_pin()?;
            Some(SpecRef {
                name: from_name,
                from_registry,
                hash_pin,
                effective: None,
            })
        } else {
            None
        };

        // Parse arrow chain constraints
        let mut commands = Vec::new();
        while self.at(&TokenKind::Arrow)? {
            self.next()?; // consume ->
            let (cmd, cmd_args) = self.parse_command()?;
            commands.push((cmd, cmd_args));
        }

        let constraints = if commands.is_empty() {
            None
        } else {
            Some(commands)
        };

        Ok((base, from_spec, constraints))
    }

    fn parse_remaining_arrow_chain(&mut self) -> Result<TypeArrowChain, Error> {
        let mut commands = Vec::new();
        while self.at(&TokenKind::Arrow)? {
            self.next()?; // consume ->
            let (cmd, cmd_args) = self.parse_command()?;
            commands.push((cmd, cmd_args));
        }
        let constraints = if commands.is_empty() {
            None
        } else {
            Some(commands)
        };
        Ok((ParentType::Custom(String::new()), None, constraints))
    }

    fn parse_command(&mut self) -> Result<(TypeConstraintCommand, Vec<CommandArg>), Error> {
        let name_tok = self.next()?;
        if !can_be_label(&name_tok.kind) && !is_type_keyword(&name_tok.kind) {
            return Err(self.error_at_token(
                &name_tok,
                format!("Expected a command name, found {}", name_tok.kind),
            ));
        }
        let cmd = try_parse_type_constraint_command(&name_tok.text).ok_or_else(|| {
            self.error_at_token(
                &name_tok,
                format!(
                    "Unknown constraint command '{}'. Valid commands: help, default, unit, minimum, maximum, decimals, precision, option, options, length",
                    name_tok.text
                ),
            )
        })?;

        let mut args = Vec::new();
        loop {
            // Command args: number, boolean, text, or label
            // Stop at: ->, ], newlines (next keyword), EOF
            if self.at(&TokenKind::Arrow)?
                || self.at(&TokenKind::RBracket)?
                || self.at(&TokenKind::Eof)?
                || is_spec_body_keyword(&self.peek()?.kind)
                || self.at(&TokenKind::Spec)?
            {
                break;
            }

            let peek_kind = self.peek()?.kind.clone();
            match peek_kind {
                TokenKind::NumberLit => {
                    let tok = self.next()?;
                    args.push(CommandArg::Number(tok.text));
                }
                TokenKind::Minus | TokenKind::Plus => {
                    let second = self.lexer.peek_second()?.kind.clone();
                    if second == TokenKind::NumberLit {
                        let sign = self.next()?;
                        let num = self.next()?;
                        let text = format!("{}{}", sign.text, num.text);
                        args.push(CommandArg::Number(text));
                    } else {
                        break;
                    }
                }
                TokenKind::StringLit => {
                    let tok = self.next()?;
                    let content = unquote_string(&tok.text);
                    args.push(CommandArg::Text(content));
                }
                ref k if is_boolean_keyword(k) => {
                    let tok = self.next()?;
                    args.push(CommandArg::Boolean(token_kind_to_boolean_value(&tok.kind)));
                }
                ref k if can_be_label(k) || is_type_keyword(k) => {
                    let tok = self.next()?;
                    args.push(CommandArg::Label(tok.text));
                }
                _ => break,
            }
        }

        Ok((cmd, args))
    }

    // ========================================================================
    // Meta parsing
    // ========================================================================

    fn parse_meta(&mut self) -> Result<MetaField, Error> {
        let meta_tok = self.expect(&TokenKind::Meta)?;
        let start_span = meta_tok.span.clone();

        let key_tok = self.next()?;
        let key = key_tok.text.clone();

        self.expect(&TokenKind::Colon)?;

        let value = self.parse_meta_value()?;

        let end_span = self.peek()?.span.clone();
        let span = self.span_covering(&start_span, &end_span);

        Ok(MetaField {
            key,
            value,
            source_location: self.make_source(span),
        })
    }

    fn parse_meta_value(&mut self) -> Result<MetaValue, Error> {
        // Try literal first (string, number, boolean, date)
        let peeked = self.peek()?;
        match &peeked.kind {
            TokenKind::StringLit => {
                let value = self.parse_literal_value()?;
                return Ok(MetaValue::Literal(value));
            }
            TokenKind::NumberLit => {
                let value = self.parse_literal_value()?;
                return Ok(MetaValue::Literal(value));
            }
            k if is_boolean_keyword(k) => {
                let value = self.parse_literal_value()?;
                return Ok(MetaValue::Literal(value));
            }
            _ => {}
        }

        // Otherwise, consume as unquoted meta identifier
        // meta_identifier: (ASCII_ALPHANUMERIC | "_" | "-" | "." | "/")+
        let mut ident = String::new();
        loop {
            let peeked = self.peek()?;
            match &peeked.kind {
                k if k.is_identifier_like() => {
                    let tok = self.next()?;
                    ident.push_str(&tok.text);
                }
                TokenKind::Dot => {
                    self.next()?;
                    ident.push('.');
                }
                TokenKind::Slash => {
                    self.next()?;
                    ident.push('/');
                }
                TokenKind::Minus => {
                    self.next()?;
                    ident.push('-');
                }
                TokenKind::NumberLit => {
                    let tok = self.next()?;
                    ident.push_str(&tok.text);
                }
                _ => break,
            }
        }

        if ident.is_empty() {
            let tok = self.peek()?.clone();
            return Err(self.error_at_token(&tok, "Expected a meta value"));
        }

        Ok(MetaValue::Unquoted(ident))
    }

    // ========================================================================
    // Literal value parsing
    // ========================================================================

    fn parse_literal_value(&mut self) -> Result<Value, Error> {
        let peeked = self.peek()?;
        match &peeked.kind {
            TokenKind::StringLit => {
                let tok = self.next()?;
                let content = unquote_string(&tok.text);
                Ok(Value::Text(content))
            }
            k if is_boolean_keyword(k) => {
                let tok = self.next()?;
                Ok(Value::Boolean(token_kind_to_boolean_value(&tok.kind)))
            }
            TokenKind::NumberLit => self.parse_number_literal(),
            TokenKind::Minus | TokenKind::Plus => self.parse_signed_number_literal(),
            _ => {
                let tok = self.next()?;
                Err(self.error_at_token(
                    &tok,
                    format!(
                        "Expected a value (number, text, boolean, date, etc.), found '{}'",
                        tok.text
                    ),
                ))
            }
        }
    }

    fn parse_signed_number_literal(&mut self) -> Result<Value, Error> {
        let sign_tok = self.next()?;
        let sign_span = sign_tok.span.clone();
        let is_negative = sign_tok.kind == TokenKind::Minus;

        if !self.at(&TokenKind::NumberLit)? {
            let tok = self.peek()?.clone();
            return Err(self.error_at_token(
                &tok,
                format!(
                    "Expected a number after '{}', found '{}'",
                    sign_tok.text, tok.text
                ),
            ));
        }

        let value = self.parse_number_literal()?;
        if !is_negative {
            return Ok(value);
        }
        match value {
            Value::Number(d) => Ok(Value::Number(-d)),
            Value::Scale(d, unit) => Ok(Value::Scale(-d, unit)),
            Value::Duration(d, unit) => Ok(Value::Duration(-d, unit)),
            Value::Ratio(d, label) => Ok(Value::Ratio(-d, label)),
            other => Err(Error::parsing(
                format!("Cannot negate this value: {}", other),
                self.make_source(sign_span),
                None::<String>,
            )),
        }
    }

    fn parse_number_literal(&mut self) -> Result<Value, Error> {
        let num_tok = self.next()?;
        let num_text = &num_tok.text;
        let num_span = num_tok.span.clone();

        // Check if followed by - which could make it a date (YYYY-MM-DD)
        if num_text.len() == 4
            && num_text.chars().all(|c| c.is_ascii_digit())
            && self.at(&TokenKind::Minus)?
        {
            return self.parse_date_literal(num_text.clone(), num_span);
        }

        // Check what follows the number
        let peeked = self.peek()?;

        // Number followed by : could be a time literal (HH:MM:SS)
        if num_text.len() == 2
            && num_text.chars().all(|c| c.is_ascii_digit())
            && peeked.kind == TokenKind::Colon
        {
            // Only if we're in a fact value context... this is ambiguous.
            // Time literals look like: 14:30:00 or 14:30
            // But we might also have "rule x: expr" where : is assignment.
            // The grammar handles this at the grammar level. For us,
            // we need to check if the context is right.
            // Let's try to parse as time if the following pattern matches.
            return self.try_parse_time_literal(num_text.clone(), num_span);
        }

        // Check for %% (permille) - must be before %
        if peeked.kind == TokenKind::PercentPercent {
            let pp_tok = self.next()?;
            // Check it's not followed by a digit
            if let Ok(next_peek) = self.peek() {
                if next_peek.kind == TokenKind::NumberLit {
                    return Err(self.error_at_token(
                        &pp_tok,
                        "Permille literal cannot be followed by a digit",
                    ));
                }
            }
            let decimal = parse_decimal_string(num_text, &num_span, self)?;
            let ratio_value = decimal / Decimal::from(1000);
            return Ok(Value::Ratio(ratio_value, Some("permille".to_string())));
        }

        // Check for % (percent)
        if peeked.kind == TokenKind::Percent {
            let pct_tok = self.next()?;
            // Check it's not followed by a digit or another %
            if let Ok(next_peek) = self.peek() {
                if next_peek.kind == TokenKind::NumberLit || next_peek.kind == TokenKind::Percent {
                    return Err(self.error_at_token(
                        &pct_tok,
                        "Percent literal cannot be followed by a digit",
                    ));
                }
            }
            let decimal = parse_decimal_string(num_text, &num_span, self)?;
            let ratio_value = decimal / Decimal::from(100);
            return Ok(Value::Ratio(ratio_value, Some("percent".to_string())));
        }

        // Check for "percent" keyword
        if peeked.kind == TokenKind::PercentKw {
            self.next()?; // consume "percent"
            let decimal = parse_decimal_string(num_text, &num_span, self)?;
            let ratio_value = decimal / Decimal::from(100);
            return Ok(Value::Ratio(ratio_value, Some("percent".to_string())));
        }

        // Check for "permille" keyword
        if peeked.kind == TokenKind::Permille {
            self.next()?; // consume "permille"
            let decimal = parse_decimal_string(num_text, &num_span, self)?;
            let ratio_value = decimal / Decimal::from(1000);
            return Ok(Value::Ratio(ratio_value, Some("permille".to_string())));
        }

        // Check for duration unit
        if is_duration_unit(&peeked.kind) && peeked.kind != TokenKind::PercentKw {
            let unit_tok = self.next()?;
            let decimal = parse_decimal_string(num_text, &num_span, self)?;
            let duration_unit = token_kind_to_duration_unit(&unit_tok.kind);
            return Ok(Value::Duration(decimal, duration_unit));
        }

        // Check for user-defined unit (identifier after number)
        if can_be_label(&peeked.kind) {
            let unit_tok = self.next()?;
            let decimal = parse_decimal_string(num_text, &num_span, self)?;
            return Ok(Value::Scale(decimal, unit_tok.text.clone()));
        }

        // Plain number
        let decimal = parse_decimal_string(num_text, &num_span, self)?;
        Ok(Value::Number(decimal))
    }

    fn parse_date_literal(&mut self, year_text: String, start_span: Span) -> Result<Value, Error> {
        let mut dt_str = year_text;

        // Consume -MM
        self.expect(&TokenKind::Minus)?;
        dt_str.push('-');
        let month_tok = self.expect(&TokenKind::NumberLit)?;
        dt_str.push_str(&month_tok.text);

        // Consume -DD
        self.expect(&TokenKind::Minus)?;
        dt_str.push('-');
        let day_tok = self.expect(&TokenKind::NumberLit)?;
        dt_str.push_str(&day_tok.text);

        // Check for T (time component)
        if self.at(&TokenKind::Identifier)? {
            let peeked = self.peek()?;
            if peeked.text.len() >= 2
                && (peeked.text.starts_with('T') || peeked.text.starts_with('t'))
            {
                // The lexer may have tokenized T14 as a single identifier
                let t_tok = self.next()?;
                dt_str.push_str(&t_tok.text);

                // Consume :MM
                if self.at(&TokenKind::Colon)? {
                    self.next()?;
                    dt_str.push(':');
                    let min_tok = self.next()?;
                    dt_str.push_str(&min_tok.text);

                    // Consume :SS and optional fractional seconds
                    if self.at(&TokenKind::Colon)? {
                        self.next()?;
                        dt_str.push(':');
                        let sec_tok = self.next()?;
                        dt_str.push_str(&sec_tok.text);

                        // Check for fractional seconds .NNNNNN
                        if self.at(&TokenKind::Dot)? {
                            self.next()?;
                            dt_str.push('.');
                            let frac_tok = self.expect(&TokenKind::NumberLit)?;
                            dt_str.push_str(&frac_tok.text);
                        }
                    }
                }

                // Check for timezone
                self.try_consume_timezone(&mut dt_str)?;
            }
        }

        if let Ok(dtv) = dt_str.parse::<crate::literals::DateTimeValue>() {
            return Ok(Value::Date(dtv));
        }

        Err(Error::parsing(
            format!("Invalid date/time format: '{}'", dt_str),
            self.make_source(start_span),
            None::<String>,
        ))
    }

    fn try_consume_timezone(&mut self, dt_str: &mut String) -> Result<(), Error> {
        // Z timezone
        if self.at(&TokenKind::Identifier)? {
            let peeked = self.peek()?;
            if peeked.text == "Z" || peeked.text == "z" {
                let z_tok = self.next()?;
                dt_str.push_str(&z_tok.text);
                return Ok(());
            }
        }

        // +HH:MM or -HH:MM
        if self.at(&TokenKind::Plus)? || self.at(&TokenKind::Minus)? {
            let sign_tok = self.next()?;
            dt_str.push_str(&sign_tok.text);
            let hour_tok = self.expect(&TokenKind::NumberLit)?;
            dt_str.push_str(&hour_tok.text);
            if self.at(&TokenKind::Colon)? {
                self.next()?;
                dt_str.push(':');
                let min_tok = self.expect(&TokenKind::NumberLit)?;
                dt_str.push_str(&min_tok.text);
            }
        }

        Ok(())
    }

    fn try_parse_time_literal(
        &mut self,
        hour_text: String,
        start_span: Span,
    ) -> Result<Value, Error> {
        let mut time_str = hour_text;

        // Consume :MM
        self.expect(&TokenKind::Colon)?;
        time_str.push(':');
        let min_tok = self.expect(&TokenKind::NumberLit)?;
        time_str.push_str(&min_tok.text);

        // Optional :SS
        if self.at(&TokenKind::Colon)? {
            self.next()?;
            time_str.push(':');
            let sec_tok = self.expect(&TokenKind::NumberLit)?;
            time_str.push_str(&sec_tok.text);
        }

        // Try timezone
        self.try_consume_timezone(&mut time_str)?;

        if let Ok(t) = time_str.parse::<chrono::NaiveTime>() {
            use chrono::Timelike;
            return Ok(Value::Time(TimeValue {
                hour: t.hour() as u8,
                minute: t.minute() as u8,
                second: t.second() as u8,
                timezone: None,
            }));
        }

        Err(Error::parsing(
            format!("Invalid time format: '{}'", time_str),
            self.make_source(start_span),
            None::<String>,
        ))
    }

    // ========================================================================
    // Expression parsing (Pratt parser / precedence climbing)
    // ========================================================================

    fn new_expression(
        &mut self,
        kind: ExpressionKind,
        source: Source,
    ) -> Result<Expression, Error> {
        self.expression_count += 1;
        if self.expression_count > self.max_expression_count {
            return Err(Error::resource_limit_exceeded(
                "max_expression_count",
                self.max_expression_count.to_string(),
                self.expression_count.to_string(),
                "Split logic into multiple rules to reduce expression count",
                Some(source),
                None,
                None,
            ));
        }
        Ok(Expression::new(kind, source))
    }

    fn check_depth(&mut self) -> Result<(), Error> {
        if let Err(actual) = self.depth_tracker.push_depth() {
            let span = self.peek()?.span.clone();
            self.depth_tracker.pop_depth();
            return Err(Error::resource_limit_exceeded(
                "max_expression_depth",
                self.depth_tracker.max_depth().to_string(),
                actual.to_string(),
                "Simplify nested expressions or break into separate rules",
                Some(self.make_source(span)),
                None,
                None,
            ));
        }
        Ok(())
    }

    fn parse_expression(&mut self) -> Result<Expression, Error> {
        self.check_depth()?;
        let result = self.parse_and_expression();
        self.depth_tracker.pop_depth();
        result
    }

    fn parse_and_expression(&mut self) -> Result<Expression, Error> {
        let start_span = self.peek()?.span.clone();
        let mut left = self.parse_and_operand()?;

        while self.at(&TokenKind::And)? {
            self.next()?; // consume 'and'
            let right = self.parse_and_operand()?;
            let span = self.span_covering(
                &start_span,
                &right
                    .source_location
                    .as_ref()
                    .map(|s| s.span.clone())
                    .unwrap_or_else(|| start_span.clone()),
            );
            left = self.new_expression(
                ExpressionKind::LogicalAnd(Arc::new(left), Arc::new(right)),
                self.make_source(span),
            )?;
        }

        Ok(left)
    }

    fn parse_and_operand(&mut self) -> Result<Expression, Error> {
        // not expression
        if self.at(&TokenKind::Not)? {
            return self.parse_not_expression();
        }

        // base_with_suffix: base_expression followed by optional suffix
        self.parse_base_with_suffix()
    }

    fn parse_not_expression(&mut self) -> Result<Expression, Error> {
        let not_tok = self.expect(&TokenKind::Not)?;
        let start_span = not_tok.span.clone();

        self.check_depth()?;
        let operand = self.parse_and_operand()?;
        self.depth_tracker.pop_depth();

        let end_span = operand
            .source_location
            .as_ref()
            .map(|s| s.span.clone())
            .unwrap_or_else(|| start_span.clone());
        let span = self.span_covering(&start_span, &end_span);

        self.new_expression(
            ExpressionKind::LogicalNegation(Arc::new(operand), NegationType::Not),
            self.make_source(span),
        )
    }

    fn parse_base_with_suffix(&mut self) -> Result<Expression, Error> {
        let start_span = self.peek()?.span.clone();
        let base = self.parse_base_expression()?;

        // Check for suffixes
        let peeked = self.peek()?;

        // Comparison suffix: >, <, >=, <=, ==, !=, is, is not
        if is_comparison_operator(&peeked.kind) {
            return self.parse_comparison_suffix(base, start_span);
        }

        // "not in calendar <unit>" suffix: expr not in calendar year|month|week
        // After a base_expression, "not" must be this suffix (prefix "not" is only
        // at and_operand level, and "X and not Y" would have consumed "and" first).
        if peeked.kind == TokenKind::Not {
            return self.parse_not_in_calendar_suffix(base, start_span);
        }

        // "in" suffix: conversion, date relative, date calendar
        if peeked.kind == TokenKind::In {
            return self.parse_in_suffix(base, start_span);
        }

        Ok(base)
    }

    fn parse_comparison_suffix(
        &mut self,
        left: Expression,
        start_span: Span,
    ) -> Result<Expression, Error> {
        let operator = self.parse_comparison_operator()?;

        // Right side can be: not_expr | base_expression (optionally with "in unit")
        let right = if self.at(&TokenKind::Not)? {
            self.parse_not_expression()?
        } else {
            let rhs = self.parse_base_expression()?;
            // Check for "in unit" conversion on the rhs
            if self.at(&TokenKind::In)? {
                self.parse_in_suffix(rhs, start_span.clone())?
            } else {
                rhs
            }
        };

        let end_span = right
            .source_location
            .as_ref()
            .map(|s| s.span.clone())
            .unwrap_or_else(|| start_span.clone());
        let span = self.span_covering(&start_span, &end_span);

        self.new_expression(
            ExpressionKind::Comparison(Arc::new(left), operator, Arc::new(right)),
            self.make_source(span),
        )
    }

    fn parse_comparison_operator(&mut self) -> Result<ComparisonComputation, Error> {
        let tok = self.next()?;
        match tok.kind {
            TokenKind::Gt => Ok(ComparisonComputation::GreaterThan),
            TokenKind::Lt => Ok(ComparisonComputation::LessThan),
            TokenKind::Gte => Ok(ComparisonComputation::GreaterThanOrEqual),
            TokenKind::Lte => Ok(ComparisonComputation::LessThanOrEqual),
            TokenKind::EqEq => Ok(ComparisonComputation::Equal),
            TokenKind::BangEq => Ok(ComparisonComputation::NotEqual),
            TokenKind::Is => {
                // Check for "is not"
                if self.at(&TokenKind::Not)? {
                    self.next()?; // consume 'not'
                    Ok(ComparisonComputation::IsNot)
                } else {
                    Ok(ComparisonComputation::Is)
                }
            }
            _ => Err(self.error_at_token(
                &tok,
                format!("Expected a comparison operator, found {}", tok.kind),
            )),
        }
    }

    fn parse_not_in_calendar_suffix(
        &mut self,
        base: Expression,
        start_span: Span,
    ) -> Result<Expression, Error> {
        self.expect(&TokenKind::Not)?;
        self.expect(&TokenKind::In)?;
        self.expect(&TokenKind::Calendar)?;
        let unit = self.parse_calendar_unit()?;
        let end = self.peek()?.span.clone();
        let span = self.span_covering(&start_span, &end);
        self.new_expression(
            ExpressionKind::DateCalendar(DateCalendarKind::NotIn, unit, Arc::new(base)),
            self.make_source(span),
        )
    }

    fn parse_in_suffix(&mut self, base: Expression, start_span: Span) -> Result<Expression, Error> {
        self.expect(&TokenKind::In)?;

        let peeked = self.peek()?;

        // "in past calendar <unit>" or "in future calendar <unit>"
        if peeked.kind == TokenKind::Past || peeked.kind == TokenKind::Future {
            let direction = self.next()?;
            let rel_kind = if direction.kind == TokenKind::Past {
                DateRelativeKind::InPast
            } else {
                DateRelativeKind::InFuture
            };

            // Check for "calendar" keyword
            if self.at(&TokenKind::Calendar)? {
                self.next()?; // consume "calendar"
                let cal_kind = if direction.kind == TokenKind::Past {
                    DateCalendarKind::Past
                } else {
                    DateCalendarKind::Future
                };
                let unit = self.parse_calendar_unit()?;
                let end = self.peek()?.span.clone();
                let span = self.span_covering(&start_span, &end);
                return self.new_expression(
                    ExpressionKind::DateCalendar(cal_kind, unit, Arc::new(base)),
                    self.make_source(span),
                );
            }

            // "in past [tolerance]" or "in future [tolerance]"
            let tolerance = if !self.at(&TokenKind::And)?
                && !self.at(&TokenKind::Unless)?
                && !self.at(&TokenKind::Then)?
                && !self.at(&TokenKind::Eof)?
                && !is_comparison_operator(&self.peek()?.kind)
            {
                let peek_kind = self.peek()?.kind.clone();
                if peek_kind == TokenKind::NumberLit
                    || peek_kind == TokenKind::LParen
                    || can_be_reference_segment(&peek_kind)
                    || is_math_function(&peek_kind)
                {
                    Some(Arc::new(self.parse_base_expression()?))
                } else {
                    None
                }
            } else {
                None
            };

            let end = self.peek()?.span.clone();
            let span = self.span_covering(&start_span, &end);
            return self.new_expression(
                ExpressionKind::DateRelative(rel_kind, Arc::new(base), tolerance),
                self.make_source(span),
            );
        }

        // "in calendar <unit>"
        if peeked.kind == TokenKind::Calendar {
            self.next()?; // consume "calendar"
            let unit = self.parse_calendar_unit()?;
            let end = self.peek()?.span.clone();
            let span = self.span_covering(&start_span, &end);
            return self.new_expression(
                ExpressionKind::DateCalendar(DateCalendarKind::Current, unit, Arc::new(base)),
                self.make_source(span),
            );
        }

        // "in <unit>" — unit conversion
        let target_tok = self.next()?;
        let target = conversion_target_from_token(&target_tok.kind, &target_tok.text);

        let converted = self.new_expression(
            ExpressionKind::UnitConversion(Arc::new(base), target),
            self.make_source(self.span_covering(&start_span, &target_tok.span)),
        )?;

        // Check if followed by comparison operator
        if is_comparison_operator(&self.peek()?.kind) {
            return self.parse_comparison_suffix(converted, start_span);
        }

        Ok(converted)
    }

    fn parse_calendar_unit(&mut self) -> Result<CalendarUnit, Error> {
        let tok = self.next()?;
        if !is_calendar_unit_token(&tok.kind) {
            return Err(self.error_at_token(
                &tok,
                format!("Expected 'year', 'month', or 'week', found '{}'", tok.text),
            ));
        }
        Ok(token_kind_to_calendar_unit(&tok.kind))
    }

    // ========================================================================
    // Arithmetic expressions (precedence climbing)
    // ========================================================================

    fn parse_base_expression(&mut self) -> Result<Expression, Error> {
        let start_span = self.peek()?.span.clone();
        let mut left = self.parse_term()?;

        while self.at_any(&[TokenKind::Plus, TokenKind::Minus])? {
            // Check if this minus is really a binary operator or could be part of something else
            // In "X not in calendar year", we don't want to consume "not" as an operator
            let op_tok = self.next()?;
            let operation = match op_tok.kind {
                TokenKind::Plus => ArithmeticComputation::Add,
                TokenKind::Minus => ArithmeticComputation::Subtract,
                _ => unreachable!("BUG: only + and - should reach here"),
            };

            let right = self.parse_term()?;
            let end_span = right
                .source_location
                .as_ref()
                .map(|s| s.span.clone())
                .unwrap_or_else(|| start_span.clone());
            let span = self.span_covering(&start_span, &end_span);

            left = self.new_expression(
                ExpressionKind::Arithmetic(Arc::new(left), operation, Arc::new(right)),
                self.make_source(span),
            )?;
        }

        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expression, Error> {
        let start_span = self.peek()?.span.clone();
        let mut left = self.parse_power()?;

        while self.at_any(&[TokenKind::Star, TokenKind::Slash, TokenKind::Percent])? {
            // Be careful: % could be a percent literal suffix (e.g. 50%)
            // But here in term context, it's modulo since we already parsed the number
            let op_tok = self.next()?;
            let operation = match op_tok.kind {
                TokenKind::Star => ArithmeticComputation::Multiply,
                TokenKind::Slash => ArithmeticComputation::Divide,
                TokenKind::Percent => ArithmeticComputation::Modulo,
                _ => unreachable!("BUG: only *, /, % should reach here"),
            };

            let right = self.parse_power()?;
            let end_span = right
                .source_location
                .as_ref()
                .map(|s| s.span.clone())
                .unwrap_or_else(|| start_span.clone());
            let span = self.span_covering(&start_span, &end_span);

            left = self.new_expression(
                ExpressionKind::Arithmetic(Arc::new(left), operation, Arc::new(right)),
                self.make_source(span),
            )?;
        }

        Ok(left)
    }

    fn parse_power(&mut self) -> Result<Expression, Error> {
        let start_span = self.peek()?.span.clone();
        let left = self.parse_factor()?;

        if self.at(&TokenKind::Caret)? {
            self.next()?;
            self.check_depth()?;
            let right = self.parse_power()?;
            self.depth_tracker.pop_depth();
            let end_span = right
                .source_location
                .as_ref()
                .map(|s| s.span.clone())
                .unwrap_or_else(|| start_span.clone());
            let span = self.span_covering(&start_span, &end_span);

            return self.new_expression(
                ExpressionKind::Arithmetic(
                    Arc::new(left),
                    ArithmeticComputation::Power,
                    Arc::new(right),
                ),
                self.make_source(span),
            );
        }

        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expression, Error> {
        let peeked = self.peek()?;
        let start_span = peeked.span.clone();

        if peeked.kind == TokenKind::Minus {
            self.next()?;
            let operand = self.parse_primary_or_math()?;
            let end_span = operand
                .source_location
                .as_ref()
                .map(|s| s.span.clone())
                .unwrap_or_else(|| start_span.clone());
            let span = self.span_covering(&start_span, &end_span);

            let zero = self.new_expression(
                ExpressionKind::Literal(Value::Number(Decimal::ZERO)),
                self.make_source(start_span),
            )?;
            return self.new_expression(
                ExpressionKind::Arithmetic(
                    Arc::new(zero),
                    ArithmeticComputation::Subtract,
                    Arc::new(operand),
                ),
                self.make_source(span),
            );
        }

        if peeked.kind == TokenKind::Plus {
            self.next()?;
            return self.parse_primary_or_math();
        }

        self.parse_primary_or_math()
    }

    fn parse_primary_or_math(&mut self) -> Result<Expression, Error> {
        let peeked = self.peek()?;

        // Math functions
        if is_math_function(&peeked.kind) {
            return self.parse_math_function();
        }

        self.parse_primary()
    }

    fn parse_math_function(&mut self) -> Result<Expression, Error> {
        let func_tok = self.next()?;
        let start_span = func_tok.span.clone();

        let operator = match func_tok.kind {
            TokenKind::Sqrt => MathematicalComputation::Sqrt,
            TokenKind::Sin => MathematicalComputation::Sin,
            TokenKind::Cos => MathematicalComputation::Cos,
            TokenKind::Tan => MathematicalComputation::Tan,
            TokenKind::Asin => MathematicalComputation::Asin,
            TokenKind::Acos => MathematicalComputation::Acos,
            TokenKind::Atan => MathematicalComputation::Atan,
            TokenKind::Log => MathematicalComputation::Log,
            TokenKind::Exp => MathematicalComputation::Exp,
            TokenKind::Abs => MathematicalComputation::Abs,
            TokenKind::Floor => MathematicalComputation::Floor,
            TokenKind::Ceil => MathematicalComputation::Ceil,
            TokenKind::Round => MathematicalComputation::Round,
            _ => unreachable!("BUG: only math functions should reach here"),
        };

        self.check_depth()?;
        let operand = self.parse_base_expression()?;
        self.depth_tracker.pop_depth();

        let end_span = operand
            .source_location
            .as_ref()
            .map(|s| s.span.clone())
            .unwrap_or_else(|| start_span.clone());
        let span = self.span_covering(&start_span, &end_span);

        self.new_expression(
            ExpressionKind::MathematicalComputation(operator, Arc::new(operand)),
            self.make_source(span),
        )
    }

    fn parse_primary(&mut self) -> Result<Expression, Error> {
        let peeked = self.peek()?;
        let start_span = peeked.span.clone();

        match &peeked.kind {
            // Parenthesized expression
            TokenKind::LParen => {
                self.next()?; // consume (
                let inner = self.parse_expression()?;
                self.expect(&TokenKind::RParen)?;
                Ok(inner)
            }

            // Now keyword
            TokenKind::Now => {
                let tok = self.next()?;
                self.new_expression(ExpressionKind::Now, self.make_source(tok.span))
            }

            // String literal
            TokenKind::StringLit => {
                let tok = self.next()?;
                let content = unquote_string(&tok.text);
                self.new_expression(
                    ExpressionKind::Literal(Value::Text(content)),
                    self.make_source(tok.span),
                )
            }

            // Boolean literals
            k if is_boolean_keyword(k) => {
                let tok = self.next()?;
                self.new_expression(
                    ExpressionKind::Literal(Value::Boolean(token_kind_to_boolean_value(&tok.kind))),
                    self.make_source(tok.span),
                )
            }

            // Number literal (could be: plain number, date, time, duration, percent, unit)
            TokenKind::NumberLit => self.parse_number_expression(),

            // Reference (identifier, type keyword)
            k if can_be_reference_segment(k) => {
                let reference = self.parse_expression_reference()?;
                let end_span = self.peek()?.span.clone();
                let span = self.span_covering(&start_span, &end_span);
                self.new_expression(ExpressionKind::Reference(reference), self.make_source(span))
            }

            _ => {
                let tok = self.next()?;
                Err(self.error_at_token(
                    &tok,
                    format!("Expected an expression, found '{}'", tok.text),
                ))
            }
        }
    }

    fn parse_number_expression(&mut self) -> Result<Expression, Error> {
        let num_tok = self.next()?;
        let num_text = num_tok.text.clone();
        let start_span = num_tok.span.clone();

        // Check if this is a date literal (YYYY-MM-DD)
        if num_text.len() == 4
            && num_text.chars().all(|c| c.is_ascii_digit())
            && self.at(&TokenKind::Minus)?
        {
            // Peek further: if next-next is a number, this is likely a date
            // We need to be careful: "2024 - 5" is arithmetic, "2024-01-15" is a date
            // Date format requires: YYYY-MM-DD where MM and DD are 2 digits
            // This is ambiguous at the token level. Let's check if the pattern matches.
            // Since dates use -NN- pattern and arithmetic uses - N pattern (with spaces),
            // we can use the span positions to disambiguate.
            let minus_span = self.peek()?.span.clone();
            // If minus is immediately adjacent to the number (no space), it's a date
            if minus_span.start == start_span.end {
                let value = self.parse_date_literal(num_text, start_span.clone())?;
                return self
                    .new_expression(ExpressionKind::Literal(value), self.make_source(start_span));
            }
        }

        // Check for time literal (HH:MM:SS)
        if num_text.len() == 2
            && num_text.chars().all(|c| c.is_ascii_digit())
            && self.at(&TokenKind::Colon)?
        {
            let colon_span = self.peek()?.span.clone();
            if colon_span.start == start_span.end {
                let value = self.try_parse_time_literal(num_text, start_span.clone())?;
                return self
                    .new_expression(ExpressionKind::Literal(value), self.make_source(start_span));
            }
        }

        // Check for %% (permille)
        if self.at(&TokenKind::PercentPercent)? {
            let pp_tok = self.next()?;
            if let Ok(next_peek) = self.peek() {
                if next_peek.kind == TokenKind::NumberLit {
                    return Err(self.error_at_token(
                        &pp_tok,
                        "Permille literal cannot be followed by a digit",
                    ));
                }
            }
            let decimal = parse_decimal_string(&num_text, &start_span, self)?;
            let ratio_value = decimal / Decimal::from(1000);
            return self.new_expression(
                ExpressionKind::Literal(Value::Ratio(ratio_value, Some("permille".to_string()))),
                self.make_source(start_span),
            );
        }

        // Check for % (percent)
        if self.at(&TokenKind::Percent)? {
            let pct_span = self.peek()?.span.clone();
            // Only consume % if it's directly adjacent (no space) for the shorthand syntax
            // Or if it's "50 %" (space separated is also valid per the grammar)
            let pct_tok = self.next()?;
            if let Ok(next_peek) = self.peek() {
                if next_peek.kind == TokenKind::NumberLit || next_peek.kind == TokenKind::Percent {
                    return Err(self.error_at_token(
                        &pct_tok,
                        "Percent literal cannot be followed by a digit",
                    ));
                }
            }
            let decimal = parse_decimal_string(&num_text, &start_span, self)?;
            let ratio_value = decimal / Decimal::from(100);
            return self.new_expression(
                ExpressionKind::Literal(Value::Ratio(ratio_value, Some("percent".to_string()))),
                self.make_source(self.span_covering(&start_span, &pct_span)),
            );
        }

        // Check for "percent" keyword
        if self.at(&TokenKind::PercentKw)? {
            self.next()?;
            let decimal = parse_decimal_string(&num_text, &start_span, self)?;
            let ratio_value = decimal / Decimal::from(100);
            return self.new_expression(
                ExpressionKind::Literal(Value::Ratio(ratio_value, Some("percent".to_string()))),
                self.make_source(start_span),
            );
        }

        // Check for "permille" keyword
        if self.at(&TokenKind::Permille)? {
            self.next()?;
            let decimal = parse_decimal_string(&num_text, &start_span, self)?;
            let ratio_value = decimal / Decimal::from(1000);
            return self.new_expression(
                ExpressionKind::Literal(Value::Ratio(ratio_value, Some("permille".to_string()))),
                self.make_source(start_span),
            );
        }

        // Check for duration unit
        if is_duration_unit(&self.peek()?.kind) && self.peek()?.kind != TokenKind::PercentKw {
            let unit_tok = self.next()?;
            let decimal = parse_decimal_string(&num_text, &start_span, self)?;
            let duration_unit = token_kind_to_duration_unit(&unit_tok.kind);
            return self.new_expression(
                ExpressionKind::Literal(Value::Duration(decimal, duration_unit)),
                self.make_source(self.span_covering(&start_span, &unit_tok.span)),
            );
        }

        // Check for user-defined unit (identifier after number)
        if can_be_label(&self.peek()?.kind) {
            let unit_tok = self.next()?;
            let decimal = parse_decimal_string(&num_text, &start_span, self)?;
            return self.new_expression(
                ExpressionKind::UnresolvedUnitLiteral(decimal, unit_tok.text.clone()),
                self.make_source(self.span_covering(&start_span, &unit_tok.span)),
            );
        }

        // Plain number
        let decimal = parse_decimal_string(&num_text, &start_span, self)?;
        self.new_expression(
            ExpressionKind::Literal(Value::Number(decimal)),
            self.make_source(start_span),
        )
    }

    fn parse_expression_reference(&mut self) -> Result<Reference, Error> {
        let mut segments = Vec::new();

        let first = self.next()?;
        segments.push(first.text.clone());

        while self.at(&TokenKind::Dot)? {
            self.next()?; // consume .
            let seg = self.next()?;
            if !can_be_reference_segment(&seg.kind) {
                return Err(self.error_at_token(
                    &seg,
                    format!("Expected an identifier after '.', found {}", seg.kind),
                ));
            }
            segments.push(seg.text.clone());
        }

        Ok(Reference::from_path(segments))
    }
}

// ============================================================================
// Helper functions
// ============================================================================

fn unquote_string(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn parse_decimal_string(text: &str, span: &Span, parser: &Parser) -> Result<Decimal, Error> {
    let clean = text.replace(['_', ','], "");
    Decimal::from_str(&clean).map_err(|_| {
        Error::parsing(
            format!(
                "Invalid number: '{}'. Expected a valid decimal number (e.g., 42, 3.14, 1_000_000)",
                text
            ),
            parser.make_source(span.clone()),
            None::<String>,
        )
    })
}

fn is_comparison_operator(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Gt
            | TokenKind::Lt
            | TokenKind::Gte
            | TokenKind::Lte
            | TokenKind::EqEq
            | TokenKind::BangEq
            | TokenKind::Is
    )
}

// Helper trait for TokenKind
impl TokenKind {
    fn is_identifier_like(&self) -> bool {
        matches!(self, TokenKind::Identifier)
            || can_be_label(self)
            || is_type_keyword(self)
            || is_boolean_keyword(self)
            || is_duration_unit(self)
            || is_math_function(self)
    }
}
