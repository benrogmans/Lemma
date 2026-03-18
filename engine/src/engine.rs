use crate::evaluation::Evaluator;
use crate::parsing::ast::{DateTimeValue, LemmaSpec};
use crate::planning::SpecSchema;
use crate::spec_id;
use crate::{parse, Error, ResourceLimits, Response};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashSet;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

// ─── Temporal bound for Option<DateTimeValue> comparisons ────────────

/// Explicit representation of a temporal bound, eliminating the ambiguity
/// of `Option<DateTimeValue>` where `None` means `-∞` for start bounds
/// and `+∞` for end bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TemporalBound {
    NegInf,
    At(DateTimeValue),
    PosInf,
}

impl PartialOrd for TemporalBound {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TemporalBound {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (TemporalBound::NegInf, TemporalBound::NegInf) => Ordering::Equal,
            (TemporalBound::NegInf, _) => Ordering::Less,
            (_, TemporalBound::NegInf) => Ordering::Greater,
            (TemporalBound::PosInf, TemporalBound::PosInf) => Ordering::Equal,
            (TemporalBound::PosInf, _) => Ordering::Greater,
            (_, TemporalBound::PosInf) => Ordering::Less,
            (TemporalBound::At(a), TemporalBound::At(b)) => a.cmp(b),
        }
    }
}

impl TemporalBound {
    /// Convert an `Option<&DateTimeValue>` used as a start bound (None = -∞).
    pub(crate) fn from_start(opt: Option<&DateTimeValue>) -> Self {
        match opt {
            None => TemporalBound::NegInf,
            Some(d) => TemporalBound::At(d.clone()),
        }
    }

    /// Convert an `Option<&DateTimeValue>` used as an end bound (None = +∞).
    pub(crate) fn from_end(opt: Option<&DateTimeValue>) -> Self {
        match opt {
            None => TemporalBound::PosInf,
            Some(d) => TemporalBound::At(d.clone()),
        }
    }

    /// Convert back to `Option<DateTimeValue>` for a start bound (NegInf → None).
    pub(crate) fn to_start(&self) -> Option<DateTimeValue> {
        match self {
            TemporalBound::NegInf => None,
            TemporalBound::At(d) => Some(d.clone()),
            TemporalBound::PosInf => {
                unreachable!("BUG: PosInf cannot represent a start bound")
            }
        }
    }

    /// Convert back to `Option<DateTimeValue>` for an end bound (PosInf → None).
    pub(crate) fn to_end(&self) -> Option<DateTimeValue> {
        match self {
            TemporalBound::NegInf => {
                unreachable!("BUG: NegInf cannot represent an end bound")
            }
            TemporalBound::At(d) => Some(d.clone()),
            TemporalBound::PosInf => None,
        }
    }
}

// ─── Spec store with temporal resolution ──────────────────────────────

