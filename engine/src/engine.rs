use crate::evaluation::Evaluator;
use crate::parsing::ast::{DateTimeValue, LemmaSpec};
use crate::registry::Registry;
use crate::{parse, Error, ResourceLimits, Response};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

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

    /// Insert a spec. Validates no duplicate (name, effective_from).
    pub fn insert_spec(&mut self, spec: Arc<LemmaSpec>) -> Result<(), Error> {
        let existing = self.specs_for_name(&spec.name);

        if existing
            .iter()
            .any(|o| o.effective_from() == spec.effective_from())
        {
            return Err(Error::validation(
                format!(
                    "Duplicate spec '{}' (same name and effective_from already in context)",
                    spec.name
                ),
                None,
                None::<String>,
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

/// Engine for evaluating Lemma rules
///
/// Pure Rust implementation that evaluates Lemma specs directly from the AST.
/// Uses pre-built execution plans that are self-contained and ready for evaluation.
///
/// An optional Registry can be configured to resolve external `@...` references.
/// When a Registry is set, `add_lemma_files` will automatically resolve `@...`
/// references by fetching source text from the Registry, parsing it, and including
/// the resulting Lemma specs in the spec set before planning.
pub struct Engine {
    execution_plans: HashMap<Arc<LemmaSpec>, Vec<crate::planning::ExecutionPlan>>,
    specs: Context,
    sources: HashMap<String, String>,
    evaluator: Evaluator,
    limits: ResourceLimits,
    registry: Option<Arc<dyn Registry>>,
    hash_pins: HashMap<Arc<LemmaSpec>, String>,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            execution_plans: HashMap::new(),
            specs: Context::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits: ResourceLimits::default(),
            registry: Self::default_registry(),
            hash_pins: HashMap::new(),
        }
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the default registry based on enabled features.
    ///
    /// When the `registry` feature is enabled, the default registry is `LemmaBase`,
    /// which resolves `@...` references by fetching Lemma source from LemmaBase.com.
    ///
    /// When the `registry` feature is disabled, no registry is configured and
    /// `@...` references will fail during resolution.
    fn default_registry() -> Option<Arc<dyn Registry>> {
        #[cfg(feature = "registry")]
        {
            Some(Arc::new(crate::registry::LemmaBase::new()))
        }
        #[cfg(not(feature = "registry"))]
        {
            None
        }
    }

    /// Create an engine with custom resource limits.
    ///
    /// Uses the default registry (LemmaBase when the `registry` feature is enabled).
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            execution_plans: HashMap::new(),
            specs: Context::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits,
            registry: Self::default_registry(),
            hash_pins: HashMap::new(),
        }
    }

    /// Configure a Registry for resolving external `@...` references.
    ///
    /// When set, `add_lemma_files` will resolve `@...` references automatically
    /// by fetching source text from the Registry before planning.
    pub fn with_registry(mut self, registry: Arc<dyn Registry>) -> Self {
        self.registry = Some(registry);
        self
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
            let computed = self.hash_pins.get(&spec).map(|s| s.as_str()).unwrap_or("");
            if crate::planning::content_hash::content_hash_matches(hash_pin, computed) {
                if matched.is_some() {
                    return None;
                }
                matched = Some(spec);
            }
        }
        matched
    }

    /// Add Lemma source files and (when a registry is configured) resolve any `@...` references.
    ///
    /// - Resolves registry references **once** for all specs
    /// - Validates and resolves types **once** across all specs
    /// - Collects **all** errors across all files (parse, registry, planning) instead of aborting on the first
    ///
    /// `files` maps source identifiers (e.g. file paths) to source code.
    /// For a single file, pass a one-entry `HashMap`.
    pub async fn add_lemma_files(
        &mut self,
        files: HashMap<String, String>,
    ) -> Result<(), Vec<Error>> {
        let mut errors: Vec<Error> = Vec::new();

        for (source_id, code) in &files {
            match parse(code, source_id, &self.limits) {
                Ok(new_specs) => {
                    let source_text: Arc<str> = Arc::from(code.as_str());
                    for spec in new_specs {
                        let attribute = spec.attribute.clone().unwrap_or_else(|| spec.name.clone());
                        let start_line = spec.start_line;
                        let spec_name = spec.name.clone();

                        match self.specs.insert_spec(Arc::new(spec)) {
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
                                    &spec_name,
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

        if let Some(registry) = &self.registry {
            if let Err(registry_errors) = crate::registry::resolve_registry_references(
                &mut self.specs,
                &mut self.sources,
                registry.as_ref(),
                &self.limits,
            )
            .await
            {
                errors.extend(registry_errors);
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
        Error::request(msg, None, None::<String>)
    }

    /// Get the execution plan for a spec.
    ///
    /// When `hash_pin` is `Some`, resolves the spec by content hash for that name,
    /// then returns the slice plan that covers `effective`. When `hash_pin` is `None`,
    /// resolves the temporal version active at `effective` then finds the covering slice plan.
    /// Returns `None` when the spec does not exist or has no matching plan.
    pub fn get_execution_plan(
        &self,
        spec_name: &str,
        hash_pin: Option<&str>,
        effective: &DateTimeValue,
    ) -> Option<&crate::planning::ExecutionPlan> {
        let arc = if let Some(pin) = hash_pin {
            self.get_spec_by_hash_pin(spec_name, pin)?
        } else {
            self.get_spec(spec_name, effective)?
        };
        let slice_plans = self.execution_plans.get(&arc)?;
        let plan = find_slice_plan(slice_plans, effective);
        if plan.is_none() && !slice_plans.is_empty() {
            unreachable!(
                "BUG: spec '{}' has {} slice plans but none covers effective={} — slice partition is broken",
                spec_name, slice_plans.len(), effective
            );
        }
        plan
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

    /// Evaluate rules in a spec with JSON values for facts.
    ///
    /// This is a convenience method that accepts JSON directly and converts it
    /// to typed values using the spec's fact type declarations.
    ///
    /// If `rule_names` is empty, evaluates all rules.
    /// Otherwise, only returns results for the specified rules (dependencies still computed).
    ///
    /// Values are provided as JSON bytes (e.g., `b"{\"quantity\": 5, \"is_member\": true}"`).
    /// They are automatically parsed to the expected type based on the spec schema.
    ///
    /// When `hash_pin` is `Some`, the spec is resolved by that content hash; otherwise
    /// by temporal resolution at `effective`. Evaluation uses the resolved plan.
    pub fn evaluate_json(
        &self,
        spec_name: &str,
        hash_pin: Option<&str>,
        effective: &DateTimeValue,
        rule_names: Vec<String>,
        json: &[u8],
    ) -> Result<Response, Error> {
        let base_plan = self
            .get_execution_plan(spec_name, hash_pin, effective)
            .ok_or_else(|| self.spec_not_found_error(spec_name, effective))?;

        let values = crate::serialization::from_json(json)?;
        let plan = base_plan.clone().with_fact_values(values, &self.limits)?;

        self.evaluate_plan(plan, rule_names, effective)
    }

    /// Evaluate rules in a spec with string values for facts.
    ///
    /// This is the user-friendly API that accepts raw string values and parses them
    /// to the appropriate types based on the spec's fact type declarations.
    /// Use this for CLI, HTTP APIs, and other user-facing interfaces.
    ///
    /// If `rule_names` is empty, evaluates all rules.
    /// Otherwise, only returns results for the specified rules (dependencies still computed).
    ///
    /// Fact values are provided as name -> value string pairs (e.g., "type" -> "latte").
    /// They are automatically parsed to the expected type based on the spec schema.
    ///
    /// When `hash_pin` is `Some`, the spec is resolved by that content hash; otherwise
    /// by temporal resolution at `effective`. Evaluation uses the resolved plan.
    pub fn evaluate(
        &self,
        spec_name: &str,
        hash_pin: Option<&str>,
        effective: &DateTimeValue,
        rule_names: Vec<String>,
        fact_values: HashMap<String, String>,
    ) -> Result<Response, Error> {
        let base_plan = self
            .get_execution_plan(spec_name, hash_pin, effective)
            .ok_or_else(|| self.spec_not_found_error(spec_name, effective))?;

        let plan = base_plan
            .clone()
            .with_fact_values(fact_values, &self.limits)?;

        self.evaluate_plan(plan, rule_names, effective)
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
        let base_plan = self
            .get_execution_plan(spec_name, None, effective)
            .ok_or_else(|| self.spec_not_found_error(spec_name, effective))?;

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
        rule_names: Vec<String>,
        effective: &DateTimeValue,
    ) -> Result<Response, Error> {
        let now_semantic = crate::planning::semantics::date_time_to_semantic(effective);
        let now_literal = crate::planning::semantics::LiteralValue {
            value: crate::planning::semantics::ValueKind::Date(now_semantic),
            lemma_type: crate::planning::semantics::primitive_date().clone(),
        };
        let mut response = self.evaluator.evaluate(&plan, now_literal);

        if !rule_names.is_empty() {
            response.filter_rules(&rule_names);
        }

        Ok(response)
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

    // ─── Context::effective_range tests ──────────────────────────────

    #[test]
    fn effective_range_unbounded_single_version() {
        let mut ctx = Context::new();
        let spec = Arc::new(make_spec("a"));
        ctx.insert_spec(Arc::clone(&spec)).unwrap();

        let (from, to) = ctx.effective_range(&spec);
        assert_eq!(from, None);
        assert_eq!(to, None);
    }

    #[test]
    fn effective_range_soft_end_from_next_version() {
        let mut ctx = Context::new();
        let v1 = Arc::new(make_spec_with_range("a", Some(date(2025, 1, 1))));
        let v2 = Arc::new(make_spec_with_range("a", Some(date(2025, 6, 1))));
        ctx.insert_spec(Arc::clone(&v1)).unwrap();
        ctx.insert_spec(Arc::clone(&v2)).unwrap();

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
        ctx.insert_spec(Arc::clone(&v1)).unwrap();
        ctx.insert_spec(Arc::clone(&v2)).unwrap();

        let (from, to) = ctx.effective_range(&v1);
        assert_eq!(from, None);
        assert_eq!(to, Some(date(2025, 3, 1)));
    }

    // ─── Context::version_boundaries tests ───────────────────────────

    #[test]
    fn version_boundaries_single_unversioned() {
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::new(make_spec("a"))).unwrap();

        assert!(ctx.version_boundaries("a").is_empty());
    }

    #[test]
    fn version_boundaries_multiple_versions() {
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::new(make_spec("a"))).unwrap();
        ctx.insert_spec(Arc::new(make_spec_with_range("a", Some(date(2025, 3, 1)))))
            .unwrap();
        ctx.insert_spec(Arc::new(make_spec_with_range("a", Some(date(2025, 6, 1)))))
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
        ctx.insert_spec(Arc::new(make_spec("dep"))).unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", None, None);
        assert!(gaps.is_empty());

        let gaps = ctx.dep_coverage_gaps("dep", Some(&date(2025, 1, 1)), Some(&date(2025, 12, 1)));
        assert!(gaps.is_empty());
    }

    #[test]
    fn dep_coverage_single_version_with_from_leaves_leading_gap() {
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 3, 1)),
        )))
        .unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", None, None);
        assert_eq!(gaps, vec![(None, Some(date(2025, 3, 1)))]);
    }

    #[test]
    fn dep_coverage_continuous_versions_no_gaps() {
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 1, 1)),
        )))
        .unwrap();
        ctx.insert_spec(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 6, 1)),
        )))
        .unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", Some(&date(2025, 1, 1)), Some(&date(2025, 12, 1)));
        assert!(gaps.is_empty());
    }

    #[test]
    fn dep_coverage_dep_starts_after_required_start() {
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 6, 1)),
        )))
        .unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", Some(&date(2025, 1, 1)), Some(&date(2025, 12, 1)));
        assert_eq!(gaps, vec![(Some(date(2025, 1, 1)), Some(date(2025, 6, 1)))]);
    }

    #[test]
    fn dep_coverage_unbounded_required_range() {
        let mut ctx = Context::new();
        ctx.insert_spec(Arc::new(make_spec_with_range(
            "dep",
            Some(date(2025, 6, 1)),
        )))
        .unwrap();

        let gaps = ctx.dep_coverage_gaps("dep", None, None);
        assert_eq!(gaps, vec![(None, Some(date(2025, 6, 1)))]);
    }

    fn add_lemma_code_blocking(
        engine: &mut Engine,
        code: &str,
        source: &str,
    ) -> Result<(), Vec<Error>> {
        let files: HashMap<String, String> =
            std::iter::once((source.to_string(), code.to_string())).collect();
        tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(engine.add_lemma_files(files))
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
        let response = engine
            .evaluate("test", None, &now, vec![], HashMap::new())
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
        let response = engine
            .evaluate("test", None, &now, vec![], HashMap::new())
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
        let response = engine
            .evaluate("test", None, &now, vec![], HashMap::new())
            .unwrap();
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
        let response = engine
            .evaluate("test", None, &now, vec![], HashMap::new())
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
        let result = engine.evaluate("nonexistent", None, &now, vec![], HashMap::new());
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
        let response1 = engine
            .evaluate("spec1", None, &now, vec![], HashMap::new())
            .unwrap();
        assert_eq!(
            response1.results[0].result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("20").unwrap()
            )))
        );

        let response2 = engine
            .evaluate("spec2", None, &now, vec![], HashMap::new())
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
        let result = engine.evaluate("test", None, &now, vec![], HashMap::new());
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
        let response = engine
            .evaluate("test", None, &now, vec![], HashMap::new())
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

        // Request only 'total', but it depends on 'subtotal' and 'tax'
        let now = DateTimeValue::now();
        let response = engine
            .evaluate(
                "test",
                None,
                &now,
                vec!["total".to_string()],
                HashMap::new(),
            )
            .unwrap();

        // Only 'total' should be in results
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
    // Registry integration tests
    // -------------------------------------------------------------------

    use crate::parsing::ast::DateTimeValue;
    use crate::registry::{RegistryBundle, RegistryError};

    struct EngineTestRegistry {
        bundles: std::collections::HashMap<String, RegistryBundle>,
    }

    impl EngineTestRegistry {
        fn new() -> Self {
            Self {
                bundles: std::collections::HashMap::new(),
            }
        }

        fn add(&mut self, identifier: &str, source: &str) {
            self.bundles.insert(
                identifier.to_string(),
                RegistryBundle {
                    lemma_source: source.to_string(),
                    attribute: format!("@{}", identifier),
                },
            );
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    impl Registry for EngineTestRegistry {
        async fn fetch_specs(&self, name: &str) -> Result<RegistryBundle, RegistryError> {
            self.bundles.get(name).cloned().ok_or(RegistryError {
                message: format!("not found: {}", name),
                kind: crate::registry::RegistryErrorKind::NotFound,
            })
        }

        async fn fetch_types(&self, name: &str) -> Result<RegistryBundle, RegistryError> {
            self.bundles.get(name).cloned().ok_or(RegistryError {
                message: format!("not found: {}", name),
                kind: crate::registry::RegistryErrorKind::NotFound,
            })
        }

        fn url_for_id(&self, name: &str, effective: Option<&DateTimeValue>) -> Option<String> {
            if self.bundles.contains_key(name) {
                Some(match effective {
                    None => format!("https://test/{}", name),
                    Some(d) => format!("https://test/{}?effective={}", name, d),
                })
            } else {
                None
            }
        }
    }

    /// Build an engine with no registry (regardless of feature flags).
    fn engine_without_registry() -> Engine {
        Engine {
            execution_plans: HashMap::new(),
            specs: Context::new(),
            sources: HashMap::new(),
            evaluator: Evaluator,
            limits: ResourceLimits::default(),
            registry: None,
            hash_pins: HashMap::new(),
        }
    }

    #[test]
    fn add_lemma_files_with_registry_resolves_and_evaluates_external_spec() {
        let mut registry = EngineTestRegistry::new();
        registry.add(
            "org/project/helper",
            "spec org/project/helper\nfact quantity: 42",
        );

        let mut engine = engine_without_registry().with_registry(Arc::new(registry));

        add_lemma_code_blocking(
            &mut engine,
            r#"spec main_spec
fact external: spec @org/project/helper
rule value: external.quantity"#,
            "main.lemma",
        )
        .expect("add_lemma_files should succeed with registry resolving the external spec");

        let now = DateTimeValue::now();
        let response = engine
            .evaluate("main_spec", None, &now, vec![], HashMap::new())
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
    fn add_lemma_files_without_registry_and_no_external_refs_works() {
        let mut engine = engine_without_registry();

        add_lemma_code_blocking(
            &mut engine,
            r#"spec local_only
fact price: 100
rule doubled: price * 2"#,
            "local.lemma",
        )
        .expect(
            "add_lemma_files should succeed without registry when there are no @... references",
        );

        let now = DateTimeValue::now();
        let response = engine
            .evaluate("local_only", None, &now, vec![], HashMap::new())
            .expect("evaluate should succeed");

        assert!(response.results.contains_key("doubled"));
    }

    #[test]
    fn add_lemma_files_without_registry_and_external_ref_fails() {
        let mut engine = engine_without_registry();

        let result = add_lemma_code_blocking(
            &mut engine,
            r#"spec main_spec
fact external: spec @org/project/missing
rule value: external.quantity"#,
            "main.lemma",
        );

        assert!(
            result.is_err(),
            "Should fail when @... reference exists but no registry is configured"
        );
    }

    #[test]
    fn add_lemma_files_with_registry_resolves_spec_and_type_refs() {
        let mut registry = EngineTestRegistry::new();
        registry.add(
            "org/example/helper",
            "spec org/example/helper\nfact value: 42",
        );
        registry.add(
            "lemma/std/finance",
            r#"spec lemma/std/finance
type money: scale
 -> unit eur 1.00
 -> decimals 2"#,
        );

        let mut engine = engine_without_registry().with_registry(Arc::new(registry));

        let main_content = r#"spec registry_demo
type money from @lemma/std/finance
fact unit_price: 5 eur
fact helper: spec @org/example/helper
rule helper_value: helper.value
rule line_total: unit_price * 2
rule formatted: helper_value + 0"#;

        add_lemma_code_blocking(&mut engine, main_content, "main.lemma")
            .expect("add_lemma_files with registry should resolve @ refs");

        let now = DateTimeValue::now();
        let response = engine
            .evaluate("registry_demo", None, &now, vec![], HashMap::new())
            .expect("evaluate should succeed");

        assert!(response.results.contains_key("helper_value"));
        assert!(response.results.contains_key("formatted"));
    }

    #[test]
    fn add_lemma_files_with_registry_error_propagates_as_registry_error() {
        // Empty registry — every lookup returns "not found"
        let registry = EngineTestRegistry::new();

        let mut engine = engine_without_registry().with_registry(Arc::new(registry));

        let result = add_lemma_code_blocking(
            &mut engine,
            r#"spec main_spec
fact external: spec @org/project/missing
rule value: external.quantity"#,
            "main.lemma",
        );

        assert!(
            result.is_err(),
            "Should fail when registry cannot resolve the @... reference"
        );
        let errs = result.unwrap_err();
        assert!(!errs.is_empty(), "expected at least one error");
        let registry_err = errs
            .iter()
            .find(|e| matches!(e, Error::Registry { .. }))
            .expect("error list should contain at least one Registry error");
        match registry_err {
            Error::Registry {
                identifier, kind, ..
            } => {
                assert_eq!(identifier, "org/project/missing");
                assert_eq!(*kind, crate::registry::RegistryErrorKind::NotFound);
            }
            _ => unreachable!(),
        }
        let error_message = errs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(
            error_message.contains("org/project/missing"),
            "Error should mention the unresolved identifier: {}",
            error_message
        );
        assert!(
            error_message.contains("not found"),
            "Error should mention the error kind: {}",
            error_message
        );
    }

    #[test]
    fn with_registry_replaces_default_registry() {
        let mut registry = EngineTestRegistry::new();
        registry.add("custom/spec", "spec custom/spec\nfact x: 99");

        let mut engine = Engine::new().with_registry(Arc::new(registry));

        add_lemma_code_blocking(
            &mut engine,
            r#"spec main_spec
fact ext: spec @custom/spec
rule val: ext.x"#,
            "main.lemma",
        )
        .expect("with_registry should replace the default registry");

        let now = DateTimeValue::now();
        let response = engine
            .evaluate("main_spec", None, &now, vec![], HashMap::new())
            .expect("evaluate should succeed");

        let val_result = response
            .results
            .get("val")
            .expect("rule 'val' should exist");
        assert_eq!(
            val_result.result,
            crate::OperationResult::Value(Box::new(crate::planning::LiteralValue::number(
                Decimal::from_str("99").unwrap()
            )))
        );
    }

    #[test]
    fn add_lemma_files_returns_all_errors_not_just_first() {
        // When a spec has multiple independent errors (type import from
        // non-existing spec AND spec reference to non-existing spec), the Engine
        // should surface all of them, not just the first one.
        let mut engine = engine_without_registry();

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
