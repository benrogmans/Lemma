//! AST infrastructure types
//!
//! This module contains metadata types used throughout the parser and compiler:
//! - `Span` for tracking source code locations
//! - `ExpressionId` for uniquely identifying AST nodes
//! - `ExpressionIdGenerator` for generating unique IDs during parsing

use std::fmt;

/// Span representing a location in source code
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn from_pest_span(span: pest::Span) -> Self {
        let (line, col) = span.start_pos().line_col();
        Self {
            start: span.start(),
            end: span.end(),
            line,
            col,
        }
    }
}

/// Unique identifier for each expression in the AST
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExpressionId(u64);

impl ExpressionId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl fmt::Display for ExpressionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expr_{}", self.0)
    }
}

/// Counter for generating unique expression IDs
#[derive(Default)]
pub struct ExpressionIdGenerator {
    next_id: u64,
}

impl ExpressionIdGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn next_id(&mut self) -> ExpressionId {
        let id = ExpressionId(self.next_id);
        self.next_id += 1;
        id
    }
}
