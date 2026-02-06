use lemma::{parse, LemmaDoc, LemmaError, ResourceLimits};
use std::collections::HashMap;
use tower_lsp::lsp_types::Url;

/// Result of parsing a single file's content.
enum ParseOutcome {
    /// Parsing succeeded, producing one or more LemmaDoc ASTs.
    Success(Vec<LemmaDoc>),
    /// Parsing failed with errors.
    Failed(Vec<LemmaError>),
}

/// A single file tracked by the workspace.
struct TrackedFile {
    /// The latest text content of the file (from the editor buffer or disk).
    text: String,
    /// The parsed outcome: either successfully parsed docs or parse errors.
    parse_outcome: ParseOutcome,
}

/// Per-file diagnostic result after a full workspace validation pass.
pub struct FileDiagnostics {
    /// The URL of the file.
    pub url: Url,
    /// The latest text content (for byte-offset to LSP Range conversion).
    pub text: String,
    /// The source attribute used during parsing (maps to LemmaError source locations).
    pub attribute: String,
    /// All errors for this file (parse errors + planning errors).
    pub errors: Vec<LemmaError>,
}

/// In-memory workspace model.
///
/// Tracks all `.lemma` files in the workspace, their parsed ASTs,
/// and supports re-parsing and re-planning when files change.
pub struct WorkspaceModel {
    /// Map from file URL to tracked file state.
    files: HashMap<Url, TrackedFile>,
    /// Resource limits used during parsing.
    limits: ResourceLimits,
}