/// Ordered set of specs with temporal versioning.
///
/// Specs with the same name are ordered by effective_from.
/// A temporal version's end is derived from the next temporal version's effective_from, or +inf.
#[derive(Debug, Default)]
pub struct Context {
    specs: BTreeSet<Arc<LemmaSpec>>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            specs: BTreeSet::new(),
        }
    }

    pub(crate) fn specs_for_name(&self, name: &str) -> Vec<Arc<LemmaSpec>> {
        self.specs
            .iter()
            .filter(|a| a.name == name)
            .cloned()
            .collect()
    }

    /// Exact identity lookup by (name, effective_from).
    ///
    /// None matches specs without temporal versioning.
    /// Some(d) matches the temporal version whose effective_from equals d.
    pub fn get_spec_effective_from(
        &self,
        name: &str,
        effective_from: Option<&DateTimeValue>,
    ) -> Option<Arc<LemmaSpec>> {
        self.specs_for_name(name)
            .into_iter()
            .find(|s| s.effective_from() == effective_from)
    }

    /// Temporal range resolution: find the temporal version of `name` that is active at `effective`.
    ///
    /// A spec is active at `effective` when:
    ///   effective_from <= effective < effective_to
    /// where effective_to is the next temporal version's effective_from, or +inf if no successor.
    pub fn get_spec(&self, name: &str, effective: &DateTimeValue) -> Option<Arc<LemmaSpec>> {
        let versions = self.specs_for_name(name);
        if versions.is_empty() {
            return None;
        }

        for (i, spec) in versions.iter().enumerate() {
            let from_ok = spec
                .effective_from()
                .map(|f| *effective >= *f)
                .unwrap_or(true);
            if !from_ok {
                continue;
            }

            let effective_to: Option<&DateTimeValue> =
                versions.get(i + 1).and_then(|next| next.effective_from());
            let to_ok = effective_to.map(|end| *effective < *end).unwrap_or(true);

            if to_ok {
                return Some(spec.clone());
            }
        }

        None
    }

    pub fn iter(&self) -> impl Iterator<Item = Arc<LemmaSpec>> + '_ {
        self.specs.iter().cloned()
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

        let existing = self.specs_for_name(&spec.name);

        if existing
            .iter()
            .any(|o| o.effective_from() == spec.effective_from())
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

        self.specs.insert(spec);
        Ok(())
    }

    pub fn remove_spec(&mut self, spec: &Arc<LemmaSpec>) -> bool {
        self.specs.remove(spec)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.specs.len()
    }

    // ─── Temporal helpers ────────────────────────────────────────────

    /// Returns the effective range `[from, to)` for a spec in this context.
    ///
    /// - `from`: `spec.effective_from()` (None = -∞)
    /// - `to`: next temporal version's `effective_from`, or None (+∞) if no successor.
    pub fn effective_range(
        &self,
        spec: &Arc<LemmaSpec>,
    ) -> (Option<DateTimeValue>, Option<DateTimeValue>) {
        let from = spec.effective_from().cloned();
        let versions = self.specs_for_name(&spec.name);
        let pos = versions
            .iter()
            .position(|v| Arc::ptr_eq(v, spec))
            .unwrap_or_else(|| {
                unreachable!(
                    "BUG: effective_range called with spec '{}' not in context",
                    spec.name
                )
            });
        let to = versions
            .get(pos + 1)
            .and_then(|next| next.effective_from().cloned());
        (from, to)
    }

    /// Returns all `effective_from` dates for temporal versions of `name`, sorted ascending.
    /// Temporal versions without `effective_from` are excluded (they represent -∞).
    pub fn version_boundaries(&self, name: &str) -> Vec<DateTimeValue> {
        self.specs_for_name(name)
            .iter()
            .filter_map(|s| s.effective_from().cloned())
            .collect()
    }

    /// Check if temporal versions of `dep_name` fully cover the range
    /// `[required_from, required_to)`.
    ///
    /// Returns gaps as `(start, end)` intervals. Empty vec = fully covered.
    /// Start: None = -∞, End: None = +∞.
    pub fn dep_coverage_gaps(
        &self,
        dep_name: &str,
        required_from: Option<&DateTimeValue>,
        required_to: Option<&DateTimeValue>,
    ) -> Vec<(Option<DateTimeValue>, Option<DateTimeValue>)> {
        let versions = self.specs_for_name(dep_name);
        if versions.is_empty() {
            return vec![(required_from.cloned(), required_to.cloned())];
        }

        let req_start = TemporalBound::from_start(required_from);
        let req_end = TemporalBound::from_end(required_to);

        let intervals: Vec<(TemporalBound, TemporalBound)> = versions
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let start = TemporalBound::from_start(v.effective_from());
                let end = match versions.get(i + 1).and_then(|next| next.effective_from()) {
                    Some(next_from) => TemporalBound::At(next_from.clone()),
                    None => TemporalBound::PosInf,
                };
                (start, end)
            })
            .collect();

        let mut gaps = Vec::new();
        let mut cursor = req_start.clone();

        for (v_start, v_end) in &intervals {
            if cursor >= req_end {
                break;
            }

            if *v_end <= cursor {
                continue;
            }

            if *v_start > cursor {
                let gap_end = std::cmp::min(v_start.clone(), req_end.clone());
                if cursor < gap_end {
                    gaps.push((cursor.to_start(), gap_end.to_end()));
                }
            }

            if *v_end > cursor {
                cursor = v_end.clone();
            }
        }

        if cursor < req_end {
            gaps.push((cursor.to_start(), req_end.to_end()));
        }

        gaps
    }
}

// ─── Slice plan lookup ───────────────────────────────────────────────

/// Find the plan whose `[valid_from, valid_to)` interval contains `effective`.
fn find_slice_plan<'a>(
    plans: &'a [crate::planning::ExecutionPlan],
    effective: &DateTimeValue,
) -> Option<&'a crate::planning::ExecutionPlan> {
    for plan in plans {
        let from_ok = plan
            .valid_from
            .as_ref()
            .map(|f| *effective >= *f)
            .unwrap_or(true);
        let to_ok = plan
            .valid_to
            .as_ref()
            .map(|t| *effective < *t)
            .unwrap_or(true);
        if from_ok && to_ok {
            return Some(plan);
        }
    }
    None
}

// ─── Engine ──────────────────────────────────────────────────────────

/// How a single buffer is identified in parse/plan diagnostics and the engine source map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadSource<'a> {
    /// Path, URI, test name, or any non-empty stable id.
    Labeled(&'a str),
    /// No stable path (pasted string, REPL). Stored under [`LoadSource::INLINE_KEY`].
    Inline,
}

