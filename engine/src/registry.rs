//! Registry trait, types, and resolution logic for external repository references.
//!
//! A Registry maps repository identifiers to Lemma source text (for resolution)
//! and to human-facing addresses (for editor navigation).
//!
//! The engine calls `resolve_registry_references` during the resolution step
//! (after parsing local files, before planning) to fetch external specs.
//! The Language Server calls `url_for_id` to produce clickable links.
//!
//! Input to all methods is the full repository name as it appears in source
//! (e.g. `"@org/project"` including the `@` prefix).

use crate::parsing::ast::DateTimeValue;
#[cfg(feature = "registry")]
use crate::parsing::ast::LemmaRepository;
use std::fmt;
#[cfg(feature = "registry")]
use std::sync::Arc;

#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
use std::path::{Path, PathBuf};

#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
use {
    crate::engine::Context,
    crate::error::Error,
    crate::limits::ResourceLimits,
    crate::parsing::ast::{DataValue, RepositoryQualifier, SpecRef},
    crate::parsing::source::Source,
    std::collections::{HashMap, HashSet},
};

// ---------------------------------------------------------------------------
// Trait and types
// ---------------------------------------------------------------------------

/// A bundle of Lemma source text returned by the Registry.
///
/// Contains one or more `spec ...` blocks as raw Lemma source code.
#[derive(Debug, Clone)]
pub struct RegistryBundle {
    /// Lemma source containing one or more `spec ...` blocks.
    pub lemma_source: String,

    /// Source identifier used for diagnostics and explanations
    pub source_type: crate::parsing::source::SourceType,
}

/// The kind of failure that occurred during a Registry operation.
///
/// Registry implementations classify their errors into these kinds so that
/// the engine (and ultimately the user) can distinguish between a missing
/// spec, an authorization failure, a network outage, etc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryErrorKind {
    /// The requested spec or type was not found (e.g. HTTP 404).
    NotFound,
    /// The request was unauthorized or forbidden (e.g. HTTP 401, 403).
    Unauthorized,
    /// A network or transport error occurred (DNS failure, timeout, connection refused).
    NetworkError,
    /// The registry server returned an internal error (e.g. HTTP 5xx).
    ServerError,
    /// An error that does not fit the other categories.
    Other,
}

impl fmt::Display for RegistryErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::Unauthorized => write!(f, "unauthorized"),
            Self::NetworkError => write!(f, "network error"),
            Self::ServerError => write!(f, "server error"),
            Self::Other => write!(f, "error"),
        }
    }
}

