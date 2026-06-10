use crate::evaluation::Evaluator;
use crate::parsing::ast::{DateTimeValue, LemmaRepository, LemmaSpec};
use crate::parsing::source::SourceType;
use crate::parsing::EffectiveDate;
use crate::planning::{DataOverlay, DataValueInput, LemmaSpecSet, SpecSchema};
use crate::{parse, Error, ResourceLimits, Response};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashSet;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// Load failure: errors plus the source texts we attempted to load.
#[derive(Debug, Clone)]
pub struct Errors {
    pub errors: Vec<Error>,
    pub sources: HashMap<SourceType, String>,
}

impl Errors {
    /// Iterate over the errors.
    pub fn iter(&self) -> std::slice::Iter<'_, Error> {
        self.errors.iter()
    }
}

/// Repository name reserved for the embedded standard library (`repo lemma`, `spec units`).
/// User [`Engine::load`] / [`Engine::load_batch`] must not target this name.
pub const EMBEDDED_STDLIB_REPOSITORY: &str = "lemma";

/// Collect `.lemma` source texts from filesystem paths (paths and one-level directories).
/// Does not touch an [`Engine`]; pair with [`Engine::load`] / [`Engine::load_batch`].
#[cfg(not(target_arch = "wasm32"))]
pub fn collect_lemma_sources<P: AsRef<Path>>(
    paths: &[P],
) -> Result<HashMap<SourceType, String>, Errors> {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    let mut sources = HashMap::new();
    let mut seen = HashSet::<PathBuf>::new();

    for path in paths {
        let path = path.as_ref();
        if path.is_file() {
            if path.extension().is_none_or(|e| e != "lemma") {
                continue;
            }
            let p = path.to_path_buf();
            if seen.contains(&p) {
                continue;
            }
            seen.insert(p.clone());
            let content = fs::read_to_string(path).map_err(|e| Errors {
                errors: vec![Error::request(
                    format!("Cannot read '{}': {}", path.display(), e),
                    None::<String>,
                )],
                sources: HashMap::new(),
            })?;
            sources.insert(SourceType::Path(Arc::new(p)), content);
        } else if path.is_dir() {
            let read_dir = fs::read_dir(path).map_err(|e| Errors {
                errors: vec![Error::request(
                    format!("Cannot read directory '{}': {}", path.display(), e),
                    None::<String>,
                )],
                sources: HashMap::new(),
            })?;
            for entry_result in read_dir {
                let entry = entry_result.map_err(|e| Errors {
                    errors: vec![Error::request(
                        format!("Cannot read directory entry in '{}': {}", path.display(), e),
                        None::<String>,
                    )],
                    sources: HashMap::new(),
                })?;
                let p = entry.path();
                if !p.is_file() || p.extension().is_none_or(|e| e != "lemma") {
                    continue;
                }
                if seen.contains(&p) {
                    continue;
                }
                seen.insert(p.clone());
                let content = fs::read_to_string(&p).map_err(|e| Errors {
                    errors: vec![Error::request(
                        format!("Cannot read '{}': {}", p.display(), e),
                        None::<String>,
                    )],
                    sources: HashMap::new(),
                })?;
                sources.insert(SourceType::Path(Arc::new(p)), content);
            }
        }
    }

    Ok(sources)
}

/// A loaded repository with all its spec sets.
///
/// Provenance: [`LemmaRepository::start_line`], [`LemmaRepository::source_type`], and each
/// temporal [`LemmaSpec`] in the spec sets carries [`LemmaSpec::start_line`] and
/// [`LemmaSpec::source_type`] from the parse of the `repo` / `spec` headers.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolvedRepository {
    pub repository: Arc<LemmaRepository>,
    pub specs: Vec<LemmaSpecSet>,
}

// ─── Spec store with temporal resolution ──────────────────────────────

/// Ordered store of specs keyed by `(repository, name)` and grouped into
/// [`LemmaSpecSet`]s.
///
/// Specs with the same `(repository, name)` identity are ordered by `effective_from`.
/// A spec version's temporal end is derived from the successor spec's `effective_from`, or
/// `+∞`. The repository identity is preserved as `Arc<LemmaRepository>` — never via string
/// prefixes on the spec name. Repository names include the `@` prefix when present
/// (e.g. `"@org/repo"`). Dependency isolation is enforced at `insert_spec`: all specs
/// in a repository must share the same `dependency` provenance ID.
#[derive(Debug)]
pub struct Context {
    repositories: IndexMap<Arc<LemmaRepository>, IndexMap<String, LemmaSpecSet>>,
    workspace: Arc<LemmaRepository>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    /// Empty workspace repository; specs are inserted via [`Self::insert_spec`].
    pub fn new() -> Self {
        let workspace = Arc::new(LemmaRepository::new(None));
        let mut repositories = IndexMap::new();
        repositories.insert(Arc::clone(&workspace), IndexMap::new());
        Self {
            repositories,
            workspace,
        }
    }

    /// Workspace-global grouping for every locally loaded spec. The single
    /// namespace runtime APIs operate on (entry-point specs live here).
    /// Stable identity across calls; `name = None`, `dependency = None`.
    #[must_use]
    pub fn workspace(&self) -> Arc<LemmaRepository> {
        Arc::clone(&self.workspace)
    }

    /// Look up a repository by name without creating a new one.
    #[must_use]
    pub fn find_repository(&self, name: &str) -> Option<Arc<LemmaRepository>> {
        let probe = Arc::new(LemmaRepository::new(Some(name.to_string())));
        self.repositories
            .get_key_value(&probe)
            .map(|(k, _)| Arc::clone(k))
    }

    /// All spec sets, keyed by `(repository, name)`. Iteration order: repository first
    /// (insertion order), then spec name ascending.
    #[must_use]
    pub fn repositories(&self) -> &IndexMap<Arc<LemmaRepository>, IndexMap<String, LemmaSpecSet>> {
        &self.repositories
    }

