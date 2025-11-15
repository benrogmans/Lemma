//! AST infrastructure types
//!
//! This module contains metadata types used throughout the parser and compiler:
//! - `Span` for tracking source code locations
//! - `ExpressionId` for uniquely identifying AST nodes
//! - `ExpressionIdGenerator` for generating unique IDs during parsing

use std::fmt;

/// Span representing a location in source code
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
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
pub struct ExpressionIdGenerator {
    next_id: u64,
    depth: usize,
    max_depth: usize,
}

impl ExpressionIdGenerator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_depth(max_depth: usize) -> Self {
        Self {
            next_id: 0,
            depth: 0,
            max_depth,
        }
    }

    pub fn next_id(&mut self) -> ExpressionId {
        let id = ExpressionId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn push_depth(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(format!(
                "Expression depth {} exceeds maximum of {}",
                self.depth, self.max_depth
            ));
        }
        Ok(())
    }

    pub fn pop_depth(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    pub fn max_depth(&self) -> usize {
        self.max_depth
    }
}

impl Default for ExpressionIdGenerator {
    fn default() -> Self {
        Self {
            next_id: 0,
            depth: 0,
            max_depth: 100, // Default limit
        }
    }
}
