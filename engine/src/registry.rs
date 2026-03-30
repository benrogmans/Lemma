//! Registry trait, types, and resolution logic for external `@...` references.
//!
//! A Registry maps `@`-prefixed identifiers to Lemma source text (for resolution)
//! and to human-facing addresses (for editor navigation).
//!
//! The engine calls `resolve_spec` and `resolve_type` during the resolution step
//! (after parsing local files, before planning) to fetch external specs.
//! The Language Server calls `url_for_id` to produce clickable links.
//!
//! Input to all methods is the identifier **without** the leading `@`
//! (for example `"user/workspace/somespec"` for `spec @user/workspace/somespec`).

use crate::engine::Context;
use crate::error::Error;
use crate::limits::ResourceLimits;
use crate::parsing::ast::{DateTimeValue, FactValue, TypeDef};
use crate::parsing::source::Source;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

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
    /// (for example `"@user/workspace/somespec"`).
    pub attribute: String,
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

/// Trait for resolving external `@...` references.
///
/// Implementations must be `Send + Sync` so they can be shared across threads.
/// Resolution is async so that WASM can use `fetch()` and native can use async HTTP.
///
/// `get` returns a bundle containing ALL temporal versions for the requested
/// identifier. The engine handles temporal resolution locally using
/// `effective_from` on the parsed specs. Both `spec @...` references and
/// `type ... from @...` imports are resolved through the same `get` method.
///
/// `name` is the base identifier **without** the leading `@`.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait Registry: Send + Sync {
    /// Fetch all temporal versions for an `@...` identifier.
    ///
    /// `name` is the base name (e.g. `"user/workspace/somespec"`).
    /// Returns a bundle whose `lemma_source` contains all temporal versions.
    async fn get(&self, name: &str) -> Result<RegistryBundle, RegistryError>;

    /// Map a Registry identifier to a human-facing address for navigation.
    ///
    /// `name` is the base name after `@`. `effective` is an optional datetime for
    /// linking directly to a specific temporal version in the registry UI.
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