/// An error returned by a Registry implementation.
#[derive(Debug, Clone)]
pub struct RegistryError {
    pub message: String,
    pub kind: RegistryErrorKind,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for RegistryError {}

/// Trait for resolving external repository references.
///
/// Implementations must be `Send + Sync` so they can be shared across threads.
/// Resolution is async so that WASM can use `fetch()` and native can use async HTTP.
///
/// `get` returns a bundle containing ALL temporal versions for the requested
/// identifier. The engine handles temporal resolution locally using
/// `effective_from` on the parsed specs. Registry-qualified `uses`
/// references and `uses`-backed type parents from specs share this resolution path.
///
/// `name` is the full repository name as it appears in source (e.g. `"@org/project"`).
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait Registry: Send + Sync {
    /// Fetch all temporal versions for a repository identifier.
    ///
    /// `name` is the full repository name (e.g. `"@org/project"`).
    /// Returns a bundle whose `lemma_source` contains all temporal versions.
    async fn get(&self, name: &str) -> Result<RegistryBundle, RegistryError>;

    /// Map a repository identifier to a human-facing address for navigation.
    ///
    /// `name` is the full repository name (e.g. `"@org/project"`).
    /// `effective` is an optional datetime for linking directly to a specific
    /// temporal version in the registry UI.
    fn url_for_id(&self, name: &str, effective: Option<&DateTimeValue>) -> Option<String>;
}

// ---------------------------------------------------------------------------
// LemmaBase: the default Registry implementation (feature-gated)
// ---------------------------------------------------------------------------

// Internal HTTP abstraction — async so we can use fetch() in WASM and reqwest on native.

/// Error returned by the internal HTTP fetcher layer.
///
/// Separates HTTP status errors (4xx, 5xx) from transport / parsing errors
/// so that `LemmaBase::fetch_source` can produce distinct error messages.
#[cfg(feature = "registry")]
struct HttpFetchError {
    /// If the failure was an HTTP status code (4xx, 5xx), it is stored here.
    status_code: Option<u16>,
    /// Human-readable error description.
    message: String,
}

/// Internal trait for performing async HTTP GET requests.
///
/// Native uses [`ReqwestHttpFetcher`]; WASM uses [`WasmHttpFetcher`]; tests inject a mock.
#[cfg(feature = "registry")]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
trait HttpFetcher: Send + Sync {
    async fn get(&self, url: &str) -> Result<String, HttpFetchError>;
}

/// Production HTTP fetcher for native (reqwest).
#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
struct ReqwestHttpFetcher;

#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
#[async_trait::async_trait]
impl HttpFetcher for ReqwestHttpFetcher {
    async fn get(&self, url: &str) -> Result<String, HttpFetchError> {
        let response = reqwest::get(url).await.map_err(|e| HttpFetchError {
            status_code: e.status().map(|s| s.as_u16()),
            message: e.to_string(),
        })?;
        let status = response.status();
        let body = response.text().await.map_err(|e| HttpFetchError {
            status_code: None,
            message: e.to_string(),
        })?;
        if !status.is_success() {
            return Err(HttpFetchError {
                status_code: Some(status.as_u16()),
                message: format!("HTTP {}", status),
            });
        }
        Ok(body)
    }
}

/// Production HTTP fetcher for WASM (gloo_net / fetch).
#[cfg(all(feature = "registry", target_arch = "wasm32"))]
struct WasmHttpFetcher;

#[cfg(all(feature = "registry", target_arch = "wasm32"))]
#[async_trait::async_trait(?Send)]
impl HttpFetcher for WasmHttpFetcher {
    async fn get(&self, url: &str) -> Result<String, HttpFetchError> {
        let response = gloo_net::http::Request::get(url)
            .send()
            .await
            .map_err(|e| HttpFetchError {
                status_code: None,
                message: e.to_string(),
            })?;
        let status = response.status();
        let ok = response.ok();
        if !ok {
            return Err(HttpFetchError {
                status_code: Some(status),
                message: format!("HTTP {}", status),
            });
        }
        let text = response.text().await.map_err(|e| HttpFetchError {
            status_code: None,
            message: e.to_string(),
        })?;
        Ok(text)
    }
}

// ---------------------------------------------------------------------------

/// Parse `{base}/{identifier}.lemma` URLs into registry identifiers (e.g. `@iso/countries`).
#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
fn registry_identifier_from_source_url(url: &str) -> Option<String> {
    let without_suffix = url.strip_suffix(".lemma")?;
    let path = without_suffix
        .split_once("://")
        .map_or(without_suffix, |(_, rest)| {
            rest.split_once('/').map_or(rest, |(_, p)| p)
        });
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Serves registry bundles from a `lemma_deps/`-shaped fixture directory (no network).
#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
struct FixtureDirFetcher {
    fixtures: std::collections::HashMap<String, String>,
}

#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
impl FixtureDirFetcher {
    fn from_dir(dir: &Path) -> Self {
        let mut fixtures = std::collections::HashMap::new();
        collect_fixture_files(dir, dir, &mut fixtures);
        Self { fixtures }
    }
}

#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
fn collect_fixture_files(
    dir: &Path,
    base: &Path,
    fixtures: &mut std::collections::HashMap<String, String>,
) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("BUG: read fixture dir {}: {e}", dir.display()));
    for entry in entries {
        let entry =
            entry.unwrap_or_else(|e| panic!("BUG: fixture dir entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            collect_fixture_files(&path, base, fixtures);
            continue;
        }
        if path.extension().is_none_or(|e| e != "lemma") {
            continue;
        }
        let relative = path
            .strip_prefix(base)
            .unwrap_or_else(|_| panic!("BUG: fixture path not under base: {}", path.display()));
        let identifier = relative
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("BUG: read fixture {}: {e}", path.display()));
        fixtures.insert(identifier, content);
    }
}

#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
#[async_trait::async_trait]
impl HttpFetcher for FixtureDirFetcher {
    async fn get(&self, url: &str) -> Result<String, HttpFetchError> {
        let identifier =
            registry_identifier_from_source_url(url).ok_or_else(|| HttpFetchError {
                status_code: None,
                message: format!("fixture URL must end with .lemma: {url}"),
            })?;
        self.fixtures
            .get(&identifier)
            .cloned()
            .ok_or_else(|| HttpFetchError {
                status_code: Some(404),
                message: format!("no fixture for \"{identifier}\" (url {url})"),
            })
    }
}

// ---------------------------------------------------------------------------

/// The LemmaBase registry fetches Lemma source text from LemmaBase.
///
/// This is the default registry for the Lemma engine. It resolves `@...` identifiers
/// via `GET {base}/{name}.lemma` (`name` includes the leading `@`). The base depends on compile profile:
/// [`LemmaBase::BASE_URL`] (`http://localhost:4222` in debug builds,
/// `https://lemmabase.com` in release builds).
///
/// LemmaBase.com returns the requested spec with all of its dependencies inlined,
/// so the resolution loop typically completes in a single iteration.
///
/// This struct is only available when the `registry` feature is enabled (which it is
/// by default). Users who require strict sandboxing (no network access) can compile
/// without this feature.
#[cfg(feature = "registry")]
pub struct LemmaBase {
    fetcher: Box<dyn HttpFetcher>,
}

#[cfg(feature = "registry")]
impl LemmaBase {
    /// LemmaBase registry root: `http://localhost:4222` when `debug_assertions` are on
    /// (normal `cargo build` / `cargo run`), `https://lemmabase.com` in `--release`.
    ///
    /// Same rule for any crate embedding this one (CLI, LSP, WASM) at that profile.
    #[cfg(debug_assertions)]
    pub const BASE_URL: &'static str = "http://localhost:4222";
    #[cfg(not(debug_assertions))]
    pub const BASE_URL: &'static str = "https://lemmabase.com";

