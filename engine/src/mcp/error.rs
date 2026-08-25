use crate::error::{EngineError, Error};

/// Expected MCP tool failure. Unexpected engine invariants still panic at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    InvalidArguments(String),
    NotFound(String),
    Diagnostics(String),
}

impl ToolError {
    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::InvalidArguments(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn diagnostics(errors: &[Error]) -> Self {
        let diagnostics: Vec<EngineError> = errors.iter().map(EngineError::from).collect();
        let text = serde_json::to_string_pretty(&diagnostics)
            .expect("BUG: EngineError diagnostics must serialize");
        Self::Diagnostics(text)
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArguments(message)
            | Self::NotFound(message)
            | Self::Diagnostics(message) => f.write_str(message),
        }
    }
}

/// Unknown or malformed resource URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceError {
    UnknownUri(String),
}

impl std::fmt::Display for ResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownUri(message) => f.write_str(message),
        }
    }
}