/// The LemmaBase registry fetches Lemma source text from LemmaBase.com.
///
/// This is the default registry for the Lemma engine. It resolves `@...` identifiers
/// by making HTTP GET requests to `https://lemmabase.com/@{identifier}.lemma`.
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
    /// The base URL for the LemmaBase.com registry.
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

    /// Create a LemmaBase registry with a custom HTTP fetcher (for testing).
    #[cfg(test)]
    fn with_fetcher(fetcher: Box<dyn HttpFetcher>) -> Self {
        Self { fetcher }
    }

    /// Base URL for the spec; when effective is set, appends ?effective=... for temporal resolution.
    /// `name` includes the leading `@` (e.g. `@org/repo/spec`).
    fn source_url(&self, name: &str, effective: Option<&DateTimeValue>) -> String {
        let base = format!("{}/{}.lemma", Self::BASE_URL, name);
        match effective {
            None => base,
            Some(d) => format!("{}?effective={}", base, d),
        }
    }

    /// Human-facing URL for navigation; when effective is set, appends ?effective=... for linking to a specific temporal version.
    /// `name` includes the leading `@` (e.g. `@org/repo/spec`).
    fn navigation_url(&self, name: &str, effective: Option<&DateTimeValue>) -> String {
        let base = format!("{}/{}", Self::BASE_URL, name);
        match effective {
            None => base,
            Some(d) => format!("{}?effective={}", base, d),
        }
    }

    /// Format a display identifier for error messages, e.g. `"@owner/repo/spec"` or `"@owner/repo/spec 2026-01-01"`.
    /// `name` includes the leading `@`.
    fn display_id(name: &str, effective: Option<&DateTimeValue>) -> String {
        match effective {
            None => name.to_string(),
            Some(d) => format!("{} {}", name, d),
        }
    }

    /// Fetch all zones for the given identifier (no temporal filtering).
    async fn fetch_source(&self, name: &str) -> Result<RegistryBundle, RegistryError> {
        let url = self.source_url(name, None);
        let display = Self::display_id(name, None);
        let source_url = self.source_url(name, None);

        let lemma_source = self.fetcher.get(&url).await.map_err(|error| {
            if let Some(code) = error.status_code {
                let kind = match code {
                    404 => RegistryErrorKind::NotFound,
                    401 | 403 => RegistryErrorKind::Unauthorized,
                    500..=599 => RegistryErrorKind::ServerError,
                    _ => RegistryErrorKind::Other,
                };
                RegistryError {
                    message: format!(
                        "LemmaBase returned HTTP {} {} for '{}'",
                        code, source_url, display
                    ),
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
            attribute: display,
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

/// Resolve all external `@...` references in the given spec set.
///
/// Starting from the already-parsed local specs, this function:
/// 1. Collects all `@...` identifiers referenced by the specs.
/// 2. For each identifier not already present as a spec name, calls the Registry.
/// 3. Parses the returned source text into additional Lemma specs.
/// 4. Recurses: checks the newly added specs for further `@...` references.
/// 5. Repeats until no unresolved references remain.
///
/// Fetches unresolved `@...` references from the registry and inserts resulting specs into `ctx`.
/// Updates `sources` with Registry-returned source texts.
///
/// Errors are fatal: if the Registry returns an error, or if a `@...` reference
/// cannot be resolved after calling the Registry, this function returns a `Error`.
pub async fn resolve_registry_references(
    ctx: &mut Context,
    sources: &mut HashMap<String, String>,
    registry: &dyn Registry,
    limits: &ResourceLimits,
) -> Result<(), Vec<Error>> {
    let mut already_requested: HashSet<String> = HashSet::new();

    loop {
        let unresolved = collect_unresolved_registry_references(ctx, &already_requested);

        if unresolved.is_empty() {
            break;
        }

        let mut round_errors: Vec<Error> = Vec::new();
        for reference in &unresolved {
            if already_requested.contains(&reference.name) {
                continue;
            }
            already_requested.insert(reference.name.clone());

            let bundle_result = registry.get(&reference.name).await;

            let bundle = match bundle_result {
                Ok(b) => b,
                Err(registry_error) => {
                    let suggestion = match &registry_error.kind {
                        RegistryErrorKind::NotFound => Some(
                            "Check that the identifier is spelled correctly and that the spec exists on the registry.".to_string(),
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
                    let spec_context = ctx.iter().find(|s| {
                        s.attribute.as_deref() == Some(reference.source.attribute.as_str())
                    });
                    round_errors.push(Error::registry(
                        registry_error.message,
                        reference.source.clone(),
                        &reference.name,
                        registry_error.kind,
                        suggestion,
                        spec_context,
                        None,
                    ));
                    continue;
                }
            };

            sources.insert(bundle.attribute.clone(), bundle.lemma_source.clone());

            let new_specs =
                match crate::parsing::parse(&bundle.lemma_source, &bundle.attribute, limits) {
                    Ok(result) => result.specs,
                    Err(e) => {
                        round_errors.push(e);
                        return Err(round_errors);
                    }
                };

            for spec in new_specs {
                let bare_refs = crate::planning::validation::collect_bare_registry_refs(&spec);
                if !bare_refs.is_empty() {
                    round_errors.push(Error::validation_with_context(
                        format!(
                            "Registry spec '{}' contains references without '@' prefix: {}. \
                             The registry must rewrite all references to use '@'-prefixed names",
                            spec.name,
                            bare_refs.join(", ")
                        ),
                        None,
                        Some(
                            "The registry must prefix all spec references with '@' \
                             before serving the bundle.",
                        ),
                        Some(std::sync::Arc::new(spec.clone())),
                        None,
                    ));
                    continue;
                }
                if let Err(e) = ctx.insert_spec(Arc::new(spec), true) {
                    round_errors.push(e);
                }
            }
        }

        if !round_errors.is_empty() {
            return Err(round_errors);
        }
    }

    Ok(())
}

/// A collected `@...` reference needing registry fetch.
#[derive(Debug, Clone)]
struct RegistryReference {
    name: String,
    source: Source,
}

/// Collect all unresolved `@...` references from specs in `ctx`.
/// Collects from both fact-level spec refs and type imports into a single flat set.
fn collect_unresolved_registry_references(
    ctx: &Context,
    already_requested: &HashSet<String>,
) -> Vec<RegistryReference> {
    let mut unresolved: Vec<RegistryReference> = Vec::new();
    let mut seen_in_this_round: HashSet<String> = HashSet::new();

    for spec in ctx.iter() {
        let spec = spec.as_ref();
        if spec.attribute.is_none() {
            let has_registry_refs = spec.facts.iter().any(|f| match &f.value {
                FactValue::SpecReference(ref r) => r.from_registry,
                FactValue::TypeDeclaration {
                    from: Some(ref r), ..
                } => r.from_registry,
                _ => false,
            }) || spec.types.iter().any(|t| match t {
                TypeDef::Import { from, .. } => from.from_registry,
                TypeDef::Inline {
                    from: Some(ref r), ..
                } => r.from_registry,
                _ => false,
            });
            if has_registry_refs {
                panic!(
                    "BUG: spec '{}' must have source attribute when it has registry references",
                    spec.name
                );
            }
            continue;
        }

        let mut try_collect = |name: &str, source: &Source| {
            let already_satisfied = ctx.get_spec_effective_from(name, None).is_some();
            if !already_satisfied
                && !already_requested.contains(name)
                && seen_in_this_round.insert(name.to_string())
            {
                unresolved.push(RegistryReference {
                    name: name.to_string(),
                    source: source.clone(),
                });
            }
        };

        for fact in &spec.facts {
            match &fact.value {
                FactValue::SpecReference(spec_ref) if spec_ref.from_registry => {
                    try_collect(&spec_ref.name, &fact.source_location);
                }
                FactValue::TypeDeclaration {
                    from: Some(from_ref),
                    ..
                } if from_ref.from_registry => {
                    try_collect(&from_ref.name, &fact.source_location);
                }
                _ => {}
            }
        }

        for type_def in &spec.types {
            match type_def {
                TypeDef::Import {
                    from,
                    source_location,
                    ..
                } if from.from_registry => {
                    try_collect(&from.name, source_location);
                }
                TypeDef::Inline {
                    from: Some(from_ref),
                    source_location,
                    ..
                } if from_ref.from_registry => {
                    try_collect(&from_ref.name, source_location);
                }
                _ => {}
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

        /// Add a bundle containing all zones for this identifier (including `@` prefix).
        fn add_spec_bundle(&mut self, identifier: &str, lemma_source: &str) {
            self.bundles.insert(
                identifier.to_string(),
                RegistryBundle {
                    lemma_source: lemma_source.to_string(),
                    attribute: identifier.to_string(),
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
fact price: 100"#;
        let local_specs = crate::parse(source, "local.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let mut store = Context::new();
        for spec in &local_specs {
            store.insert_spec(Arc::new(spec.clone()), false).unwrap();
        }
        let mut sources = HashMap::new();
        sources.insert("local.lemma".to_string(), source.to_string());

        let registry = TestRegistry::new();
        resolve_registry_references(
            &mut store,
            &mut sources,
            &registry,
            &ResourceLimits::default(),
        )
        .await
        .unwrap();

        assert_eq!(store.len(), 1);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert_eq!(names, ["example"]);
    }

    #[tokio::test]
    async fn resolve_fetches_single_spec_from_registry() {
        let local_source = r#"spec main_spec
fact external: spec @org/project/helper
rule value: external.quantity"#;
        let local_specs = crate::parse(local_source, "local.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let mut store = Context::new();
        for spec in local_specs {
            store.insert_spec(Arc::new(spec), false).unwrap();
        }
        let mut sources = HashMap::new();
        sources.insert("local.lemma".to_string(), local_source.to_string());

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/project/helper",
            r#"spec @org/project/helper
fact quantity: 42"#,
        );

        resolve_registry_references(
            &mut store,
            &mut sources,
            &registry,
            &ResourceLimits::default(),
        )
        .await
        .unwrap();

        assert_eq!(store.len(), 2);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "main_spec"));
        assert!(names.iter().any(|n| n == "@org/project/helper"));
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
        };
        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "org/spec",
            "spec org/spec 2025-01-01\nfact x: 1\n\nspec org/spec 2026-01-15\nfact x: 2",
        );

        let bundle = registry.get("org/spec").await.unwrap();
        assert!(bundle.lemma_source.contains("fact x: 1"));
        assert!(bundle.lemma_source.contains("fact x: 2"));

        assert_eq!(
            registry.url_for_id("org/spec", None),
            Some("https://test.registry/org/spec".to_string())
        );
        assert_eq!(
            registry.url_for_id("org/spec", Some(&effective)),
            Some("https://test.registry/org/spec?effective=2026-01-15".to_string())
        );
    }

    #[tokio::test]
    async fn resolve_fetches_transitive_dependencies() {
        let local_source = r#"spec main_spec
fact a: spec @org/project/spec_a"#;
        let local_specs = crate::parse(local_source, "local.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let mut store = Context::new();
        for spec in local_specs {
            store.insert_spec(Arc::new(spec), false).unwrap();
        }
        let mut sources = HashMap::new();
        sources.insert("local.lemma".to_string(), local_source.to_string());

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/project/spec_a",
            r#"spec @org/project/spec_a
fact b: spec @org/project/spec_b"#,
        );
        registry.add_spec_bundle(
            "@org/project/spec_b",
            r#"spec @org/project/spec_b
fact value: 99"#,
        );

        resolve_registry_references(
            &mut store,
            &mut sources,
            &registry,
            &ResourceLimits::default(),
        )
        .await
        .unwrap();

        assert_eq!(store.len(), 3);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "main_spec"));
        assert!(names.iter().any(|n| n == "@org/project/spec_a"));
        assert!(names.iter().any(|n| n == "@org/project/spec_b"));
    }

    #[tokio::test]
    async fn resolve_handles_bundle_with_multiple_specs() {
        let local_source = r#"spec main_spec
fact a: spec @org/project/spec_a"#;
        let local_specs = crate::parse(local_source, "local.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let mut store = Context::new();
        for spec in local_specs {
            store.insert_spec(Arc::new(spec), false).unwrap();
        }
        let mut sources = HashMap::new();
        sources.insert("local.lemma".to_string(), local_source.to_string());

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/project/spec_a",
            r#"spec @org/project/spec_a
fact b: spec @org/project/spec_b

spec @org/project/spec_b
fact value: 99"#,
        );

        resolve_registry_references(
            &mut store,
            &mut sources,
            &registry,
            &ResourceLimits::default(),
        )
        .await
        .unwrap();

        assert_eq!(store.len(), 3);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "main_spec"));
        assert!(names.iter().any(|n| n == "@org/project/spec_a"));
        assert!(names.iter().any(|n| n == "@org/project/spec_b"));
    }

    #[tokio::test]
    async fn resolve_returns_registry_error_when_registry_fails() {
        let local_source = r#"spec main_spec
fact external: spec @org/project/missing"#;
        let local_specs = crate::parse(local_source, "local.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let mut store = Context::new();
        for spec in local_specs {
            store.insert_spec(Arc::new(spec), false).unwrap();
        }
        let mut sources = HashMap::new();
        sources.insert("local.lemma".to_string(), local_source.to_string());

        let registry = TestRegistry::new(); // empty — no bundles

        let result = resolve_registry_references(
            &mut store,
            &mut sources,
            &registry,
            &ResourceLimits::default(),
        )
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
                assert_eq!(identifier, "@org/project/missing");
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
            error_message.contains("org/project/missing"),
            "Error should mention the identifier: {}",
            error_message
        );
    }

    #[tokio::test]
    async fn resolve_returns_all_registry_errors_when_multiple_refs_fail() {
        let local_source = r#"spec main_spec
fact helper: spec @org/example/helper
type money from @lemma/std/finance"#;
        let local_specs = crate::parse(local_source, "local.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let mut store = Context::new();
        for spec in local_specs {
            store.insert_spec(Arc::new(spec), false).unwrap();
        }
        let mut sources = HashMap::new();
        sources.insert("local.lemma".to_string(), local_source.to_string());

        let registry = TestRegistry::new(); // empty — no bundles

        let result = resolve_registry_references(
            &mut store,
            &mut sources,
            &registry,
            &ResourceLimits::default(),
        )
        .await;

        assert!(result.is_err(), "Should fail when Registry cannot resolve");
        let errors = result.unwrap_err();
        assert_eq!(
            errors.len(),
            2,
            "Both spec ref and type import ref should produce a Registry error"
        );
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
            identifiers.contains(&"@org/example/helper"),
            "Should include spec ref error: {:?}",
            identifiers
        );
        assert!(
            identifiers.contains(&"@lemma/std/finance"),
            "Should include type import error: {:?}",
            identifiers
        );
    }

    #[tokio::test]
    async fn resolve_does_not_request_same_identifier_twice() {
        let local_source = r#"spec spec_one
fact a: spec @org/shared

spec spec_two
fact b: spec @org/shared"#;
        let local_specs = crate::parse(local_source, "local.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let mut store = Context::new();
        for spec in local_specs {
            store.insert_spec(Arc::new(spec), false).unwrap();
        }
        let mut sources = HashMap::new();
        sources.insert("local.lemma".to_string(), local_source.to_string());

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@org/shared",
            r#"spec @org/shared
fact value: 1"#,
        );

        resolve_registry_references(
            &mut store,
            &mut sources,
            &registry,
            &ResourceLimits::default(),
        )
        .await
        .unwrap();

        // Should have spec_one, spec_two, and @org/shared (fetched only once).
        assert_eq!(store.len(), 3);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "@org/shared"));
    }

    #[tokio::test]
    async fn resolve_handles_type_import_from_registry() {
        let local_source = r#"spec main_spec
type money from @lemma/std/finance
fact price: [money]"#;
        let local_specs = crate::parse(local_source, "local.lemma", &ResourceLimits::default())
            .unwrap()
            .specs;
        let mut store = Context::new();
        for spec in local_specs {
            store.insert_spec(Arc::new(spec), false).unwrap();
        }
        let mut sources = HashMap::new();
        sources.insert("local.lemma".to_string(), local_source.to_string());

        let mut registry = TestRegistry::new();
        registry.add_spec_bundle(
            "@lemma/std/finance",
            r#"spec @lemma/std/finance
type money: scale
 -> unit eur 1.00
 -> unit usd 1.10
 -> decimals 2"#,
        );

        resolve_registry_references(
            &mut store,
            &mut sources,
            &registry,
            &ResourceLimits::default(),
        )
        .await
        .unwrap();

        assert_eq!(store.len(), 2);
        let names: Vec<String> = store.iter().map(|a| a.name.clone()).collect();
        assert!(names.iter().any(|n| n == "main_spec"));
        assert!(names.iter().any(|n| n == "@lemma/std/finance"));
    }

    // -----------------------------------------------------------------------
    // LemmaBase tests (feature-gated)
    // -----------------------------------------------------------------------

    #[cfg(feature = "registry")]
    mod lemmabase_tests {
        use super::super::*;
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
        fn navigation_url_for_deeply_nested_identifier() {
            let registry = LemmaBase::new();
            let url = registry.navigation_url("@org/team/project/subdir/spec", None);
            assert_eq!(
                url,
                format!("{}/@org/team/project/subdir/spec", LemmaBase::BASE_URL)
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
            let url = registry.url_for_id("@lemma/std/finance", None);
            assert_eq!(
                url,
                Some(format!("{}/@lemma/std/finance", LemmaBase::BASE_URL))
            );
        }

        #[test]
        fn default_trait_creates_same_instance_as_new() {
            let from_new = LemmaBase::new();
            let from_default = LemmaBase::default();
            assert_eq!(
                from_new.url_for_id("test/spec", None),
                from_default.url_for_id("test/spec", None)
            );
        }

        // -------------------------------------------------------------------
        // fetch_source tests (mock-based, no real HTTP calls)
        // -------------------------------------------------------------------

        #[tokio::test]
        async fn fetch_source_returns_bundle_on_success() {
            let registry = LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_returning(
                "spec org/my_spec\nfact x: 1",
            )));

            let bundle = registry.fetch_source("@org/my_spec").await.unwrap();

            assert_eq!(bundle.lemma_source, "spec org/my_spec\nfact x: 1");
            assert_eq!(bundle.attribute, "@org/my_spec");
        }

        #[tokio::test]
        async fn fetch_source_passes_correct_url_to_fetcher() {
            let captured_url = Arc::new(Mutex::new(String::new()));
            let captured = captured_url.clone();
            let mock = MockHttpFetcher::with_handler(move |url| {
                *captured.lock().unwrap() = url.to_string();
                Ok("spec test/spec\nfact x: 1".to_string())
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
        async fn fetch_source_maps_http_502_to_server_error() {
            let registry =
                LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_failing_with_status(502)));

            let err = registry.fetch_source("@org/broken").await.unwrap_err();

            assert_eq!(err.kind, RegistryErrorKind::ServerError);
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
                "spec org/resolved\nfact a: 1",
            )));

            let bundle = registry.get("@org/resolved").await.unwrap();

            assert_eq!(bundle.lemma_source, "spec org/resolved\nfact a: 1");
            assert_eq!(bundle.attribute, "@org/resolved");
        }

        #[tokio::test]
        async fn get_propagates_http_error() {
            let registry =
                LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_failing_with_status(404)));

            let err = registry.get("@org/missing").await.unwrap_err();

            assert!(err.message.contains("HTTP 404"));
        }

        #[tokio::test]
        async fn get_propagates_network_error() {
            let registry = LemmaBase::with_fetcher(Box::new(
                MockHttpFetcher::always_failing_with_network_error("timeout"),
            ));

            let err = registry.get("@lemma/std/types").await.unwrap_err();

            assert!(err.message.contains("timeout"));
        }

        #[tokio::test]
        async fn fetch_source_returns_empty_body_as_valid_bundle() {
            let registry = LemmaBase::with_fetcher(Box::new(MockHttpFetcher::always_returning("")));

            let bundle = registry.fetch_source("@org/empty").await.unwrap();

            assert_eq!(bundle.lemma_source, "");
            assert_eq!(bundle.attribute, "@org/empty");
        }
    }
}
