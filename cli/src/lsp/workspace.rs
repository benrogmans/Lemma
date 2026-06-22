use lemma::{parse, Context, Error, LemmaRepository, ParseResult, ResourceLimits};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tower_lsp::lsp_types::Url;

/// Web virtual dependency docs use `file:///lemma/repo/<hex-utf8>.lemma` (`hex` lowercase per client).
/// Attributed like [`lemma::Engine::load_batch`]'s `(sources, dependency)`.
const VIRTUAL_LEMMA_REPO_PREFIX: &str = "/lemma/repo/";

/// Result of parsing a single file's content.
enum ParseOutcome {
    /// Parsing succeeded; repositories group specs as in source order.
    Success(ParseResult),
    /// Parsing failed with errors.
    Failed(Vec<Error>),
}

/// A single file tracked by the workspace.
struct TrackedFile {
    /// The latest URL for this file (used for publishing diagnostics).
    url: Url,
    /// The latest text content of the file (from the editor buffer or disk).
    text: String,
    /// The parsed outcome: either successfully parsed specs or parse errors.
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
    /// Workspace root directory (host); `None` on WASM or single-file mode.
    workspace_root: Option<std::path::PathBuf>,
    /// Map from source attribute to tracked file state.
    files: HashMap<String, TrackedFile>,
    /// Resource limits used during parsing.
    limits: ResourceLimits,
}

