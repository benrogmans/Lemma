use lemma::{parse, Error, LemmaDoc, ResourceLimits};
use std::collections::HashMap;
use tower_lsp::lsp_types::Url;

/// Result of parsing a single file's content.
enum ParseOutcome {
    /// Parsing succeeded, producing one or more LemmaDoc ASTs.
    Success(Vec<LemmaDoc>),
    /// Parsing failed with errors.
    Failed(Vec<Error>),
}

/// A single file tracked by the workspace.
struct TrackedFile {
    /// The latest URL for this file (used for publishing diagnostics).
    url: Url,
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
    /// The source attribute used during parsing (maps to Error source locations).
    pub attribute: String,
    /// All errors for this file (parse errors + planning errors).
    pub errors: Vec<Error>,
}

/// In-memory workspace model.
///
/// Tracks all `.lemma` files in the workspace, their parsed ASTs,
/// and supports re-parsing and re-planning when files change.
///
/// Keyed by **attribute** (file path string or URL string) so that the same
/// physical file is tracked exactly once, regardless of how the URL is constructed.
#[derive(Default)]
pub struct WorkspaceModel {
    /// Map from source attribute to tracked file state.
    files: HashMap<String, TrackedFile>,
    /// Resource limits used during parsing.
    limits: ResourceLimits,
}

impl WorkspaceModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Derive a stable source attribute from a URL (path or URL string).
    fn attribute_for_url(url: &Url) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(path) = url.to_file_path() {
            return path.to_string_lossy().to_string();
        }
        url.to_string()
    }

    /// Add or update a file in the workspace. Parses immediately.
    /// If a different URL maps to the same attribute (path), the old entry is replaced.
    pub fn update_file(&mut self, url: Url, text: String) {
        let attribute = Self::attribute_for_url(&url);
        let parse_outcome = match parse(&text, &attribute, &self.limits) {
            Ok(docs) => ParseOutcome::Success(docs),
            Err(error) => ParseOutcome::Failed(vec![error]),
        };
        self.files.insert(
            attribute,
            TrackedFile {
                url,
                text,
                parse_outcome,
            },
        );
    }

    /// Remove a file from the workspace.
    pub fn remove_file(&mut self, url: &Url) {
        let attribute = Self::attribute_for_url(url);
        self.files.remove(&attribute);
    }

    /// Collect all successfully parsed LemmaDoc ASTs across the entire workspace.
    pub fn all_parsed_docs(&self) -> Vec<LemmaDoc> {
        let mut all_docs = Vec::new();
        for tracked in self.files.values() {
            if let ParseOutcome::Success(docs) = &tracked.parse_outcome {
                all_docs.extend(docs.iter().cloned());
            }
        }
        all_docs
    }

    /// Build the sources map (attribute -> source text) for planning.
    pub fn sources_map(&self) -> HashMap<String, String> {
        self.files
            .iter()
            .map(|(attribute, tracked)| (attribute.clone(), tracked.text.clone()))
            .collect()
    }

    /// Resource limits used for parsing and registry resolution.
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// Map source attribute to (Url, text) for diagnostics. One entry per file.
    pub fn attribute_to_url_and_text(&self) -> HashMap<String, (Url, String)> {
        self.files
            .iter()
            .map(|(attribute, tracked)| {
                (
                    attribute.clone(),
                    (tracked.url.clone(), tracked.text.clone()),
                )
            })
            .collect()
    }

    /// Run a full workspace validation: parse errors + planning errors for all files.
    pub fn validate_workspace(&self) -> Vec<FileDiagnostics> {
        let all_docs = self.all_parsed_docs();
        let sources = self.sources_map();
        self.validate_workspace_with_resolved_docs(&all_docs, &sources)
    }

    /// Run planning with pre-resolved docs. Returns one FileDiagnostics per workspace file.
    pub fn validate_workspace_with_resolved_docs(
        &self,
        resolved_docs: &[LemmaDoc],
        sources: &HashMap<String, String>,
    ) -> Vec<FileDiagnostics> {
        let mut planning_errors_by_attribute: HashMap<String, Vec<Error>> = HashMap::new();

        // Collect all docs to plan from successfully parsed files.
        let mut docs_to_plan: Vec<&LemmaDoc> = Vec::new();
        let mut doc_attribute_map: HashMap<String, String> = HashMap::new();
        for (attribute, tracked) in &self.files {
            if let ParseOutcome::Success(docs) = &tracked.parse_outcome {
                for doc in docs {
                    docs_to_plan.push(doc);
                    doc_attribute_map.insert(doc.name.clone(), attribute.clone());
                }
            }
        }

        // Plan all documents at once (validates and resolves types once).
        let (_plans, errors) = lemma::planning::plan(&docs_to_plan, resolved_docs, sources.clone());
        for error in errors {
            let err_attr = error
                .location()
                .map(|s| s.attribute.clone())
                .unwrap_or_default();
            planning_errors_by_attribute
                .entry(err_attr)
                .or_default()
                .push(error);
        }

        let mut results = Vec::new();
        for (attribute, tracked) in &self.files {
            let mut file_errors = Vec::new();
            if let ParseOutcome::Failed(parse_errors) = &tracked.parse_outcome {
                file_errors.extend(parse_errors.iter().cloned());
            }
            if let Some(plan_errors) = planning_errors_by_attribute.remove(attribute) {
                file_errors.extend(plan_errors);
            }
            results.push(FileDiagnostics {
                url: tracked.url.clone(),
                text: tracked.text.clone(),
                attribute: attribute.clone(),
                errors: file_errors,
            });
        }
        results
    }

    /// Get the current text content for a file, if tracked.
    pub fn get_file_text(&self, url: &Url) -> Option<&str> {
        let attribute = Self::attribute_for_url(url);
        self.files
            .get(&attribute)
            .map(|tracked| tracked.text.as_str())
    }

    /// Get the current text content and its source attribute for a file, if tracked.
    pub fn get_file_text_and_attribute(&self, url: &Url) -> Option<(&str, &str)> {
        let attribute = Self::attribute_for_url(url);
        self.files
            .get_key_value(&attribute)
            .map(|(key, tracked)| (tracked.text.as_str(), key.as_str()))
    }

    /// Get parse errors for a single file (fast path, no planning).
    pub fn get_parse_errors(&self, url: &Url) -> Vec<Error> {
        let attribute = Self::attribute_for_url(url);
        match self.files.get(&attribute) {
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
            let attribute = Self::attribute_for_url(url);
            self.files.contains_key(&attribute)
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

    #[test]
    fn same_file_different_urls_produces_single_entry() {
        let mut workspace = WorkspaceModel::new();
        let url1 = url_from_path("/tmp/test.lemma");
        let url2 = url_from_path("/tmp/test.lemma");
        workspace.update_file(url1, "doc test\nfact x = 10".to_string());
        workspace.update_file(url2, "doc test\nfact x = 20".to_string());

        let results = workspace.validate_workspace();
        assert_eq!(
            results.len(),
            1,
            "Same file should produce exactly one entry"
        );
    }
}