    pub fn iter(&self) -> impl Iterator<Item = Arc<LemmaSpec>> + '_ {
        self.repositories
            .values()
            .flat_map(|m| m.values())
            .flat_map(|ss| ss.iter_specs())
    }

    /// Every loaded spec paired with its half-open
    /// `[effective_from, effective_to)` validity range. Iteration order
    /// matches [`Self::iter`].
    pub fn iter_with_ranges(
        &self,
    ) -> impl Iterator<Item = (Arc<LemmaSpec>, Option<DateTimeValue>, Option<DateTimeValue>)> + '_
    {
        self.repositories
            .values()
            .flat_map(|m| m.values())
            .flat_map(|ss| ss.iter_with_ranges())
    }

    /// Look up a spec set by `(repository, name)`. Returns `None` if no such spec set
    /// is loaded.
    #[must_use]
    pub fn spec_set(&self, repository: &Arc<LemmaRepository>, name: &str) -> Option<&LemmaSpecSet> {
        let canonical_name = crate::parsing::ast::ascii_lowercase_logical_name(name.to_string());
        self.repositories
            .get(repository)
            .and_then(|m| m.get(&canonical_name))
    }

    /// All spec sets belonging to a repository. Panics if the repository is not in the map
    /// (caller must ensure it was returned by this Context).
    #[must_use]
    pub(crate) fn spec_sets_for(&self, repository: &Arc<LemmaRepository>) -> Vec<LemmaSpecSet> {
        self.repositories
            .get(repository)
            .expect("BUG: repository not in context")
            .values()
            .cloned()
            .collect()
    }

    /// Insert a spec under repository`. Validates that an identical
    /// `(repository, name, effective_from)` triple is not already loaded.
    /// Insert a spec under `repository`. Enforces two invariants:
    /// 1. Dependency isolation: all specs in a repo must share the same `dependency`
    ///    provenance. A workspace repo cannot be merged with a dependency repo, and
    ///    two different dependencies cannot contribute to the same repo name.
    /// 2. No duplicate `(repository, name, effective_from)` triples.
    pub fn insert_spec(
        &mut self,
        repository: Arc<LemmaRepository>,
        spec: Arc<LemmaSpec>,
    ) -> Result<(), Error> {
        if let Some((existing_repo, _)) = self.repositories.get_key_value(&repository) {
            if existing_repo.dependency != repository.dependency {
                let repo_display = repository.name.as_deref().unwrap_or("(main)");
                let existing_owner = match &existing_repo.dependency {
                    None => "the workspace".to_string(),
                    Some(id) => format!("dependency '{id}'"),
                };
                let new_owner = match &repository.dependency {
                    None => "the workspace".to_string(),
                    Some(id) => format!("dependency '{id}'"),
                };
                return Err(Error::validation_with_context(
                    format!(
                        "Repository '{repo_display}' was introduced by {existing_owner} but {new_owner} also declares it"
                    ),
                    None,
                    Some("Each dependency's repositories must be unique across all loaded sources"),
                    Some(Arc::clone(&spec)),
                    None,
                ));
            }
        }

        let entry = self
            .repositories
            .entry(Arc::clone(&repository))
            .or_default();
        if entry
            .get(&spec.name)
            .is_some_and(|ss| ss.get_exact(spec.effective_from()).is_some())
        {
            return Err(Error::validation_with_context(
                format!(
                    "Duplicate spec '{}' (same repository, name and effective_from already in context)",
                    spec.name
                ),
                None,
                None::<String>,
                Some(Arc::clone(&spec)),
                None,
            ));
        }

        let name = spec.name.clone();
        let inserted = entry
            .entry(name.clone())
            .or_insert_with(|| LemmaSpecSet::new(repository, name))
            .insert(spec);
        debug_assert!(inserted);
        Ok(())
    }

    pub fn remove_spec(
        &mut self,
        repository: &Arc<LemmaRepository>,
        spec: &Arc<LemmaSpec>,
    ) -> bool {
        let Some(inner) = self.repositories.get_mut(repository) else {
            return false;
        };
        let Some(ss) = inner.get_mut(&spec.name) else {
            return false;
        };
        if !ss.remove(spec.effective_from()) {
            return false;
        }
        if ss.is_empty() {
            inner.shift_remove(&spec.name);
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.repositories
            .values()
            .flat_map(|m| m.values())
            .map(LemmaSpecSet::len)
            .sum()
    }
}

// ─── Engine ──────────────────────────────────────────────────────────