    /// Create a new LemmaBase registry backed by the real HTTP client (reqwest on native, fetch on WASM).
    pub fn new() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            fetcher: Box::new(ReqwestHttpFetcher),
            #[cfg(target_arch = "wasm32")]
            fetcher: Box::new(WasmHttpFetcher),
        }
    }

    /// Offline registry backed by [`Self::test_fixtures_dir`] (no network).
    ///
    /// Integration tests and local runs use bundled fixtures under
    /// `engine/tests/registry_fixtures/` (`@iso/countries`, …).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn test() -> Self {
        Self::with_fixture_dir(Self::test_fixtures_dir())
    }

    /// Directory of bundled registry fixtures shipped with `lemma-engine`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn test_fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/registry_fixtures")
    }

    /// Offline registry reading `lemma_deps/`-shaped `.lemma` files from `dir`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_fixture_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            fetcher: Box::new(FixtureDirFetcher::from_dir(dir.as_ref())),
        }
    }

    /// Create a LemmaBase registry with a custom HTTP fetcher (for unit tests in this crate).
    #[cfg(test)]
    fn with_fetcher(fetcher: Box<dyn HttpFetcher>) -> Self {
        Self { fetcher }
    }

    /// Base URL for the spec; when effective is set, appends ?effective=... for temporal resolution.
    fn source_url(&self, name: &str, effective: Option<&DateTimeValue>) -> String {
        let base = format!("{}/{}.lemma", Self::BASE_URL, name);
        match effective {
            None => base,
            Some(d) => format!("{}?effective={}", base, d),
        }
    }

    /// Human-facing URL for navigation; when effective is set, appends ?effective=... for linking to a specific temporal version.
    fn navigation_url(&self, name: &str, effective: Option<&DateTimeValue>) -> String {
        let base = format!("{}/{}", Self::BASE_URL, name);
        match effective {
            None => base,
            Some(d) => format!("{}?effective={}", base, d),
        }
    }

    fn display_id(name: &str, effective: Option<&DateTimeValue>) -> String {
        match effective {
            None => name.to_string(),
            Some(d) => format!("{name} {d}"),
        }
    }

    /// Fetch all zones for the given identifier (no temporal filtering).
    async fn fetch_source(&self, name: &str) -> Result<RegistryBundle, RegistryError> {
        let url = self.source_url(name, None);
        let display = Self::display_id(name, None);

        let lemma_source = self.fetcher.get(&url).await.map_err(|error| {
            if let Some(code) = error.status_code {
                let kind = match code {
                    404 => RegistryErrorKind::NotFound,
                    401 | 403 => RegistryErrorKind::Unauthorized,
                    500..=599 => RegistryErrorKind::ServerError,
                    _ => RegistryErrorKind::Other,
                };
                RegistryError {
                    message: format!("LemmaBase returned HTTP {} {} for '{}'", code, url, display),
                    kind,
                }
            } else {
                RegistryError {
                    message: format!(
                        "Failed to reach LemmaBase for '{}': {}",
                        display, error.message
                    ),
                    kind: RegistryErrorKind::NetworkError,
                }
            }
        })?;

        Ok(RegistryBundle {
            lemma_source,
            source_type: crate::parsing::source::SourceType::Registry(Arc::new(
                LemmaRepository::new(Some(name.to_string())),
            )),
        })
    }
}

#[cfg(feature = "registry")]
impl Default for LemmaBase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "registry")]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Registry for LemmaBase {
    async fn get(&self, name: &str) -> Result<RegistryBundle, RegistryError> {
        self.fetch_source(name).await
    }

    fn url_for_id(&self, name: &str, effective: Option<&DateTimeValue>) -> Option<String> {
        Some(self.navigation_url(name, effective))
    }
}

// ---------------------------------------------------------------------------
// Resolution: fetching external `@...` specs from a Registry
// ---------------------------------------------------------------------------

