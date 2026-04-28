use crate::evaluation::Evaluator;
use crate::parsing::ast::{DateTimeValue, LemmaSpec};
use crate::parsing::EffectiveDate;
use crate::planning::{LemmaSpecSet, SpecSchema};
use crate::{parse, Error, ResourceLimits, Response};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashSet;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// Load failure: errors plus the source files we attempted to load.
#[derive(Debug, Clone)]
pub struct Errors {
    pub errors: Vec<Error>,
    pub sources: HashMap<String, String>,
}

impl Errors {
    /// Iterate over the errors.
    pub fn iter(&self) -> std::slice::Iter<'_, Error> {
        self.errors.iter()
    }
}

// ─── Spec store with temporal resolution ──────────────────────────────

/// Ordered set of specs grouped into `LemmaSpecSet`s by name.
///
/// Specs with the same name are ordered by effective_from.
/// A spec's temporal end is derived from the next spec's effective_from, or +inf.
#[derive(Debug, Default)]
pub struct Context {
    spec_sets: BTreeMap<String, LemmaSpecSet>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            spec_sets: BTreeMap::new(),
        }
    }

    /// All spec sets (name → specs keyed by effective_from), ordered by spec name.
    #[must_use]
    pub fn spec_sets(&self) -> &BTreeMap<String, LemmaSpecSet> {
        &self.spec_sets
    }

    pub fn iter(&self) -> impl Iterator<Item = Arc<LemmaSpec>> + '_ {
        self.spec_sets.values().flat_map(|ss| ss.iter_specs())
    }

    /// Every loaded spec paired with its half-open
    /// `[effective_from, effective_to)` validity range.
    ///
    /// Iteration order: spec name ascending, then by `effective_from` ascending
    /// within the same name — identical to [`Self::iter`]. Each tuple is
    /// `(spec, effective_from, effective_to)`; see
    /// [`crate::planning::LemmaSpecSet::iter_with_ranges`] for the range
    /// semantics (the last row of each name has `effective_to = None`).
    pub fn iter_with_ranges(
        &self,
    ) -> impl Iterator<Item = (Arc<LemmaSpec>, Option<DateTimeValue>, Option<DateTimeValue>)> + '_
    {
        self.spec_sets
            .values()
            .flat_map(|spec_set| spec_set.iter_with_ranges())
    }

    /// Insert a spec. Set `from_registry` to `true` for pre-fetched registry
    /// specs; `false` rejects `@`-prefixed spec definitions.
    ///
    /// When `from_registry` is true, only `@`-prefixed specs are accepted —
    /// registry bundles must not introduce bare-named specs into the local namespace.
    pub fn insert_spec(&mut self, spec: Arc<LemmaSpec>, from_registry: bool) -> Result<(), Error> {
        if spec.from_registry && !from_registry {
            return Err(Error::validation_with_context(
                format!(
                    "Spec '{}' uses '@' registry prefix, which is reserved for dependencies",
                    spec.name
                ),
                None,
                Some("Remove the '@' prefix, or load this file as a dependency."),
                Some(Arc::clone(&spec)),
                None,
            ));
        }

        if from_registry && !spec.from_registry {
            return Err(Error::validation_with_context(
                format!(
                    "Registry bundle contains spec '{}' without '@' prefix; \
                     all specs in a registry bundle must use '@'-prefixed names \
                     to avoid conflicts with local specs",
                    spec.name
                ),
                None,
                Some("Prefix the spec name with '@' (e.g. spec @org/project/name)."),
                Some(Arc::clone(&spec)),
                None,
            ));
        }

        let name = spec.name.clone();
        if self
            .spec_sets
            .get(&name)
            .is_some_and(|ss| ss.get_exact(spec.effective_from()).is_some())
        {
            return Err(Error::validation_with_context(
                format!(
                    "Duplicate spec '{}' (same name and effective_from already in context)",
                    spec.name
                ),
                None,
                None::<String>,
                Some(Arc::clone(&spec)),
                None,
            ));
        }

        let inserted = self
            .spec_sets
            .entry(name.clone())
            .or_insert_with(|| LemmaSpecSet::new(name))
            .insert(spec);
        debug_assert!(inserted);
        Ok(())
    }

    pub fn remove_spec(&mut self, spec: &Arc<LemmaSpec>) -> bool {
        let key = spec.effective_from().cloned();
        let Some(ss) = self.spec_sets.get_mut(&spec.name) else {
            return false;
        };
        if !ss.remove(key.as_ref()) {
            return false;
        }
        if ss.is_empty() {
            self.spec_sets.remove(&spec.name);
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.spec_sets.values().map(LemmaSpecSet::len).sum()
    }
}

// ─── Engine ──────────────────────────────────────────────────────────