impl LoadSource<'_> {
    /// Source map key and span attribute for [`LoadSource::Inline`].
    pub const INLINE_KEY: &'static str = "inline source (no path)";

    fn storage_key(self) -> Result<String, Vec<Error>> {
        match self {
            LoadSource::Labeled(s) => {
                if s.trim().is_empty() {
                    return Err(vec![Error::request(
                        "load source label must be non-empty, or use LoadSource::Inline",
                        None::<String>,
                        None,
                    )]);
                }
                Ok(s.to_string())
            }
            LoadSource::Inline => Ok(Self::INLINE_KEY.to_string()),
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
    execution_plans: HashMap<Arc<LemmaSpec>, Vec<crate::planning::ExecutionPlan>>,
    specs: Context,
    sources: HashMap<String, String>,
    evaluator: Evaluator,
    limits: ResourceLimits,
    hash_pins: HashMap<Arc<LemmaSpec>, String>,
    total_expression_count: usize,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            execution_plans: HashMap::new(),
            specs: Context::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits: ResourceLimits::default(),
            hash_pins: HashMap::new(),
            total_expression_count: 0,
        }
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an engine with custom resource limits.
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            execution_plans: HashMap::new(),
            specs: Context::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits,
            hash_pins: HashMap::new(),
            total_expression_count: 0,
        }
    }

    /// Get the content hash (hash pin) for the temporal version active at `effective`.
    pub fn hash_pin(&self, spec_name: &str, effective: &DateTimeValue) -> Option<&str> {
        let spec_arc = self.get_spec(spec_name, effective)?;
        self.hash_pin_for_spec(&spec_arc)
    }

    /// Get the content hash for a specific spec (by arc). Used when the resolved spec is already known.
    pub fn hash_pin_for_spec(&self, spec: &Arc<LemmaSpec>) -> Option<&str> {
        self.hash_pins.get(spec).map(|s| s.as_str())
    }

    /// Get all hash pins as (spec_name, effective_from_display, hash) triples.
    pub fn all_hash_pins(&self) -> Vec<(&str, Option<String>, &str)> {
        self.hash_pins
            .iter()
            .map(|(spec, hash)| {
                (
                    spec.name.as_str(),
                    spec.effective_from().map(|af| af.to_string()),
                    hash.as_str(),
                )
            })
            .collect()
    }

    /// Get the spec with the given name whose content hash matches `hash_pin`.
    /// Returns `None` if no such spec exists or if multiple versions match (hash collision).
    pub fn get_spec_by_hash_pin(&self, spec_name: &str, hash_pin: &str) -> Option<Arc<LemmaSpec>> {
        let mut matched: Option<Arc<LemmaSpec>> = None;
        for spec in self.specs.specs_for_name(spec_name) {
            let computed = match self.hash_pins.get(&spec) {
                Some(h) => h.as_str(),
                None => continue,
            };
            if crate::planning::content_hash::content_hash_matches(hash_pin, computed) {
                if matched.is_some() {
                    return None;
                }
                matched = Some(spec);
            }
        }
        matched
    }

    /// Load a single spec from source code.
    pub fn load(&mut self, code: &str, source: LoadSource<'_>) -> Result<(), Vec<Error>> {
        let mut files = HashMap::new();
        files.insert(source.storage_key()?, code.to_string());
        self.add_files_inner(files, false)
    }

    /// Load .lemma files from paths (files and/or directories). Directories are expanded one level only (direct child .lemma files). Enforces `max_files`, `max_loaded_bytes`, `max_file_size_bytes`. Not available on wasm32 (no filesystem).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from_paths<P: AsRef<Path>>(&mut self, paths: &[P]) -> Result<(), Vec<Error>> {
        use std::fs;
        use std::io::Read;

        let mut to_load: Vec<(String, String)> = Vec::new();
        let mut total_bytes: usize = 0;
        let mut seen = HashSet::<String>::new();

        for path in paths {
            let path = path.as_ref();
            if path.is_file() {
                if path.extension().map(|e| e == "lemma").unwrap_or(false) {
                    let key = path.display().to_string();
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.insert(key.clone());
                    if to_load.len() >= self.limits.max_files {
                        return Err(vec![Error::resource_limit_exceeded(
                            "max_files",
                            self.limits.max_files.to_string(),
                            (to_load.len() + 1).to_string(),
                            "Reduce the number of paths or files",
                            None::<crate::Source>,
                            None,
                            None,
                        )]);
                    }
                    let meta = match fs::metadata(path) {
                        Ok(m) => m,
                        Err(e) => {
                            return Err(vec![Error::request(
                                format!("Cannot read path '{}': {}", path.display(), e),
                                None::<String>,
                                None,
                            )]);
                        }
                    };
                    if meta.len() as usize > self.limits.max_file_size_bytes {
                        return Err(vec![Error::resource_limit_exceeded(
                            "max_file_size_bytes",
                            self.limits.max_file_size_bytes.to_string(),
                            meta.len().to_string(),
                            "Use a smaller file or increase limit",
                            None::<crate::Source>,
                            None,
                            None,
                        )]);
                    }
                    total_bytes += meta.len() as usize;
                    if total_bytes > self.limits.max_loaded_bytes {
                        return Err(vec![Error::resource_limit_exceeded(
                            "max_loaded_bytes",
                            self.limits.max_loaded_bytes.to_string(),
                            total_bytes.to_string(),
                            "Load fewer or smaller files",
                            None::<crate::Source>,
                            None,
                            None,
                        )]);
                    }
                    let mut f = match fs::File::open(path) {
                        Ok(f) => f,
                        Err(e) => {
                            return Err(vec![Error::request(
                                format!("Cannot open '{}': {}", path.display(), e),
                                None::<String>,
                                None,
                            )]);
                        }
                    };
                    let mut s = String::new();
                    if f.read_to_string(&mut s).is_err() {
                        return Err(vec![Error::request(
                            format!("Cannot read '{}'", path.display()),
                            None::<String>,
                            None,
                        )]);
                    }
                    to_load.push((key, s));
                }
            } else if path.is_dir() {
                let read_dir = match fs::read_dir(path) {
                    Ok(d) => d,
                    Err(e) => {
                        return Err(vec![Error::request(
                            format!("Cannot read directory '{}': {}", path.display(), e),
                            None::<String>,
                            None,
                        )]);
                    }
                };
                for entry in read_dir.filter_map(Result::ok) {
                    let p = entry.path();
                    if p.is_file() && p.extension().map(|e| e == "lemma").unwrap_or(false) {
                        let key = p.display().to_string();
                        if seen.contains(&key) {
                            continue;
                        }
                        seen.insert(key.clone());
                        if to_load.len() >= self.limits.max_files {
                            return Err(vec![Error::resource_limit_exceeded(
                                "max_files",
                                self.limits.max_files.to_string(),
                                (to_load.len() + 1).to_string(),
                                "Reduce the number of paths or files",
                                None::<crate::Source>,
                                None,
                                None,
                            )]);
                        }
                        let meta = match fs::metadata(&p) {
                            Ok(m) => m,
                            Err(_) => continue,
                        };
                        if meta.len() as usize > self.limits.max_file_size_bytes {
                            return Err(vec![Error::resource_limit_exceeded(
                                "max_file_size_bytes",
                                self.limits.max_file_size_bytes.to_string(),
                                meta.len().to_string(),
                                "Use a smaller file or increase limit",
                                None::<crate::Source>,
                                None,
                                None,
                            )]);
                        }
                        total_bytes += meta.len() as usize;
                        if total_bytes > self.limits.max_loaded_bytes {
                            return Err(vec![Error::resource_limit_exceeded(
                                "max_loaded_bytes",
                                self.limits.max_loaded_bytes.to_string(),
                                total_bytes.to_string(),
                                "Load fewer or smaller files",
                                None::<crate::Source>,
                                None,
                                None,
                            )]);
                        }
                        let mut f = match fs::File::open(&p) {
                            Ok(f) => f,
                            Err(_) => continue,
                        };
                        let mut s = String::new();
                        if f.read_to_string(&mut s).is_err() {
                            continue;
                        }
                        to_load.push((key, s));
                    }
                }
            }
        }

        let files: HashMap<String, String> = to_load.into_iter().collect();
        self.add_files_inner(files, false)
    }

    /// Add pre-fetched registry dependency files. These are allowed to declare
    /// `@`-prefixed spec names. Call this before [`load`] / [`load_from_paths`] so that
    /// user specs can reference the imported types and facts.
    pub fn add_dependency_files(
        &mut self,
        files: HashMap<String, String>,
    ) -> Result<(), Vec<Error>> {
        self.add_files_inner(files, true)
    }

    fn add_files_inner(
        &mut self,
        files: HashMap<String, String>,
        allow_registry_specs: bool,
    ) -> Result<(), Vec<Error>> {
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
                        return Err(errors);
                    }
                    let new_specs = result.specs;
                    let source_text: Arc<str> = Arc::from(code.as_str());
                    for spec in new_specs {
                        let attribute = spec.attribute.clone().unwrap_or_else(|| spec.name.clone());
                        let start_line = spec.start_line;

                        if allow_registry_specs {
                            let bare_refs =
                                crate::planning::validation::collect_bare_registry_refs(&spec);
                            if !bare_refs.is_empty() {
                                let source = crate::Source::new(
                                    &attribute,
                                    crate::parsing::ast::Span {
                                        start: 0,
                                        end: 0,
                                        line: start_line,
                                        col: 0,
                                    },
                                    Arc::clone(&source_text),
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

                        match self.specs.insert_spec(Arc::new(spec), allow_registry_specs) {
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
                                    Arc::clone(&source_text),
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

        let planning_result = crate::planning::plan(&self.specs, self.sources.clone());
        for spec_result in &planning_result.per_spec {
            self.execution_plans
                .insert(Arc::clone(&spec_result.spec), spec_result.plans.clone());
            self.hash_pins
                .insert(Arc::clone(&spec_result.spec), spec_result.hash_pin.clone());
        }
        errors.extend(planning_result.global_errors);
        for spec_result in planning_result.per_spec {
            for err in spec_result.errors {
                errors.push(err.with_spec_context(Arc::clone(&spec_result.spec)));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn remove_spec(&mut self, spec: Arc<LemmaSpec>) {
        self.execution_plans.remove(&spec);
        self.specs.remove_spec(&spec);
    }

    /// All specs, all temporal versions, ordered by (name, effective_from).
    pub fn list_specs(&self) -> Vec<Arc<LemmaSpec>> {
        self.specs.iter().collect()
    }

    /// Specs active at `effective` (one per name).
    pub fn list_specs_effective(&self, effective: &DateTimeValue) -> Vec<Arc<LemmaSpec>> {
        let mut seen_names = std::collections::HashSet::new();
        let mut result = Vec::new();
        for spec in self.specs.iter() {
            if seen_names.contains(&spec.name) {
                continue;
            }
            if let Some(active) = self.specs.get_spec(&spec.name, effective) {
                if seen_names.insert(active.name.clone()) {
                    result.push(active);
                }
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    /// Get spec by name at a specific time.
    pub fn get_spec(
        &self,
        spec_name: &str,
        effective: &DateTimeValue,
    ) -> Option<std::sync::Arc<LemmaSpec>> {
        self.specs.get_spec(spec_name, effective)
    }

    /// Build a "not found" error that includes the effective date and lists
    /// available temporal versions when the spec name exists but no temporal version
    /// matches the requested time.
    fn spec_not_found_error(&self, spec_name: &str, effective: &DateTimeValue) -> Error {
        let versions = self.specs.specs_for_name(spec_name);
        let msg = if versions.is_empty() {
            format!("Spec '{}' not found", spec_name)
        } else {
            let version_list: Vec<String> = versions
                .iter()
                .map(|s| match s.effective_from() {
                    Some(dt) => format!("  {} (effective from {})", s.name, dt),
                    None => format!("  {} (no effective_from)", s.name),
                })
                .collect();
            format!(
                "Spec '{}' not found for effective {}. Available temporal versions:\n{}",
                spec_name,
                effective,
                version_list.join("\n")
            )
        };
        Error::request(msg, None::<String>, None)
    }

    /// Resolve spec identifier (name or name~hash) and return the spec schema. Uses `effective` or now when None.
    pub fn show(&self, spec: &str, effective: Option<&DateTimeValue>) -> Result<SpecSchema, Error> {
        let plan = self.plan(spec, effective)?;
        Ok(plan.schema())
    }

    /// Resolve spec identifier and return the execution plan. Uses `effective` or now when None.
    pub fn plan(
        &self,
        spec: &str,
        effective: Option<&DateTimeValue>,
    ) -> Result<&crate::planning::ExecutionPlan, Error> {
        let (name, hash) = spec_id::parse_spec_id(spec)?;
        let eff_val = effective.cloned().unwrap_or_else(DateTimeValue::now);
        let arc = hash
            .as_ref()
            .and_then(|pin| self.get_spec_by_hash_pin(&name, pin))
            .or_else(|| self.get_spec(&name, &eff_val))
            .ok_or_else(|| self.spec_not_found_error(&name, &eff_val))?;
        let slice_plans = self
            .execution_plans
            .get(&arc)
            .ok_or_else(|| self.spec_not_found_error(&name, &eff_val))?;
        let plan = find_slice_plan(slice_plans, &eff_val);
        if let Some(p) = plan {
            Ok(p)
        } else {
            if !slice_plans.is_empty() {
                unreachable!(
                    "BUG: spec '{}' has {} slice plans but none covers effective={} — slice partition is broken",
                    name, slice_plans.len(), eff_val
                );
            }
            Err(self.spec_not_found_error(&name, &eff_val))
        }
    }

    /// Run a plan from [`plan`]: apply fact values and evaluate all rules.
    pub fn run_plan(
        &self,
        plan: &crate::planning::ExecutionPlan,
        effective: &DateTimeValue,
        fact_values: HashMap<String, String>,
    ) -> Result<Response, Error> {
        let plan = plan.clone().with_fact_values(fact_values, &self.limits)?;
        self.evaluate_plan(plan, effective)
    }

    /// Run a spec: resolve by spec id, then [`run_plan`]. Returns all rules; filter via [`Response::filter_rules`] if needed.
    pub fn run(
        &self,
        spec: &str,
        effective: Option<&DateTimeValue>,
        fact_values: HashMap<String, String>,
    ) -> Result<Response, Error> {
        let eff_val = effective.cloned().unwrap_or_else(DateTimeValue::now);
        let plan = self.plan(spec, effective)?;
        self.run_plan(plan, &eff_val, fact_values)
    }

    pub fn get_spec_rules(
        &self,
        spec_name: &str,
        effective: &DateTimeValue,
    ) -> Result<Vec<crate::LemmaRule>, Error> {
        let arc = self
            .get_spec(spec_name, effective)
            .ok_or_else(|| self.spec_not_found_error(spec_name, effective))?;
        Ok(arc.rules.clone())
    }

    /// Run with fact values from JSON body. Same spec id rules as [`run`].
    pub fn run_json(
        &self,
        spec: &str,
        effective: Option<&DateTimeValue>,
        json: &[u8],
    ) -> Result<Response, Error> {
        let eff_val = effective.cloned().unwrap_or_else(DateTimeValue::now);
        let plan = self.plan(spec, effective)?;
        let values = crate::serialization::from_json(json)?;
        self.run_plan(plan, &eff_val, values)
    }

    /// Invert a rule to find input domains that produce a desired outcome.
    ///
    /// Values are provided as name -> value string pairs (e.g., "quantity" -> "5").
    /// They are automatically parsed to the expected type based on the spec schema.
    pub fn invert(
        &self,
        spec_name: &str,
        effective: &DateTimeValue,
        rule_name: &str,
        target: crate::inversion::Target,
        values: HashMap<String, String>,
    ) -> Result<crate::InversionResponse, Error> {
        let base_plan = self.plan(spec_name, Some(effective))?;

        let plan = base_plan.clone().with_fact_values(values, &self.limits)?;
        let provided_facts: std::collections::HashSet<_> = plan
            .facts
            .iter()
            .filter(|(_, d)| d.value().is_some())
            .map(|(p, _)| p.clone())
            .collect();

        crate::inversion::invert(rule_name, target, &plan, &provided_facts)
    }

    fn evaluate_plan(
        &self,
        plan: crate::planning::ExecutionPlan,
        effective: &DateTimeValue,
    ) -> Result<Response, Error> {
        let now_semantic = crate::planning::semantics::date_time_to_semantic(effective);
        let now_literal = crate::planning::semantics::LiteralValue {
            value: crate::planning::semantics::ValueKind::Date(now_semantic),
            lemma_type: crate::planning::semantics::primitive_date().clone(),
        };
        Ok(self.evaluator.evaluate(&plan, now_literal))
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

    fn make_spec(name: &str) -> LemmaSpec {
        LemmaSpec::new(name.to_string())
    }

    fn make_spec_with_range(name: &str, effective_from: Option<DateTimeValue>) -> LemmaSpec {
        let mut spec = LemmaSpec::new(name.to_string());
        spec.effective_from = effective_from;
        spec
    }

    /// list_specs (and Context::iter) return specs in (name, effective_from) ascending order.
    /// So same-name temporal versions appear in temporal order; definition order in the file
    /// is irrelevant once inserted into the BTreeSet.
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

    // ─── Context::effective_range tests ──────────────────────────────

    #[test]
    fn effective_range_unbounded_single_version() {
        let mut ctx = Context::new();
        let spec = Arc::new(make_spec("a"));
        ctx.insert_spec(Arc::clone(&spec), false).unwrap();

        let (from, to) = ctx.effective_range(&spec);
        assert_eq!(from, None);
        assert_eq!(to, None);
    }

    #[test]
    fn effective_range_soft_end_from_next_version() {
        let mut ctx = Context::new();
        let v1 = Arc::new(make_spec_with_range("a", Some(date(2025, 1, 1))));
        let v2 = Arc::new(make_spec_with_range("a", Some(date(2025, 6, 1))));
        ctx.insert_spec(Arc::clone(&v1), false).unwrap();
        ctx.insert_spec(Arc::clone(&v2), false).unwrap();

        let (from, to) = ctx.effective_range(&v1);
        assert_eq!(from, Some(date(2025, 1, 1)));
        assert_eq!(to, Some(date(2025, 6, 1)));

        let (from, to) = ctx.effective_range(&v2);
        assert_eq!(from, Some(date(2025, 6, 1)));
        assert_eq!(to, None);
    }

    #[test]
    fn effective_range_unbounded_start_with_successor() {
        let mut ctx = Context::new();
        let v1 = Arc::new(make_spec("a"));
        let v2 = Arc::new(make_spec_with_range("a", Some(date(2025, 3, 1))));
        ctx.insert_spec(Arc::clone(&v1), false).unwrap();
        ctx.insert_spec(Arc::clone(&v2), false).unwrap();

        let (from, to) = ctx.effective_range(&v1);
        assert_eq!(from, None);
        assert_eq!(to, Some(date(2025, 3, 1)));
    }

    // ─── Context::version_boundaries tests ───────────────────────────

    #[test]
    fn version_boundaries_single_unversioned() {
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::new(make_spec("a")), false).unwrap();

        assert!(ctx.version_boundaries("a").is_empty());
    }

    #[test]
    fn version_boundaries_multiple_versions() {
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::new(make_spec("a")), false).unwrap();
        ctx.insert_spec(
            Arc::new(make_spec_with_range("a", Some(date(2025, 3, 1)))),
            false,
        )
        .unwrap();
        ctx.insert_spec(
            Arc::new(make_spec_with_range("a", Some(date(2025, 6, 1)))),
            false,
        )
        .unwrap();

        let boundaries = ctx.version_boundaries("a");
        assert_eq!(boundaries, vec![date(2025, 3, 1), date(2025, 6, 1)]);
    }

    #[test]
    fn version_boundaries_nonexistent_name() {
        let ctx = Context::new();
        assert!(ctx.version_boundaries("nope").is_empty());
    }

    // ─── Context::dep_coverage_gaps tests ────────────────────────────

    #[test]
    fn dep_coverage_no_versions_is_full_gap() {
        let ctx = Context::new();
        let gaps =
            ctx.dep_coverage_gaps("missing", Some(&date(2025, 1, 1)), Some(&date(2025, 6, 1)));
        assert_eq!(gaps, vec![(Some(date(2025, 1, 1)), Some(date(2025, 6, 1)))]);
    }

    #[test]
    fn dep_coverage_single_unbounded_version_covers_everything() {
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::new(make_spec("dep")), false).unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", None, None);
        assert!(gaps.is_empty());

        let gaps = ctx.dep_coverage_gaps("dep", Some(&date(2025, 1, 1)), Some(&date(2025, 12, 1)));
        assert!(gaps.is_empty());
    }

    #[test]
    fn dep_coverage_single_version_with_from_leaves_leading_gap() {
        let mut ctx = Context::new();
        ctx.insert_spec(
            Arc::new(make_spec_with_range("dep", Some(date(2025, 3, 1)))),
            false,
        )
        .unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", None, None);
        assert_eq!(gaps, vec![(None, Some(date(2025, 3, 1)))]);
    }

    #[test]
    fn dep_coverage_continuous_versions_no_gaps() {
        let mut ctx = Context::new();
        ctx.insert_spec(
            Arc::new(make_spec_with_range("dep", Some(date(2025, 1, 1)))),
            false,
        )
        .unwrap();
        ctx.insert_spec(
            Arc::new(make_spec_with_range("dep", Some(date(2025, 6, 1)))),
            false,
        )
        .unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", Some(&date(2025, 1, 1)), Some(&date(2025, 12, 1)));
        assert!(gaps.is_empty());
    }

    #[test]
    fn dep_coverage_dep_starts_after_required_start() {
        let mut ctx = Context::new();
        ctx.insert_spec(
            Arc::new(make_spec_with_range("dep", Some(date(2025, 6, 1)))),
            false,
        )
        .unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", Some(&date(2025, 1, 1)), Some(&date(2025, 12, 1)));
        assert_eq!(gaps, vec![(Some(date(2025, 1, 1)), Some(date(2025, 6, 1)))]);
    }

    #[test]
    fn dep_coverage_unbounded_required_range() {
        let mut ctx = Context::new();
        ctx.insert_spec(
            Arc::new(make_spec_with_range("dep", Some(date(2025, 6, 1)))),
            false,
        )
        .unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", None, None);
        assert_eq!(gaps, vec![(None, Some(date(2025, 6, 1)))]);
    }

    fn add_lemma_code_blocking(
        engine: &mut Engine,
        code: &str,
        source: &str,
    ) -> Result<(), Vec<Error>> {
        engine.load(code, LoadSource::Labeled(source))
    }

    #[test]
    fn test_evaluate_spec_all_rules() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        spec test
        fact x: 10
        fact y: 5
        rule sum: x + y
        rule product: x * y
    "#,
            "test.lemma",
        )
        .unwrap();

        let now = DateTimeValue::now();
        let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
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
    fn test_evaluate_empty_facts() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        spec test
        fact price: 100
        rule total: price * 2
    "#,
            "test.lemma",
        )
        .unwrap();

        let now = DateTimeValue::now();
        let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
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
        add_lemma_code_blocking(
            &mut engine,
            r#"
        spec test
        fact age: 25
        rule is_adult: age >= 18
    "#,
            "test.lemma",
        )
        .unwrap();

        let now = DateTimeValue::now();
        let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
        assert_eq!(
            response.results.values().next().unwrap().result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::from_bool(true)))
        );
    }

    #[test]
    fn test_evaluate_with_unless_clause() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        spec test
        fact quantity: 15
        rule discount: 0
          unless quantity >= 10 then 10
    "#,
            "test.lemma",
        )
        .unwrap();

        let now = DateTimeValue::now();
        let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
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
        let result = engine.run("nonexistent", Some(&now), HashMap::new());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_multiple_specs() {
        let mut engine = Engine::new();
        add_lemma_code_blocking(
            &mut engine,
            r#"
        spec spec1
        fact x: 10
        rule result: x * 2
    "#,
            "spec 1.lemma",
        )
        .unwrap();

        add_lemma_code_blocking(
            &mut engine,
            r#"
        spec spec2
        fact y: 5
        rule result: y * 3
    "#,
            "spec 2.lemma",
        )
        .unwrap();

        let now = DateTimeValue::now();
        let response1 = engine.run("spec1", Some(&now), HashMap::new()).unwrap();
        assert_eq!(
            response1.results[0].result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("20").unwrap()
            )))
        );

        let response2 = engine.run("spec2", Some(&now), HashMap::new()).unwrap();
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
        add_lemma_code_blocking(
            &mut engine,
            r#"
        spec test
        fact numerator: 10
        fact denominator: 0
        rule division: numerator / denominator
    "#,
            "test.lemma",
        )
        .unwrap();

        let now = DateTimeValue::now();
        let result = engine.run("test", Some(&now), HashMap::new());
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
            crate::OperationResult::Veto(message) => {
                assert!(
                    message
                        .as_ref()
                        .map(|m| m.contains("Division by zero"))
                        .unwrap_or(false),
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
        add_lemma_code_blocking(
            &mut engine,
            r#"
        spec test
        fact a: 1
        fact b: 2
        rule z: a + b
        rule y: a * b
        rule x: a - b
    "#,
            "test.lemma",
        )
        .unwrap();

        let now = DateTimeValue::now();
        let response = engine.run("test", Some(&now), HashMap::new()).unwrap();
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
        add_lemma_code_blocking(
            &mut engine,
            r#"
        spec test
        fact base: 100
        rule subtotal: base * 2
        rule tax: subtotal * 10%
        rule total: subtotal + tax
    "#,
            "test.lemma",
        )
        .unwrap();

        // User filters to 'total' after run (deps were still computed)
        let now = DateTimeValue::now();
        let rules = vec!["total".to_string()];
        let mut response = engine.run("test", Some(&now), HashMap::new()).unwrap();
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

        let mut deps = HashMap::new();
        deps.insert(
            "deps/org_project_helper.lemma".to_string(),
            "spec @org/project/helper\nfact quantity: 42".to_string(),
        );
        engine
            .add_dependency_files(deps)
            .expect("should load dependency files");

        engine
            .load(
                r#"spec main_spec
fact external: spec @org/project/helper
rule value: external.quantity"#,
                LoadSource::Labeled("main.lemma"),
            )
            .expect("should succeed with pre-resolved deps");

        let now = DateTimeValue::now();
        let response = engine
            .run("main_spec", Some(&now), HashMap::new())
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

        add_lemma_code_blocking(
            &mut engine,
            r#"spec local_only
fact price: 100
rule doubled: price * 2"#,
            "local.lemma",
        )
        .expect("should succeed when there are no @... references");

        let now = DateTimeValue::now();
        let response = engine
            .run("local_only", Some(&now), HashMap::new())
            .expect("evaluate should succeed");

        assert!(response.results.contains_key("doubled"));
    }

    #[test]
    fn unresolved_external_ref_without_deps_fails() {
        let mut engine = Engine::new();

        let result = add_lemma_code_blocking(
            &mut engine,
            r#"spec main_spec
fact external: spec @org/project/missing
rule value: external.quantity"#,
            "main.lemma",
        );

        assert!(
            result.is_err(),
            "Should fail when @... dep is not in file map"
        );
    }

    #[test]
    fn pre_resolved_deps_with_spec_and_type_refs() {
        let mut engine = Engine::new();

        let mut deps = HashMap::new();
        deps.insert(
            "deps/helper.lemma".to_string(),
            "spec @org/example/helper\nfact value: 42".to_string(),
        );
        deps.insert(
            "deps/finance.lemma".to_string(),
            "spec @lemma/std/finance\ntype money: scale\n -> unit eur 1.00\n -> decimals 2"
                .to_string(),
        );
        engine
            .add_dependency_files(deps)
            .expect("should load dependency files");

        engine
            .load(
                r#"spec registry_demo
type money from @lemma/std/finance
fact unit_price: 5 eur
fact helper: spec @org/example/helper
rule helper_value: helper.value
rule line_total: unit_price * 2
rule formatted: helper_value + 0"#,
                LoadSource::Labeled("main.lemma"),
            )
            .expect("should succeed with pre-resolved spec and type deps");

        let now = DateTimeValue::now();
        let response = engine
            .run("registry_demo", Some(&now), HashMap::new())
            .expect("evaluate should succeed");

        assert!(response.results.contains_key("helper_value"));
        assert!(response.results.contains_key("formatted"));
    }

    #[test]
    fn load_empty_labeled_source_is_error() {
        let mut engine = Engine::new();
        let err = engine
            .load("spec x\nfact a: 1", LoadSource::Labeled("  "))
            .unwrap_err();
        assert!(err.iter().any(|e| e.message().contains("non-empty")));
    }

    #[test]
    fn load_inline_source_succeeds() {
        let mut engine = Engine::new();
        engine
            .load("spec x\nfact a: 1", LoadSource::Inline)
            .expect("inline load");
    }

    #[test]
    fn load_rejects_registry_spec_definitions() {
        let mut engine = Engine::new();
        let result = engine.load(
            "spec @org/example/helper\nfact x: 1",
            LoadSource::Labeled("bad.lemma"),
        );
        assert!(result.is_err(), "should reject @-prefixed spec in load");
        let errors = result.unwrap_err();
        assert!(
            errors
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
            "spec @org/my/helper\nfact x: 1".to_string(),
        );
        engine
            .add_dependency_files(files)
            .expect("add_dependency_files should accept @-prefixed specs");
    }

    #[test]
    fn add_dependency_files_rejects_bare_named_spec_in_registry_bundle() {
        let mut engine = Engine::new();
        let mut files = HashMap::new();
        files.insert(
            "deps/bundle.lemma".to_string(),
            "spec local_looking_name\nfact x: 1".to_string(),
        );
        let result = engine.add_dependency_files(files);
        assert!(
            result.is_err(),
            "should reject non-@-prefixed spec in registry bundle"
        );
        let errors = result.unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.message().contains("without '@' prefix")),
            "error should mention missing @ prefix, got: {:?}",
            errors
        );
    }

    #[test]
    fn add_dependency_files_rejects_spec_with_bare_spec_reference() {
        let mut engine = Engine::new();
        let mut files = HashMap::new();
        files.insert(
            "deps/billing.lemma".to_string(),
            "spec @org/billing\nfact rates: spec local_rates".to_string(),
        );
        let result = engine.add_dependency_files(files);
        assert!(
            result.is_err(),
            "should reject registry spec referencing non-@ spec"
        );
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| e.message().contains("local_rates")),
            "error should mention bare ref name, got: {:?}",
            errors
        );
    }

    #[test]
    fn add_dependency_files_rejects_spec_with_bare_type_import() {
        let mut engine = Engine::new();
        let mut files = HashMap::new();
        files.insert(
            "deps/billing.lemma".to_string(),
            "spec @org/billing\ntype money from local_finance".to_string(),
        );
        let result = engine.add_dependency_files(files);
        assert!(
            result.is_err(),
            "should reject registry spec importing type from non-@ spec"
        );
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| e.message().contains("local_finance")),
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
fact rates: spec @org/rates