/// Resolve every `uses` reference that carries a registry repository qualifier in the loaded specs.
///
/// Starting from the already-parsed local specs, this function:
/// 1. Collects every distinct registry repository qualifier referenced by the specs.
/// 2. For each repository qualifier not already loaded into `ctx`, calls the Registry.
/// 3. Parses the returned source text and inserts every spec from the bundle
///    under the registry [`LemmaRepository`] for that fetch (using each reference's
///    [`crate::parsing::ast::SpecRef::repository`] qualifier when present).
/// 4. Recurses: the newly inserted specs may themselves reference further
///    registry repositories.
/// 5. Repeats until no unresolved repository qualifiers remain.
///
/// Errors are fatal: any registry failure or any unresolved qualifier produces
/// errors that are returned to the caller without partial loads being silently
/// retained.
#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
pub async fn resolve_registry_references(
    ctx: &mut Context,
    sources: &mut HashMap<crate::parsing::source::SourceType, String>,
    registry: &dyn Registry,
    limits: &ResourceLimits,
) -> Result<(), Vec<Error>> {
    let mut already_requested: HashSet<String> = HashSet::new();

    loop {
        let unresolved = find_missing_repositories(ctx, &already_requested);

        if unresolved.is_empty() {
            break;
        }

        let mut round_errors: Vec<Error> = Vec::new();
        for reference in &unresolved {
            if already_requested.contains(&reference.repository.name) {
                continue;
            }
            already_requested.insert(reference.repository.name.clone());

            let bundle_result = registry.get(&reference.repository.name).await;

            let bundle = match bundle_result {
                Ok(b) => b,
                Err(registry_error) => {
                    let suggestion = match &registry_error.kind {
                        RegistryErrorKind::NotFound => Some(
                            "Check that the repository qualifier is spelled correctly and that the repository exists on the registry.".to_string(),
                        ),
                        RegistryErrorKind::Unauthorized => Some(
                            "Check your authentication credentials or permissions for this registry.".to_string(),
                        ),
                        RegistryErrorKind::NetworkError => Some(
                            "Check your network connection. To compile without registry access, disable the 'registry' feature.".to_string(),
                        ),
                        RegistryErrorKind::ServerError => Some(
                            "The registry server returned an internal error. Try again later.".to_string(),
                        ),
                        RegistryErrorKind::Other => None,
                    };
                    let spec_context = ctx
                        .iter()
                        .find(|s| s.source_type == Some(reference.source.source_type.clone()));
                    round_errors.push(Error::registry(
                        registry_error.message,
                        reference.source.clone(),
                        reference.repository.name.clone(),
                        registry_error.kind,
                        suggestion,
                        spec_context,
                        None,
                    ));
                    continue;
                }
            };

            sources.insert(bundle.source_type.clone(), bundle.lemma_source.clone());

            let parsed = match crate::parsing::parse(
                &bundle.lemma_source,
                bundle.source_type.clone(),
                limits,
            ) {
                Ok(result) => result,
                Err(e) => {
                    round_errors.push(e);
                    return Err(round_errors);
                }
            };

            for (parsed_repo, specs) in parsed.repositories {
                let repo_name = parsed_repo
                    .name
                    .clone()
                    .unwrap_or_else(|| reference.repository.name.clone());
                let header = LemmaRepository::new(Some(repo_name))
                    .with_dependency(reference.repository.name.clone())
                    .with_start_line(parsed_repo.start_line)
                    .with_source_type(bundle.source_type.clone());
                let repository_arc = Arc::new(header);

                for spec in specs {
                    if let Err(e) = ctx.insert_spec(Arc::clone(&repository_arc), Arc::new(spec)) {
                        round_errors.push(e);
                    }
                }
            }
        }

        if !round_errors.is_empty() {
            return Err(round_errors);
        }
    }

    Ok(())
}

/// A collected registry repository reference needing fetch.
#[derive(Debug, Clone)]
#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
struct RegistryReference {
    repository: RepositoryQualifier,
    source: Source,
}

#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
fn collect_repository_qualifiers_from_spec_ref(
    spec_ref: &SpecRef,
    source: &Source,
    ctx: &Context,
    already_requested: &HashSet<String>,
    seen_in_this_round: &mut HashSet<String>,
    out: &mut Vec<RegistryReference>,
) {
    let Some(qualifier) = spec_ref.repository.as_ref() else {
        return;
    };
    if !qualifier.is_registry() {
        return;
    }
    if ctx.find_repository(&qualifier.name).is_some() {
        return;
    }
    if already_requested.contains(&qualifier.name) {
        return;
    }
    if !seen_in_this_round.insert(qualifier.name.clone()) {
        return;
    }
    out.push(RegistryReference {
        repository: qualifier.clone(),
        source: source.clone(),
    });
}