/// How a single buffer is identified in parse/plan diagnostics and the engine source map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType<'a> {
    /// Path, URI, test name, or any non-empty stable id.
    Labeled(&'a str),
    /// No stable path (pasted string, REPL). Stored under [`SourceType::INLINE_KEY`].
    Inline,
    // Pre-resolved registry bundle
    Dependency(&'a str),
}

impl SourceType<'_> {
    /// Source map key and span attribute for [`SourceType::Inline`].
    pub const INLINE_KEY: &'static str = "inline source (no path)";

    fn storage_key(self) -> Result<String, Vec<Error>> {
        match self {
            SourceType::Labeled(s) => {
                if s.trim().is_empty() {
                    return Err(vec![Error::request(
                        "source label must be non-empty, or use SourceType::Inline",
                        None::<String>,
                    )]);
                }
                Ok(s.to_string())
            }
            SourceType::Inline => Ok(Self::INLINE_KEY.to_string()),
            SourceType::Dependency(s) => Ok(s.to_string()),
        }
    }
}

/// Engine for evaluating Lemma rules.
///
/// Pure Rust implementation that evaluates Lemma specs directly from the AST.
/// Uses pre-built execution plans that are self-contained and ready for evaluation.
///
/// The engine never performs network calls. External `@...` references must be
/// pre-resolved before loading — either by including dep files
/// in the file map or by calling `resolve_registry_references` separately
/// (e.g. in a `lemma fetch` command).
pub struct Engine {
    /// Spec name → resolved plans (ordered by `effective`; slice end from next plan).
    plan_sets: HashMap<String, crate::planning::ExecutionPlanSet>,
    specs: Context,
    sources: HashMap<String, String>,
    evaluator: Evaluator,
    limits: ResourceLimits,
    total_expression_count: usize,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            plan_sets: HashMap::new(),
            specs: Context::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits: ResourceLimits::default(),
            total_expression_count: 0,
        }
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Source code map (attribute -> content). Used for error display.
    pub fn sources(&self) -> &HashMap<String, String> {
        &self.sources
    }

    /// Create an engine with custom resource limits.
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            plan_sets: HashMap::new(),
            specs: Context::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits,
            total_expression_count: 0,
        }
    }

    fn apply_planning_result(&mut self, pr: crate::planning::PlanningResult) {
        self.plan_sets.clear();
        for r in &pr.results {
            self.plan_sets
                .insert(r.name.clone(), r.execution_plan_set());
        }
    }

    /// Load a single spec from source code.
    /// When `source` is [`SourceType::Dependency`], content is treated as from a registry bundle (`from_registry: true`).
    pub fn load(&mut self, code: &str, source: SourceType<'_>) -> Result<(), Errors> {
        let from_registry = matches!(source, SourceType::Dependency(_));
        let mut files = HashMap::new();
        let key = source.storage_key().map_err(|errs| Errors {
            errors: errs,
            sources: HashMap::new(),
        })?;
        files.insert(key, code.to_string());
        self.add_files_inner(files, from_registry)
    }

    /// Load .lemma files from paths (files and/or directories). Directories are expanded one level only (direct child .lemma files). Resource limits `max_files`, `max_loaded_bytes`, `max_file_size_bytes` are enforced in [`add_files_inner`].
    ///
    /// Set `from_registry` to `true` for pre-fetched registry bundles (same rules as [`Context::insert_spec`] with `from_registry`). Not available on wasm32 (no filesystem).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from_paths<P: AsRef<Path>>(
        &mut self,
        paths: &[P],
        from_registry: bool,
    ) -> Result<(), Errors> {
        use std::fs;

        let mut files = HashMap::new();
        let mut seen = HashSet::<String>::new();

        for path in paths {
            let path = path.as_ref();
            if path.is_file() {
                // Skip non-`.lemma` files (extension missing or wrong).
                if !path.extension().map(|e| e == "lemma").unwrap_or(false) {
                    continue;
                }
                let key = path.display().to_string();
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key.clone());
                let content = fs::read_to_string(path).map_err(|e| Errors {
                    errors: vec![Error::request(
                        format!("Cannot read '{}': {}", path.display(), e),
                        None::<String>,
                    )],
                    sources: HashMap::new(),
                })?;
                files.insert(key, content);
            } else if path.is_dir() {
                let read_dir = fs::read_dir(path).map_err(|e| Errors {
                    errors: vec![Error::request(
                        format!("Cannot read directory '{}': {}", path.display(), e),
                        None::<String>,
                    )],
                    sources: HashMap::new(),
                })?;
                for entry in read_dir.filter_map(Result::ok) {
                    let p = entry.path();
                    if !p.is_file() || !p.extension().map(|e| e == "lemma").unwrap_or(false) {
                        continue;
                    }
                    let key = p.display().to_string();
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.insert(key.clone());
                    let Ok(content) = fs::read_to_string(&p) else {
                        continue;
                    };
                    files.insert(key, content);
                }
            }
        }

        self.add_files_inner(files, from_registry)
    }

    fn add_files_inner(
        &mut self,
        files: HashMap<String, String>,
        from_registry: bool,
    ) -> Result<(), Errors> {
        let limits = &self.limits;
        if files.len() > limits.max_files {
            return Err(Errors {
                errors: vec![Error::resource_limit_exceeded(
                    "max_files",
                    limits.max_files.to_string(),
                    files.len().to_string(),
                    "Reduce the number of paths or files",
                    None::<crate::Source>,
                    None,
                    None,
                )],
                sources: files,
            });
        }
        let total_loaded_bytes: usize = files.values().map(|s| s.len()).sum();
        if total_loaded_bytes > limits.max_loaded_bytes {
            return Err(Errors {
                errors: vec![Error::resource_limit_exceeded(
                    "max_loaded_bytes",
                    limits.max_loaded_bytes.to_string(),
                    total_loaded_bytes.to_string(),
                    "Load fewer or smaller files",
                    None::<crate::Source>,
                    None,
                    None,
                )],
                sources: files,
            });
        }
        for code in files.values() {
            if code.len() > limits.max_file_size_bytes {
                return Err(Errors {
                    errors: vec![Error::resource_limit_exceeded(
                        "max_file_size_bytes",
                        limits.max_file_size_bytes.to_string(),
                        code.len().to_string(),
                        "Use a smaller file or increase limit",
                        None::<crate::Source>,
                        None,
                        None,
                    )],
                    sources: files,
                });
            }
        }

        let mut errors: Vec<Error> = Vec::new();

        for (source_id, code) in &files {
            match parse(code, source_id, &self.limits) {
                Ok(result) => {
                    self.total_expression_count += result.expression_count;
                    if self.total_expression_count > self.limits.max_total_expression_count {
                        errors.push(Error::resource_limit_exceeded(
                            "max_total_expression_count",
                            self.limits.max_total_expression_count.to_string(),
                            self.total_expression_count.to_string(),
                            "Split logic across fewer files or reduce expression complexity",
                            None::<crate::Source>,
                            None,
                            None,
                        ));
                        return Err(Errors {
                            errors,
                            sources: files,
                        });
                    }
                    let new_specs = result.specs;
                    for spec in new_specs {
                        let attribute = spec.attribute.clone().unwrap_or_else(|| spec.name.clone());
                        let start_line = spec.start_line;

                        if from_registry {
                            let bare_refs =
                                crate::planning::graph::collect_bare_registry_refs(&spec);
                            if !bare_refs.is_empty() {
                                let source = crate::Source::new(
                                    &attribute,
                                    crate::parsing::ast::Span {
                                        start: 0,
                                        end: 0,
                                        line: start_line,
                                        col: 0,
                                    },
                                );
                                errors.push(Error::validation(
                                    format!(
                                        "Registry spec '{}' contains references without '@' prefix: {}. \
                                         The registry must rewrite all references to use '@'-prefixed names",
                                        spec.name,
                                        bare_refs.join(", ")
                                    ),
                                    Some(source),
                                    Some(
                                        "The registry must prefix all spec references with '@' \
                                         before serving the bundle.",
                                    ),
                                ));
                                continue;
                            }
                        }

                        match self.specs.insert_spec(Arc::new(spec), from_registry) {
                            Ok(()) => {
                                self.sources.insert(attribute, code.clone());
                            }
                            Err(e) => {
                                let source = crate::Source::new(
                                    &attribute,
                                    crate::parsing::ast::Span {
                                        start: 0,
                                        end: 0,
                                        line: start_line,
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
                Err(e) => errors.push(e),
            }
        }

        let planning_result = crate::planning::plan(&self.specs);
        for set_result in &planning_result.results {
            for spec_result in &set_result.specs {
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
            Err(Errors {
                errors,
                sources: files,
            })
        }
    }

    /// Name-scoped access to all temporal versions of a spec.
    ///
    /// Returns the full `LemmaSpecSet` (every row and its `[effective_from, next)` range),
    /// or `None` when no spec by that name is loaded. This is the primitive for catalog
    /// and version-inventory queries. Point-in-time resolution goes through
    /// [`Engine::get_spec`], which delegates here.
    #[must_use]
    pub fn get_spec_set(&self, name: &str) -> Option<&LemmaSpecSet> {
        self.specs.spec_sets().get(name)
    }

    pub fn get_spec(
        &self,
        name: &str,
        effective: Option<&DateTimeValue>,
    ) -> Result<Arc<LemmaSpec>, Error> {
        let effective = self.effective_or_now(effective);

        self.get_spec_set(name)
            .and_then(|spec_set| spec_set.spec_at(&EffectiveDate::DateTimeValue(effective.clone())))
            .ok_or_else(|| self.spec_not_found_error(name, &effective))
    }

    /// All specs ordered by (name, effective_from).
    pub fn list_specs(&self) -> Vec<Arc<LemmaSpec>> {
        self.specs.iter().collect()
    }

    /// All specs paired with their half-open
    /// `[effective_from, effective_to)` validity ranges.
    ///
    /// Same order as [`Self::list_specs`]. Each entry is
    /// `(spec, effective_from, effective_to)`; for every spec name, the last
    /// row's `effective_to` is `None` (no successor).
    pub fn list_specs_with_ranges(
        &self,
    ) -> Vec<(Arc<LemmaSpec>, Option<DateTimeValue>, Option<DateTimeValue>)> {
        self.specs.iter_with_ranges().collect()
    }

    /// Specs active at `effective` (one per name).
    /// Todo: clone the specs instead of returning references
    /// Consider removing this method: does it make sense to list specs by effective date?
    pub fn list_specs_effective(&self, effective: &DateTimeValue) -> Vec<Arc<LemmaSpec>> {
        let mut seen_names = std::collections::HashSet::new();
        let mut result = Vec::new();
        for spec in self.specs.iter() {
            if seen_names.contains(&spec.name) {
                continue;
            }
            if let Some(active) = self
                .specs
                .spec_sets()
                .get(&spec.name)
                .and_then(|ss| ss.spec_at(&EffectiveDate::DateTimeValue(effective.clone())))
            {
                if seen_names.insert(active.name.clone()) {
                    result.push(active);
                }
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    /// Resolve spec identifier and return the spec schema. Uses `effective` or now when None.
    pub fn schema(
        &self,
        name: &str,
        effective: Option<&DateTimeValue>,
    ) -> Result<SpecSchema, Error> {
        let effective = self.effective_or_now(effective);
        Ok(self.get_plan(name, Some(&effective))?.schema())
    }

    /// Run a spec: resolve by SpecSet id, then [`run_plan`]. Returns all rules; filter via [`Response::filter_rules`] if needed.
    ///
    /// When `record_operations` is true, each rule's [`RuleResult::operations`] will
    /// contain a trace of data used, rules used, computations, and branch evaluations.
    pub fn run(
        &self,
        name: &str,
        effective: Option<&DateTimeValue>,
        data_values: HashMap<String, String>,
        record_operations: bool,
    ) -> Result<Response, Error> {
        let effective = self.effective_or_now(effective);
        let plan = self.get_plan(name, Some(&effective))?;
        self.run_plan(plan, Some(&effective), data_values, record_operations)
    }

    /// Invert a rule to find input domains that produce a desired outcome.
    ///
    /// Values are provided as name -> value string pairs (e.g., "quantity" -> "5").
    /// They are automatically parsed to the expected type based on the spec schema.
    pub fn invert(
        &self,
        name: &str,
        effective: Option<&DateTimeValue>,
        rule_name: &str,
        target: crate::inversion::Target,
        values: HashMap<String, String>,
    ) -> Result<crate::InversionResponse, Error> {
        let effective = self.effective_or_now(effective);
        let base_plan = self.get_plan(name, Some(&effective))?;

        let plan = base_plan.clone().with_data_values(values, &self.limits)?;
        let provided_data: std::collections::HashSet<_> = plan
            .data
            .iter()
            .filter(|(_, d)| d.value().is_some())
            .map(|(p, _)| p.clone())
            .collect();

        crate::inversion::invert(rule_name, target, &plan, &provided_data)
    }

    /// Resolve spec identifier and return the execution plan. Uses `effective` or now when None.
    pub fn get_plan(
        &self,
        name: &str,
        effective: Option<&DateTimeValue>,
    ) -> Result<&crate::planning::ExecutionPlan, Error> {
        let effective = self.effective_or_now(effective);

        if self
            .specs
            .spec_sets()
            .get(name)
            .and_then(|ss| ss.spec_at(&EffectiveDate::DateTimeValue(effective.clone())))
            .is_none()
        {
            return Err(self.spec_not_found_error(name, &effective));
        }

        let plan_set = self.plan_sets.get(name).ok_or_else(|| {
            Error::request_not_found(
                format!("No execution plans for spec '{}'", name),
                Some("Ensure sources loaded and planning succeeded"),
            )
        })?;

        plan_set
            .plan_at(&EffectiveDate::DateTimeValue(effective.clone()))
            .ok_or_else(|| {
                Error::request_not_found(
                    format!(
                        "No execution plan slice for spec '{}' at effective {}",
                        name, effective
                    ),
                    None::<String>,
                )
            })
    }

    /// Run a plan from [`get_plan`]: apply data values and evaluate all rules.
    ///
    /// When `record_operations` is true, each rule's [`RuleResult::operations`] will
    /// contain a trace of data used, rules used, computations, and branch evaluations.
    pub fn run_plan(
        &self,
        plan: &crate::planning::ExecutionPlan,
        effective: Option<&DateTimeValue>,
        data_values: HashMap<String, String>,
        record_operations: bool,
    ) -> Result<Response, Error> {
        let effective = self.effective_or_now(effective);
        let plan = plan.clone().with_data_values(data_values, &self.limits)?;
        self.evaluate_plan(plan, &effective, record_operations)
    }

    pub fn remove(&mut self, name: &str, effective: Option<&DateTimeValue>) -> Result<(), Error> {
        let effective = self.effective_or_now(effective);
        let arc = self.get_spec(name, Some(&effective))?;
        self.specs.remove_spec(&arc);
        let pr = crate::planning::plan(&self.specs);
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

    /// Build a "not found" error listing available specs when the name exists
    /// but no spec covers the requested effective date.
    fn spec_not_found_error(&self, spec_name: &str, effective: &DateTimeValue) -> Error {
        let available = self
            .specs
            .spec_sets()
            .get(spec_name)
            .map(|ss| ss.iter_specs().collect::<Vec<_>>())
            .unwrap_or_default();
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
                "Spec '{}' not found for effective {}. Available specs:\n{}",
                spec_name,
                effective,
                listing.join("\n")
            )
        };
        Error::request_not_found(msg, None::<String>)
    }

    fn evaluate_plan(
        &self,
        plan: crate::planning::ExecutionPlan,
        effective: &DateTimeValue,
        record_operations: bool,
    ) -> Result<Response, Error> {
        let now_semantic = crate::planning::semantics::date_time_to_semantic(effective);
        let now_literal = crate::planning::semantics::LiteralValue {
            value: crate::planning::semantics::ValueKind::Date(now_semantic),
            lemma_type: crate::planning::semantics::primitive_date().clone(),
        };
        Ok(self
            .evaluator
            .evaluate(&plan, now_literal, record_operations))
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
    use rust_decimal::Decimal;
    use std::str::FromStr;

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
        }
    }

    fn make_spec_with_range(name: &str, effective_from: Option<DateTimeValue>) -> LemmaSpec {
        let mut spec = LemmaSpec::new(name.to_string());
        spec.effective_from = crate::parsing::ast::EffectiveDate::from_option(effective_from);
        spec
    }

    /// list_specs (and Context::iter) return specs in (name, effective_from) ascending order.
    /// Same-name specs appear in temporal order; definition order in the file is irrelevant.
    #[test]
    fn list_specs_order_is_name_then_effective_from_ascending() {
        let mut ctx = Context::new();
        let s_2026 = Arc::new(make_spec_with_range("mortgage", Some(date(2026, 1, 1))));
        let s_2025 = Arc::new(make_spec_with_range("mortgage", Some(date(2025, 1, 1))));
        ctx.insert_spec(Arc::clone(&s_2026), false).unwrap();
        ctx.insert_spec(Arc::clone(&s_2025), false).unwrap();
        let listed: Vec<_> = ctx.iter().collect();
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
                SourceType::Labeled("a.lemma"),
            )
            .unwrap();
        engine
            .load(
                r#"
        spec pricing 2025-06-01
        data x: 2
        rule r: x
    "#,
                SourceType::Labeled("b.lemma"),
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
        };

        let s_jan = engine.get_spec("pricing", Some(&jan)).expect("jan spec");
        let s_jul = engine.get_spec("pricing", Some(&jul)).expect("jul spec");
        assert_eq!(s_jan.effective_from(), Some(&v1));
        assert_eq!(s_jul.effective_from(), Some(&v2));
    }

    /// `get_spec_set` exposes every temporal version of a spec name with its
    /// half-open `[effective_from, effective_to)` range. The latest row's
    /// `effective_to` is `None` (no successor); earlier rows' `effective_to`
    /// equals the next row's `effective_from`.
    #[test]
    fn get_spec_set_returns_all_versions_with_half_open_ranges() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec pricing 2025-01-01
        data x: 1
        rule r: x
    "#,
                SourceType::Labeled("a.lemma"),
            )
            .unwrap();
        engine
            .load(
                r#"
        spec pricing 2025-06-01
        data x: 2
        rule r: x
    "#,
                SourceType::Labeled("b.lemma"),
            )
            .unwrap();

        let january = date(2025, 1, 1);
        let june = date(2025, 6, 1);

        let spec_set = engine
            .get_spec_set("pricing")
            .expect("spec set must exist after load");

        let versions: Vec<_> = spec_set
            .iter_specs()
            .map(|spec| spec_set.effective_range(&spec))
            .collect();

        assert_eq!(versions.len(), 2);
        assert_eq!(
            versions[0],
            (Some(january.clone()), Some(june.clone())),
            "earlier row ends at the next row's effective_from"
        );
        assert_eq!(
            versions[1],
            (Some(june.clone()), None),
            "latest row has no successor; effective_to is None"
        );

        assert!(engine.get_spec_set("unknown").is_none());
    }

    /// `Engine::list_specs_with_ranges` flattens every spec set into a flat
    /// list of `(spec, effective_from, effective_to)` triples in the same
    /// order as [`Engine::list_specs`]. This is the canonical flat surface
    /// consumed by language bindings (Hex NIF, WASM) so both `effective_from`
    /// and `effective_to` reach every engine user without a second lookup.
    #[test]
    fn list_specs_with_ranges_flattens_all_spec_sets_with_half_open_ranges() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
        spec pricing 2025-01-01
        data x: 1
        rule r: x
    "#,
                SourceType::Labeled("pricing_v1.lemma"),
            )
            .unwrap();
        engine
            .load(
                r#"
        spec pricing 2026-01-01
        data x: 2
        rule r: x
    "#,
                SourceType::Labeled("pricing_v2.lemma"),
            )
            .unwrap();
        engine
            .load(
                r#"
        spec taxes
        data rate: 0.21
        rule amount: rate
    "#,
                SourceType::Labeled("taxes.lemma"),
            )
            .unwrap();

        let entries = engine.list_specs_with_ranges();
        assert_eq!(
            entries.len(),
            3,
            "one row per loaded spec version across all names"
        );

        let names: Vec<&str> = entries
            .iter()
            .map(|(spec, _, _)| spec.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["pricing", "pricing", "taxes"],
            "ordered by spec name ascending, then by effective_from ascending"
        );

        let (_, pricing_v1_from, pricing_v1_to) = &entries[0];
        assert_eq!(pricing_v1_from, &Some(date(2025, 1, 1)));
        assert_eq!(
            pricing_v1_to,
            &Some(date(2026, 1, 1)),
            "earlier pricing row ends at the next pricing row's effective_from"
        );

        let (_, pricing_v2_from, pricing_v2_to) = &entries[1];
        assert_eq!(pricing_v2_from, &Some(date(2026, 1, 1)));
        assert_eq!(
            pricing_v2_to, &None,
            "latest pricing row has no successor; effective_to is None"
        );

        let (_, taxes_from, taxes_to) = &entries[2];
        assert_eq!(
            taxes_from, &None,
            "unversioned spec has no declared effective_from"
        );
        assert_eq!(
            taxes_to, &None,
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
                SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run("test", Some(&now), HashMap::new(), false)
            .unwrap();
        assert_eq!(response.results.len(), 2);

        let sum_result = response
            .results
            .values()
            .find(|r| r.rule.name == "sum")
            .unwrap();
        assert_eq!(
            sum_result.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("15").unwrap()
            )))
        );

        let product_result = response
            .results
            .values()
            .find(|r| r.rule.name == "product")
            .unwrap();
        assert_eq!(
            product_result.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("50").unwrap()
            )))
        );
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
                SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run("test", Some(&now), HashMap::new(), false)
            .unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("200").unwrap()
            )))
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
                SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run("test", Some(&now), HashMap::new(), false)
            .unwrap();
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::from_bool(true)))
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
                SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run("test", Some(&now), HashMap::new(), false)
            .unwrap();
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("10").unwrap()
            )))
        );
    }

    #[test]
    fn test_spec_not_found() {
        let engine = Engine::new();
        let now = DateTimeValue::now();
        let result = engine.run("nonexistent", Some(&now), HashMap::new(), false);
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
                SourceType::Labeled("spec 1.lemma"),
            )
            .unwrap();

        engine
            .load(
                r#"
        spec spec2
        data y: 5
        rule result: y * 3
    "#,
                SourceType::Labeled("spec 2.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response1 = engine
            .run("spec1", Some(&now), HashMap::new(), false)
            .unwrap();
        assert_eq!(
            response1.results[0].result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("20").unwrap()
            )))
        );

        let response2 = engine
            .run("spec2", Some(&now), HashMap::new(), false)
            .unwrap();
        assert_eq!(
            response2.results[0].result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("15").unwrap()
            )))
        );
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
                SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let result = engine.run("test", Some(&now), HashMap::new(), false);
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
        match &division_result.unwrap().result {
            crate::OperationResult::Veto(crate::VetoType::Computation { message }) => {
                assert!(
                    message.contains("Division by zero"),
                    "Veto message should mention division by zero: {:?}",
                    message
                );
            }
            other => panic!("Expected Veto for division by zero, got {:?}", other),
        }
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
                SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let response = engine
            .run("test", Some(&now), HashMap::new(), false)
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
                SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        // User filters to 'total' after run (deps were still computed)
        let now = DateTimeValue::now();
        let rules = vec!["total".to_string()];
        let mut response = engine
            .run("test", Some(&now), HashMap::new(), false)
            .unwrap();
        response.filter_rules(&rules);

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results.keys().next().unwrap(), "total");

        // But the value should be correct (dependencies were computed)
        let total = response.results.values().next().unwrap();
        assert_eq!(
            total.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("220").unwrap()
            )))
        );
    }

    // -------------------------------------------------------------------
    // Pre-resolved dependency tests (Engine never fetches from registry)
    // -------------------------------------------------------------------

    use crate::parsing::ast::DateTimeValue;

    #[test]
    fn pre_resolved_deps_in_file_map_evaluates_external_spec() {
        let mut engine = Engine::new();

        engine
            .load(
                "spec @org/project/helper\ndata quantity: 42",
                SourceType::Dependency("deps/org_project_helper.lemma"),
            )
            .expect("should load dependency files");

        engine
            .load(
                r#"spec main_spec
with external: @org/project/helper
rule value: external.quantity"#,
                SourceType::Labeled("main.lemma"),
            )
            .expect("should succeed with pre-resolved deps");

        let now = DateTimeValue::now();
        let response = engine
            .run("main_spec", Some(&now), HashMap::new(), false)
            .expect("evaluate should succeed");

        let value_result = response
            .results
            .get("value")
            .expect("rule 'value' should exist");
        assert_eq!(
            value_result.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("42").unwrap()
            )))
        );
    }

    #[test]
    fn load_no_external_refs_works() {
        let mut engine = Engine::new();

        engine
            .load(
                r#"spec local_only
data price: 100
rule doubled: price * 2"#,
                SourceType::Labeled("local.lemma"),
            )
            .expect("should succeed when there are no @... references");

        let now = DateTimeValue::now();
        let response = engine
            .run("local_only", Some(&now), HashMap::new(), false)
            .expect("evaluate should succeed");

        let doubled = response
            .results
            .get("doubled")
            .expect("doubled rule")
            .result
            .value()
            .expect("value");
        assert_eq!(doubled.to_string(), "200");
    }

    #[test]
    fn unresolved_external_ref_without_deps_fails() {
        let mut engine = Engine::new();

        let result = engine.load(
            r#"spec main_spec
with external: @org/project/missing
rule value: external.quantity"#,
            SourceType::Labeled("main.lemma"),
        );

        let errs = result.expect_err("Should fail when @... dep is not in file map");
        let msg = errs
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            msg.contains("missing") || msg.contains("not found") || msg.contains("Unknown"),
            "error should indicate missing dep: {msg}"
        );
    }

    #[test]
    fn pre_resolved_deps_with_spec_and_type_refs() {
        let mut engine = Engine::new();

        let mut deps = HashMap::new();
        deps.insert(
            "deps/helper.lemma".to_string(),
            "spec @org/example/helper\ndata value: 42".to_string(),
        );
        deps.insert(
            "deps/finance.lemma".to_string(),
            "spec @lemma/std/finance\ndata money: scale\n -> unit eur 1.00\n -> decimals 2"
                .to_string(),
        );
        engine
            .load(
                "spec @org/example/helper\ndata value: 42",
                SourceType::Dependency("deps/helper.lemma"),
            )
            .expect("should load helper file");

        engine
            .load(
                "spec @lemma/std/finance\ndata money: scale\n -> unit eur 1.00\n -> decimals 2",
                SourceType::Dependency("deps/finance.lemma"),
            )
            .expect("should load finance file");

        engine
            .load(
                r#"spec registry_demo
data money from @lemma/std/finance
data unit_price: 5 eur
with @org/example/helper
rule helper_value: helper.value
rule line_total: unit_price * 2
rule formatted: helper_value + 0"#,
                SourceType::Labeled("main.lemma"),
            )
            .expect("should succeed with pre-resolved spec and type deps");

        let now = DateTimeValue::now();
        let response = engine
            .run("registry_demo", Some(&now), HashMap::new(), false)
            .expect("evaluate should succeed");

        assert_eq!(
            response
                .results
                .get("helper_value")
                .expect("helper_value")
                .result
                .value()
                .expect("value")
                .to_string(),
            "42"
        );
        let line = response
            .results
            .get("line_total")
            .expect("line_total")
            .result
            .value()
            .expect("value")
            .to_string();
        assert!(
            line.contains("10") && line.to_lowercase().contains("eur"),
            "5 eur * 2 => ~10 eur, got {line}"
        );
        assert_eq!(
            response
                .results
                .get("formatted")
                .expect("formatted")
                .result
                .value()
                .expect("value")
                .to_string(),
            "42"
        );
    }

    #[test]
    fn load_empty_labeled_source_is_error() {
        let mut engine = Engine::new();
        let err = engine
            .load("spec x\ndata a: 1", SourceType::Labeled("  "))
            .unwrap_err();
        assert!(err.errors.iter().any(|e| e.message().contains("non-empty")));
    }

    #[test]
    fn load_rejects_registry_spec_definitions() {
        let mut engine = Engine::new();
        let result = engine.load(
            "spec @org/example/helper\ndata x: 1",
            SourceType::Labeled("bad.lemma"),
        );
        assert!(result.is_err(), "should reject @-prefixed spec in load");
        let errors = result.unwrap_err();
        assert!(
            errors
                .errors
                .iter()
                .any(|e| e.message().contains("registry prefix")),
            "error should mention registry prefix, got: {:?}",
            errors
        );
    }

    #[test]
    fn add_dependency_files_accepts_registry_spec_definitions() {
        let mut engine = Engine::new();
        let mut files = HashMap::new();
        files.insert(
            "deps/helper.lemma".to_string(),
            "spec @org/my/helper\ndata x: 1".to_string(),
        );
        engine
            .load(
                "spec @org/my/helper\ndata x: 1",
                SourceType::Dependency("helper.lemma"),
            )
            .expect("add_dependency_files should accept @-prefixed specs");
    }

    #[test]
    fn add_dependency_files_rejects_bare_named_spec_in_registry_bundle() {
        let mut engine = Engine::new();
        let result = engine.load(
            "spec local_looking_name\ndata x: 1",
            SourceType::Dependency("bundle.lemma"),
        );
        assert!(
            result.is_err(),
            "should reject non-@-prefixed spec in registry bundle"
        );
        let errors = result.unwrap_err();
        assert!(
            errors
                .errors
                .iter()
                .any(|e| e.message().contains("without '@' prefix")),
            "error should mention missing @ prefix, got: {:?}",
            errors
        );
    }

    #[test]
    fn add_dependency_files_rejects_spec_with_bare_spec_reference() {
        let mut engine = Engine::new();
        let result = engine.load(
            "spec @org/billing\nwith rates: local_rates",
            SourceType::Dependency("billing.lemma"),
        );
        assert!(
            result.is_err(),
            "should reject registry spec referencing non-@ spec"
        );
        let errors = result.unwrap_err();
        assert!(
            errors
                .errors
                .iter()
                .any(|e| e.message().contains("local_rates")),
            "error should mention bare ref name, got: {:?}",
            errors
        );
    }

    #[test]
    fn add_dependency_files_rejects_spec_with_bare_type_import() {
        let mut engine = Engine::new();
        let result = engine.load(
            "spec @org/billing\ndata money from local_finance",
            SourceType::Dependency("billing.lemma"),
        );
        assert!(
            result.is_err(),
            "should reject registry spec importing type from non-@ spec"
        );
        let errors = result.unwrap_err();
        assert!(
            errors
                .errors
                .iter()
                .any(|e| e.message().contains("local_finance")),
            "error should mention bare ref name, got: {:?}",
            errors
        );
    }

    #[test]
    fn add_dependency_files_accepts_fully_qualified_references() {
        let mut engine = Engine::new();
        let mut files = HashMap::new();
        files.insert(
            "deps/bundle.lemma".to_string(),
            r#"spec @org/billing
with @org/rates

spec @org/rates
data rate: 10"#
                .to_string(),
        );
        engine
            .load(
                r#"spec @org/billing
with @org/rates

spec @org/rates
data rate: 10"#,
                SourceType::Dependency("bundle.lemma"),
            )
            .expect("fully @-prefixed bundle should be accepted");
    }

    #[test]
    fn load_returns_all_errors_not_just_first() {
        let mut engine = Engine::new();

        let result = engine.load(
            r#"spec demo
data money from nonexistent_type_source
with helper: nonexistent_spec
data price: 10
rule total: helper.value + price"#,
            SourceType::Labeled("test.lemma"),
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
            "Should mention type import source spec. Got:\n{}",
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
            SourceType::Labeled("t.lemma"),
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
            SourceType::Labeled("t.lemma"),
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
            SourceType::Labeled("t.lemma"),
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
        let result = engine.load("spec t\ndata custom: number -> minimum 0\ndata x: [custom -> default \"abc\"]\nrule r: x", SourceType::Labeled("t.lemma",));
        assert!(
            result.is_err(),
            "must reject non-numeric default on named number type"
        );
    }
}