spec @org/rates
fact rate: 10"#
                .to_string(),
        );
        engine
            .add_dependency_files(files)
            .expect("fully @-prefixed bundle should be accepted");
    }

    #[test]
    fn load_returns_all_errors_not_just_first() {
        let mut engine = Engine::new();

        let result = add_lemma_code_blocking(
            &mut engine,
            r#"spec demo
type money from nonexistent_type_source
fact helper: spec nonexistent_spec
fact price: 10
rule total: helper.value + price"#,
            "test.lemma",
        );

        assert!(result.is_err(), "Should fail with multiple errors");
        let errs = result.unwrap_err();
        assert!(
            errs.len() >= 2,
            "expected at least 2 errors (type + spec ref), got {}",
            errs.len()
        );
        let error_message = errs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");

        assert!(
            error_message.contains("money"),
            "Should mention type error about 'money'. Got:\n{}",
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
        let result = add_lemma_code_blocking(
            &mut engine,
            "spec t\nfact x: [number -> default \"10 $$\"]\nrule r: x",
            "t.lemma",
        );
        assert!(
            result.is_err(),
            "must reject non-numeric default on number type"
        );
    }

    #[test]
    fn planning_rejects_text_literal_as_number_default() {
        // The parser produces CommandArg::Text("10") for `default "10"`.
        // Planning now checks the CommandArg variant: a Text literal is
        // rejected where a Number literal is required, even though the
        // string content "10" could be parsed as a valid Decimal.
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "spec t\nfact x: [number -> default \"10\"]\nrule r: x",
            "t.lemma",
        );
        assert!(
            result.is_err(),
            "must reject text literal \"10\" as default for number type"
        );
    }

    #[test]
    fn planning_rejects_invalid_boolean_default() {
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "spec t\nfact x: [boolean -> default \"maybe\"]\nrule r: x",
            "t.lemma",
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
        let result = add_lemma_code_blocking(
            &mut engine,
            "spec t\ntype custom: number -> minimum 0\nfact x: [custom -> default \"abc\"]\nrule r: x",
            "t.lemma",
        );
        assert!(
            result.is_err(),
            "must reject non-numeric default on named number type"
        );
    }

    #[test]
    fn planning_accepts_valid_number_default() {
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "spec t\nfact x: [number -> default 10]\nrule r: x",
            "t.lemma",
        );
        assert!(result.is_ok(), "must accept valid number default");
    }

    #[test]
    fn planning_accepts_valid_boolean_default() {
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "spec t\nfact x: [boolean -> default true]\nrule r: x",
            "t.lemma",
        );
        assert!(result.is_ok(), "must accept valid boolean default");
    }

    #[test]
    fn planning_accepts_valid_text_default() {
        let mut engine = Engine::new();
        let result = add_lemma_code_blocking(
            &mut engine,
            "spec t\nfact x: [text -> default \"hello\"]\nrule r: x",
            "t.lemma",
        );
        assert!(result.is_ok(), "must accept valid text default");
    }
}
