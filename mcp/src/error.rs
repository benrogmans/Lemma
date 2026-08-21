use lemma::EngineError;

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

    pub fn diagnostics(errors: &[lemma::Error]) -> Self {
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

pub fn map_engine_error(error: lemma::Error) -> ToolError {
    match error.request_kind() {
        Some(lemma::RequestErrorKind::SpecNotFound) => ToolError::not_found(error.message()),
        Some(lemma::RequestErrorKind::RuleNotFound)
        | Some(lemma::RequestErrorKind::InvalidRequest) => {
            ToolError::invalid_arguments(error.message())
        }
        None => match error.kind() {
            lemma::ErrorKind::MissingRepository => ToolError::not_found(error.message()),
            lemma::ErrorKind::Request => ToolError::not_found(error.message()),
            lemma::ErrorKind::Parsing
            | lemma::ErrorKind::Validation
            | lemma::ErrorKind::Inversion
            | lemma::ErrorKind::Registry
            | lemma::ErrorKind::ResourceLimit => ToolError::invalid_arguments(error.message()),
        },
    }
}