impl WorkspaceModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the workspace root used to locate `lemma_deps/` and attribute disk paths.
    pub fn set_workspace_root(&mut self, root: std::path::PathBuf) {
        self.workspace_root = Some(root);
    }

    /// Root directory when the host workspace folder is known.
    #[must_use]
    pub fn workspace_root(&self) -> Option<&std::path::PathBuf> {
        self.workspace_root.as_ref()
    }

    /// Decode hex UTF-8 path segment emitted by Lemma Base virtual bundle URIs.
    fn dependency_id_from_hex_segment(seg: &str) -> Option<String> {
        if seg.is_empty() || seg == "_" {
            return None;
        }
        if !seg.len().is_multiple_of(2) {
            return None;
        }
        let mut bytes = Vec::with_capacity(seg.len() / 2);
        for chunk in seg.as_bytes().chunks_exact(2) {
            let h = std::str::from_utf8(chunk).ok()?;
            let b = u8::from_str_radix(h, 16).ok()?;
            bytes.push(b);
        }
        let s = String::from_utf8(bytes).ok()?;
        (!s.trim().is_empty()).then_some(s)
    }

    /// Canonical dependency id extracted from `/lemma/repo/<hex>.lemma` (RFC `file:` path).
    fn virtual_bundle_dependency_id(url: &Url) -> Option<String> {
        let path = url.path();
        let rest = path.strip_prefix(VIRTUAL_LEMMA_REPO_PREFIX)?;
        let seg = rest.strip_suffix(".lemma")?;
        Self::dependency_id_from_hex_segment(seg)
    }

    /// Parity with `Engine::load_batch` when the second argument is `Some(dependency_id)`:
    /// keep parsed repository names; for anonymous repositories use the virtual bundle id as
    /// [`LemmaRepository::name`] and set [`LemmaRepository::dependency`].
    ///
    /// Files under `<workspace_root>/lemma_deps/` are attributed like dependency bundles (same as CLI).
    fn repository_arc_for_workspace_file(
        url: &Url,
        parsed_repo: &Arc<LemmaRepository>,
        workspace_root: Option<&Path>,
    ) -> Arc<LemmaRepository> {
        #[cfg(target_arch = "wasm32")]
        let _ = workspace_root;

        if let Some(bundle_id) = Self::virtual_bundle_dependency_id(url) {
            let repo = parsed_repo.as_ref();
            let name = repo.name.clone().or_else(|| Some(bundle_id.clone()));
            let mut out = LemmaRepository::new(name)
                .with_start_line(repo.start_line)
                .with_dependency(bundle_id.clone());
            if let Some(st) = repo.source_type.clone() {
                out = out.with_source_type(st);
            }
            return Arc::new(out);
        }

        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(root), Ok(path)) = (workspace_root, url.to_file_path()) {
            let deps_dir = lemma::deps::lemma_deps_dir(root);
            if path.starts_with(&deps_dir) {
                let dep_id = lemma::deps::dependency_identifier_from_dependency_path(root, &path);
                let repo = parsed_repo.as_ref();
                let repo_name = repo.name.clone().or_else(|| Some(dep_id.clone()));
                return Arc::new(
                    LemmaRepository::new(repo_name)
                        .with_start_line(repo.start_line)
                        .with_dependency(dep_id),
                );
            }
        }

        Arc::clone(parsed_repo)
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
        let parse_outcome = match parse(
            &text,
            lemma::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(&attribute))),
            &self.limits,
        ) {
            Ok(result) => ParseOutcome::Success(result),
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

    /// Successful parse tree for a tracked file, if any.
    pub fn parse_success_for_url(&self, url: &Url) -> Option<&ParseResult> {
        let attribute = Self::attribute_for_url(url);
        self.files
            .get(&attribute)
            .and_then(|t| match &t.parse_outcome {
                ParseOutcome::Success(pr) => Some(pr),
                ParseOutcome::Failed(_) => None,
            })
    }

    /// Insert all successfully parsed workspace files into `ctx` (same attribution as validation).
    pub fn insert_specs_into_context(&self, ctx: &mut Context) -> Vec<(String, Error)> {
        let mut insert_errors = Vec::new();
        for tracked in self.files.values() {
            if let ParseOutcome::Success(parse_result) = &tracked.parse_outcome {
                for (parsed_repo, specs) in &parse_result.repositories {
                    let repository_arc = Self::repository_arc_for_workspace_file(
                        &tracked.url,
                        parsed_repo,
                        self.workspace_root.as_deref(),
                    );
                    for spec in specs {
                        let attr = spec
                            .source_type
                            .as_ref()
                            .expect("BUG: spec missing source_type after parsing")
                            .to_string();
                        match ctx.insert_spec(Arc::clone(&repository_arc), Arc::new(spec.clone())) {
                            Ok(()) => {}
                            Err(e) => insert_errors.push((attr, e)),
                        }
                    }
                }
            }
        }
        insert_errors
    }

    /// Embedded stdlib plus all workspace specs (same composition as [`Engine::new`] after load).
    pub fn engine_with_workspace(&self) -> lemma::Engine {
        let mut engine = lemma::Engine::new();
        let _ = self.insert_specs_into_context(engine.specs_mut());
        engine
    }

    /// Run a full workspace validation: parse errors + planning errors for all files.
    pub fn validate_workspace(&self) -> Vec<FileDiagnostics> {
        let mut engine = lemma::Engine::new();
        let insert_errors = self.insert_specs_into_context(engine.specs_mut());
        let mut results = self.validate_workspace_with_resolved_specs(engine.specs());
        for (attr, e) in insert_errors {
            if let Some(r) = results.iter_mut().find(|d| d.attribute == attr) {
                r.errors.push(e);
            } else if let Some(r) = results.first_mut() {
                r.errors.push(e);
            }
        }
        results
    }

    /// Run planning with the given context. Returns one FileDiagnostics per workspace file.
    pub fn validate_workspace_with_resolved_specs(&self, ctx: &Context) -> Vec<FileDiagnostics> {
        let mut planning_errors_by_attribute: HashMap<String, Vec<Error>> = HashMap::new();

        let planning_result = lemma::plan(ctx, &self.limits);
        let all_planning_errors: Vec<Error> = planning_result
            .results
            .into_iter()
            .flat_map(|set| {
                set.slice_results
                    .into_iter()
                    .flat_map(|sr| {
                        let ctx_spec = Arc::clone(&sr.spec);
                        sr.errors
                            .into_iter()
                            .map(move |e| e.with_spec_context(Arc::clone(&ctx_spec)))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        for error in all_planning_errors {
            let err_attr = error
                .location()
                .map(|s| s.source_type.to_string())
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
    fn update_file_and_validate_single_valid_spec() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/test.lemma");
        workspace.update_file(
            url.clone(),
            "spec test\ndata x: 10\nrule y: x + 1".to_string(),
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
    fn cross_spec_reference_resolves_when_both_files_present() {
        let mut workspace = WorkspaceModel::new();
        let url_a = url_from_path("/tmp/a.lemma");
        let url_b = url_from_path("/tmp/b.lemma");

        workspace.update_file(
            url_a.clone(),
            "spec person\ndata name: \"Alice\"\ndata age: 30".to_string(),
        );
        workspace.update_file(
            url_b.clone(),
            "spec company\nuses employee: person\nwith employee.name: \"Bob\"".to_string(),
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
    fn missing_cross_spec_reference_produces_planning_error() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/orphan.lemma");
        workspace.update_file(
            url.clone(),
            "spec orphan\nuses other: nonexistent".to_string(),
        );

        let results = workspace.validate_workspace();
        assert_eq!(results.len(), 1);
        assert!(
            !results[0].errors.is_empty(),
            "Expected planning error for missing spec reference"
        );
    }

    #[test]
    fn remove_file_clears_it_from_workspace() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/remove_me.lemma");
        workspace.update_file(url.clone(), "spec test\ndata x: 10".to_string());
        assert!(workspace.contains_file(&url));

        workspace.remove_file(&url);
        assert!(!workspace.contains_file(&url));

        let results = workspace.validate_workspace();
        assert!(results.is_empty());
    }

    #[test]
    fn same_file_different_urls_produces_single_entry() {
        let mut workspace = WorkspaceModel::new();
        let url1 = url_from_path("/tmp/test.lemma");
        let url2 = url_from_path("/tmp/test.lemma");
        workspace.update_file(url1, "spec test\ndata x: 10".to_string());
        workspace.update_file(url2, "spec test\ndata x: 20".to_string());

        let results = workspace.validate_workspace();
        assert_eq!(
            results.len(),
            1,
            "Same file should produce exactly one entry"
        );
    }

    #[test]
    fn planning_error_stays_on_owning_file_only() {
        let mut workspace = WorkspaceModel::new();
        let url_bad = url_from_path("/tmp/lsp_bad_import.lemma");
        let url_ok = url_from_path("/tmp/lsp_clean.lemma");
        workspace.update_file(
            url_bad.clone(),
            "spec consumer\nuses dep: no_such_dep\nwith dep.money: 10\ndata x: 1".to_string(),
        );
        workspace.update_file(url_ok.clone(), "spec other\ndata y: 2".to_string());

        let results = workspace.validate_workspace();
        let diag_bad = results
            .iter()
            .find(|d| d.url == url_bad)
            .expect("bad file diagnostics");
        let diag_ok = results
            .iter()
            .find(|d| d.url == url_ok)
            .expect("ok file diagnostics");
        assert!(
            !diag_bad.errors.is_empty(),
            "bad file should have errors: {:?}",
            diag_bad.errors
        );
        assert!(
            diag_ok.errors.is_empty(),
            "clean file should have no errors, got {:?}",
            diag_ok.errors
        );
    }

    #[test]
    fn deps_lemma_files_use_registry_identity_like_cli_load_batch() {
        let root = std::env::temp_dir().join("lemma_lsp_deps_workspace_test");
        let _ = std::fs::remove_dir_all(&root);
        let dep_path = lemma::deps::dependency_cache_file(&root, "@iso/countries");
        std::fs::create_dir_all(dep_path.parent().expect("dep parent")).expect("create dep dir");
        std::fs::write(
            &dep_path,
            "spec alpha2 2024\ndata code: text\n -> option \"NL\"\n",
        )
        .expect("write dep");
        let main_path = root.join("main.lemma");
        std::fs::write(
            &main_path,
            "spec demo\nuses iso: @iso/countries alpha2 2026\nwith iso.code: \"NL\"\n",
        )
        .expect("write main");

        let mut workspace = WorkspaceModel::new();
        workspace.set_workspace_root(root.clone());
        let url_main = Url::from_file_path(&main_path).expect("main url");
        let url_dep = Url::from_file_path(&dep_path).expect("dep url");
        workspace.update_file(
            url_main,
            std::fs::read_to_string(&main_path).expect("read main"),
        );
        workspace.update_file(
            url_dep,
            std::fs::read_to_string(&dep_path).expect("read dep"),
        );

        let results = workspace.validate_workspace();
        for diag in &results {
            for err in &diag.errors {
                let msg = format!("{err}");
                assert!(
                    !msg.contains("Missing repository"),
                    "unexpected missing repository: {msg}"
                );
                assert!(!msg.contains("not loaded"), "unexpected not loaded: {msg}");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn inline_registry_repo_spec_keeps_host_file_as_source_type() {
        let root = std::env::temp_dir().join("lemma_lsp_inline_registry_repo_test");
        let _ = std::fs::remove_dir_all(&root);
        let dep_path = lemma::deps::dependency_cache_file(&root, "@iso/countries");
        std::fs::create_dir_all(dep_path.parent().expect("dep parent")).expect("create dep dir");
        let src = "spec consumer\nuses @user/somedep some_spec\ndata x: 1\n\nrepo @user/somedep\nspec some_spec\ndata y: 2\n";
        std::fs::write(&dep_path, src).expect("write dep");

        let mut workspace = WorkspaceModel::new();
        workspace.set_workspace_root(root.clone());
        let url_dep = Url::from_file_path(&dep_path).expect("dep url");
        workspace.update_file(url_dep, src.to_string());

        let mut ctx = Context::new();
        let insert_errs = workspace.insert_specs_into_context(&mut ctx);
        assert!(insert_errs.is_empty(), "insert errors: {:?}", insert_errs);

        let repo = ctx
            .find_repository("@user/somedep")
            .expect("@user/somedep repo");
        let spec_set = ctx.spec_set(&repo, "some_spec").expect("some_spec set");
        let resolved = spec_set
            .spec_at(&lemma::EffectiveDate::Origin)
            .expect("some_spec at origin");

        let path_from_spec = match resolved.source_type.as_ref() {
            Some(lemma::SourceType::Path(p)) => p.as_ref().clone(),
            o => panic!("expected Path source_type, got {:?}", o),
        };
        assert_eq!(path_from_spec, dep_path);

        let cache_path = lemma::deps::dependency_cache_file(&root, "@user/somedep");
        assert!(
            !cache_path.exists(),
            "test assumes no fetched bundle at {:?}",
            cache_path
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    fn hex_utf8_path_segment(s: &str) -> String {
        s.bytes().map(|b| format!("{:02x}", b)).collect()
    }

    #[test]
    fn virtual_bundle_hex_path_scopes_unnamed_repo_like_load_batch() {
        let dep_id = "@scope/pkg";
        let hex = hex_utf8_path_segment(dep_id);
        let dep_url = Url::parse(&format!("file:///lemma/repo/{hex}.lemma")).unwrap();
        let main_url = url_from_path("/tmp/main.lemma");

        let mut workspace = WorkspaceModel::new();
        workspace.update_file(dep_url, "spec constants\ndata x: 1".to_string());
        workspace.update_file(
            main_url,
            "spec root\nuses @scope/pkg constants\nrule ok: constants.x".to_string(),
        );

        let results = workspace.validate_workspace();
        for result in &results {
            assert!(
                result.errors.is_empty(),
                "file {}: {:?}",
                result.url,
                result.errors
            );
        }
    }

    #[test]
    fn validate_workspace_uses_lemma_duration_compound_units() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/contractor.lemma");
        workspace.update_file(
            url,
            r#"spec contractor
uses lemma units

data money: quantity
  -> unit eur 1.00

data wage_rate: quantity
  -> unit eur_per_second eur/second
  -> unit eur_per_hour eur/hour

rule smoke: true
"#
            .to_string(),
        );

        let results = workspace.validate_workspace();
        assert_eq!(results.len(), 1);
        assert!(
            results[0].errors.is_empty(),
            "uses lemma units must resolve stdlib duration units: {:?}",
            results[0].errors
        );
    }

    #[test]
    fn validate_workspace_rejects_forward_pin_same_name_import() {
        let mut workspace = WorkspaceModel::new();
        let url = url_from_path("/tmp/finance_consumer.lemma");
        workspace.update_file(
            url.clone(),
            r#"spec finance
data rate: number -> default 0

spec finance 2026-05-20
uses fin: finance 2027
"#
            .to_string(),
        );

        let results = workspace.validate_workspace();
        let consumer = results
            .iter()
            .find(|r| r.url == url)
            .expect("consumer file diagnostics");
        assert!(
            !consumer.errors.is_empty(),
            "forward pin without exact slice must produce diagnostics: {:?}",
            consumer.errors
        );
        let joined: String = consumer
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            joined.contains("active at that instant") || joined.contains("cannot reference itself"),
            "expected planning import error, got: {joined}"
        );
    }
}