/// Collect every distinct registry repository qualifier referenced by specs in `ctx`.
#[cfg(all(feature = "registry", not(target_arch = "wasm32")))]
fn find_missing_repositories(
    ctx: &Context,
    already_requested: &HashSet<String>,
) -> Vec<RegistryReference> {
    let mut unresolved: Vec<RegistryReference> = Vec::new();
    let mut seen_in_this_round: HashSet<String> = HashSet::new();

    for spec in ctx.iter() {
        let spec = spec.as_ref();

        for data in &spec.data {
            // `uses <repository> <spec>`
            if let DataValue::Import(spec_ref) = &data.value {
                collect_repository_qualifiers_from_spec_ref(
                    spec_ref,
                    &data.source_location,
                    ctx,
                    already_requested,
                    &mut seen_in_this_round,
                    &mut unresolved,
                );
            }
        }
    }

    unresolved
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Context, Engine};
    use crate::literals::DateGranularity;

    /// A test Registry that returns predefined bundles keyed by name.
    struct TestRegistry {
        bundles: HashMap<String, RegistryBundle>,
    }

    impl TestRegistry {
        fn new() -> Self {
            Self {
                bundles: HashMap::new(),
            }
        }

        /// Add a bundle containing all zones for this identifier (e.g. `"@org/repo"`).
        fn add_spec_bundle(&mut self, identifier: &str, lemma_source: &str) {
            self.bundles.insert(
                identifier.to_string(),
                RegistryBundle {
                    lemma_source: lemma_source.to_string(),
                    source_type: crate::parsing::source::SourceType::Registry(Arc::new(
                        LemmaRepository::new(Some(identifier.to_string())),
                    )),
                },
            );
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    impl Registry for TestRegistry {
        async fn get(&self, name: &str) -> Result<RegistryBundle, RegistryError> {
            self.bundles
                .get(name)
                .cloned()
                .ok_or_else(|| RegistryError {
                    message: format!("'{}' not found in test registry", name),
                    kind: RegistryErrorKind::NotFound,
                })
        }

        fn url_for_id(&self, name: &str, effective: Option<&DateTimeValue>) -> Option<String> {
            if self.bundles.contains_key(name) {
                Some(match effective {
                    None => format!("https://test.registry/{}", name),
                    Some(d) => format!("https://test.registry/{}?effective={}", name, d),
                })
            } else {
                None
            }
        }
    }

    #[tokio::test]
    async fn resolve_with_no_registry_references_returns_local_specs_unchanged() {
        let source = r#"spec example
data price: 100"#;
        let local_specs = crate::parse(
            source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut engine = Engine::new();
        let store = engine.specs_mut();
        let local_repository = store.workspace();
        for spec in &local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec.clone()))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            source.to_string(),
        );

        let registry = TestRegistry::new();
        resolve_registry_references(store, &mut sources, &registry, &ResourceLimits::default())
            .await
            .unwrap();

        assert_eq!(store.len(), 2, "embedded spec units plus workspace example");
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "example"));
        assert!(names.iter().any(|n| n == "units"));
    }

    /// Mirrors `lemma fetch --all`: bare `Context::new()` without embedded stdlib.
    #[tokio::test]
    async fn resolve_does_not_fetch_non_at_qualified_repositories() {
        let local_source = r#"spec burn_baby_burn
uses lemma units
rule x: 1 hour"#;
        let local_specs = crate::parse(
            local_source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut store = Context::new();
        let local_repository = store.workspace();
        for spec in local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            local_source.to_string(),
        );

        let registry = TestRegistry::new();
        let result = resolve_registry_references(
            &mut store,
            &mut sources,
            &registry,
            &ResourceLimits::default(),
        )
        .await;

        assert!(
            result.is_ok(),
            "non-@ repository qualifiers must not be sent to the registry, got: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn resolve_fetches_single_spec_from_registry() {
        let local_source = r#"spec main_spec
uses external: @org/project helper
rule value: external.quantity"#;
        let local_specs = crate::parse(
            local_source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut engine = Engine::new();
        let store = engine.specs_mut();
        let local_repository = store.workspace();
        for spec in local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            local_source.to_string(),
        );

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/project",
            r#"repo @org/project
spec helper
data quantity: 42"#,
        );

        resolve_registry_references(store, &mut sources, &registry, &ResourceLimits::default())
            .await
            .unwrap();

        assert_eq!(store.len(), 3);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "main_spec"));
        assert!(names.iter().any(|n| n == "helper"));
        assert!(names.iter().any(|n| n == "units"));
    }

    #[tokio::test]
    async fn resolve_registry_bundle_without_repo_decl_uses_reference_repository_name() {
        let local_source = r#"spec main_spec
uses external: @org/project helper
rule value: external.quantity"#;
        let local_specs = crate::parse(
            local_source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut engine = Engine::new();
        let store = engine.specs_mut();
        let local_repository = store.workspace();
        for spec in local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            local_source.to_string(),
        );

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/project",
            r#"spec helper
data quantity: 42"#,
        );

        resolve_registry_references(store, &mut sources, &registry, &ResourceLimits::default())
            .await
            .unwrap();

        let ext_repo = store
            .find_repository("@org/project")
            .expect("registry bundle must land under fetched @ id");
        let spec_names: Vec<String> = store
            .repositories()
            .get(&ext_repo)
            .expect("spec sets for @org/project")
            .keys()
            .cloned()
            .collect();
        assert!(
            spec_names.iter().any(|n| n == "helper"),
            "helper spec should live under @org/project, got {:?}",
            spec_names
        );
    }

    #[tokio::test]
    async fn get_returns_all_zones_and_url_for_id_supports_effective() {
        let effective = DateTimeValue {
            year: 2026,
            month: 1,
            day: 15,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };
        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/spec",
            "spec org/spec 2025-01-01\ndata x: 1\n\nspec org/spec 2026-01-15\ndata x: 2",
        );

        let bundle = registry.get("@org/spec").await.unwrap();
        assert!(bundle.lemma_source.contains("data x: 1"));
        assert!(bundle.lemma_source.contains("data x: 2"));

        assert_eq!(
            registry.url_for_id("@org/spec", None),
            Some("https://test.registry/@org/spec".to_string())
        );
        assert_eq!(
            registry.url_for_id("@org/spec", Some(&effective)),
            Some("https://test.registry/@org/spec?effective=2026-01-15".to_string())
        );
    }

    #[tokio::test]
    async fn resolve_fetches_transitive_dependencies() {
        let local_source = r#"spec main_spec
uses a: @org/project spec_a"#;
        let local_specs = crate::parse(
            local_source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut engine = Engine::new();
        let store = engine.specs_mut();
        let local_repository = store.workspace();
        for spec in local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            local_source.to_string(),
        );

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/project",
            r#"repo @org/project
spec spec_a
uses b: @org/sub spec_b"#,
        );
        registry.add_spec_bundle(
            "@org/sub",
            r#"repo @org/sub
spec spec_b
data value: 99"#,
        );

        resolve_registry_references(store, &mut sources, &registry, &ResourceLimits::default())
            .await
            .unwrap();

        assert_eq!(store.len(), 4);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "main_spec"));
        assert!(names.iter().any(|n| n == "spec_a"));
        assert!(names.iter().any(|n| n == "spec_b"));
        assert!(names.iter().any(|n| n == "units"));
    }

    #[tokio::test]
    async fn resolve_handles_bundle_with_multiple_specs() {
        let local_source = r#"spec main_spec
uses a: @org/project spec_a"#;
        let local_specs = crate::parse(
            local_source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut engine = Engine::new();
        let store = engine.specs_mut();
        let local_repository = store.workspace();
        for spec in local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            local_source.to_string(),
        );

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/project",
            r#"repo @org/project
spec spec_a
uses b: spec_b

spec spec_b
data value: 99"#,
        );

        resolve_registry_references(store, &mut sources, &registry, &ResourceLimits::default())
            .await
            .unwrap();

        assert_eq!(store.len(), 4);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "main_spec"));
        assert!(names.iter().any(|n| n == "spec_a"));
        assert!(names.iter().any(|n| n == "spec_b"));
        assert!(names.iter().any(|n| n == "units"));
    }

    #[tokio::test]
    async fn resolve_returns_registry_error_when_registry_fails() {
        let local_source = r#"spec main_spec
uses external: @org/project missing"#;
        let local_specs = crate::parse(
            local_source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut engine = Engine::new();
        let store = engine.specs_mut();
        let local_repository = store.workspace();
        for spec in local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            local_source.to_string(),
        );

        let registry = TestRegistry::new(); // empty — no bundles

        let result =
            resolve_registry_references(store, &mut sources, &registry, &ResourceLimits::default())
                .await;

        assert!(result.is_err(), "Should fail when Registry cannot resolve");
        let errs = result.unwrap_err();
        let registry_err = errs
            .iter()
            .find(|e| matches!(e, Error::Registry { .. }))
            .expect("expected at least one Registry error");
        match registry_err {
            Error::Registry {
                identifier,
                kind,
                details,
            } => {
                assert_eq!(identifier, "@org/project");
                assert_eq!(*kind, RegistryErrorKind::NotFound);
                assert!(
                    details.suggestion.is_some(),
                    "NotFound errors should include a suggestion"
                );
            }
            _ => unreachable!(),
        }

        let error_message = errs
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            error_message.contains("@org/project"),
            "Error should mention the identifier: {}",
            error_message
        );
    }

    #[tokio::test]
    async fn resolve_returns_all_registry_errors_when_multiple_repositorys_fail() {
        let local_source = r#"spec main_spec
uses @org/example helper
uses @iso/countries alpha2
data country: alpha2.code"#;
        let local_specs = crate::parse(
            local_source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut engine = Engine::new();
        let store = engine.specs_mut();
        let local_repository = store.workspace();
        for spec in local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            local_source.to_string(),
        );

        let registry = TestRegistry::new(); // empty — no bundles

        let result =
            resolve_registry_references(store, &mut sources, &registry, &ResourceLimits::default())
                .await;

        assert!(result.is_err(), "Should fail when Registry cannot resolve");
        let errors = result.unwrap_err();
        let identifiers: Vec<&str> = errors
            .iter()
            .filter_map(|e| {
                if let Error::Registry { identifier, .. } = e {
                    Some(identifier.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            identifiers.contains(&"@org/example"),
            "Should include repository error: {:?}",
            identifiers
        );
        assert!(
            identifiers.contains(&"@iso/countries"),
            "Should include data import repository error: {:?}",
            identifiers
        );
    }

    #[tokio::test]
    async fn resolve_does_not_request_same_repository_twice() {
        let local_source = r#"spec spec_one
uses a: @org/shared shared

spec spec_two
uses b: @org/shared shared"#;
        let local_specs = crate::parse(
            local_source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut engine = Engine::new();
        let store = engine.specs_mut();
        let local_repository = store.workspace();
        for spec in local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            local_source.to_string(),
        );

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/shared",
            r#"repo @org/shared
spec shared
data value: 1"#,
        );

        resolve_registry_references(store, &mut sources, &registry, &ResourceLimits::default())
            .await
            .unwrap();

        assert_eq!(store.len(), 4);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "shared"));
        assert!(names.iter().any(|n| n == "units"));
    }

    #[tokio::test]
    async fn resolve_handles_data_import_from_registry() {
        let local_source = r#"spec main_spec
uses @iso/countries alpha2
data country: alpha2.code
data home: country"#;
        let local_specs = crate::parse(
            local_source,
            crate::parsing::source::SourceType::Volatile,
            &ResourceLimits::default(),
        )
        .unwrap()
        .into_flattened_specs();
        let mut engine = Engine::new();
        let store = engine.specs_mut();
        let local_repository = store.workspace();
        for spec in local_specs {
            store
                .insert_spec(Arc::clone(&local_repository), Arc::new(spec))
                .unwrap();
        }
        let mut sources: HashMap<crate::parsing::source::SourceType, String> = HashMap::new();
        sources.insert(
            crate::parsing::source::SourceType::Volatile,
            local_source.to_string(),
        );

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@iso/countries",
            r#"repo @iso/countries
spec alpha2
data code: text
 -> option "NL""#,
        );

        resolve_registry_references(store, &mut sources, &registry, &ResourceLimits::default())
            .await
            .unwrap();

        assert_eq!(store.len(), 3);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "main_spec"));
        assert!(names.iter().any(|n| n == "alpha2"));
        assert!(names.iter().any(|n| n == "units"));
    }

    // -----------------------------------------------------------------------
    // LemmaBase tests (feature-gated)
    // -----------------------------------------------------------------------

    #[cfg(feature = "registry")]
    mod lemmabase_tests {
        use super::super::*;
        use crate::literals::DateGranularity;
        use std::sync::{Arc, Mutex};

        // -------------------------------------------------------------------
        // MockHttpFetcher — drives LemmaBase without touching the network
        // -------------------------------------------------------------------

        type HttpFetchHandler = Box<dyn Fn(&str) -> Result<String, HttpFetchError> + Send + Sync>;

        struct MockHttpFetcher {
            handler: HttpFetchHandler,
        }

        impl MockHttpFetcher {
            /// Create a mock that delegates every `.get(url)` call to `handler`.
            fn with_handler(
                handler: impl Fn(&str) -> Result<String, HttpFetchError> + Send + Sync + 'static,
            ) -> Self {
                Self {
                    handler: Box::new(handler),
                }
            }

            /// Create a mock that always returns the given body for every URL.
            fn always_returning(body: &str) -> Self {
                let body = body.to_string();
                Self::with_handler(move |_| Ok(body.clone()))
            }

            /// Create a mock that always fails with the given HTTP status code.
            fn always_failing_with_status(code: u16) -> Self {
                Self::with_handler(move |_| {
                    Err(HttpFetchError {
                        status_code: Some(code),
                        message: format!("HTTP {}", code),
                    })
                })
            }

            /// Create a mock that always fails with a transport / network error.
            fn always_failing_with_network_error(msg: &str) -> Self {
                let msg = msg.to_string();
                Self::with_handler(move |_| {
                    Err(HttpFetchError {
                        status_code: None,
                        message: msg.clone(),
                    })
                })
            }
        }

        #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
        #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
        impl HttpFetcher for MockHttpFetcher {
            async fn get(&self, url: &str) -> Result<String, HttpFetchError> {
                (self.handler)(url)
            }
        }

        // -------------------------------------------------------------------
        // URL construction tests
        // -------------------------------------------------------------------

        #[test]
        fn source_url_without_effective() {
            let registry = LemmaBase::new();
            let url = registry.source_url("@user/workspace/somespec", None);
            assert_eq!(
                url,
                format!("{}/@user/workspace/somespec.lemma", LemmaBase::BASE_URL)
            );
        }

        #[test]
        fn source_url_with_effective() {
            let registry = LemmaBase::new();
            let effective = DateTimeValue {
                year: 2026,
                month: 1,
                day: 15,
                hour: 0,
                minute: 0,
                second: 0,
                microsecond: 0,
                timezone: None,

                granularity: DateGranularity::Full,
            };
            let url = registry.source_url("@user/workspace/somespec", Some(&effective));
            assert_eq!(
                url,
                format!(
                    "{}/@user/workspace/somespec.lemma?effective=2026-01-15",
                    LemmaBase::BASE_URL
                )
            );
        }

        #[test]
        fn source_url_for_deeply_nested_identifier() {
            let registry = LemmaBase::new();
            let url = registry.source_url("@org/team/project/subdir/spec", None);
            assert_eq!(
                url,
                format!(
                    "{}/@org/team/project/subdir/spec.lemma",
                    LemmaBase::BASE_URL
                )
            );
        }

        #[test]
        fn navigation_url_without_effective() {
            let registry = LemmaBase::new();
            let url = registry.navigation_url("@user/workspace/somespec", None);
            assert_eq!(
                url,
                format!("{}/@user/workspace/somespec", LemmaBase::BASE_URL)
            );
        }

        #[test]
        fn navigation_url_with_effective() {
            let registry = LemmaBase::new();
            let effective = DateTimeValue {
                year: 2026,
                month: 1,
                day: 15,
                hour: 0,
                minute: 0,
                second: 0,
                microsecond: 0,
                timezone: None,

                granularity: DateGranularity::Full,
            };
            let url = registry.navigation_url("@user/workspace/somespec", Some(&effective));
            assert_eq!(
                url,
                format!(
                    "{}/@user/workspace/somespec?effective=2026-01-15",
                    LemmaBase::BASE_URL
                )
            );
        }

        #[test]
        fn url_for_id_returns_navigation_url() {
            let registry = LemmaBase::new();
            let url = registry.url_for_id("@user/workspace/somespec", None);
            assert_eq!(
                url,
                Some(format!("{}/@user/workspace/somespec", LemmaBase::BASE_URL))
            );
        }

        #[test]
        fn url_for_id_with_effective() {
            let registry = LemmaBase::new();
            let effective = DateTimeValue {
                year: 2026,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                microsecond: 0,
                timezone: None,

                granularity: DateGranularity::Full,
            };
            let url = registry.url_for_id("@owner/repo/spec", Some(&effective));
            assert_eq!(
                url,
                Some(format!(
                    "{}/@owner/repo/spec?effective=2026-01-01",
                    LemmaBase::BASE_URL
                ))
            );
        }

        #[test]
        fn url_for_id_returns_navigation_url_for_nested_path() {
            let registry = LemmaBase::new();
            let url = registry.url_for_id("@iso/countries/alpha2", None);
            assert_eq!(
                url,
                Some(format!("{}/@iso/countries/alpha2", LemmaBase::BASE_URL))
            );
        }

        // -------------------------------------------------------------------
        // fetch_source tests (mock-based, no real HTTP calls)
        // -------------------------------------------------------------------

        #[tokio::test]
        async fn test_mode_serves_bundled_fixtures() {
            let registry = LemmaBase::test();
            let iso = registry.get("@iso/countries").await.unwrap();
            assert!(iso.lemma_source.contains("spec alpha2"));
        }

        #[tokio::test]
        async fn fetch_source_returns_bundle_on_success() {
            let registry = LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_returning(
                "spec org/my_spec\ndata x: 1",
            )));

            let bundle = registry.fetch_source("@org/my_spec").await.unwrap();

            assert_eq!(bundle.lemma_source, "spec org/my_spec\ndata x: 1");
            assert_eq!(bundle.source_type.to_string(), "@org/my_spec");
        }

        #[tokio::test]
        async fn fetch_source_passes_correct_url_to_fetcher() {
            let captured_url = Arc::new(Mutex::new(String::new()));
            let captured = captured_url.clone();
            let mock = MockHttpFetcher::with_handler(move |url| {
                *captured.lock().unwrap() = url.to_string();
                Ok("spec test/spec\ndata x: 1".to_string())
            });
            let registry = LemmaBase::with_fetcher(Box::new(mock));

            let _ = registry.fetch_source("@user/workspace/somespec").await;

            assert_eq!(
                *captured_url.lock().unwrap(),
                format!("{}/@user/workspace/somespec.lemma", LemmaBase::BASE_URL)
            );
        }

        #[tokio::test]
        async fn fetch_source_maps_http_404_to_not_found() {
            let registry =
                LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_failing_with_status(404)));

            let err = registry.fetch_source("@org/missing").await.unwrap_err();

            assert_eq!(err.kind, RegistryErrorKind::NotFound);
            assert!(
                err.message.contains("HTTP 404"),
                "Expected 'HTTP 404' in: {}",
                err.message
            );
            assert!(
                err.message.contains("@org/missing"),
                "Expected '@org/missing' in: {}",
                err.message
            );
        }

        #[tokio::test]
        async fn fetch_source_maps_http_500_to_server_error() {
            let registry =
                LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_failing_with_status(500)));

            let err = registry.fetch_source("@org/broken").await.unwrap_err();

            assert_eq!(err.kind, RegistryErrorKind::ServerError);
            assert!(
                err.message.contains("HTTP 500"),
                "Expected 'HTTP 500' in: {}",
                err.message
            );
        }

        #[tokio::test]
        async fn fetch_source_maps_http_401_to_unauthorized() {
            let registry =
                LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_failing_with_status(401)));

            let err = registry.fetch_source("@org/secret").await.unwrap_err();

            assert_eq!(err.kind, RegistryErrorKind::Unauthorized);
            assert!(err.message.contains("HTTP 401"));
        }

        #[tokio::test]
        async fn fetch_source_maps_http_403_to_unauthorized() {
            let registry =
                LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_failing_with_status(403)));

            let err = registry.fetch_source("@org/private").await.unwrap_err();

            assert_eq!(err.kind, RegistryErrorKind::Unauthorized);
            assert!(
                err.message.contains("HTTP 403"),
                "Expected 'HTTP 403' in: {}",
                err.message
            );
        }

        #[tokio::test]
        async fn fetch_source_maps_unexpected_status_to_other() {
            let registry =
                LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_failing_with_status(418)));

            let err = registry.fetch_source("@org/teapot").await.unwrap_err();

            assert_eq!(err.kind, RegistryErrorKind::Other);
            assert!(err.message.contains("HTTP 418"));
        }

        #[tokio::test]
        async fn fetch_source_maps_network_error_to_network_error_kind() {
            let registry = LemmaBase::with_fetcher(Box::new(
                MockHttpFetcher::always_failing_with_network_error("connection refused"),
            ));

            let err = registry.fetch_source("@org/unreachable").await.unwrap_err();

            assert_eq!(err.kind, RegistryErrorKind::NetworkError);
            assert!(
                err.message.contains("connection refused"),
                "Expected 'connection refused' in: {}",
                err.message
            );
            assert!(
                err.message.contains("@org/unreachable"),
                "Expected '@org/unreachable' in: {}",
                err.message
            );
        }

        #[tokio::test]
        async fn fetch_source_maps_dns_error_to_network_error_kind() {
            let registry = LemmaBase::with_fetcher(Box::new(
                MockHttpFetcher::always_failing_with_network_error(
                    "dns error: failed to lookup address",
                ),
            ));

            let err = registry.fetch_source("@org/spec").await.unwrap_err();

            assert_eq!(err.kind, RegistryErrorKind::NetworkError);
            assert!(
                err.message.contains("dns error"),
                "Expected 'dns error' in: {}",
                err.message
            );
            assert!(
                err.message.contains("Failed to reach LemmaBase"),
                "Expected 'Failed to reach LemmaBase' in: {}",
                err.message
            );
        }

        // -------------------------------------------------------------------
        // Registry trait delegation tests (mock-based)
        // -------------------------------------------------------------------

        #[tokio::test]
        async fn get_delegates_to_fetch_source() {
            let registry = LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_returning(
                "spec org/resolved\ndata a: 1",
            )));

            let bundle = registry.get("@org/resolved").await.unwrap();

            assert_eq!(bundle.lemma_source, "spec org/resolved\ndata a: 1");
            assert_eq!(bundle.source_type.to_string(), "@org/resolved");
        }

        #[tokio::test]
        async fn fetch_source_returns_empty_body_as_valid_bundle() {
            let registry = LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_returning("")));

            let bundle = registry.fetch_source("@org/empty").await.unwrap();

            assert_eq!(bundle.lemma_source, "");
            assert_eq!(bundle.source_type.to_string(), "@org/empty");
        }
    }
}