impl WorkspaceModel {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            limits: ResourceLimits::default(),
        }
    }

    /// Derive a stable source attribute from a URL.
    ///
    /// Uses the file path if available, otherwise the full URL string.
    fn attribute_for_url(url: &Url) -> String {
        url.to_file_path()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|_| url.to_string())
    }

    /// Add or update a file in the workspace.
    ///
    /// Parses the content immediately. Planning is deferred to `validate_workspace`.
    pub fn update_file(&mut self, url: Url, text: String) {
        let attribute = Self::attribute_for_url(&url);
        let parse_outcome = match parse(&text, &attribute, &self.limits) {
            Ok(docs) => ParseOutcome::Success(docs),
            Err(error) => ParseOutcome::Failed(vec![error]),
        };
        self.files.insert(
            url,
            TrackedFile {
                text,
                parse_outcome,
            },
        );
    }

    /// Remove a file from the workspace.
    pub fn remove_file(&mut self, url: &Url) {
        self.files.remove(url);
    }

    /// Collect all successfully parsed LemmaDoc ASTs across the entire workspace.
    fn all_parsed_docs(&self) -> Vec<LemmaDoc> {
        let mut all_docs = Vec::new();
        for tracked in self.files.values() {
            if let ParseOutcome::Success(docs) = &tracked.parse_outcome {
                all_docs.extend(docs.iter().cloned());
            }
        }
        all_docs
    }

    /// Build the sources map (attribute -> source text) for planning.
    fn sources_map(&self) -> HashMap<String, String> {
        let mut sources = HashMap::new();
        for (url, tracked) in &self.files {
            let attribute = Self::attribute_for_url(url);
            sources.insert(attribute, tracked.text.clone());
        }
        sources
    }

    /// Run a full workspace validation: parse errors + planning errors for all files.
    ///
    /// Returns diagnostics grouped by file. Each file gets:
    /// - Its own parse errors (if parsing failed).
    /// - Planning errors attributed to this file (filtered by source attribute).
    ///
    /// This is the method called after the debounce window settles.
    pub fn validate_workspace(&self) -> Vec<FileDiagnostics> {
        let all_docs = self.all_parsed_docs();
        let sources = self.sources_map();

        // Run plan() for each successfully parsed document's main doc.
        // Collect all planning errors, keyed by their source attribute.
        let mut planning_errors_by_attribute: HashMap<String, Vec<LemmaError>> = HashMap::new();

        for (url, tracked) in &self.files {
            let attribute = Self::attribute_for_url(url);

            if let ParseOutcome::Success(docs) = &tracked.parse_outcome {
                for doc in docs {
                    match lemma::planning::plan(doc, &all_docs, sources.clone()) {
                        Ok(_) => {
                            // Planning succeeded for this document — no errors.
                        }
                        Err(errors) => {
                            for error in errors {
                                // Determine which file this error belongs to based on its
                                // source location attribute. Errors without a location are
                                // attributed to the file that owns the document being planned.
                                let error_attribute = error
                                    .location()
                                    .map(|source| source.attribute.clone())
                                    .unwrap_or_else(|| attribute.clone());

                                planning_errors_by_attribute
                                    .entry(error_attribute)
                                    .or_default()
                                    .push(error);
                            }
                        }
                    }
                }
            }
        }

        // Build per-file diagnostics.
        let mut results = Vec::new();

        for (url, tracked) in &self.files {
            let attribute = Self::attribute_for_url(url);
            let mut file_errors = Vec::new();

            // Add parse errors for this file.
            if let ParseOutcome::Failed(parse_errors) = &tracked.parse_outcome {
                file_errors.extend(parse_errors.iter().cloned());
            }

            // Add planning errors attributed to this file.
            if let Some(plan_errors) = planning_errors_by_attribute.remove(&attribute) {
                file_errors.extend(plan_errors);
            }

            results.push(FileDiagnostics {
                url: url.clone(),
                text: tracked.text.clone(),
                attribute,
                errors: file_errors,
            });
        }

        results
    }

    /// Get the current text content for a file, if tracked.
    pub fn get_file_text(&self, url: &Url) -> Option<&str> {
        self.files.get(url).map(|tracked| tracked.text.as_str())
    }

    /// Get parse errors for a single file (fast path, no planning).
    ///
    /// Used to immediately publish parse-level diagnostics before the
    /// debounced full workspace validation runs.
    pub fn get_parse_errors(&self, url: &Url) -> Vec<LemmaError> {
        match self.files.get(url) {
            Some(tracked) => match &tracked.parse_outcome {
                ParseOutcome::Failed(errors) => errors.clone(),
                ParseOutcome::Success(_) => Vec::new(),
            },
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl WorkspaceModel {
        fn contains_file(&self, url: &Url) -> bool {
            self.files.contains_key(url)
        }
    }

    fn url_from_path(path: &str) -> Url {
        Url::from_file_path(path).expect("valid file path for test URL")
    }

    #[test]
    fn update_file_and_validate_single_valid_document() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/test.lemma");
        workspace.update_file(
            url.clone(),
            "doc test\nfact x = 10\nrule y = x + 1".to_string(),
        );

        let results = workspace.validate_workspace();
        assert_eq!(results.len(), 1);
        assert!(
            results[0].errors.is_empty(),
            "Expected no errors, got: {:?}",
            results[0].errors
        );
    }

    #[test]
    fn update_file_with_parse_error_produces_diagnostics() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/broken.lemma");
        workspace.update_file(url.clone(), "this is not valid lemma syntax".to_string());

        let results = workspace.validate_workspace();
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].errors.is_empty(),
            "Expected parse errors for invalid input"
        );
    }

    #[test]
    fn cross_document_reference_resolves_when_both_files_present() {
        let mut workspace = WorkspaceModel::new();
        let url_a = url_from_path("/tmp/a.lemma");
        let url_b = url_from_path("/tmp/b.lemma");

        workspace.update_file(
            url_a.clone(),
            "doc person\nfact name = \"Alice\"\nfact age = 30".to_string(),
        );
        workspace.update_file(
            url_b.clone(),
            "doc company\nfact employee = doc person\nfact employee.name = \"Bob\"".to_string(),
        );

        let results = workspace.validate_workspace();
        for result in &results {
            assert!(
                result.errors.is_empty(),
                "Expected no errors for file {}, got: {:?}",
                result.url,
                result.errors
            );
        }
    }

    #[test]
    fn missing_cross_document_reference_produces_planning_error() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/orphan.lemma");
        workspace.update_file(
            url.clone(),
            "doc orphan\nfact other = doc nonexistent".to_string(),
        );

        let results = workspace.validate_workspace();
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].errors.is_empty(),
            "Expected planning error for missing document reference"
        );
    }

    #[test]
    fn remove_file_clears_it_from_workspace() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/remove_me.lemma");
        workspace.update_file(url.clone(), "doc test\nfact x = 10".to_string());
        assert!(workspace.contains_file(&url));

        workspace.remove_file(&url);
        assert!(!workspace.contains_file(&url));

        let results = workspace.validate_workspace();
        assert!(results.is_empty());
    }

    #[test]
    fn get_parse_errors_returns_empty_for_valid_file() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/valid.lemma");
        workspace.update_file(url.clone(), "doc test\nfact x = 10".to_string());

        let errors = workspace.get_parse_errors(&url);
        assert!(errors.is_empty());
    }

    #[test]
    fn get_parse_errors_returns_errors_for_invalid_file() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/invalid.lemma");
        workspace.update_file(url.clone(), "not valid lemma".to_string());

        let errors = workspace.get_parse_errors(&url);
        assert!(!errors.is_empty());
    }
}