/// Engine for evaluating Lemma rules.
///
/// Pure Rust implementation that evaluates Lemma specs directly from the AST.
/// Uses pre-built execution plans that are self-contained and ready for evaluation.
///
/// The engine never performs network calls. External `@...` references must be
/// pre-resolved before loading — either by including dependency sources
/// in the source map or by calling `resolve_registry_references` separately
/// (e.g. in a `lemma fetch` command).
pub struct Engine {
    /// Repository → spec name → resolved plans (ordered by `effective`; slice end from next plan).
    plan_sets: HashMap<
        Arc<crate::parsing::ast::LemmaRepository>,
        HashMap<String, crate::planning::ExecutionPlanSet>,
    >,
    specs: Context,
    evaluator: Evaluator,
    limits: ResourceLimits,
    total_expression_count: usize,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::with_limits(ResourceLimits::default())
    }

    pub fn with_limits(limits: ResourceLimits) -> Self {
        let mut engine = Self {
            plan_sets: HashMap::new(),
            specs: Context::new(),
            evaluator: Evaluator,
            limits,
            total_expression_count: 0,
        };
        engine
            .add_sources_inner(
                HashMap::from([(SourceType::Volatile, crate::stdlib::UNITS_LEMMA.to_string())]),
                Some("lemma"),
                true,
            )
            .expect("BUG: embedded stdlib must load");
        engine
    }

    pub fn specs(&self) -> &Context {
        &self.specs
    }

    pub fn specs_mut(&mut self) -> &mut Context {
        &mut self.specs
    }

    /// Resolve an optional effective datetime string for planning or evaluation.
    ///
    /// `None` or whitespace-only input resolves to [`DateTimeValue::now`].
    /// Non-empty invalid strings return a request [`Error`].
    pub fn resolve_effective(raw: Option<&str>) -> Result<DateTimeValue, Error> {
        match raw {
            Some(s) if !s.trim().is_empty() => s.trim().parse::<DateTimeValue>().map_err(|_| {
                Error::request(
                    format!(
                        "Invalid effective value '{}'. Expected: YYYY, YYYY-MM, YYYY-MM-DD, or ISO 8601 datetime",
                        s.trim()
                    ),
                    None::<String>,
                )
            }),
            _ => Ok(DateTimeValue::now()),
        }
    }

    fn apply_planning_result(&mut self, pr: crate::planning::PlanningResult) {
        self.plan_sets.clear();
        for r in &pr.results {
            self.plan_sets
                .entry(Arc::clone(&r.repository))
                .or_default()
                .insert(r.name.clone(), r.execution_plan_set());
        }
    }

    /// Load one Lemma source (workspace; not a tagged dependency).
    pub fn load(&mut self, code: impl Into<String>, source: SourceType) -> Result<(), Errors> {
        self.load_batch(HashMap::from([(source, code.into())]), None)
    }

    /// Load many sources in one planning pass. Pairs are `(source_text, source_id)`.
    ///
    /// `dependency`: when `Some`, repositories parsed from these sources are tagged with that id
    /// (same as previous `load(..., Some(id))` for path bundles).
    pub fn load_batch(
        &mut self,
        sources: HashMap<SourceType, String>,
        dependency: Option<&str>,
    ) -> Result<(), Errors> {
        self.add_sources_inner(sources, dependency, false)
    }

    fn validate_source_for_load(source: &SourceType) -> Result<(), Errors> {
        match source {
            SourceType::Path(p) if p.as_os_str().to_string_lossy().trim().is_empty() => {
                Err(Errors {
                    errors: vec![Error::request(
                        "Source path must be non-empty",
                        None::<String>,
                    )],
                    sources: HashMap::new(),
                })
            }
            SourceType::Registry(repo) => {
                if repo.name.as_deref().unwrap_or("").is_empty() {
                    Err(Errors {
                        errors: vec![Error::request(
                            "Registry source identifier must be non-empty",
                            None::<String>,
                        )],
                        sources: HashMap::new(),
                    })
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    fn reject_reserved_stdlib_repository(repository: &Arc<LemmaRepository>) -> Option<Error> {
        if repository.name.as_deref() == Some(EMBEDDED_STDLIB_REPOSITORY) {
            Some(Error::validation_with_context(
                format!(
                    "Repository '{EMBEDDED_STDLIB_REPOSITORY}' is reserved for the embedded standard library and cannot be loaded via load or load_batch; use '@lemma/std' for registry packages such as finance"
                ),
                None,
                Some("Load registry dependencies as '@lemma/std', not the reserved 'lemma' stdlib repository".to_string()),
                None,
                None,
            ))
        } else {
            None
        }
    }

    fn add_sources_inner(
        &mut self,
        sources: HashMap<SourceType, String>,
        dependency: Option<&str>,
        embedded_stdlib: bool,
    ) -> Result<(), Errors> {
        for st in sources.keys() {
            Self::validate_source_for_load(st)?;
        }
        if !embedded_stdlib {
            let limits = &self.limits;
            if sources.len() > limits.max_sources {
                return Err(Errors {
                    errors: vec![Error::resource_limit_exceeded(
                        "max_sources",
                        limits.max_sources.to_string(),
                        sources.len().to_string(),
                        "Reduce the number of paths or sources in one load",
                        None::<crate::parsing::source::Source>,
                        None,
                        None,
                    )],
                    sources,
                });
            }
            let total_loaded_bytes: usize = sources.values().map(|s| s.len()).sum();
            if total_loaded_bytes > limits.max_loaded_bytes {
                return Err(Errors {
                    errors: vec![Error::resource_limit_exceeded(
                        "max_loaded_bytes",
                        limits.max_loaded_bytes.to_string(),
                        total_loaded_bytes.to_string(),
                        "Load fewer or smaller sources",
                        None::<crate::parsing::source::Source>,
                        None,
                        None,
                    )],
                    sources,
                });
            }
            for code in sources.values() {
                if code.len() > limits.max_source_size_bytes {
                    return Err(Errors {
                        errors: vec![Error::resource_limit_exceeded(
                            "max_source_size_bytes",
                            limits.max_source_size_bytes.to_string(),
                            code.len().to_string(),
                            "Use a smaller source text or increase limit",
                            None::<crate::parsing::source::Source>,
                            None,
                            None,
                        )],
                        sources,
                    });
                }
            }
        }

        let parse_limits = if embedded_stdlib {
            &ResourceLimits::default()
        } else {
            &self.limits
        };
        let mut errors: Vec<Error> = Vec::new();

        for (source_id, code) in &sources {
            match parse(code, source_id.clone(), parse_limits) {
                Ok(result) => {
                    if !embedded_stdlib {
                        self.total_expression_count += result.expression_count;
                        if self.total_expression_count > self.limits.max_total_expression_count {
                            errors.push(Error::resource_limit_exceeded(
                                "max_total_expression_count",
                                self.limits.max_total_expression_count.to_string(),
                                self.total_expression_count.to_string(),
                                "Split logic across fewer sources or reduce expression complexity",
                                None::<crate::parsing::source::Source>,
                                None,
                                None,
                            ));
                            return Err(Errors { errors, sources });
                        }
                    }
                    if result.repositories.is_empty() {
                        continue;
                    }

                    for (parsed_repo, specs) in &result.repositories {
                        let repository_arc = if let Some(dep_id) = dependency {
                            let repo_name = parsed_repo
                                .name
                                .clone()
                                // Use the dependency id as the repository name for the dependency's workspace specs
                                .or_else(|| Some(dep_id.to_string()));
                            Arc::new(
                                LemmaRepository::new(repo_name)
                                    .with_dependency(dep_id)
                                    .with_start_line(parsed_repo.start_line),
                            )
                        } else {
                            Arc::clone(parsed_repo)
                        };
                        if !embedded_stdlib {
                            if let Some(reserved_err) =
                                Self::reject_reserved_stdlib_repository(&repository_arc)
                            {
                                let source = crate::parsing::source::Source::new(
                                    source_id.clone(),
                                    crate::parsing::ast::Span {
                                        start: 0,
                                        end: 0,
                                        line: parsed_repo.start_line,
                                        col: 0,
                                    },
                                );
                                errors.push(Error::validation(
                                    reserved_err.to_string(),
                                    Some(source),
                                    reserved_err.suggestion().map(str::to_string),
                                ));
                                continue;
                            }
                        }
                        for spec in specs {
                            match self
                                .specs
                                .insert_spec(Arc::clone(&repository_arc), Arc::new(spec.clone()))
                            {
                                Ok(()) => {}
                                Err(e) => {
                                    let source = crate::parsing::source::Source::new(
                                        source_id.clone(),
                                        crate::parsing::ast::Span {
                                            start: 0,
                                            end: 0,
                                            line: spec.start_line,
                                            col: 0,
                                        },
                                    );
                                    errors.push(Error::validation(
                                        e.to_string(),
                                        Some(source),
                                        None::<String>,
                                    ));
                                }
                            }
                        }
                    }
                }
                Err(e) => errors.push(e),
            }
        }

        let planning_result = crate::planning::plan(&self.specs, &self.limits);
        for set_result in &planning_result.results {
            for spec_result in &set_result.slice_results {
                let ctx = Arc::clone(&spec_result.spec);
                for err in &spec_result.errors {
                    errors.push(err.clone().with_spec_context(Arc::clone(&ctx)));
                }
            }
        }
        self.apply_planning_result(planning_result);

        if errors.is_empty() {
            Ok(())
        } else {
            Err(Errors { errors, sources })
        }
    }

    /// Active [`LemmaSpec`] slice for `name` at the resolved effective instant.
    ///
    /// When `effective` is `None`, uses the current time. The name must be unique
    /// across loaded repositories at that instant (same rule as [`Self::get_plan`] with
    /// `repo: None`). For repository scope or all temporal rows, use [`Self::get_workspace`]
    /// or [`Self::get_repository`].
    pub fn get_spec(
        &self,
        name: &str,
        effective: Option<&DateTimeValue>,
    ) -> Result<Arc<LemmaSpec>, Error> {
        let effective_dt = self.effective_or_now(effective);
        let instant = EffectiveDate::DateTimeValue(effective_dt.clone());
        let repository = self.specs.workspace();
        let spec_set = self
            .specs
            .spec_set(&repository, name)
            .ok_or_else(|| self.spec_not_found_error(name, &effective_dt))?;
        spec_set
            .spec_at(&instant)
            .ok_or_else(|| self.spec_not_found_error(name, &effective_dt))
    }

    /// Every loaded repository in insertion order (workspace, embedded stdlib [`EMBEDDED_STDLIB_REPOSITORY`], dependencies).
    ///
    /// Each [`ResolvedRepository::repository`] and every [`LemmaSpec`] under [`ResolvedRepository::specs`]
    /// includes source metadata (`start_line`, `source_type`) from load.
    /// Inspectable stdlib text: [`Self::format_repository`] with `"lemma"`.
    #[must_use]
    pub fn list(&self) -> Vec<ResolvedRepository> {
        self.specs
            .repositories()
            .iter()
            .map(|(repo, inner)| ResolvedRepository {
                repository: Arc::clone(repo),
                specs: inner.values().cloned().collect(),
            })
            .collect()
    }

    /// Workspace-local repository (`name == None`).
    #[must_use]
    pub fn get_workspace(&self) -> ResolvedRepository {
        let repo = self.specs.workspace();
        let specs = self.specs.spec_sets_for(&repo);
        ResolvedRepository {
            repository: repo,
            specs,
        }
    }

    /// Resolve a loaded repository by qualifier string. Matches against
    /// repository names (which include `@` when present).
    pub fn get_repository(&self, qualifier: &str) -> Result<ResolvedRepository, Error> {
        let q = qualifier.trim();
        if q.is_empty() {
            return Err(Error::request(
                "Repository qualifier cannot be empty",
                None::<String>,
            ));
        }
        match self.specs.find_repository(q) {
            Some(repo) => {
                let specs = self.specs.spec_sets_for(&repo);
                Ok(ResolvedRepository {
                    repository: repo,
                    specs,
                })
            }
            None => Err(Error::request_not_found(
                format!("Repository '{qualifier}' not loaded"),
                Some(format!(
                    "List repositories with `{}` after loading your workspace",
                    "lemma list"
                )),
            )),
        }
    }

    /// Canonical Lemma source for every spec in `repository`, formatted from the in-engine AST.
    pub fn format_repository(&self, repository: &str) -> Result<String, Error> {
        let resolved = self.get_repository(repository)?;
        let mut specs: Vec<Arc<LemmaSpec>> = resolved
            .specs
            .iter()
            .flat_map(|ss| ss.iter_specs())
            .collect();
        specs.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| a.effective_from.cmp(&b.effective_from))
        });
        let spec_refs: Vec<&LemmaSpec> = specs.iter().map(AsRef::as_ref).collect();
        let body = crate::formatting::format_spec_refs(&spec_refs);
        let mut out = String::new();
        if let Some(name) = resolved.repository.name.as_deref() {
            out.push_str("repo ");
            out.push_str(name);
            out.push_str("\n\n");
        }
        out.push_str(&body);
        Ok(out)
    }

    /// Planning schema for `name`. When `repo` is `None`, the spec must be
    /// unambiguous across all loaded repositories; when `Some`, scoped to that
    /// repository qualifier (e.g. `"@org/pkg"`).
    pub fn schema(
        &self,
        repo: Option<&str>,
        spec: &str,
        effective: Option<&DateTimeValue>,
    ) -> Result<SpecSchema, Error> {
        Ok(self
            .get_plan(repo, spec, effective)?
            .schema(&DataOverlay::default()))
    }

    /// Return the resource limits configured for this engine.
    ///
    /// Exposed so callers can pass the same limits to [`DataOverlay::resolve`].
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// Evaluate a spec. When `repo` is `None`, the spec must be unambiguous
    /// across loaded repositories; when `Some`, scoped to that repository
    /// qualifier (e.g. `"@org/pkg"`).
    ///
    /// When `explain` is `true`, each requested rule's [`RuleResult::explanation`]
    /// contains a structural explanation tree; when `false`, explanations are omitted.
    ///
    /// `rules` scopes which rule results appear in the response. `None` means all local
    /// rules. `Some(&[])` is an error. When `explain` is `true`, all local rules (and
    /// nested `uses` rules needed for explanation resolution) are evaluated so explanation
    /// trees can embed dependency rules.
    pub fn run(
        &self,
        repo: Option<&str>,
        spec: &str,
        effective: Option<&DateTimeValue>,
        data_values: HashMap<String, String>,
        explain: bool,
        rules: Option<&[String]>,
    ) -> Result<Response, Error> {
        let effective = self.effective_or_now(effective);
        let plan = self.get_plan(repo, spec, Some(&effective))?;
        let data_values: HashMap<String, DataValueInput> = data_values
            .into_iter()
            .map(|(k, v)| (k, DataValueInput::convenience(v)))
            .collect();
        self.run_plan(plan, Some(&effective), data_values, explain, rules)
    }

    /// Execution plan for `name`. When `repo` is `None`, the spec must be
    /// unambiguous across all loaded repositories; when `Some`, scoped to that
    /// repository qualifier (e.g. `"@org/pkg"`).
    pub fn get_plan(
        &self,
        repo: Option<&str>,
        name: &str,
        effective: Option<&DateTimeValue>,
    ) -> Result<&crate::planning::ExecutionPlan, Error> {
        let effective_dt = self.effective_or_now(effective);
        let instant = EffectiveDate::DateTimeValue(effective_dt.clone());
        let canonical_name = crate::parsing::ast::ascii_lowercase_logical_name(name.to_string());

        let repository = match repo {
            Some(q) => self.specs.find_repository(q).ok_or_else(|| {
                Error::request_not_found(
                    format!("Repository '{q}' not loaded"),
                    Some("List repositories with `lemma list` after loading your workspace"),
                )
            })?,
            None => self.specs.workspace(),
        };

        let Some(spec_set) = self.specs.spec_set(&repository, &canonical_name) else {
            return Err(self.spec_not_found_in_repository_error(&repository, name, &effective_dt));
        };

        if spec_set.spec_at(&instant).is_none() {
            return Err(self.spec_not_found_in_repository_error(&repository, name, &effective_dt));
        }

        let plan_set = self
            .plan_sets
            .get(&repository)
            .and_then(|by_name| by_name.get(&canonical_name))
            .ok_or_else(|| {
                Error::request_not_found(
                    format!("No execution plans for spec '{name}'"),
                    Some("Ensure sources loaded and planning succeeded"),
                )
            })?;

        plan_set.plan_at(&instant).ok_or_else(|| {
            Error::request_not_found(
                format!("No execution plan slice for spec '{name}' at effective {effective_dt}"),
                None::<String>,
            )
        })
    }

    /// Run a plan from [`get_plan`]: apply data values and evaluate rules.
    ///
    /// Spec-declared defaults (`-> default ...`) are applied by the evaluator when
    /// the caller does not supply a value for that data path.
    ///
    /// `rules`: `None` evaluates all local rules in the response; `Some(slice)` evaluates
    /// only those names (`Some(&[])` errors). Each name in `Some` must be a local rule.
    ///
    /// When `explain` is `false`, only the VM scope runs (requested rules, or all local
    /// when `None`). When `explain` is `true`, all local and nested `uses` rules run so
    /// explanation trees have full `rule_results`, but only the response scope appears in
    /// `response.results` and receives attached explanations.
    pub fn run_plan(
        &self,
        plan: &crate::planning::ExecutionPlan,
        effective: Option<&DateTimeValue>,
        data_values: HashMap<String, DataValueInput>,
        explain: bool,
        rules: Option<&[String]>,
    ) -> Result<Response, Error> {
        let response_rules = plan.validated_response_rule_names(rules)?;
        let effective = self.effective_or_now(effective);
        let overlay = DataOverlay::resolve(plan, data_values, &self.limits)?;
        crate::planning::execution_plan::validate_unit_index_references(plan)?;
        let now_semantic = crate::planning::semantics::date_time_to_semantic(&effective);
        let now_literal = crate::planning::semantics::LiteralValue {
            value: crate::planning::semantics::ValueKind::Date(now_semantic),
            lemma_type: crate::planning::semantics::primitive_date_arc().clone(),
        };
        let (mut response, context) =
            self.evaluator
                .evaluate(plan, &overlay, now_literal, explain, &response_rules);
        if explain {
            self.evaluator.explain(&mut response, plan, &context);
        }
        Ok(response)
    }

    pub fn remove(&mut self, name: &str, effective: Option<&DateTimeValue>) -> Result<(), Error> {
        let effective = self.effective_or_now(effective);
        let repository_arc = self.specs.workspace();
        let spec_arc = self.get_spec(name, Some(&effective))?;
        self.specs.remove_spec(&repository_arc, &spec_arc);
        let pr = crate::planning::plan(&self.specs, &self.limits);
        let planning_errs: Vec<Error> = pr
            .results
            .iter()
            .flat_map(|r| r.errors().cloned())
            .collect();
        self.apply_planning_result(pr);
        if let Some(e) = planning_errs.into_iter().next() {
            return Err(e);
        }
        Ok(())
    }

    /// Build a "not found" error listing available temporal versions when the name exists
    /// but no version covers the requested effective date.
    fn spec_not_found_error(&self, spec_name: &str, effective: &DateTimeValue) -> Error {
        let workspace = self.specs.workspace();
        let available = match self.specs.spec_set(&workspace, spec_name) {
            Some(ss) => ss.iter_specs().collect::<Vec<_>>(),
            None => Vec::new(),
        };
        let msg = if available.is_empty() {
            format!("Spec '{}' not found", spec_name)
        } else {
            let listing: Vec<String> = available
                .iter()
                .map(|s| match s.effective_from() {
                    Some(dt) => format!("  {} (effective from {})", s.name, dt),
                    None => format!("  {} (no effective_from)", s.name),
                })
                .collect();
            format!(
                "Spec '{}' not found for effective {}. Available versions:\n{}",
                spec_name,
                effective,
                listing.join("\n")
            )
        };
        Error::request_not_found(msg, None::<String>)
    }

    fn spec_not_found_in_repository_error(
        &self,
        repository: &LemmaRepository,
        spec_name: &str,
        effective: &DateTimeValue,
    ) -> Error {
        let repo_label = match &repository.name {
            Some(n) => n.clone(),
            None => "(workspace)".to_string(),
        };
        Error::request_not_found(
            format!(
                "Spec '{spec_name}' not found in repository {repo_label} at effective {effective}",
            ),
            Some("Try `lemma list`"),
        )
    }

    /// Effective datetime for a request: `explicit` or now.
    #[must_use]
    fn effective_or_now(&self, effective: Option<&DateTimeValue>) -> DateTimeValue {
        effective.cloned().unwrap_or_else(DateTimeValue::now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> DateTimeValue {
        DateTimeValue {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
            granularity: crate::literals::DateGranularity::Full,
        }
    }

    fn make_spec_with_range(name: &str, effective_from: Option<DateTimeValue>) -> LemmaSpec {
        let mut spec = LemmaSpec::new(name.to_string());
        spec.effective_from = crate::parsing::ast::EffectiveDate::from_option(effective_from);
        spec
    }

    /// Context::iter returns specs in (name, effective_from) ascending order.
    /// Same-name specs appear in temporal order; definition order in the source is irrelevant.
    #[test]
    fn list_specs_order_is_name_then_effective_from_ascending() {
        let mut ctx = Context::new();
        let repository = ctx.workspace();
        let s_2026 = Arc::new(make_spec_with_range("mortgage", Some(date(2026, 1, 1))));
        let s_2025 = Arc::new(make_spec_with_range("mortgage", Some(date(2025, 1, 1))));
        ctx.insert_spec(Arc::clone(&repository), Arc::clone(&s_2026))
            .unwrap();
        ctx.insert_spec(Arc::clone(&repository), Arc::clone(&s_2025))
            .unwrap();
        let listed: Vec<_> = ctx
            .spec_set(&repository, "mortgage")
            .expect("mortgage set")
            .iter_specs()
            .collect();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].effective_from(), Some(&date(2025, 1, 1)));
        assert_eq!(listed[1].effective_from(), Some(&date(2026, 1, 1)));
    }

    #[test]
    fn get_spec_resolves_temporal_version_by_effective() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec pricing 2025-01-01
        data x: 1
        rule r: x
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("a.lemma"))),
            )
            .unwrap();
        engine
            .load(
                r#"
        spec pricing 2025-06-01
        data x: 2
        rule r: x
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("b.lemma"))),
            )
            .unwrap();

        let jan = DateTimeValue {
            year: 2025,
            month: 1,
            day: 15,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
            granularity: crate::literals::DateGranularity::Full,
        };
        let jul = DateTimeValue {
            year: 2025,
            month: 7,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
            granularity: crate::literals::DateGranularity::Full,
        };

        let v1 = DateTimeValue {
            year: 2025,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
            granularity: crate::literals::DateGranularity::Full,
        };
        let v2 = DateTimeValue {
            year: 2025,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
            granularity: crate::literals::DateGranularity::Full,
        };

        let s_jan = engine.get_spec("pricing", Some(&jan)).expect("jan spec");
        let s_jul = engine.get_spec("pricing", Some(&jul)).expect("jul spec");
        assert_eq!(s_jan.effective_from(), Some(&v1));
        assert_eq!(s_jul.effective_from(), Some(&v2));
    }

    /// Every temporal row for a workspace spec name exposes half-open
    /// `[effective_from, effective_to)` via [`LemmaSpecSet::iter_with_ranges`]. The latest row's
    /// `effective_to` is `None` (no successor); earlier rows' `effective_to`
    /// equals the next row's `effective_from`.
    #[test]
    fn list_specs_returns_half_open_ranges_per_temporal_version() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec pricing 2025-01-01
        data x: 1
        rule r: x
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("a.lemma"))),
            )
            .unwrap();
        engine
            .load(
                r#"
        spec pricing 2025-06-01
        data x: 2
        rule r: x
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("b.lemma"))),
            )
            .unwrap();

        let january = date(2025, 1, 1);
        let june = date(2025, 6, 1);

        let workspace = engine.get_workspace();
        let pricing_set = workspace
            .specs
            .iter()
            .find(|ss| ss.name == "pricing")
            .expect("pricing spec set exists");
        let mut ranges: Vec<(Option<DateTimeValue>, Option<DateTimeValue>)> = pricing_set
            .iter_with_ranges()
            .map(|(_, from, to)| (from, to))
            .collect();
        ranges.sort_by(|a, b| match (&a.0, &b.0) {
            (Some(x), Some(y)) => x.cmp(y),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        assert_eq!(ranges.len(), 2);
        assert_eq!(
            ranges[0],
            (Some(january.clone()), Some(june.clone())),
            "earlier row ends at the next row's effective_from"
        );
        assert_eq!(
            ranges[1],
            (Some(june.clone()), None),
            "latest row has no successor; effective_to is None"
        );

        assert!(
            !engine
                .get_workspace()
                .specs
                .iter()
                .any(|ss| ss.name == "unknown"),
            "no rows for unknown spec"
        );
    }

    /// `Engine::get_workspace()` provides spec sets grouped by name.
    /// Each spec set exposes half-open `[effective_from, effective_to)` ranges.
    #[test]
    fn get_workspace_specs_with_half_open_ranges() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec pricing 2025-01-01
        data x: 1
        rule r: x
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("pricing_v1.lemma"))),
            )
            .unwrap();
        engine
            .load(
                r#"
        spec pricing 2026-01-01
        data x: 2
        rule r: x
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("pricing_v2.lemma"))),
            )
            .unwrap();
        engine
            .load(
                r#"
        spec taxes
        data rate: 0.21
        rule amount: rate
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("taxes.lemma"))),
            )
            .unwrap();

        let workspace = engine.get_workspace();
        assert_eq!(workspace.specs.len(), 2, "two spec sets: pricing and taxes");

        let pricing_set = workspace
            .specs
            .iter()
            .find(|ss| ss.name == "pricing")
            .expect("pricing spec set exists");
        let ranges: Vec<_> = pricing_set.iter_with_ranges().collect();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].1, Some(date(2025, 1, 1)));
        assert_eq!(
            ranges[0].2,
            Some(date(2026, 1, 1)),
            "earlier pricing row ends at the next pricing row's effective_from"
        );
        assert_eq!(ranges[1].1, Some(date(2026, 1, 1)));
        assert_eq!(
            ranges[1].2, None,
            "latest pricing row has no successor; effective_to is None"
        );

        let taxes_set = workspace
            .specs
            .iter()
            .find(|ss| ss.name == "taxes")
            .expect("taxes spec set exists");
        let tax_ranges: Vec<_> = taxes_set.iter_with_ranges().collect();
        assert_eq!(tax_ranges.len(), 1);
        assert_eq!(
            tax_ranges[0].1, None,
            "unversioned spec has no declared effective_from"
        );
        assert_eq!(
            tax_ranges[0].2, None,
            "unversioned spec has no successor; effective_to is None"
        );
    }

    #[test]
    fn test_evaluate_spec_all_rules() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec test
        data x: 10
        data y: 5
        rule sum: x + y
        rule product: x * y
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("test.lemma"))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run(None, "test", Some(&now), HashMap::new(), false, None)
            .unwrap();
        assert_eq!(response.results.len(), 2);

        let sum_result = response
            .results
            .values()
            .find(|r| r.rule.name == "sum")
            .unwrap();
        assert_eq!(sum_result.display.clone().expect("display"), "15");

        let product_result = response
            .results
            .values()
            .find(|r| r.rule.name == "product")
            .unwrap();
        assert_eq!(product_result.display.clone().expect("display"), "50");
    }

    #[test]
    fn test_evaluate_empty_data() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec test
        data price: 100
        rule total: price * 2
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("test.lemma"))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run(None, "test", Some(&now), HashMap::new(), false, None)
            .unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(
            response
                .results
                .values()
                .next()
                .unwrap()
                .display
                .clone()
                .expect("display"),
            "200"
        );
    }

    #[test]
    fn test_evaluate_boolean_rule() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec test
        data age: 25
        rule is_adult: age >= 18
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("test.lemma"))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run(None, "test", Some(&now), HashMap::new(), false, None)
            .unwrap();
        assert_eq!(
            response.results.values().next().unwrap().boolean,
            Some(true)
        );
    }

    #[test]
    fn test_evaluate_with_unless_clause() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec test
        data quantity: 15
        rule discount: 0
          unless quantity >= 10 then 10
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("test.lemma"))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run(None, "test", Some(&now), HashMap::new(), false, None)
            .unwrap();
        assert_eq!(
            response
                .results
                .values()
                .next()
                .unwrap()
                .display
                .clone()
                .expect("display"),
            "10"
        );
    }

    #[test]
    fn test_spec_not_found() {
        let engine = Engine::new();
        let now = DateTimeValue::now();
        let result = engine.run(None, "nonexistent", Some(&now), HashMap::new(), false, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_multiple_specs() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec spec1
        data x: 10
        rule result: x * 2
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("spec 1.lemma"))),
            )
            .unwrap();

        engine
            .load(
                r#"
        spec spec2
        data y: 5
        rule result: y * 3
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("spec 2.lemma"))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response1 = engine
            .run(None, "spec1", Some(&now), HashMap::new(), false, None)
            .unwrap();
        assert_eq!(response1.results[0].display.clone().expect("display"), "20");
        let response2 = engine
            .run(None, "spec2", Some(&now), HashMap::new(), false, None)
            .unwrap();
        assert_eq!(response2.results[0].display.clone().expect("display"), "15");
    }

    #[test]
    fn test_runtime_error_mapping() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec test
        data numerator: 10
        data denominator: 0
        rule division: numerator / denominator
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("test.lemma"))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let result = engine.run(None, "test", Some(&now), HashMap::new(), false, None);
        // Division by zero returns a Veto (not an error)
        assert!(result.is_ok(), "Evaluation should succeed");
        let response = result.unwrap();
        let division_result = response
            .results
            .values()
            .find(|r| r.rule.name == "division");
        assert!(
            division_result.is_some(),
            "Should have division rule result"
        );
        let division = division_result.unwrap();
        assert!(division.vetoed);
        assert!(
            division
                .veto_reason
                .as_deref()
                .unwrap()
                .contains("Division by zero"),
            "Veto message should mention division by zero: {:?}",
            division.veto_reason
        );
    }

    #[test]
    fn test_rules_sorted_by_source_order() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec test
        data a: 1
        data b: 2
        rule z: a + b
        rule y: a * b
        rule x: a - b
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("test.lemma"))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run(None, "test", Some(&now), HashMap::new(), false, None)
            .unwrap();
        assert_eq!(response.results.len(), 3);

        // Verify source positions increase (z < y < x)
        let z_pos = response
            .results
            .values()
            .find(|r| r.rule.name == "z")
            .unwrap()
            .rule
            .source_location
            .span
            .start;
        let y_pos = response
            .results
            .values()
            .find(|r| r.rule.name == "y")
            .unwrap()
            .rule
            .source_location
            .span
            .start;
        let x_pos = response
            .results
            .values()
            .find(|r| r.rule.name == "x")
            .unwrap()
            .rule
            .source_location
            .span
            .start;

        assert!(z_pos < y_pos);
        assert!(y_pos < x_pos);
    }

    #[test]
    fn test_rule_filtering_evaluates_dependencies() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec test
        data base: 100
        rule subtotal: base * 2
        rule tax: subtotal * 10%
        rule total: subtotal + tax
    "#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("test.lemma"))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run(
                None,
                "test",
                Some(&now),
                HashMap::new(),
                false,
                Some(&["total".to_string()]),
            )
            .unwrap();

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results.keys().next().unwrap(), "total");

        // But the value should be correct (dependencies were computed)
        let total = response.results.values().next().unwrap();
        assert_eq!(total.display.clone().expect("display"), "220");
    }

    // -------------------------------------------------------------------
    // Pre-resolved dependency tests (Engine never fetches from registry)
    // -------------------------------------------------------------------

    use crate::parsing::ast::DateTimeValue;

    #[test]
    fn pre_resolved_deps_in_file_map_evaluates_external_spec() {
        let mut engine = Engine::new();

        engine
            .load_batch(
                HashMap::from([(
                    SourceType::Volatile,
                    "repo @org/project\nspec helper\ndata quantity: 42".to_string(),
                )]),
                Some("@org/project"),
            )
            .expect("should load dependency files");

        engine
            .load(
                r#"spec main_spec
uses external: @org/project helper
rule value: external.quantity"#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("main.lemma"))),
            )
            .expect("should succeed with pre-resolved deps");

        let now = DateTimeValue::now();
        let response = engine
            .run(None, "main_spec", Some(&now), HashMap::new(), false, None)
            .expect("evaluate should succeed");

        let value_result = response
            .results
            .get("value")
            .expect("rule 'value' should exist");
        assert_eq!(value_result.display.clone().expect("display"), "42");
    }

    #[test]
    fn schema_with_repo_resolves_registry_spec() {
        let mut engine = Engine::new();
        engine
            .load_batch(
                HashMap::from([(
                    SourceType::Volatile,
                    "repo @org/project\nspec helper\ndata quantity: 42\nrule expose: quantity"
                        .to_string(),
                )]),
                Some("@org/project"),
            )
            .expect("registry bundle loads");

        engine
            .load(
                r#"spec main_spec
data x: 1"#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("main.lemma"))),
            )
            .expect("main loads");

        let now = DateTimeValue::now();
        let schema = engine
            .schema(Some("@org/project"), "helper", Some(&now))
            .expect("schema for registry spec");
        assert!(schema.data.contains_key("quantity"));
    }

    #[test]
    fn load_no_external_refs_works() {
        let mut engine = Engine::new();

        engine
            .load(
                r#"spec local_only
data price: 100
rule doubled: price * 2"#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("local.lemma"))),
            )
            .expect("should succeed when there are no @... references");

        let now = DateTimeValue::now();
        let response = engine
            .run(None, "local_only", Some(&now), HashMap::new(), false, None)
            .expect("evaluate should succeed");

        let doubled = response.results.get("doubled").expect("doubled rule");
        assert_eq!(doubled.display.clone().expect("display"), "200");
    }

    #[test]
    fn unresolved_external_ref_without_deps_fails() {
        let mut engine = Engine::new();

        let result = engine.load(
            r#"spec main_spec
uses external: @org/project missing
rule value: external.quantity"#,
            SourceType::Path(Arc::new(std::path::PathBuf::from("main.lemma"))),
        );

        let errs = result.expect_err("Should fail when registry dep is not loaded");
        assert!(
            errs.iter()
                .any(|e| e.kind() == crate::ErrorKind::MissingRepository),
            "expected MissingRepository, got: {:?}",
            errs.iter().map(|e| e.kind()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn pre_resolved_deps_with_spec_and_type_refs() {
        let mut engine = Engine::new();

        engine
            .load_batch(
                HashMap::from([(
                    SourceType::Volatile,
                    "repo @org/example\nspec helper\ndata value: 42".to_string(),
                )]),
                Some("@org/example"),
            )
            .expect("should load helper file");

        engine
            .load_batch(
                HashMap::from([(
                    SourceType::Volatile,
                    "repo @lemma/std\nspec finance\ndata money: quantity\n -> unit eur 1.00\n -> decimals 2".to_string(),
                )]),
                Some("@lemma/std"),
            )
            .expect("should load finance file");

        engine
            .load(
                r#"spec registry_demo
uses @lemma/std finance
data money: finance.money
data unit_price: 5 eur
uses @org/example helper
rule helper_value: helper.value
rule line_total: unit_price * 2
rule formatted: helper_value + 0"#,
                SourceType::Path(Arc::new(std::path::PathBuf::from("main.lemma"))),
            )
            .expect("should succeed with pre-resolved spec and type deps");

        let now = DateTimeValue::now();
        let response = engine
            .run(
                None,
                "registry_demo",
                Some(&now),
                HashMap::new(),
                false,
                None,
            )
            .expect("evaluate should succeed");

        assert_eq!(
            response
                .results
                .get("helper_value")
                .expect("helper_value")
                .display
                .clone()
                .expect("display"),
            "42"
        );
        let line = response
            .results
            .get("line_total")
            .expect("line_total")
            .display
            .clone()
            .expect("display");
        assert!(
            line.contains("10") && line.to_lowercase().contains("eur"),
            "5 eur * 2 => ~10 eur, got {line}"
        );
        assert_eq!(
            response
                .results
                .get("formatted")
                .expect("formatted")
                .display
                .clone()
                .expect("display"),
            "42"
        );
    }

    #[test]
    fn load_empty_labeled_source_is_error() {
        let mut engine = Engine::new();
        let err = engine
            .load(
                "spec x\ndata a: 1",
                SourceType::Path(Arc::new(std::path::PathBuf::from("  "))),
            )
            .unwrap_err();
        assert!(err.errors.iter().any(|e| e.message().contains("non-empty")));
    }

    #[test]
    fn add_dependency_files_accepts_registry_bundle_specs() {
        let mut engine = Engine::new();
        engine
            .load_batch(
                HashMap::from([(
                    SourceType::Volatile,
                    "repo @org/my\nspec helper\ndata x: 1".to_string(),
                )]),
                Some("@org/my"),
            )
            .expect("dependency bundle specs should be accepted");
    }

    #[test]
    fn user_load_rejects_reserved_embedded_stdlib_repository() {
        let mut engine = Engine::new();
        let batch = engine.load_batch(
            HashMap::from([(
                SourceType::Volatile,
                "spec finance\ndata money: ratio -> decimals 2".to_string(),
            )]),
            Some(EMBEDDED_STDLIB_REPOSITORY),
        );
        assert!(
            batch.is_err(),
            "load_batch must not write reserved lemma stdlib repo"
        );
        let msg = batch
            .unwrap_err()
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            msg.contains(EMBEDDED_STDLIB_REPOSITORY) && msg.contains("reserved"),
            "expected reserved-repo error, got: {msg}"
        );

        let workspace = engine.load("repo lemma\nspec x\ndata a: 1", SourceType::Volatile);
        assert!(workspace.is_err(), "workspace repo lemma must be rejected");
        let msg = workspace
            .unwrap_err()
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            msg.contains(EMBEDDED_STDLIB_REPOSITORY) && msg.contains("reserved"),
            "expected reserved-repo error, got: {msg}"
        );
    }

    #[test]
    fn dependency_cannot_merge_with_workspace_repo() {
        let mut engine = Engine::new();
        engine
            .load(
                "repo billing\nspec local_billing\ndata x: 1",
                SourceType::Path(Arc::new(std::path::PathBuf::from("local.lemma"))),
            )
            .expect("workspace load");

        let result = engine.load_batch(
            HashMap::from([(
                SourceType::Volatile,
                "repo billing\nspec dep_billing\ndata y: 2".to_string(),
            )]),
            Some("@evil/pkg"),
        );
        assert!(
            result.is_err(),
            "dependency declaring same repo name as workspace must be rejected"
        );
        let msg = result
            .unwrap_err()
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            msg.contains("billing") && msg.contains("workspace"),
            "error should mention repo name and workspace provenance, got: {msg}"
        );
    }

    #[test]
    fn load_rejects_empty_registry_source_identifier() {
        let mut engine = Engine::new();
        let result = engine.load(
            "spec helper\ndata x: 1",
            SourceType::Registry(Arc::new(LemmaRepository::new(Some("".to_string())))),
        );
        assert!(
            result.is_err(),
            "empty registry dependency source identifier must be rejected"
        );
    }

    #[test]
    fn load_dependency_accepts_split_bundles() {
        let mut engine = Engine::new();
        engine
            .load_batch(
                HashMap::from([(
                    SourceType::Volatile,
                    "repo @org/rates\nspec rates\ndata rate: 10".to_string(),
                )]),
                Some("@org/rates"),
            )
            .expect("rates bundle should load");
        engine
            .load_batch(
                HashMap::from([(
                    SourceType::Volatile,
                    "repo @org/billing\nspec billing\nuses @org/rates rates".to_string(),
                )]),
                Some("@org/billing"),
            )
            .expect("billing bundle should load");
    }

    #[test]
    fn load_returns_all_errors_not_just_first() {
        let mut engine = Engine::new();

        let result = engine.load(
            r#"spec demo
uses type_src: nonexistent_type_source
with type_src.amount: 10
uses helper: nonexistent_spec
data price: 10
rule total: helper.value + price"#,
            SourceType::Path(Arc::new(std::path::PathBuf::from("test.lemma"))),
        );

        assert!(result.is_err(), "Should fail with multiple errors");
        let load_err = result.unwrap_err();
        assert!(
            load_err.errors.len() >= 2,
            "expected at least 2 errors (type + spec ref), got {}",
            load_err.errors.len()
        );
        let error_message = load_err
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");

        assert!(
            error_message.contains("nonexistent_type_source"),
            "Should mention data import source spec. Got:\n{}",
            error_message
        );
        assert!(
            error_message.contains("nonexistent_spec"),
            "Should mention spec reference error about 'nonexistent_spec'. Got:\n{}",
            error_message
        );
    }

    // ── Default value type validation ────────────────────────────────
    // Planning must reject default values that don't match the type.
    // These tests cover both primitives and named types (which the parser
    // can't validate because it doesn't resolve type names).

    #[test]
    fn planning_rejects_invalid_number_default() {
        let mut engine = Engine::new();
        let result = engine.load(
            "spec t\ndata x: number -> default \"10 $$\"]\nrule r: x",
            SourceType::Path(Arc::new(std::path::PathBuf::from("t.lemma"))),
        );
        assert!(
            result.is_err(),
            "must reject non-numeric default on number type"
        );
    }

    #[test]
    fn planning_rejects_text_literal_as_number_default() {
        // `default "10"` produces a typed `CommandArg::Literal(Value::Text("10"))`.
        // Planning matches on the literal's variant — a `Text` literal is rejected
        // where a `Number` literal is required, even though `"10"` would parse as
        // a valid `Decimal` if coerced.
        let mut engine = Engine::new();
        let result = engine.load(
            "spec t\ndata x: number -> default \"10\"]\nrule r: x",
            SourceType::Path(Arc::new(std::path::PathBuf::from("t.lemma"))),
        );
        assert!(
            result.is_err(),
            "must reject text literal \"10\" as default for number type"
        );
    }

    #[test]
    fn planning_rejects_invalid_boolean_default() {
        let mut engine = Engine::new();
        let result = engine.load(
            "spec t\ndata x: [boolean -> default \"maybe\"]\nrule r: x",
            SourceType::Path(Arc::new(std::path::PathBuf::from("t.lemma"))),
        );
        assert!(
            result.is_err(),
            "must reject non-boolean default on boolean type"
        );
    }

    #[test]
    fn planning_rejects_invalid_named_type_default() {
        // Named type: the parser can't validate this, only planning can.
        let mut engine = Engine::new();
        let result = engine.load("spec t\ndata custom: number -> minimum 0\ndata x: [custom -> default \"abc\"]\nrule r: x", SourceType::Path(Arc::new(std::path::PathBuf::from("t.lemma"))));
        assert!(
            result.is_err(),
            "must reject non-numeric default on named number type"
        );
    }

    #[test]
    fn context_merges_cross_file_repo_identities() {
        let mut engine = Engine::new();

        // Load two files with the same named repo, but different spec names.
        engine
            .load(
                "repo shared\nspec a\ndata x: 1",
                SourceType::Path(Arc::new(std::path::PathBuf::from("file1.lemma"))),
            )
            .expect("first file should load");

        engine
            .load(
                "repo shared\nspec b\ndata y: 2",
                SourceType::Path(Arc::new(std::path::PathBuf::from("file2.lemma"))),
            )
            .expect("second file should load");

        // Both specs should land under the same repo entry.
        // Workspace, embedded stdlib (`lemma`), plus the "shared" repo.
        assert_eq!(
            engine.specs.repositories().len(),
            3,
            "should have workspace, stdlib repository, and one named user repository"
        );

        let shared_repo = engine
            .specs
            .find_repository("shared")
            .expect("shared repo should exist");
        let shared_specs = engine.specs.repositories().get(&shared_repo).unwrap();
        assert_eq!(
            shared_specs.len(),
            2,
            "shared repo should contain both specs"
        );
        assert!(shared_specs.contains_key("a"));
        assert!(shared_specs.contains_key("b"));

        // Loading a dependency with the same repo name should be rejected.
        let result = engine.load_batch(
            HashMap::from([(
                SourceType::Volatile,
                "repo shared\nspec c\ndata z: 3".to_string(),
            )]),
            Some("@some/dep"),
        );
        assert!(
            result.is_err(),
            "dependency repo with same name as workspace repo must be rejected"
        );
    }

    #[test]
    fn context_rejects_duplicate_spec_in_same_repo_across_files() {
        let mut engine = Engine::new();

        engine
            .load(
                "repo shared\nspec a\ndata x: 1",
                SourceType::Path(Arc::new(std::path::PathBuf::from("file1.lemma"))),
            )
            .expect("first file should load");

        let result = engine.load(
            "repo shared\nspec a\ndata y: 2",
            SourceType::Path(Arc::new(std::path::PathBuf::from("file2.lemma"))),
        );

        assert!(
            result.is_err(),
            "should reject duplicate spec name in same repo"
        );
        let err_msg = result.unwrap_err().errors[0].to_string();
        assert!(
            err_msg.contains("Duplicate spec 'a'"),
            "error should mention duplicate spec"
        );
    }

    #[test]
    fn test_list_serialization() {
        let mut engine = Engine::new();
        engine
            .load(
                "repo shared\nspec a\ndata x: 1\nrule r: x",
                SourceType::Path(Arc::new(std::path::PathBuf::from("file1.lemma"))),
            )
            .expect("file should load");

        let repos = engine.list();
        let json = serde_json::to_string(&repos).expect("should serialize");

        // Should include expected nesting
        assert!(json.contains("\"repository\""));
        assert!(json.contains("\"name\":\"shared\""));
        assert!(json.contains("\"specs\""));
        assert!(json.contains("\"name\":\"a\""));
        assert!(json.contains("\"data\""));
        assert!(json.contains("\"rules\""));
    }

    #[test]
    fn out_of_memory_during_exact_multiply_vetoes_without_crashing() {
        use crate::computation::bigint::{test_clear_alloc_fail, test_force_alloc_fail};

        let code = r#"
spec oom_multiply
data a: 9999999999999999999999999999
data b: 9999999999999999999999999999
rule huge: a * b
"#;

        let mut engine = Engine::new();
        engine
            .load(code, SourceType::Volatile)
            .expect("load must succeed");

        test_force_alloc_fail(50);

        let now = DateTimeValue::now();
        let response = engine
            .run(
                None,
                "oom_multiply",
                Some(&now),
                HashMap::new(),
                false,
                None,
            )
            .expect("evaluation must complete without process crash");

        test_clear_alloc_fail();

        let rule = response
            .results
            .get("huge")
            .expect("huge rule must be present");

        assert!(rule.vetoed);
        assert_eq!(rule.veto_reason.as_deref(), Some("out of memory"));
    }
}
