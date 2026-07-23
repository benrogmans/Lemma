//! Execution plan for evaluated specs
//!
//! Provides a complete self-contained execution plan ready for the evaluator.
//! The plan holds an expanded shared normal-form graph in a dense table
//! ([`ExecutionPlan::normal_forms`]) addressed by [`NormalFormId`], plus all data —
//! no spec structure needed during evaluation.
//!
//! Reliability model:
//! - [`Show`] is the IO contract surface for consumers (data and rule outputs).
//!   IO compatibility is the consumer-facing guarantee.

use crate::computation::UnitResolutionContext;
use crate::parsing::ast::{EffectiveDate, MetaValue};
use crate::parsing::source::Source;
use crate::planning::graph::Graph;
use crate::planning::graph::ResolvedSpecTypes;
use crate::planning::normalize::{
    NormalForm, NormalFormId, NormalFormInterner, NormalizeContext, NormalizedRule,
};
use crate::planning::semantics::{
    value_kind_matches_spec, ComparisonComputation, DataDefinition, DataPath, LemmaType,
    LiteralValue, ReferenceTarget, RulePath, TypeSpecification, ValueKind,
};
use crate::Error;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

#[cfg(test)]
thread_local! {
    /// Test-only: NormalFormId entries during exclusivity / structural DAG walks.
    static STRUCTURAL_DAG_VISIT_COUNT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static STRUCTURAL_DAG_VISIT_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn record_structural_dag_visit() {
    STRUCTURAL_DAG_VISIT_ENABLED.with(|enabled| {
        if enabled.get() {
            STRUCTURAL_DAG_VISIT_COUNT.with(|count| count.set(count.get() + 1));
        }
    });
}

/// A complete execution plan ready for the evaluator
///
/// Contains drift-free normal-form equations in a table, named rule roots, and all data.
/// Self-contained structure - no spec lookups required during evaluation.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    /// Main spec name
    pub spec_name: String,

    /// Optional commentary from the `"""..."""` block in the spec source.
    pub commentary: Option<String>,

    /// Per-data data in definition order: value, type-only, or spec reference.
    pub data: IndexMap<DataPath, DataDefinition>,

    /// Dense table of interned NormalForm cells — the expanded shared graph.
    /// Index = [`NormalFormId`].
    pub(crate) normal_forms: Vec<NormalForm>,

    /// Named rules → root of each rule in [`Self::normal_forms`].
    /// Insertion order is planning topo order (deps before consumers).
    pub rules: IndexMap<RulePath, ExecutableRule>,

    /// Data→data [`DataDefinition::Reference`] paths in dependency order for
    /// evaluation prepop (chained reference → reference → data). Empty when the
    /// plan has none. Rule-target references are not included.
    pub data_reference_order: Vec<DataPath>,

    /// Spec metadata
    pub meta: HashMap<String, MetaValue>,

    /// Main-spec types from planning. [`ResolvedSpecTypes::unit_index`] is expression-scope
    /// units (local types plus direct `uses` imports). Rule-result units live on each
    /// [`ExecutableRule::rule_type`], not in this index.
    pub resolved_types: ResolvedSpecTypes,

    /// Reverse index: canonical-form unit signature `Vec<(unit_name, exponent)>` →
    /// (unit_name, owning type). Built from expression-scope units during planning so
    /// cross-type Multiply/Divide arithmetic can deterministically resolve a combined
    /// signature back to a single named unit. Ambiguous signatures (the same key matched
    /// by units in two distinct types) are rejected at planning time.
    pub signature_index: crate::computation::arithmetic::SignatureIndex,

    pub effective: EffectiveDate,

    /// Per local rule, per And/Piecewise node: DataPaths certainly unused when a
    /// control outcome holds (rule-root exclusivity). Filled after the sealed DAG.
    pub(crate) data_releases: HashMap<RulePath, HashMap<NormalFormId, ControlDataReleases>>,

    /// Local rule root [`NormalFormId`] → unrestricted structural [`DataPath`] leaves
    /// (`structural_needed(rule.root)`). Filled once after the sealed DAG; used by
    /// exclusivity fill, Show, and eval `missing_data`.
    pub(crate) structural_needed: HashMap<NormalFormId, HashSet<DataPath>>,
}

/// Release lists for one And or Piecewise under one rule root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ControlDataReleases {
    And {
        /// Left conjunct is boolean false → right child edge dead.
        on_left_false: Vec<DataPath>,
    },
    Piecewise {
        /// Index aligned with arms; index 0 unused. Condition false → that arm body dead.
        on_arm_not_taken: Vec<Vec<DataPath>>,
        /// Condition true and this unless arm wins → multi-edge kill (default body,
        /// other unless bodies, skipped lower unless conditions).
        on_arm_taken: Vec<Vec<DataPath>>,
        /// All unless conditions false → default wins → all unless bodies dead.
        on_default_wins: Vec<DataPath>,
    },
}

/// A named rule's root in the normal-form table, plus declaration metadata.
#[derive(Debug, Clone)]
pub struct ExecutableRule {
    /// Unique identifier for this rule
    pub path: RulePath,

    /// Root of this rule in the shared [`ExecutionPlan::normal_forms`] graph.
    pub normal_form: NormalFormId,

    /// Source location for error messages (always present for rules from parsed specs)
    pub source: Source,

    /// Computed type of this rule's result
    /// Every rule MUST have a type (Lemma is strictly typed)
    pub rule_type: Arc<LemmaType>,
}

impl ExecutableRule {
    pub fn name(&self) -> &str {
        &self.path.rule
    }
}

/// Select the plan whose half-open `[effective, next.effective)` covers `instant`
/// (greatest key `<= instant` in the map).
pub(crate) fn plan_at<'a>(
    plans: &'a BTreeMap<EffectiveDate, ExecutionPlan>,
    instant: &EffectiveDate,
) -> Option<&'a ExecutionPlan> {
    plans
        .range(..=instant.clone())
        .next_back()
        .map(|(_, plan)| plan)
}

/// Builds an execution plan from a Graph for one temporal slice.
/// Internal implementation detail - only called by plan()
pub(crate) fn build_execution_plan(
    graph: &Graph<'_>,
    resolved_types: ResolvedSpecTypes,
    effective: &EffectiveDate,
    limits: &crate::limits::ResourceLimits,
) -> Result<ExecutionPlan, Vec<Error>> {
    let rule_order = graph.rule_order();

    let main_spec = graph.main_spec();
    let data = graph.build_data(&resolved_types.resolved)?;

    // Planning gate: every data-target reference and plain data declaration
    // must carry a fully resolved type. Rule-target references are exempt:
    // they deliberately ship `Undetermined` so runtime veto propagation
    // surfaces the target rule's veto reason directly. A residual
    // `Undetermined` anywhere else would violate the invariant evaluation
    // and show consumers rely on — report it instead of shipping the plan.
    let undetermined_errors: Vec<Error> = data
        .iter()
        .filter_map(|(path, definition)| {
            let (resolved_type, source) = match definition {
                DataDefinition::TypeDeclaration {
                    resolved_type,
                    source,
                    ..
                } => (resolved_type, source),
                DataDefinition::Reference {
                    target: ReferenceTarget::Data(_),
                    resolved_type,
                    source,
                    ..
                } => (resolved_type, source),
                DataDefinition::Reference {
                    target: ReferenceTarget::Rule(_),
                    ..
                }
                | DataDefinition::Value { .. }
                | DataDefinition::Import { .. } => return None,
            };
            if resolved_type.is_undetermined() {
                Some(Error::validation(
                    format!("could not determine the type of '{path}'"),
                    Some(source.clone()),
                    None::<String>,
                ))
            } else {
                None
            }
        })
        .collect();
    if !undetermined_errors.is_empty() {
        return Err(undetermined_errors);
    }

    let signature_index =
        crate::planning::graph::build_signature_index(&main_spec.name, &resolved_types.unit_index)
            .expect("BUG: signature_index build already validated during resolve_and_validate");

    let mut interner = NormalFormInterner::new();
    let mut rules: IndexMap<RulePath, ExecutableRule> = IndexMap::new();
    let mut completed_rules: HashMap<RulePath, NormalFormId> = HashMap::new();

    for rule_path in rule_order {
        let rule_node = graph.rules().get(rule_path).expect(
            "bug: rule from topological sort not in graph - validation should have caught this",
        );

        let unit_ctx = UnitResolutionContext::WithIndex(&resolved_types.unit_index);
        let normalize_ctx = NormalizeContext {
            data: &data,
            unit_ctx: &unit_ctx,
            max_normalized_expression_nodes: limits.max_normalized_expression_nodes,
            max_normal_form_depth: limits.max_normal_form_depth,
        };
        let normalized = crate::planning::normalize::build_normalized_rule(
            &normalize_ctx,
            &completed_rules,
            &rule_node.branches,
            Some(rule_node.source.clone()),
            &mut interner,
        )
        .map_err(|error| vec![error])?;
        let NormalizedRule { body } = normalized;
        completed_rules.insert(rule_path.clone(), body);

        rules.insert(
            rule_path.clone(),
            ExecutableRule {
                path: rule_path.clone(),
                normal_form: body,
                source: rule_node.source.clone(),
                rule_type: Arc::clone(&rule_node.rule_type),
            },
        );
    }

    let root_ids: Vec<NormalFormId> = rules.values().map(|rule| rule.normal_form).collect();
    let (normal_forms, remapped_roots) = interner.into_reachable(&root_ids);
    for (rule, remapped) in rules.values_mut().zip(remapped_roots) {
        rule.normal_form = remapped;
    }

    let mut plan = ExecutionPlan {
        spec_name: main_spec.name.clone(),
        commentary: main_spec.commentary.clone(),
        data,
        normal_forms,
        rules,
        data_reference_order: graph.data_reference_order().to_vec(),
        meta: main_spec
            .meta_fields
            .iter()
            .map(|f| (f.key.clone(), f.value.clone()))
            .collect(),
        resolved_types,
        signature_index,
        effective: effective.clone(),
        data_releases: HashMap::new(),
        structural_needed: HashMap::new(),
    };
    fill_structural_needed(&mut plan);
    fill_data_releases(&mut plan);

    let mut plan_errors = validate_literal_data_against_types(&plan);
    if let Err(error) = validate_unit_conversion_targets(&plan) {
        plan_errors.push(error);
    }
    if !plan_errors.is_empty() {
        return Err(plan_errors);
    }

    Ok(plan)
}

/// One data entry in a [`Show`].
///
/// A named struct instead of a tuple so JSON-native consumers (TypeScript, Python, ...)
/// get stable field names. `prefilled` is a spec literal or literal `with` binding;
/// `suggestion` is a `-> suggest ...` hint only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataEntry {
    #[serde(rename = "type")]
    pub lemma_type: LemmaType,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        serialize_with = "crate::planning::semantics::api_wire_literal::serialize_option",
        deserialize_with = "crate::planning::semantics::api_wire_literal::deserialize_option"
    )]
    pub prefilled: Option<LiteralValue>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        serialize_with = "crate::planning::semantics::api_wire_literal::serialize_option",
        deserialize_with = "crate::planning::semantics::api_wire_literal::deserialize_option"
    )]
    pub suggestion: Option<LiteralValue>,
    pub needed_by_rules: Vec<String>,
}

/// Half-open `[effective_from, effective_to)` for one loaded temporal row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShowVersion {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effective_from: Option<crate::parsing::ast::DateTimeValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effective_to: Option<crate::parsing::ast::DateTimeValue>,
}

impl std::fmt::Display for ShowVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.effective_from, &self.effective_to) {
            (Some(from), Some(to)) => write!(f, "{from} → {to}"),
            (Some(from), None) => write!(f, "{from} →"),
            (None, Some(to)) => write!(f, "→ {to}"),
            (None, None) => write!(f, "—"),
        }
    }
}

/// Consumer [`Engine::show`] result: data used by the spec's rules, local rule
/// result types, and resolved temporal window. Source: [`Engine::source`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Show {
    pub spec: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub commentary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effective_from: Option<crate::parsing::ast::DateTimeValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effective_to: Option<crate::parsing::ast::DateTimeValue>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub versions: Vec<ShowVersion>,
    pub start_line: usize,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_type: Option<crate::parsing::source::SourceType>,
    pub data: indexmap::IndexMap<String, DataEntry>,
    pub rules: indexmap::IndexMap<String, LemmaType>,
    pub meta: HashMap<String, MetaValue>,
}

impl std::fmt::Display for Show {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Spec: {}", self.spec)?;

        if let Some(commentary) = &self.commentary {
            write!(f, "\n  {}", commentary)?;
        }

        if let Some(from) = &self.effective_from {
            write!(f, "\n  effective_from: {}", from)?;
        }
        if let Some(to) = &self.effective_to {
            write!(f, "\n  effective_to: {}", to)?;
        }

        if self.versions.len() > 1 {
            let version_strs: Vec<String> = self
                .versions
                .iter()
                .map(|v| match (&v.effective_from, &v.effective_to) {
                    (Some(f), Some(t)) => format!("{f} → {t}"),
                    (Some(f), None) => format!("{f} →"),
                    (None, Some(t)) => format!("→ {t}"),
                    (None, None) => "—".to_string(),
                })
                .collect();
            write!(f, "\n  versions: {}", version_strs.join(", "))?;
        }

        if !self.meta.is_empty() {
            write!(f, "\n\nMeta:")?;
            let mut entries: Vec<(&String, &MetaValue)> = self.meta.iter().collect();
            entries.sort_by_key(|(k, _)| *k);
            for (key, value) in entries {
                write!(f, "\n  {}: {}", key, value)?;
            }
        }

        if !self.data.is_empty() {
            write!(f, "\n\nData:")?;
            for (name, entry) in &self.data {
                write!(f, "\n  {} ({})", name, entry.lemma_type.specifications)?;
                for line in type_detail_lines(&entry.lemma_type.specifications) {
                    write!(f, "\n    {}", line)?;
                }
                let help = entry.lemma_type.specifications.help();
                if !help.is_empty() {
                    write!(f, "\n    help: {}", help)?;
                }
                if let Some(val) = &entry.prefilled {
                    write!(f, "\n    prefilled: {}", val)?;
                }
                if let Some(val) = &entry.suggestion {
                    write!(f, "\n    suggestion: {}", val)?;
                }
            }
        }

        if !self.rules.is_empty() {
            write!(f, "\n\nRules:")?;
            for (name, rule_type) in &self.rules {
                write!(f, "\n  {} ({})", name, rule_type.specifications)?;
            }
        }

        if self.data.is_empty() && self.rules.is_empty() {
            write!(f, "\n  (no data or rules)")?;
        }

        Ok(())
    }
}

/// Produce a human-readable summary of type constraints, or `None` when there
/// are no constraints worth showing (e.g. bare `boolean`).
/// Returns one formatted string per constraint or property of the type specification.
/// Uses `display_str` for all rational bounds so they render as decimals,
/// not as raw fractions.
pub fn type_detail_lines(spec: &TypeSpecification) -> Vec<String> {
    let mut lines = Vec::new();
    match spec {
        TypeSpecification::Measure {
            minimum,
            maximum,
            decimals,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                lines.push(format!("units: {}", unit_names.join(", ")));
            }
            if let Some(d) = decimals {
                lines.push(format!("decimals: {}", d));
            }
            if let Some((magnitude, unit_name)) = minimum {
                lines.push(format!(
                    "minimum: {} {}",
                    magnitude.display_str(),
                    unit_name
                ));
            }
            if let Some((magnitude, unit_name)) = maximum {
                lines.push(format!(
                    "maximum: {} {}",
                    magnitude.display_str(),
                    unit_name
                ));
            }
        }
        TypeSpecification::Number {
            minimum,
            maximum,
            decimals,
            ..
        } => {
            if let Some(d) = decimals {
                lines.push(format!("decimals: {}", d));
            }
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", v.display_str()));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", v.display_str()));
            }
        }
        TypeSpecification::Ratio {
            minimum,
            maximum,
            decimals,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                lines.push(format!("units: {}", unit_names.join(", ")));
            }
            if let Some(d) = decimals {
                lines.push(format!("decimals: {}", d));
            }
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", v.display_str()));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", v.display_str()));
            }
        }
        TypeSpecification::Text {
            options, length, ..
        } => {
            if let Some(l) = length {
                lines.push(format!("length: {}", l));
            }
            if !options.is_empty() {
                let quoted: Vec<String> = options.iter().map(|o| format!("\"{}\"", o)).collect();
                lines.push(format!("options: {}", quoted.join(", ")));
            }
        }
        TypeSpecification::Date {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Time {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::MeasureRange {
            lower,
            upper,
            minimum,
            maximum,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                lines.push(format!("units: {}", unit_names.join(", ")));
            }
            if let Some((magnitude, unit_name)) = lower {
                lines.push(format!("lower: {} {}", magnitude.display_str(), unit_name));
            }
            if let Some((magnitude, unit_name)) = upper {
                lines.push(format!("upper: {} {}", magnitude.display_str(), unit_name));
            }
            if let Some((magnitude, unit_name)) = minimum {
                lines.push(format!(
                    "minimum: {} {}",
                    magnitude.display_str(),
                    unit_name
                ));
            }
            if let Some((magnitude, unit_name)) = maximum {
                lines.push(format!(
                    "maximum: {} {}",
                    magnitude.display_str(),
                    unit_name
                ));
            }
        }
        TypeSpecification::RatioRange {
            lower,
            upper,
            minimum,
            maximum,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                lines.push(format!("units: {}", unit_names.join(", ")));
            }
            if let Some(v) = lower {
                lines.push(format!("lower: {}", v.display_str()));
            }
            if let Some(v) = upper {
                lines.push(format!("upper: {}", v.display_str()));
            }
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", v.display_str()));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", v.display_str()));
            }
        }
        TypeSpecification::NumberRange {
            lower,
            upper,
            minimum,
            maximum,
            ..
        } => {
            if let Some(v) = lower {
                lines.push(format!("lower: {}", v.display_str()));
            }
            if let Some(v) = upper {
                lines.push(format!("upper: {}", v.display_str()));
            }
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", v.display_str()));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", v.display_str()));
            }
        }
        TypeSpecification::DateRange {
            lower,
            upper,
            minimum,
            maximum,
            ..
        } => {
            if let Some(v) = lower {
                lines.push(format!("lower: {}", v));
            }
            if let Some(v) = upper {
                lines.push(format!("upper: {}", v));
            }
            if let Some((magnitude, unit_name)) = minimum {
                lines.push(format!(
                    "minimum: {} {}",
                    magnitude.display_str(),
                    unit_name
                ));
            }
            if let Some((magnitude, unit_name)) = maximum {
                lines.push(format!(
                    "maximum: {} {}",
                    magnitude.display_str(),
                    unit_name
                ));
            }
        }
        TypeSpecification::TimeRange {
            lower,
            upper,
            minimum,
            maximum,
            ..
        } => {
            if let Some(v) = lower {
                lines.push(format!("lower: {}", v));
            }
            if let Some(v) = upper {
                lines.push(format!("upper: {}", v));
            }
            if let Some((magnitude, unit_name)) = minimum {
                lines.push(format!(
                    "minimum: {} {}",
                    magnitude.display_str(),
                    unit_name
                ));
            }
            if let Some((magnitude, unit_name)) = maximum {
                lines.push(format!(
                    "maximum: {} {}",
                    magnitude.display_str(),
                    unit_name
                ));
            }
        }
        TypeSpecification::Boolean { .. }
        | TypeSpecification::Veto { .. }
        | TypeSpecification::Undetermined => {}
    }
    lines
}

impl ExecutionPlan {
    /// Expression-scope unit index (local types plus direct `uses` imports).
    /// Rule-result units outside this scope are resolved from [`ExecutableRule::rule_type`]
    /// at materialization time.
    pub(crate) fn expression_unit_index(&self) -> &HashMap<String, Arc<LemmaType>> {
        &self.resolved_types.unit_index
    }

    /// Names of local (main-spec) rules in plan topological order.
    pub fn local_rule_names(&self) -> Vec<String> {
        self.rules
            .values()
            .filter(|r| r.path.segments.is_empty())
            .map(|r| r.path.rule.clone())
            .collect()
    }

    /// For each typed data input key, local rule names that transitively need it.
    pub(crate) fn needed_by_rules_index(&self) -> Result<HashMap<String, Vec<String>>, Error> {
        let mut index: HashMap<String, Vec<String>> = HashMap::new();
        for rule_name in self.local_rule_names() {
            let needed = self.collect_needed_data_paths(std::slice::from_ref(&rule_name))?;
            for path in needed {
                index
                    .entry(path.input_key())
                    .or_default()
                    .push(rule_name.clone());
            }
        }
        for rules in index.values_mut() {
            rules.sort();
            rules.dedup();
        }
        Ok(index)
    }

    /// Validate caller-requested rule names and return canonical local rule names.
    ///
    /// `None` means all local rules. `Some(&[])` is an error. Unknown names in `Some` slice error.
    pub fn validated_response_rule_names(
        &self,
        rules: Option<&[String]>,
    ) -> Result<std::collections::HashSet<String>, Error> {
        let Some(rules) = rules else {
            return Ok(self.local_rule_names().into_iter().collect());
        };
        if rules.is_empty() {
            return Err(Error::request(
                "at least one rule required".to_string(),
                None::<String>,
            ));
        }
        let mut names = std::collections::HashSet::new();
        for rule_name in rules {
            let rule = self.get_rule(rule_name).ok_or_else(|| {
                Error::request(
                    format!("Rule '{rule_name}' not found in spec '{}'", self.spec_name),
                    None::<String>,
                )
            })?;
            names.insert(rule.path.rule.clone());
        }
        Ok(names)
    }

    /// Look up a local rule by its name (rule in the main spec).
    pub fn get_rule(&self, name: &str) -> Option<&ExecutableRule> {
        let canonical_name = crate::parsing::ast::ascii_lowercase_logical_name(name.to_string());
        self.rules
            .values()
            .find(|r| r.path.rule == canonical_name && r.path.segments.is_empty())
    }

    /// Look up a normal-form cell by id.
    pub(crate) fn normal_form(&self, id: NormalFormId) -> &NormalForm {
        self.normal_forms.get(id.index()).unwrap_or_else(|| {
            panic!(
                "BUG: NormalFormId {} out of range (table len {})",
                id.index(),
                self.normal_forms.len()
            )
        })
    }

    /// Statically-needed data paths for a set of rules (Show).
    ///
    /// Every DataPath leaf in the rules' normalized bodies, walking the shared
    /// DAG (including rule-embed use-sites). Normalize has already removed
    /// constant-dead unless arms; remaining arms are all included. Overlay-aware
    /// last-wins pruning for a concrete `run` is done by the tree evaluator into
    /// per-rule `RuleResult.missing_data` — not here.
    pub fn collect_needed_data_paths(
        &self,
        rule_names: &[String],
    ) -> Result<HashSet<DataPath>, Error> {
        let mut needed: HashSet<DataPath> = HashSet::new();
        for rule_name in rule_names {
            let rule = self.get_rule(rule_name).ok_or_else(|| {
                Error::request(
                    format!(
                        "Rule '{}' not found in spec '{}'",
                        rule_name, self.spec_name
                    ),
                    None::<String>,
                )
            })?;
            collect_structural_data_paths(self, rule.normal_form, &mut needed);
        }
        Ok(needed)
    }
}

/// Append every DataPath leaf reachable from `id` to `out`.
///
/// Local rule roots use [`ExecutionPlan::structural_needed`] when present.
/// Other ids walk the NormalForm DAG (each `NormalFormId` at most once).
pub(crate) fn collect_structural_data_paths(
    plan: &ExecutionPlan,
    id: NormalFormId,
    out: &mut HashSet<DataPath>,
) {
    if let Some(needed) = plan.structural_needed.get(&id) {
        out.extend(needed.iter().cloned());
        return;
    }
    let mut visited = HashSet::new();
    collect_structural_data_paths_dag(plan, id, out, &mut visited);
}

fn collect_structural_data_paths_dag(
    plan: &ExecutionPlan,
    id: NormalFormId,
    out: &mut HashSet<DataPath>,
    visited: &mut HashSet<NormalFormId>,
) {
    use crate::planning::normalize::LeafKind;
    use crate::planning::normalize::NormalFormKind;
    if !visited.insert(id) {
        return;
    }
    #[cfg(test)]
    record_structural_dag_visit();
    let nf = plan.normal_form(id);
    match &nf.kind {
        NormalFormKind::Leaf(LeafKind::DataPath(path)) => {
            out.insert(path.clone());
        }
        NormalFormKind::Sum(children)
        | NormalFormKind::Product(children)
        | NormalFormKind::And(children) => {
            for child in children {
                collect_structural_data_paths_dag(plan, *child, out, visited);
            }
        }
        NormalFormKind::Subtract(a, b)
        | NormalFormKind::Divide(a, b)
        | NormalFormKind::Power(a, b)
        | NormalFormKind::Modulo(a, b)
        | NormalFormKind::Comparison(a, _, b)
        | NormalFormKind::RangeLiteral(a, b)
        | NormalFormKind::RangeContainment(a, b) => {
            collect_structural_data_paths_dag(plan, *a, out, visited);
            collect_structural_data_paths_dag(plan, *b, out, visited);
        }
        NormalFormKind::Negate(x)
        | NormalFormKind::Reciprocal(x)
        | NormalFormKind::Not(x)
        | NormalFormKind::MathOp(_, x)
        | NormalFormKind::UnitConversion(x, _)
        | NormalFormKind::DateRelative(_, x)
        | NormalFormKind::DateCalendar(_, _, x)
        | NormalFormKind::PastFutureRange(_, x)
        | NormalFormKind::ResultIsVeto(x) => {
            collect_structural_data_paths_dag(plan, *x, out, visited);
        }
        NormalFormKind::Piecewise(arms) => {
            for (cond, body) in arms {
                collect_structural_data_paths_dag(plan, *cond, out, visited);
                collect_structural_data_paths_dag(plan, *body, out, visited);
            }
        }
        NormalFormKind::Now | NormalFormKind::Veto(_) => {}
        NormalFormKind::Leaf(LeafKind::Literal(_)) => {}
    }
}

fn order_paths_by_plan_data(plan: &ExecutionPlan, paths: HashSet<DataPath>) -> Vec<DataPath> {
    plan.data
        .keys()
        .filter(|path| paths.contains(*path))
        .cloned()
        .collect()
}

/// Order a set of data paths by [`ExecutionPlan::data`] declaration order.
pub(crate) fn order_data_paths_by_plan(
    plan: &ExecutionPlan,
    paths: HashSet<DataPath>,
) -> Vec<DataPath> {
    order_paths_by_plan_data(plan, paths)
}

/// Unrestricted structural DataPath leaves for every local rule root.
fn fill_structural_needed(plan: &mut ExecutionPlan) {
    let roots: Vec<NormalFormId> = plan
        .rules
        .values()
        .filter(|rule| rule.path.segments.is_empty())
        .map(|rule| rule.normal_form)
        .collect();

    for root in roots {
        if plan.structural_needed.contains_key(&root) {
            continue;
        }
        let mut needed = HashSet::new();
        let mut visited = HashSet::new();
        collect_structural_data_paths_dag(plan, root, &mut needed, &mut visited);
        plan.structural_needed.insert(root, needed);
    }
}

fn collect_control_node_ids(
    plan: &ExecutionPlan,
    id: NormalFormId,
    out: &mut Vec<NormalFormId>,
    visited: &mut HashSet<NormalFormId>,
) {
    use crate::planning::normalize::NormalFormKind;
    if !visited.insert(id) {
        return;
    }
    let nf = plan.normal_form(id);
    match &nf.kind {
        NormalFormKind::And(_) | NormalFormKind::Piecewise(_) => {
            out.push(id);
        }
        _ => {}
    }
    match &nf.kind {
        NormalFormKind::Leaf(_) | NormalFormKind::Veto(_) | NormalFormKind::Now => {}
        NormalFormKind::Sum(children)
        | NormalFormKind::Product(children)
        | NormalFormKind::And(children) => {
            for child in children {
                collect_control_node_ids(plan, *child, out, visited);
            }
        }
        NormalFormKind::Subtract(a, b)
        | NormalFormKind::Divide(a, b)
        | NormalFormKind::Power(a, b)
        | NormalFormKind::Modulo(a, b)
        | NormalFormKind::Comparison(a, _, b)
        | NormalFormKind::RangeLiteral(a, b)
        | NormalFormKind::RangeContainment(a, b) => {
            collect_control_node_ids(plan, *a, out, visited);
            collect_control_node_ids(plan, *b, out, visited);
        }
        NormalFormKind::Negate(x)
        | NormalFormKind::Reciprocal(x)
        | NormalFormKind::Not(x)
        | NormalFormKind::MathOp(_, x)
        | NormalFormKind::UnitConversion(x, _)
        | NormalFormKind::DateRelative(_, x)
        | NormalFormKind::DateCalendar(_, _, x)
        | NormalFormKind::PastFutureRange(_, x)
        | NormalFormKind::ResultIsVeto(x) => {
            collect_control_node_ids(plan, *x, out, visited);
        }
        NormalFormKind::Piecewise(arms) => {
            for (cond, body) in arms {
                collect_control_node_ids(plan, *cond, out, visited);
                collect_control_node_ids(plan, *body, out, visited);
            }
        }
    }
}

/// Memoized DataPath leaves under every node reachable from `root`.
fn build_leaf_sets(
    plan: &ExecutionPlan,
    root: NormalFormId,
) -> HashMap<NormalFormId, HashSet<DataPath>> {
    use crate::planning::normalize::LeafKind;
    use crate::planning::normalize::NormalFormKind;

    let mut memo: HashMap<NormalFormId, HashSet<DataPath>> = HashMap::new();

    fn leaf_set(
        plan: &ExecutionPlan,
        id: NormalFormId,
        memo: &mut HashMap<NormalFormId, HashSet<DataPath>>,
    ) -> HashSet<DataPath> {
        if let Some(cached) = memo.get(&id) {
            return cached.clone();
        }
        #[cfg(test)]
        record_structural_dag_visit();
        let nf = plan.normal_form(id);
        let result = match &nf.kind {
            NormalFormKind::Leaf(LeafKind::DataPath(path)) => {
                let mut set = HashSet::new();
                set.insert(path.clone());
                set
            }
            NormalFormKind::Leaf(LeafKind::Literal(_))
            | NormalFormKind::Now
            | NormalFormKind::Veto(_) => HashSet::new(),
            NormalFormKind::Sum(children)
            | NormalFormKind::Product(children)
            | NormalFormKind::And(children) => {
                let mut set = HashSet::new();
                for child in children {
                    set.extend(leaf_set(plan, *child, memo));
                }
                set
            }
            NormalFormKind::Subtract(a, b)
            | NormalFormKind::Divide(a, b)
            | NormalFormKind::Power(a, b)
            | NormalFormKind::Modulo(a, b)
            | NormalFormKind::Comparison(a, _, b)
            | NormalFormKind::RangeLiteral(a, b)
            | NormalFormKind::RangeContainment(a, b) => {
                let mut set = leaf_set(plan, *a, memo);
                set.extend(leaf_set(plan, *b, memo));
                set
            }
            NormalFormKind::Negate(x)
            | NormalFormKind::Reciprocal(x)
            | NormalFormKind::Not(x)
            | NormalFormKind::MathOp(_, x)
            | NormalFormKind::UnitConversion(x, _)
            | NormalFormKind::DateRelative(_, x)
            | NormalFormKind::DateCalendar(_, _, x)
            | NormalFormKind::PastFutureRange(_, x)
            | NormalFormKind::ResultIsVeto(x) => leaf_set(plan, *x, memo),
            NormalFormKind::Piecewise(arms) => {
                let mut set = HashSet::new();
                for (cond, body) in arms {
                    set.extend(leaf_set(plan, *cond, memo));
                    set.extend(leaf_set(plan, *body, memo));
                }
                set
            }
        };
        memo.insert(id, result.clone());
        result
    }

    leaf_set(plan, root, &mut memo);
    memo
}

/// DataPaths reachable from `root` without entering `skip`.
fn bypass_leaves(
    plan: &ExecutionPlan,
    root: NormalFormId,
    skip: NormalFormId,
) -> HashSet<DataPath> {
    use crate::planning::normalize::LeafKind;
    use crate::planning::normalize::NormalFormKind;

    let mut out = HashSet::new();
    let mut visited = HashSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if id == skip {
            continue;
        }
        if !visited.insert(id) {
            continue;
        }
        #[cfg(test)]
        record_structural_dag_visit();
        let nf = plan.normal_form(id);
        match &nf.kind {
            NormalFormKind::Leaf(LeafKind::DataPath(path)) => {
                out.insert(path.clone());
            }
            NormalFormKind::Leaf(LeafKind::Literal(_))
            | NormalFormKind::Now
            | NormalFormKind::Veto(_) => {}
            NormalFormKind::Sum(children)
            | NormalFormKind::Product(children)
            | NormalFormKind::And(children) => {
                stack.extend(children.iter().copied());
            }
            NormalFormKind::Subtract(a, b)
            | NormalFormKind::Divide(a, b)
            | NormalFormKind::Power(a, b)
            | NormalFormKind::Modulo(a, b)
            | NormalFormKind::Comparison(a, _, b)
            | NormalFormKind::RangeLiteral(a, b)
            | NormalFormKind::RangeContainment(a, b) => {
                stack.push(*a);
                stack.push(*b);
            }
            NormalFormKind::Negate(x)
            | NormalFormKind::Reciprocal(x)
            | NormalFormKind::Not(x)
            | NormalFormKind::MathOp(_, x)
            | NormalFormKind::UnitConversion(x, _)
            | NormalFormKind::DateRelative(_, x)
            | NormalFormKind::DateCalendar(_, _, x)
            | NormalFormKind::PastFutureRange(_, x)
            | NormalFormKind::ResultIsVeto(x) => {
                stack.push(*x);
            }
            NormalFormKind::Piecewise(arms) => {
                for (cond, body) in arms {
                    stack.push(*cond);
                    stack.push(*body);
                }
            }
        }
    }
    out
}

/// Paths exclusive to `dead_children` under rule-root exclusivity.
fn released_for_dead_children(
    plan: &ExecutionPlan,
    bypass: &HashSet<DataPath>,
    slots: &HashMap<DataPath, HashSet<NormalFormId>>,
    dead_children: &HashSet<NormalFormId>,
) -> Vec<DataPath> {
    let mut released = HashSet::new();
    for (path, path_slots) in slots {
        if bypass.contains(path) {
            continue;
        }
        if path_slots.is_empty() {
            continue;
        }
        if path_slots.is_subset(dead_children) {
            released.insert(path.clone());
        }
    }
    order_paths_by_plan_data(plan, released)
}

fn fill_data_releases(plan: &mut ExecutionPlan) {
    use crate::planning::normalize::NormalFormKind;

    let local_rules: Vec<(RulePath, NormalFormId)> = plan
        .rules
        .values()
        .filter(|rule| rule.path.segments.is_empty())
        .map(|rule| (rule.path.clone(), rule.normal_form))
        .collect();

    for (rule_path, root) in local_rules {
        if !plan.structural_needed.contains_key(&root) {
            panic!(
                "BUG: structural_needed missing for local rule root {root:?} (rule '{}')",
                rule_path.rule
            );
        }

        let leaf_sets = build_leaf_sets(plan, root);

        let mut control_ids = Vec::new();
        let mut visited = HashSet::new();
        collect_control_node_ids(plan, root, &mut control_ids, &mut visited);

        let mut rule_releases: HashMap<NormalFormId, ControlDataReleases> = HashMap::new();

        for control_id in control_ids {
            let bypass = bypass_leaves(plan, root, control_id);

            // Owned child ids only — do not clone NormalFormKind (wide Piecewise).
            let (and_right, piecewise_arms, children): (
                Option<NormalFormId>,
                Vec<(NormalFormId, NormalFormId)>,
                Vec<NormalFormId>,
            ) = match &plan.normal_form(control_id).kind {
                NormalFormKind::And(and_children) => {
                    assert!(
                        and_children.len() == 2,
                        "BUG: And must be binary after lower (got {} children)",
                        and_children.len()
                    );
                    (Some(and_children[1]), Vec::new(), and_children.clone())
                }
                NormalFormKind::Piecewise(arms) => {
                    assert!(!arms.is_empty(), "BUG: empty Piecewise");
                    let arms = arms.clone();
                    let mut children = Vec::with_capacity(arms.len() * 2);
                    for (cond, body) in &arms {
                        children.push(*cond);
                        children.push(*body);
                    }
                    (None, arms, children)
                }
                other => panic!(
                    "BUG: control id {:?} is not And or Piecewise: {other:?}",
                    control_id
                ),
            };

            let mut slots: HashMap<DataPath, HashSet<NormalFormId>> = HashMap::new();
            for child in &children {
                let leaves = leaf_sets.get(child).unwrap_or_else(|| {
                    panic!("BUG: leaf set missing for control child {child:?} under {control_id:?}")
                });
                for path in leaves {
                    slots.entry(path.clone()).or_default().insert(*child);
                }
            }

            let releases = if let Some(right) = and_right {
                let mut dead = HashSet::new();
                dead.insert(right);
                ControlDataReleases::And {
                    on_left_false: released_for_dead_children(plan, &bypass, &slots, &dead),
                }
            } else {
                let arms = &piecewise_arms;
                let mut on_arm_not_taken = vec![Vec::new(); arms.len()];
                let mut on_arm_taken = vec![Vec::new(); arms.len()];
                for i in 1..arms.len() {
                    let mut not_taken_dead = HashSet::new();
                    not_taken_dead.insert(arms[i].1);
                    on_arm_not_taken[i] =
                        released_for_dead_children(plan, &bypass, &slots, &not_taken_dead);

                    let mut taken_dead = HashSet::new();
                    taken_dead.insert(arms[0].1);
                    for (k, (cond, body)) in arms.iter().enumerate().skip(1) {
                        if k != i {
                            taken_dead.insert(*body);
                        }
                        if k < i {
                            taken_dead.insert(*cond);
                        }
                    }
                    on_arm_taken[i] =
                        released_for_dead_children(plan, &bypass, &slots, &taken_dead);
                }
                let mut default_wins_dead = HashSet::new();
                for (_, body) in arms.iter().skip(1) {
                    default_wins_dead.insert(*body);
                }
                ControlDataReleases::Piecewise {
                    on_arm_not_taken,
                    on_arm_taken,
                    on_default_wins: released_for_dead_children(
                        plan,
                        &bypass,
                        &slots,
                        &default_wins_dead,
                    ),
                }
            };
            rule_releases.insert(control_id, releases);
        }

        plan.data_releases.insert(rule_path, rule_releases);
    }
}

pub(crate) fn validate_value_against_type(
    expected_type: &LemmaType,
    value: &LiteralValue,
    unit_index: &HashMap<String, Arc<LemmaType>>,
) -> Result<(), String> {
    use crate::computation::rational::{
        checked_mul, rational_new, try_pow_i32, BigInt, RationalInteger,
    };
    use crate::planning::semantics::TypeSpecification;

    fn exceeds_decimal_places(magnitude: &RationalInteger, max_decimals: u8) -> bool {
        let scale = match try_pow_i32(&rational_new(10, 1), i32::from(max_decimals)) {
            Ok(value) => value,
            Err(_) => return true,
        };
        let scaled = match checked_mul(magnitude, &scale) {
            Ok(value) => value,
            Err(_) => return true,
        };
        match RationalInteger::try_reduce_ref(&scaled) {
            Ok(reduced) => *reduced.denom() != BigInt::one(),
            Err(_) => true,
        }
    }

    fn format_rational_for_validation_message(
        expected_type: &crate::planning::semantics::LemmaType,
        magnitude: &RationalInteger,
    ) -> String {
        expected_type
            .try_materialize_rational_as_decimal_string(magnitude)
            .unwrap_or_else(|_| magnitude.display_str())
    }

    match (&expected_type.specifications, &value.value) {
        (
            TypeSpecification::Number {
                minimum,
                maximum,
                decimals,
                ..
            },
            ValueKind::Number(n),
        ) => {
            if let Some(d) = decimals {
                if exceeds_decimal_places(n, *d) {
                    return Err(format!(
                        "{} exceeds decimals constraint {d}",
                        n.display_str()
                    ));
                }
            }
            if let Some(min) = minimum {
                if n < min {
                    return Err(format!(
                        "{} is below minimum {}",
                        format_rational_for_validation_message(expected_type, n),
                        format_rational_for_validation_message(expected_type, min)
                    ));
                }
            }
            if let Some(max) = maximum {
                if n > max {
                    return Err(format!(
                        "{} is above maximum {}",
                        format_rational_for_validation_message(expected_type, n),
                        format_rational_for_validation_message(expected_type, max)
                    ));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Measure {
                minimum,
                maximum,
                decimals,
                units,
                ..
            },
            ValueKind::Measure(magnitude, signature),
        ) => {
            use crate::computation::rational::checked_div;
            use crate::planning::semantics::measure_declared_bound_to_canonical;
            let unit = signature
                .first()
                .map(|(n, _)| n.as_str())
                .expect("BUG: Measure value has empty signature in execution plan validation");
            let measure_unit = units.get(unit)?;
            let factor = &measure_unit.factor;
            let in_unit = checked_div(magnitude, factor).map_err(|failure| {
                format!("cannot de-canonicalize measure for validation: {failure}")
            })?;
            if let Some(d) = decimals {
                if exceeds_decimal_places(&in_unit, *d) {
                    return Err(format!(
                        "{} {unit} exceeds decimals constraint {d}",
                        in_unit.display_str()
                    ));
                }
            }
            if let Some(bound) = minimum {
                let canonical_min = measure_declared_bound_to_canonical(
                    &bound.0,
                    &bound.1,
                    units,
                    expected_type.name().as_str(),
                    "minimum",
                )?;
                if magnitude < &canonical_min {
                    let min_in_unit = checked_div(&canonical_min, factor).map_err(|failure| {
                        format!("cannot de-canonicalize minimum for validation: {failure}")
                    })?;
                    let value_display = format!(
                        "{} {}",
                        format_rational_for_validation_message(expected_type, &in_unit),
                        unit
                    );
                    let bound_display = format!(
                        "{} {}",
                        format_rational_for_validation_message(expected_type, &min_in_unit),
                        measure_unit.name
                    );
                    return Err(format!("{value_display} is below minimum {bound_display}"));
                }
            }
            if let Some(bound) = maximum {
                let canonical_max = measure_declared_bound_to_canonical(
                    &bound.0,
                    &bound.1,
                    units,
                    expected_type.name().as_str(),
                    "maximum",
                )?;
                if magnitude > &canonical_max {
                    let max_in_unit = checked_div(&canonical_max, factor).map_err(|failure| {
                        format!("cannot de-canonicalize maximum for validation: {failure}")
                    })?;
                    let value_display = format!(
                        "{} {}",
                        format_rational_for_validation_message(expected_type, &in_unit),
                        unit
                    );
                    let bound_display = format!(
                        "{} {}",
                        format_rational_for_validation_message(expected_type, &max_in_unit),
                        measure_unit.name
                    );
                    return Err(format!("{value_display} is above maximum {bound_display}"));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Text {
                length, options, ..
            },
            ValueKind::Text(s),
        ) => {
            let len = s.chars().count();
            if let Some(exact) = length {
                if len != *exact {
                    return Err(format!(
                        "'{}' has length {} but required length is {}",
                        s, len, exact
                    ));
                }
            }
            if !options.is_empty() && !options.iter().any(|opt| opt == s) {
                return Err(format!(
                    "'{}' is not in allowed options: {}",
                    s,
                    options.join(", ")
                ));
            }
            Ok(())
        }
        (
            TypeSpecification::Ratio {
                minimum,
                maximum,
                decimals,
                units,
                ..
            },
            ValueKind::Ratio(r, unit_name),
        ) => {
            use crate::computation::rational::checked_mul;

            if let Some(d) = decimals {
                let magnitude_for_decimals = match unit_name.as_deref() {
                    Some(unit) => {
                        let ratio_unit = units.get(unit)?;
                        checked_mul(r, &ratio_unit.value).map_err(|failure| failure.to_string())?
                    }
                    None => r.clone(),
                };
                if exceeds_decimal_places(&magnitude_for_decimals, *d) {
                    return Err(format!(
                        "{} exceeds decimals constraint {d}",
                        magnitude_for_decimals.display_str()
                    ));
                }
            }
            if let Some(type_minimum) = minimum {
                if r < type_minimum {
                    let message = match unit_name.as_deref() {
                        Some(unit) => {
                            let ratio_unit = units.get(unit)?;
                            let value_per_unit = checked_mul(r, &ratio_unit.value)
                                .map_err(|failure| failure.to_string())?;
                            let bound_per_unit = ratio_unit.minimum.clone().expect(
                                "BUG: RatioUnit.minimum missing after type minimum set by sync_ratio_units_from_canonical",
                            );
                            format!(
                                "{} {unit} is below minimum {} {unit}",
                                format_rational_for_validation_message(
                                    expected_type,
                                    &value_per_unit
                                ),
                                format_rational_for_validation_message(
                                    expected_type,
                                    &bound_per_unit.clone()
                                ),
                            )
                        }
                        None => format!(
                            "{} is below minimum {}",
                            format_rational_for_validation_message(expected_type, r),
                            format_rational_for_validation_message(expected_type, type_minimum),
                        ),
                    };
                    return Err(message);
                }
            }
            if let Some(type_maximum) = maximum {
                if r > type_maximum {
                    let message = match unit_name.as_deref() {
                        Some(unit) => {
                            let ratio_unit = units.get(unit)?;
                            let value_per_unit = checked_mul(r, &ratio_unit.value)
                                .map_err(|failure| failure.to_string())?;
                            let bound_per_unit = ratio_unit.maximum.clone().expect(
                                "BUG: RatioUnit.maximum missing after type maximum set by sync_ratio_units_from_canonical",
                            );
                            format!(
                                "{} {unit} is above maximum {} {unit}",
                                format_rational_for_validation_message(
                                    expected_type,
                                    &value_per_unit
                                ),
                                format_rational_for_validation_message(
                                    expected_type,
                                    &bound_per_unit.clone()
                                ),
                            )
                        }
                        None => format!(
                            "{} is above maximum {}",
                            format_rational_for_validation_message(expected_type, r),
                            format_rational_for_validation_message(expected_type, type_maximum),
                        ),
                    };
                    return Err(message);
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Ratio {
                minimum,
                maximum,
                decimals,
                units: _,
                ..
            },
            ValueKind::Number(n),
        ) => {
            if let Some(d) = decimals {
                if exceeds_decimal_places(n, *d) {
                    return Err(format!(
                        "{} exceeds decimals constraint {d}",
                        n.display_str()
                    ));
                }
            }
            if let Some(type_minimum) = minimum {
                if n < type_minimum {
                    return Err(format!(
                        "{} is below minimum {}",
                        format_rational_for_validation_message(expected_type, n),
                        format_rational_for_validation_message(expected_type, type_minimum)
                    ));
                }
            }
            if let Some(type_maximum) = maximum {
                if n > type_maximum {
                    return Err(format!(
                        "{} is above maximum {}",
                        format_rational_for_validation_message(expected_type, n),
                        format_rational_for_validation_message(expected_type, type_maximum)
                    ));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Date {
                minimum, maximum, ..
            },
            ValueKind::Date(dt),
        ) => {
            use crate::planning::semantics::{compare_semantic_dates, date_time_to_semantic};
            use std::cmp::Ordering;
            if let Some(min) = minimum {
                let min_sem = date_time_to_semantic(min);
                if compare_semantic_dates(dt, &min_sem) == Ordering::Less {
                    return Err(format!("{} is below minimum {}", dt, min));
                }
            }
            if let Some(max) = maximum {
                let max_sem = date_time_to_semantic(max);
                if compare_semantic_dates(dt, &max_sem) == Ordering::Greater {
                    return Err(format!("{} is above maximum {}", dt, max));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Time {
                minimum, maximum, ..
            },
            ValueKind::Time(t),
        ) => {
            use crate::planning::semantics::{compare_semantic_times, time_to_semantic};
            use std::cmp::Ordering;
            if let Some(min) = minimum {
                let min_sem = time_to_semantic(min);
                if compare_semantic_times(t, &min_sem) == Ordering::Less {
                    return Err(format!("{} is below minimum {}", t, min));
                }
            }
            if let Some(max) = maximum {
                let max_sem = time_to_semantic(max);
                if compare_semantic_times(t, &max_sem) == Ordering::Greater {
                    return Err(format!("{} is above maximum {}", t, max));
                }
            }
            Ok(())
        }
        (TypeSpecification::Boolean { .. }, ValueKind::Boolean(_)) => Ok(()),
        (
            range_spec @ (TypeSpecification::NumberRange { .. }
            | TypeSpecification::DateRange { .. }
            | TypeSpecification::TimeRange { .. }
            | TypeSpecification::MeasureRange { .. }
            | TypeSpecification::RatioRange { .. }),
            ValueKind::Range(left, right),
        ) => validate_range_literal(
            expected_type,
            range_spec,
            left.as_ref(),
            right.as_ref(),
            unit_index,
        ),
        (TypeSpecification::Veto { .. }, _) | (TypeSpecification::Undetermined, _) => Ok(()),
        (spec, value_kind) if !value_kind_matches_spec(value_kind, spec) => unreachable!(
            "BUG: validate_value_against_type called with mismatched type/value: \
             spec={:?}, value={:?} — typing must be enforced before validation",
            spec, value_kind
        ),
        (spec, value_kind) => unreachable!(
            "BUG: validate_value_against_type missed a value_kind_matches_spec pair: \
             spec={:?}, value={:?}",
            spec, value_kind
        ),
    }
}

fn validate_range_literal(
    expected_type: &LemmaType,
    range_spec: &TypeSpecification,
    left: &LiteralValue,
    right: &LiteralValue,
    unit_index: &HashMap<String, Arc<LemmaType>>,
) -> Result<(), String> {
    use crate::computation::comparison_operation;
    use crate::computation::UnitResolutionContext;
    use crate::evaluation::operations::OperationResult;
    use crate::planning::semantics::{
        compare_semantic_dates, compare_semantic_times, measure_declared_bound_to_canonical,
        ValueKind,
    };
    use std::cmp::Ordering;
    use std::sync::Arc;

    let mut element_spec = range_spec
        .element_from_range()
        .expect("BUG: element_from_range missing arm for validated range");
    if let TypeSpecification::Measure {
        units,
        decomposition,
        ..
    } = &mut element_spec
    {
        if decomposition.is_none() && !units.0.is_empty() {
            *decomposition = Some([(expected_type.name(), 1i32)].into_iter().collect());
        }
    }
    let element_type = Arc::new(LemmaType::primitive(element_spec));
    let left = LiteralValue {
        value: left.value.clone(),
        lemma_type: Arc::clone(&element_type),
    };
    let right = LiteralValue {
        value: right.value.clone(),
        lemma_type: Arc::clone(&element_type),
    };
    validate_value_against_type(element_type.as_ref(), &left, unit_index)?;
    validate_value_against_type(element_type.as_ref(), &right, unit_index)?;

    let ordering = match (&left.value, &right.value) {
        (ValueKind::Number(l), ValueKind::Number(r)) => l.cmp(r),
        (ValueKind::Date(l), ValueKind::Date(r)) => compare_semantic_dates(l, r),
        (ValueKind::Time(l), ValueKind::Time(r)) => compare_semantic_times(l, r),
        (ValueKind::Ratio(l, _), ValueKind::Ratio(r, _)) => l.cmp(r),
        (ValueKind::Measure(l, ls), ValueKind::Measure(r, rs)) => {
            if ls != rs {
                unreachable!(
                    "BUG: range endpoints have mismatched unit signatures after typing: {ls:?} vs {rs:?}"
                );
            }
            l.cmp(r)
        }
        (left_kind, right_kind) => unreachable!(
            "BUG: range endpoints have mismatched value kinds after typing: {left_kind:?} vs {right_kind:?}"
        ),
    };
    if ordering == Ordering::Greater {
        return Err(format!(
            "range left endpoint {left} is above right endpoint {right}"
        ));
    }

    let range_lit = LiteralValue {
        value: ValueKind::Range(Box::new(left.clone()), Box::new(right.clone())),
        lemma_type: Arc::new(expected_type.clone()),
    };

    let compare_width = |bound: &LiteralValue,
                         op: ComparisonComputation,
                         fail_msg: String|
     -> Result<(), String> {
        match comparison_operation(
            &range_lit,
            &op,
            bound,
            UnitResolutionContext::WithIndex(unit_index),
        ) {
            OperationResult::Value(result) => match &result.value {
                ValueKind::Boolean(true) => Ok(()),
                ValueKind::Boolean(false) => Err(fail_msg),
                other => unreachable!("BUG: width comparison must return boolean, got {other:?}"),
            },
            OperationResult::Veto(veto) => Err(veto.to_string()),
        }
    };

    match range_spec {
        TypeSpecification::NumberRange {
            minimum, maximum, ..
        } => {
            if let Some(min_w) = minimum {
                compare_width(
                    &LiteralValue::number(min_w.clone()),
                    ComparisonComputation::GreaterThanOrEqual,
                    format!("span is below minimum width {}", min_w.display_str()),
                )?;
            }
            if let Some(max_w) = maximum {
                compare_width(
                    &LiteralValue::number(max_w.clone()),
                    ComparisonComputation::LessThanOrEqual,
                    format!("span is above maximum width {}", max_w.display_str()),
                )?;
            }
        }
        TypeSpecification::RatioRange {
            minimum, maximum, ..
        } => {
            if let Some(min_w) = minimum {
                compare_width(
                    &LiteralValue::ratio(min_w.clone(), None),
                    ComparisonComputation::GreaterThanOrEqual,
                    format!("span is below minimum width {}", min_w.display_str()),
                )?;
            }
            if let Some(max_w) = maximum {
                compare_width(
                    &LiteralValue::ratio(max_w.clone(), None),
                    ComparisonComputation::LessThanOrEqual,
                    format!("span is above maximum width {}", max_w.display_str()),
                )?;
            }
        }
        TypeSpecification::MeasureRange {
            minimum,
            maximum,
            units,
            ..
        } => {
            if let Some(min_w) = minimum {
                let canonical = measure_declared_bound_to_canonical(
                    &min_w.0, &min_w.1, units, "range", "minimum",
                )?;
                let bound = LiteralValue::measure_with_type(
                    canonical,
                    min_w.1.clone(),
                    Arc::clone(&element_type),
                );
                compare_width(
                    &bound,
                    ComparisonComputation::GreaterThanOrEqual,
                    format!(
                        "span is below minimum width {} {}",
                        min_w.0.display_str(),
                        min_w.1
                    ),
                )?;
            }
            if let Some(max_w) = maximum {
                let canonical = measure_declared_bound_to_canonical(
                    &max_w.0, &max_w.1, units, "range", "maximum",
                )?;
                let bound = LiteralValue::measure_with_type(
                    canonical,
                    max_w.1.clone(),
                    Arc::clone(&element_type),
                );
                compare_width(
                    &bound,
                    ComparisonComputation::LessThanOrEqual,
                    format!(
                        "span is above maximum width {} {}",
                        max_w.0.display_str(),
                        max_w.1
                    ),
                )?;
            }
        }
        TypeSpecification::DateRange {
            minimum, maximum, ..
        }
        | TypeSpecification::TimeRange {
            minimum, maximum, ..
        } => {
            let allow_calendar = matches!(range_spec, TypeSpecification::DateRange { .. });
            let resolve_bound = |bound: &(
                crate::computation::rational::RationalInteger,
                String,
            ),
                                 command: &str|
             -> Result<LiteralValue, String> {
                let owner = unit_index.get(bound.1.as_str()).ok_or_else(|| {
                        format!(
                            "{command} width unit '{}' is not in scope (add `uses lemma units` or declare the unit)",
                            bound.1
                        )
                    })?;
                if allow_calendar {
                    if !owner.is_duration_like() && !owner.is_calendar_like() {
                        return Err(format!(
                            "{command} width unit '{}' must be a duration or calendar unit",
                            bound.1
                        ));
                    }
                } else if !owner.is_duration_like() {
                    return Err(format!(
                        "{command} width unit '{}' must be a duration unit",
                        bound.1
                    ));
                }
                let TypeSpecification::Measure { units, .. } = &owner.specifications else {
                    return Err(format!(
                        "{command} width unit '{}' must resolve to a measure type",
                        bound.1
                    ));
                };
                let type_name = owner.name();
                let canonical = measure_declared_bound_to_canonical(
                    &bound.0,
                    &bound.1,
                    units,
                    type_name.as_str(),
                    command,
                )?;
                Ok(LiteralValue::measure_with_type(
                    canonical,
                    bound.1.clone(),
                    Arc::clone(owner),
                ))
            };
            if let Some(min_w) = minimum {
                let bound = resolve_bound(min_w, "minimum")?;
                compare_width(
                    &bound,
                    ComparisonComputation::GreaterThanOrEqual,
                    format!(
                        "span is below minimum width {} {}",
                        min_w.0.display_str(),
                        min_w.1
                    ),
                )?;
            }
            if let Some(max_w) = maximum {
                let bound = resolve_bound(max_w, "maximum")?;
                compare_width(
                    &bound,
                    ComparisonComputation::LessThanOrEqual,
                    format!(
                        "span is above maximum width {} {}",
                        max_w.0.display_str(),
                        max_w.1
                    ),
                )?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn validate_literal_data_against_types(plan: &ExecutionPlan) -> Vec<Error> {
    let mut errors = Vec::new();

    for (data_path, data_definition) in &plan.data {
        let (expected_type, lit) = match data_definition {
            DataDefinition::Value { value, .. } => (&value.lemma_type, value),
            DataDefinition::TypeDeclaration { .. }
            | DataDefinition::Import { .. }
            | DataDefinition::Reference { .. } => continue,
        };

        if let Err(msg) =
            validate_value_against_type(expected_type, lit, plan.expression_unit_index())
        {
            let source = data_definition.source().clone();
            errors.push(Error::validation(
                format!(
                    "Invalid value for data {} (expected {}): {}",
                    data_path,
                    expected_type.name().as_str(),
                    msg
                ),
                Some(source),
                None::<String>,
            ));
        }
    }

    errors
}

fn validate_unit_conversion_targets(plan: &ExecutionPlan) -> Result<(), Error> {
    use crate::planning::normalize::NormalFormKind;

    fn walk(
        plan: &ExecutionPlan,
        id: NormalFormId,
        errors: &mut Vec<Error>,
        visited: &mut HashSet<NormalFormId>,
        spec_name: &str,
    ) {
        if !visited.insert(id) {
            return;
        }
        let nf = plan.normal_form(id);
        match &nf.kind {
            NormalFormKind::UnitConversion(inner, target) => {
                if let Some((unit_name, owning_type)) =
                    crate::computation::units::conversion_target_declares_unit(target)
                {
                    if !crate::computation::units::owning_type_declares_unit_name(
                        owning_type.as_ref(),
                        unit_name,
                    ) {
                        errors.push(Error::validation(
                            format!(
                                "Unit conversion target '{unit_name}' is not declared on owning type '{}'",
                                owning_type.name()
                            ),
                            None::<Source>,
                            Some(spec_name.to_string()),
                        ));
                    }
                }
                walk(plan, *inner, errors, visited, spec_name);
            }
            NormalFormKind::Leaf(_) | NormalFormKind::Veto(_) | NormalFormKind::Now => {}
            NormalFormKind::Sum(children)
            | NormalFormKind::Product(children)
            | NormalFormKind::And(children) => {
                for child in children {
                    walk(plan, *child, errors, visited, spec_name);
                }
            }
            NormalFormKind::Subtract(a, b)
            | NormalFormKind::Divide(a, b)
            | NormalFormKind::Power(a, b)
            | NormalFormKind::Modulo(a, b)
            | NormalFormKind::Comparison(a, _, b)
            | NormalFormKind::RangeLiteral(a, b)
            | NormalFormKind::RangeContainment(a, b) => {
                walk(plan, *a, errors, visited, spec_name);
                walk(plan, *b, errors, visited, spec_name);
            }
            NormalFormKind::Negate(x)
            | NormalFormKind::Reciprocal(x)
            | NormalFormKind::Not(x)
            | NormalFormKind::MathOp(_, x)
            | NormalFormKind::DateRelative(_, x)
            | NormalFormKind::DateCalendar(_, _, x)
            | NormalFormKind::PastFutureRange(_, x)
            | NormalFormKind::ResultIsVeto(x) => {
                walk(plan, *x, errors, visited, spec_name);
            }
            NormalFormKind::Piecewise(arms) => {
                for (condition, result) in arms.iter() {
                    walk(plan, *condition, errors, visited, spec_name);
                    walk(plan, *result, errors, visited, spec_name);
                }
            }
        }
    }

    let mut errors: Vec<Error> = Vec::new();
    let mut visited = HashSet::new();
    for rule in plan.rules.values() {
        walk(
            plan,
            rule.normal_form,
            &mut errors,
            &mut visited,
            &plan.spec_name,
        );
    }
    if let Some(error) = errors.into_iter().next() {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::{rational_new, rational_zero};
    use crate::evaluation::data_input::DataOverlay;
    use crate::evaluation::operations::{OperationResult, VetoType};
    use crate::literals::DateGranularity;
    use crate::literals::TimezoneValue;
    use crate::parsing::ast::DateTimeValue;
    use crate::planning::semantics::{DataDefinition, DataPath, PathSegment, TypeSpecification};
    use crate::Engine;
    use crate::{DataValueInput, ResourceLimits};
    use serde_json;
    use std::collections::HashMap;
    use std::str::FromStr;
    use std::sync::Arc;

    fn default_limits() -> ResourceLimits {
        ResourceLimits::default()
    }

    fn resolve_overlay(
        plan: &ExecutionPlan,
        values: HashMap<String, DataValueInput>,
    ) -> DataOverlay {
        DataOverlay::resolve(plan, values, &default_limits()).expect("resolve")
    }

    fn veto_reason<'a>(overlay: &'a DataOverlay, path: &DataPath) -> Option<&'a str> {
        match overlay.bindings.get(path) {
            Some(OperationResult::Veto(veto)) => Some(match veto {
                VetoType::Computation { message } => message.as_str(),
                other => panic!("expected Computation veto, got {other:?}"),
            }),
            _ => None,
        }
    }

    fn bound_value<'a>(overlay: &'a DataOverlay, path: &DataPath) -> Option<&'a LiteralValue> {
        overlay.bindings.get(path).and_then(OperationResult::value)
    }

    fn input_data(pairs: &[(&str, &str)]) -> HashMap<String, DataValueInput> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), DataValueInput::convenience(*v)))
            .collect()
    }

    #[test]
    fn test_with_raw_values() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
                r#"
                spec test
                data age: number -> suggest 25
                "#
                .to_string(),
            )])
            .unwrap();

        let plans = engine
            .plans
            .get_plans(None, "test")
            .expect("plans for test");
        let plan = plans.values().next().expect("plan");
        let data_path = DataPath::new(vec![], "age".to_string());

        let values = input_data(&[("age", "30")]);

        let overlay = resolve_overlay(plan, values);
        let updated_value = bound_value(&overlay, &data_path).expect("bound value");
        match &updated_value.value {
            crate::planning::semantics::ValueKind::Number(n) => {
                assert_eq!(n, &rational_new(30, 1));
            }
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    #[test]
    fn test_with_raw_values_type_mismatch() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
                r#"
                spec test
                data age: number
                "#
                .to_string(),
            )])
            .unwrap();

        let plans = engine
            .plans
            .get_plans(None, "test")
            .expect("plans for test");
        let plan = plans.values().next().expect("plan");

        let values = input_data(&[("age", "thirty")]);

        let overlay = resolve_overlay(plan, values);
        let data_path = DataPath::new(vec![], "age".to_string());
        match veto_reason(&overlay, &data_path) {
            Some(reason) => {
                assert!(
                    reason.contains("number"),
                    "type mismatch must record violation reason, got: {reason}"
                );
            }
            None => panic!("expected veto-bound data for age=thirty"),
        }
    }

    #[test]
    fn test_with_raw_values_unknown_data_ignored() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
                r#"
                spec test
                data known: number
                "#
                .to_string(),
            )])
            .unwrap();

        let plans = engine
            .plans
            .get_plans(None, "test")
            .expect("plans for test");
        let plan = plans.values().next().expect("plan");

        let values = input_data(&[("unknown", "30")]);

        let overlay = resolve_overlay(plan, values);
        assert!(overlay.bindings.is_empty());
        assert!(overlay.ignored_unknown.iter().any(|k| k == "unknown"));
    }

    #[test]
    fn test_with_raw_values_nested() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
                r#"
                spec private
                data base_price: number

                spec test
                uses rules: private
                "#
                .to_string(),
            )])
            .unwrap();

        let plans = engine
            .plans
            .get_plans(None, "test")
            .expect("plans for test");
        let plan = plans.values().next().expect("plan");

        let values = input_data(&[("rules.base_price", "100")]);

        let overlay = resolve_overlay(plan, values);
        let data_path = DataPath {
            segments: vec![PathSegment {
                data: "rules".to_string(),
                spec: "private".to_string(),
            }],
            data: "base_price".to_string(),
        };
        let updated_value = bound_value(&overlay, &data_path).expect("bound value");
        match &updated_value.value {
            crate::planning::semantics::ValueKind::Number(n) => {
                assert_eq!(n, &rational_new(100, 1));
            }
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    #[test]
    fn with_values_should_enforce_number_maximum_constraint() {
        // Higher-standard requirement: user input must be validated against type constraints.
        // If this test fails, Lemma accepts invalid values and gives false reassurance.
        let data_path = DataPath::new(vec![], "x".to_string());

        let max10 = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Number {
                minimum: None,
                maximum: Some(rational_new(10, 1)),
                decimals: None,
                help: String::new(),
            },
        );
        let source = Source::new(
            crate::parsing::source::SourceType::Volatile,
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: crate::planning::semantics::LiteralValue::number_with_type(
                    rational_new(0, 1),
                    Arc::new(max10.clone()),
                ),
                source: source.clone(),
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            normal_forms: Vec::new(),
            rules: IndexMap::new(),
            data_reference_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            data_releases: HashMap::new(),
            structural_needed: HashMap::new(),
        };

        let values = input_data(&[("x", "11")]);

        let overlay = resolve_overlay(&plan, values);
        match veto_reason(&overlay, &data_path) {
            Some(reason) => {
                assert!(
                    reason.contains("maximum") || reason.contains("10"),
                    "x=11 must violate maximum 10, got: {reason}"
                );
            }
            None => panic!("expected veto-bound data for x=11"),
        }
    }

    #[test]
    fn with_values_should_enforce_text_enum_options() {
        // Higher-standard requirement: enum options must be enforced for text types.
        let data_path = DataPath::new(vec![], "tier".to_string());

        let tier = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Text {
                length: None,
                options: vec!["silver".to_string(), "gold".to_string()],
                help: String::new(),
            },
        );
        let source = Source::new(
            crate::parsing::source::SourceType::Volatile,
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: crate::planning::semantics::LiteralValue::text_with_type(
                    "silver".to_string(),
                    Arc::new(tier.clone()),
                ),
                source,
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            normal_forms: Vec::new(),
            rules: IndexMap::new(),
            data_reference_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            data_releases: HashMap::new(),
            structural_needed: HashMap::new(),
        };

        let values = input_data(&[("tier", "platinum")]);

        let overlay = resolve_overlay(&plan, values);
        match veto_reason(&overlay, &data_path) {
            Some(reason) => {
                assert!(
                    reason.contains("allowed options") || reason.contains("platinum"),
                    "invalid enum must record violation, got: {reason}"
                );
            }
            None => panic!("expected veto-bound data for tier=platinum"),
        }
    }

    #[test]
    fn with_values_should_enforce_measure_decimals() {
        // Higher-standard requirement: decimals should be enforced on measure inputs,
        // unless the language explicitly defines rounding semantics.
        let data_path = DataPath::new(vec![], "price".to_string());

        let money = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Measure {
                minimum: None,
                maximum: None,
                decimals: Some(2),
                units: crate::planning::semantics::MeasureUnits::from(vec![
                    crate::planning::semantics::MeasureUnit::from_decimal_factor(
                        "eur".to_string(),
                        rust_decimal::Decimal::from_str("1.0").unwrap(),
                        Vec::new(),
                    )
                    .expect("eur unit factor must be exact decimal"),
                ]),
                traits: Vec::new(),
                decomposition: None,
                help: String::new(),
            },
        );
        let source = Source::new(
            crate::parsing::source::SourceType::Volatile,
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: crate::planning::semantics::LiteralValue::measure_with_type(
                    rational_zero(),
                    "eur".to_string(),
                    Arc::new(money.clone()),
                ),
                source,
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            normal_forms: Vec::new(),
            rules: IndexMap::new(),
            data_reference_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            data_releases: HashMap::new(),
            structural_needed: HashMap::new(),
        };

        let values = input_data(&[("price", "1.234 eur")]);

        let overlay = resolve_overlay(&plan, values);
        match veto_reason(&overlay, &data_path) {
            Some(reason) => {
                assert!(
                    reason.contains("decimals") || reason.contains("decimal"),
                    "1.234 eur must violate decimals=2, got: {reason}"
                );
            }
            None => panic!("expected veto-bound data for price=1.234 eur"),
        }
    }

    fn empty_plan(effective: crate::parsing::ast::EffectiveDate) -> ExecutionPlan {
        ExecutionPlan {
            spec_name: "s".into(),
            commentary: None,
            data: IndexMap::new(),
            normal_forms: Vec::new(),
            rules: IndexMap::new(),
            data_reference_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective,
            data_releases: HashMap::new(),
            structural_needed: HashMap::new(),
        }
    }

    fn plans_by_effective(
        plans: impl IntoIterator<Item = ExecutionPlan>,
    ) -> BTreeMap<EffectiveDate, ExecutionPlan> {
        plans
            .into_iter()
            .map(|plan| (plan.effective.clone(), plan))
            .collect()
    }

    /// Compiled plans for one spec name are ordered by `effective` key.
    #[test]
    fn plan_set_plans_are_in_ascending_effective_order() {
        let june = DateTimeValue {
            year: 2025,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
            granularity: DateGranularity::Full,
        };
        let dec = DateTimeValue {
            year: 2025,
            month: 12,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
            granularity: DateGranularity::Full,
        };

        let plans = plans_by_effective([
            empty_plan(EffectiveDate::Origin),
            empty_plan(EffectiveDate::DateTimeValue(june)),
            empty_plan(EffectiveDate::DateTimeValue(dec)),
        ]);

        let effectives: Vec<_> = plans.keys().cloned().collect();
        for window in effectives.windows(2) {
            assert!(
                window[0] < window[1],
                "plans must be strictly ascending: {:?} >= {:?}",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn plan_at_exact_boundary_selects_later_slice() {
        use crate::parsing::ast::{DateTimeValue, EffectiveDate};

        let june = DateTimeValue {
            year: 2025,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };
        let dec = DateTimeValue {
            year: 2025,
            month: 12,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };

        let june_key = EffectiveDate::DateTimeValue(june.clone());
        let dec_key = EffectiveDate::DateTimeValue(dec.clone());
        let plans = plans_by_effective([
            empty_plan(EffectiveDate::Origin),
            empty_plan(june_key.clone()),
            empty_plan(dec_key.clone()),
        ]);

        let june_plan = plan_at(&plans, &june_key).expect("boundary instant");
        assert!(std::ptr::eq(
            june_plan,
            plans.get(&june_key).expect("june slice")
        ));

        let dec_plan = plan_at(&plans, &dec_key).expect("dec boundary");
        assert!(std::ptr::eq(
            dec_plan,
            plans.get(&dec_key).expect("dec slice")
        ));
    }

    #[test]
    fn plan_at_day_before_boundary_stays_in_earlier_slice() {
        use crate::parsing::ast::{DateTimeValue, EffectiveDate};

        let june = DateTimeValue {
            year: 2025,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };
        let may_end = DateTimeValue {
            year: 2025,
            month: 5,
            day: 31,
            hour: 23,
            minute: 59,
            second: 59,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::DateTime,
        };

        let origin = EffectiveDate::Origin;
        let plans = plans_by_effective([
            empty_plan(origin.clone()),
            empty_plan(EffectiveDate::DateTimeValue(june)),
        ]);

        let may_instant = EffectiveDate::DateTimeValue(may_end);
        let may_plan = plan_at(&plans, &may_instant).expect("may 31");
        assert!(std::ptr::eq(
            may_plan,
            plans.get(&origin).expect("origin slice")
        ));
    }

    #[test]
    fn plan_at_single_plan_matches_any_instant_after_start() {
        use crate::parsing::ast::{DateTimeValue, EffectiveDate};

        let t = DateTimeValue {
            year: 2025,
            month: 3,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };
        let start = EffectiveDate::DateTimeValue(DateTimeValue {
            year: 2025,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        });
        let plans = plans_by_effective([empty_plan(start.clone())]);
        let instant = EffectiveDate::DateTimeValue(t);
        let selected = plan_at(&plans, &instant).expect("inside single slice");
        assert!(std::ptr::eq(
            selected,
            plans.get(&start).expect("single slice")
        ));
    }

    /// The show JSON shape is the IO contract for every non-Rust consumer
    /// (WASM playground, Hex, HTTP, TypeScript). Nail the exact envelope.
    #[test]
    fn show_json_shape_contract() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
                r#"
                spec pricing
                data bridge_height: measure
                  -> unit meter 1
                  -> suggest 100 meter
                data quantity: number -> minimum 0
                rule cost: bridge_height * quantity
                "#
                .to_string(),
            )])
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine.show(None, "pricing", Some(&now)).unwrap();

        let value: serde_json::Value = serde_json::to_value(&schema).unwrap();

        let bh = &value["data"]["bridge_height"];
        assert!(
            bh.is_object(),
            "data entry must be a named object, not tuple"
        );
        assert!(
            bh.get("type").is_some(),
            "data entry must expose `type` field"
        );
        assert!(
            bh.get("suggestion").is_some(),
            "bridge_height exposes `-> suggest` as schema suggestion"
        );
        assert!(
            bh.get("prefilled").is_none(),
            "bridge_height is not prefilled from spec"
        );

        let ty = &bh["type"];
        assert_eq!(
            ty["kind"], "measure",
            "kind tag sits on the type object itself"
        );
        assert!(
            ty["units"].is_array(),
            "measure-only fields flatten up to top level"
        );
        assert!(
            ty.get("options").is_none(),
            "text-only fields must not leak"
        );

        let quantity = &value["data"]["quantity"];
        assert_eq!(quantity["type"]["kind"], "number");
        assert!(
            quantity.get("suggestion").is_none(),
            "quantity has no suggestion"
        );
        assert!(
            quantity.get("prefilled").is_none(),
            "quantity has no prefilled literal"
        );

        let cost = &value["rules"]["cost"];
        assert_eq!(
            cost["kind"], "measure",
            "rule types use the same flat shape"
        );
        assert!(
            cost["units"].is_array() && !cost["units"].as_array().unwrap().is_empty(),
            "measure rule result types expose declared units"
        );
        assert!(
            cost["units"][0].get("factor").is_some(),
            "measure rule units use factor field"
        );
    }

    #[test]
    fn show_rule_result_units_contract() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "units_contract.lemma",
                ))),
                r#"
                spec units_contract
                data money: measure
                  -> unit eur 1
                  -> unit usd 0.91
                data rate: ratio
                  -> unit basis_points 10000
                  -> unit percent 100
                  -> suggest 500 basis_points
                rule total: money
                rule rate_out: rate
                "#
                .to_string(),
            )])
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine.show(None, "units_contract", Some(&now)).unwrap();
        let value: serde_json::Value = serde_json::to_value(&schema).unwrap();

        let money_units = &value["data"]["money"]["type"]["units"];
        assert!(money_units.is_array() && !money_units.as_array().unwrap().is_empty());
        assert!(money_units[0].get("name").is_some());
        assert!(money_units[0].get("factor").is_some());
        assert!(money_units[0]["factor"].get("numer").is_some());
        assert!(money_units[0]["factor"].get("denom").is_some());

        let rate_units = &value["data"]["rate"]["type"]["units"];
        assert!(rate_units.is_array() && !rate_units.as_array().unwrap().is_empty());
        assert!(rate_units[0].get("name").is_some());
        assert!(rate_units[0].get("value").is_some());
        assert!(rate_units[0]["value"].get("numer").is_some());
        assert!(rate_units[0]["value"].get("denom").is_some());

        let total_rule_units = &value["rules"]["total"]["units"];
        let money_unit_names: Vec<_> = money_units
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["name"].as_str().unwrap())
            .collect();
        let total_rule_unit_names: Vec<_> = total_rule_units
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["name"].as_str().unwrap())
            .collect();
        assert_eq!(total_rule_unit_names, money_unit_names);

        let rate_out_rule_units = &value["rules"]["rate_out"]["units"];
        let rate_unit_names: Vec<_> = rate_units
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["name"].as_str().unwrap())
            .collect();
        let rate_out_rule_unit_names: Vec<_> = rate_out_rule_units
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["name"].as_str().unwrap())
            .collect();
        assert_eq!(rate_out_rule_unit_names, rate_unit_names);
    }

    #[test]
    fn show_json_round_trip_preserves_shape() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("s.lemma"))),
                r#"
                spec s
                data age: number -> minimum 0 -> suggest 18
                data grade: text -> options "A" "B" "C"
                rule adult: age >= 18
                "#
                .to_string(),
            )])
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine.show(None, "s", Some(&now)).unwrap();

        let json = serde_json::to_string(&schema).unwrap();
        let round_tripped: Show = serde_json::from_str(&json).unwrap();
        assert_eq!(schema, round_tripped);
    }

    const COST_PRICE_SPEC: &str = r#"
spec cost_price
uses lemma units

data money: measure
  -> unit eur 1.00
  -> unit inr 0.0092
  -> decimals 2

data labor_cost: measure
  -> unit eur_per_hour eur/hour
  -> unit inr_per_hour inr/hour
  -> suggest 25 eur_per_hour

data product_cost: measure
  -> unit eur_per_kg eur/kilogram
  -> unit inr_per_kg inr/kilogram
  -> suggest 4 eur_per_kg

data throughput: measure
  -> unit kg_per_hour kilogram/hour
  -> suggest 12 kg_per_hour

rule cost_price: product_cost + labor_cost / throughput
"#;

    fn cost_price_inputs() -> HashMap<String, DataValueInput> {
        let mut data = HashMap::new();
        data.insert(
            "product_cost".into(),
            DataValueInput::convenience("4 eur_per_kg"),
        );
        data.insert(
            "labor_cost".into(),
            DataValueInput::convenience("25 eur_per_hour"),
        );
        data.insert(
            "throughput".into(),
            DataValueInput::convenience("12 kg_per_hour"),
        );
        data
    }

    const FILM_ACCESS: &str = r#"
spec premium_membership
uses lemma units
data start: date
data length: units.calendar
rule valid: now in start...start + length

spec film_access
uses premium_membership
data type: text
  -> option "rental"
  -> option "purchase"
data views_consumed: number
data premium_member: boolean
rule max_views: 3
  unless premium_membership.valid then 10
  unless premium_member then 5
rule can_view: no
  unless type is "rental" and views_consumed < max_views then yes
  unless type is "purchase" then yes
"#;

    fn film_access_effective() -> DateTimeValue {
        DateTimeValue {
            year: 2027,
            month: 2,
            day: 14,
            hour: 12,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: Some(TimezoneValue {
                offset_hours: 0,
                offset_minutes: 0,
            }),
            granularity: DateGranularity::DateTime,
        }
    }

    #[test]
    fn overlay_accepts_per_unit_measure_equivalent_to_canonical_magnitude() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "cost_price.lemma",
                ))),
                COST_PRICE_SPEC.to_string(),
            )])
            .expect("load");
        let plans = engine
            .plans
            .get_plans(None, "cost_price")
            .expect("plans for cost_price");
        let plan = plans.values().next().expect("plan");
        let mut data = HashMap::new();
        data.insert(
            "product_cost".into(),
            DataValueInput::convenience("4 eur_per_kg"),
        );
        data.insert(
            "labor_cost".into(),
            DataValueInput::convenience("0.0069444444444444444444444444 eur_per_hour"),
        );
        data.insert(
            "throughput".into(),
            DataValueInput::convenience("0.0033333333333333333333333333 kg_per_hour"),
        );
        let overlay = resolve_overlay(plan, data);
        assert!(
            !overlay
                .bindings
                .values()
                .any(|b| matches!(b, OperationResult::Veto(_))),
            "parsed decimal overlay values must not be veto-bound after input boundary: {:?}",
            overlay.bindings
        );
    }

    #[test]
    fn overlay_accepts_per_unit_measure() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "cost_price.lemma",
                ))),
                COST_PRICE_SPEC.to_string(),
            )])
            .expect("load");
        let plans = engine
            .plans
            .get_plans(None, "cost_price")
            .expect("plans for cost_price");
        let plan = plans.values().next().expect("plan");
        let overlay = resolve_overlay(plan, cost_price_inputs());
        assert!(!overlay
            .bindings
            .values()
            .any(|b| matches!(b, OperationResult::Veto(_))));
    }

    #[test]
    fn overlay_rejects_oversize_input() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "cost_price.lemma",
                ))),
                COST_PRICE_SPEC.to_string(),
            )])
            .expect("load");
        let plans = engine
            .plans
            .get_plans(None, "cost_price")
            .expect("plans for cost_price");
        let plan = plans.values().next().expect("plan");
        let mut data = cost_price_inputs();
        data.insert(
            "labor_cost".into(),
            DataValueInput::convenience(
                "1000000000000000000000000000000000000000000000000000000000000 eur_per_hour",
            ),
        );
        let overlay = resolve_overlay(plan, data);
        assert!(matches!(
            overlay.bindings.get(&DataPath::local("labor_cost".into())),
            Some(OperationResult::Veto(_))
        ));
    }
    #[test]
    fn typedecl_default_stays_typedecl_on_immutable_plan() {
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("s.lemma"))),
                r#"
        spec s
        data n: number -> suggest 42
        rule r: n
    "#
                .to_string(),
            )])
            .expect("load");

        let plans = engine.plans.get_plans(None, "s").expect("plans for s");
        let plan = plans.values().next().expect("plan");
        let path = DataPath::local("n".into());
        match plan.data.get(&path).expect("n") {
            DataDefinition::TypeDeclaration {
                declared_suggestion: Some(_),
                ..
            } => {}
            other => panic!("expected TypeDeclaration with default, got {other:?}"),
        }
    }

    fn response_missing_data_union(response: &crate::Response) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut names = Vec::new();
        for result in response.results.values() {
            for key in &result.missing_data {
                if seen.insert(key.clone()) {
                    names.push(key.clone());
                }
            }
        }
        names
    }

    #[test]
    fn run_prunes_inactive_nut_branches_for_total_price() {
        let code = r#"
spec bag
uses lemma units

data weight: measure
  -> unit kg 1

data money: measure
  -> unit eur 1

data price_per_weight: measure
  -> unit eur_per_kg eur/kg

data item_cost: price_per_weight
data roasting: price_per_weight
data chocolatizing: price_per_weight

rule total_price: weight * (item_cost + roasting + chocolatizing)

spec calc
uses bag
with bag.item_cost: item_cost
with bag.roasting: roasting

data type_of_nut: text -> options "peanut" "cashew"

rule price_peanut: 1.5 eur_per_kg
rule price_peanut_roasting: 0.45 eur_per_kg

rule price_cashew: 2.0 eur_per_kg
rule price_cashew_roasting: 0.55 eur_per_kg

rule item_cost: veto "No item cost"
  unless type_of_nut is "peanut" then price_peanut
  unless type_of_nut is "cashew" then price_cashew

rule roasting: veto "No roasting"
  unless type_of_nut is "peanut" then price_peanut_roasting
  unless type_of_nut is "cashew" then price_cashew_roasting

rule total_price: bag.total_price
"#;

        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "calc.lemma",
                ))),
                code.to_string(),
            )])
            .unwrap();

        let now = DateTimeValue::now();
        let mut inputs = HashMap::new();
        inputs.insert("type_of_nut".to_string(), "peanut".to_string());
        let response = engine
            .run(
                None,
                "calc",
                Some(&now),
                inputs,
                Some(&["total_price".to_string()]),
                false,
            )
            .expect("run must succeed");

        let names = response_missing_data_union(&response);
        assert!(
            !names.contains(&"type_of_nut".to_string()),
            "supplied type_of_nut is bound and must not appear in missing_data: {names:?}"
        );
        assert!(names.contains(&"bag.weight".to_string()));
        assert!(names.contains(&"bag.chocolatizing".to_string()));
        assert!(!names.contains(&"bag.item_cost".to_string()));
        assert!(!names.contains(&"bag.roasting".to_string()));
    }

    #[test]
    fn run_includes_membership_dates_when_premium_member_false() {
        let mut engine = Engine::new();
        engine
            .load([(crate::SourceType::Volatile, FILM_ACCESS.to_string())])
            .expect("film_access spec must load");
        let now = film_access_effective();
        let mut inputs = HashMap::new();
        inputs.insert("type".to_string(), "rental".to_string());
        inputs.insert("views_consumed".to_string(), "6".to_string());
        inputs.insert("premium_member".to_string(), "false".to_string());
        let response = engine
            .run(
                None,
                "film_access",
                Some(&now),
                inputs,
                Some(&["can_view".to_string()]),
                false,
            )
            .expect("run must succeed");

        let names = response_missing_data_union(&response);
        assert!(names.contains(&"premium_membership.start".to_string()));
        assert!(names.contains(&"premium_membership.length".to_string()));
    }

    #[test]
    fn run_includes_membership_dates_when_premium_member_unknown() {
        let mut engine = Engine::new();
        engine
            .load([(crate::SourceType::Volatile, FILM_ACCESS.to_string())])
            .expect("film_access spec must load");
        let now = film_access_effective();
        let mut inputs = HashMap::new();
        inputs.insert("type".to_string(), "rental".to_string());
        inputs.insert("views_consumed".to_string(), "6".to_string());
        let response = engine
            .run(
                None,
                "film_access",
                Some(&now),
                inputs,
                Some(&["can_view".to_string()]),
                false,
            )
            .expect("run must succeed");

        let names = response_missing_data_union(&response);
        assert!(names.contains(&"premium_member".to_string()));
        assert!(names.contains(&"premium_membership.start".to_string()));
        assert!(names.contains(&"premium_membership.length".to_string()));
    }

    const UNITS_SPEC: &str = r#"
spec units
uses lemma units
data money: measure
  -> unit eur 1
  -> decimals 2
"#;

    const WAREHOUSING_SPEC: &str = r#"
spec warehousing
uses units
uses si: lemma units

data units_per_pallet: number
  -> minimum 1
  -> suggest 1

data storage_duration: si.duration
  -> minimum 0 week
  -> suggest 10 day

data interbranch_transport_per_pallet: units.money
  -> minimum 0 eur
  -> suggest 0 eur

data inbound_handling_per_pallet: units.money
  -> minimum 0 eur
  -> suggest 0 eur

data storage_per_pallet_per_week: units.money
  -> minimum 0 eur
  -> suggest 10 eur

data labeling_per_pallet: units.money
  -> minimum 0 eur
  -> suggest 0 eur

data outbound_handling_per_pallet: units.money
  -> minimum 0 eur
  -> suggest 0 eur

rule storage_cost_per_pallet:
  storage_per_pallet_per_week
  * ceil storage_duration as week as Number

rule total_logistics_per_pallet:
  interbranch_transport_per_pallet
  + inbound_handling_per_pallet
  + storage_cost_per_pallet
  + labeling_per_pallet
  + outbound_handling_per_pallet

rule total_logistics_per_ce:
  total_logistics_per_pallet / units_per_pallet
"#;

    const QUOTATION_SPEC: &str = r#"
spec quotation
uses wh: warehousing
rule total: wh.total_logistics_per_ce
"#;

    fn load_cross_spec_fixtures(engine: &mut Engine) {
        engine
            .load([(crate::SourceType::Volatile, UNITS_SPEC.to_string())])
            .expect("units spec must load");
        engine
            .load([(crate::SourceType::Volatile, WAREHOUSING_SPEC.to_string())])
            .expect("warehousing spec must load");
    }

    #[test]
    fn quotation_plans_without_consumer_stdlib_units() {
        let mut engine = Engine::new();
        load_cross_spec_fixtures(&mut engine);
        engine
            .load([(crate::SourceType::Volatile, QUOTATION_SPEC.to_string())])
            .expect("quotation must plan without uses lemma units");
        let plans = engine
            .plans
            .get_plans(None, "quotation")
            .expect("plans for quotation");
        let plan = plans.values().next().expect("plan");

        let expression_units = &plan.resolved_types.unit_index;
        assert!(
            !expression_units.contains_key("week"),
            "consumer expression scope must not contain week: {:?}",
            expression_units.keys().collect::<Vec<_>>()
        );
        assert!(
            !expression_units.contains_key("minute"),
            "consumer expression scope must not contain minute: {:?}",
            expression_units.keys().collect::<Vec<_>>()
        );
        let mut keys: Vec<_> = expression_units.keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["percent", "permille"],
            "consumer expression scope must only have builtin ratio units, not dependency units"
        );
    }

    fn warehousing_default_inputs(prefix: &str) -> HashMap<String, String> {
        let key = |name: &str| {
            if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}.{name}")
            }
        };
        HashMap::from([
            (key("units_per_pallet"), "1".into()),
            (key("storage_duration"), "10 day".into()),
            (key("interbranch_transport_per_pallet"), "0 eur".into()),
            (key("inbound_handling_per_pallet"), "0 eur".into()),
            (key("storage_per_pallet_per_week"), "10 eur".into()),
            (key("labeling_per_pallet"), "0 eur".into()),
            (key("outbound_handling_per_pallet"), "0 eur".into()),
        ])
    }

    #[test]
    fn quotation_evaluates_cross_spec_duration_conversion() {
        let mut engine = Engine::new();
        load_cross_spec_fixtures(&mut engine);
        engine
            .load([(crate::SourceType::Volatile, QUOTATION_SPEC.to_string())])
            .expect("quotation must load");
        let plans = engine
            .plans
            .get_plans(None, "quotation")
            .expect("plans for quotation");
        let plan = plans.values().next().expect("plan");
        assert!(
            !plan.resolved_types.unit_index.contains_key("week"),
            "consumer unit_index must not contain week"
        );
        let now = DateTimeValue::now();
        let response = engine
            .run(
                None,
                "quotation",
                Some(&now),
                warehousing_default_inputs("wh"),
                None,
                false,
            )
            .expect("quotation must evaluate");
        let display = response
            .results
            .get("total")
            .expect("rule total must be present")
            .display
            .clone()
            .expect("total must have display");
        assert_eq!(
            display, "20.00 eur",
            "10 eur/week * ceil(10 day as week) / 1 CE must be 20.00 eur, got: {display}"
        );
    }

    #[test]
    fn ratio_range_default_endpoints_must_be_ratio_not_measure() {
        let code = r#"
spec policy
data allowed_band: ratio range -> suggest 10%...50%
rule band: allowed_band
"#;
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "ratio_range_endpoint_typing.lemma",
                ))),
                code.to_string(),
            )])
            .unwrap();

        let plans = engine
            .plans
            .get_plans(None, "policy")
            .expect("plans for policy");
        let plan = plans.values().next().expect("plan");
        let path = DataPath::local("allowed_band".into());
        let def = plan.data.get(&path).expect("allowed_band in plan.data");
        let suggestion = def.suggestion().expect("declared default must exist");

        let (left, right) = match &suggestion.value {
            crate::planning::semantics::ValueKind::Range(l, r) => (l.as_ref(), r.as_ref()),
            other => panic!("expected Range, got {other:?}"),
        };
        for (label, endpoint) in [("left", left), ("right", right)] {
            assert!(
                !matches!(
                    &endpoint.lemma_type.specifications,
                    TypeSpecification::Measure { .. }
                ),
                "{label} endpoint must not be lifted as Measure for a percent literal in a ratio range default",
            );
            assert!(
                matches!(
                    &endpoint.value,
                    crate::planning::semantics::ValueKind::Ratio(_, _)
                ),
                "{label} endpoint ValueKind must be Ratio (got {:?})",
                endpoint.value
            );
        }
    }

    #[test]
    fn ratio_range_typedef_with_second_ratio_field_loads() {
        let code = r#"
spec policy
data margin_pct: ratio -> suggest 15%
data allowed_band: ratio range
rule margin: margin_pct
rule band_slot: allowed_band
"#;
        let mut engine = Engine::new();
        engine
            .load([(
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "ratio_range_load.lemma",
                ))),
                code.to_string(),
            )])
            .unwrap();

        let plans = engine
            .plans
            .get_plans(None, "policy")
            .expect("plans for policy");
        let plan = plans.values().next().expect("plan");
        let path = DataPath::local("allowed_band".into());
        let def = plan.data.get(&path).expect("allowed_band in plan.data");
        let lemma_type = def
            .schema_type()
            .expect("allowed_band must be a typed data slot");
        match &lemma_type.specifications {
            TypeSpecification::RatioRange { units, .. } => {
                let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
                assert!(
                    names.contains(&"percent"),
                    "ratio range must inherit builtin percent, got {names:?}"
                );
            }
            other => panic!("allowed_band must be RatioRange, got {other:?}"),
        }
    }

    fn plan_from_code(code: &str, spec_name: &str) -> ExecutionPlan {
        let mut engine = Engine::new();
        engine
            .load([(crate::SourceType::Volatile, code.to_string())])
            .expect("spec must load");
        let plans = engine
            .plans
            .get_plans(None, spec_name)
            .expect("plans for spec");
        plans.values().next().expect("plan").clone()
    }

    fn local_rule_path(plan: &ExecutionPlan, rule_name: &str) -> RulePath {
        plan.get_rule(rule_name).expect("local rule").path.clone()
    }

    fn path_keys(paths: &[DataPath]) -> Vec<String> {
        paths.iter().map(|p| p.input_key()).collect()
    }

    fn and_releases<'a>(plan: &'a ExecutionPlan, rule: &str) -> Vec<&'a ControlDataReleases> {
        let path = local_rule_path(plan, rule);
        plan.data_releases
            .get(&path)
            .expect("data_releases for rule")
            .values()
            .filter(|r| matches!(r, ControlDataReleases::And { .. }))
            .collect()
    }

    fn piecewise_releases<'a>(plan: &'a ExecutionPlan, rule: &str) -> &'a ControlDataReleases {
        let path = local_rule_path(plan, rule);
        plan.data_releases
            .get(&path)
            .expect("data_releases for rule")
            .values()
            .find(|r| matches!(r, ControlDataReleases::Piecewise { .. }))
            .expect("Piecewise ControlDataReleases")
    }

    #[test]
    fn data_releases_and_left_false_releases_exclusive_right() {
        let plan = plan_from_code(
            r#"
spec demo
data flag: boolean
data expensive: number
rule main: flag and (expensive > 0)
"#,
            "demo",
        );
        let ands = and_releases(&plan, "main");
        assert_eq!(ands.len(), 1, "expected one And: {ands:?}");
        match ands[0] {
            ControlDataReleases::And { on_left_false } => {
                let keys = path_keys(on_left_false);
                assert!(
                    keys.contains(&"expensive".to_string()),
                    "expected expensive in on_left_false: {keys:?}"
                );
                assert!(
                    !keys.contains(&"flag".to_string()),
                    "flag must not be released: {keys:?}"
                );
            }
            other => panic!("expected And releases, got {other:?}"),
        }
    }

    #[test]
    fn data_releases_and_does_not_release_path_still_reachable_outside() {
        let plan = plan_from_code(
            r#"
spec demo
data flag: boolean
data expensive: number
rule main: expensive
  unless flag and (expensive > 100) then 0
"#,
            "demo",
        );
        let ands = and_releases(&plan, "main");
        assert_eq!(ands.len(), 1, "expected one And: {ands:?}");
        match ands[0] {
            ControlDataReleases::And { on_left_false } => {
                let keys = path_keys(on_left_false);
                assert!(
                    !keys.contains(&"expensive".to_string()),
                    "expensive still reachable via default body: {keys:?}"
                );
            }
            other => panic!("expected And releases, got {other:?}"),
        }
    }

    #[test]
    fn data_releases_nested_and_each_node_has_own_entry() {
        let plan = plan_from_code(
            r#"
spec demo
data a: boolean
data b: boolean
data c: boolean
rule main: a and b and c
"#,
            "demo",
        );
        let ands = and_releases(&plan, "main");
        assert_eq!(
            ands.len(),
            2,
            "nested binary And must yield two control entries: {ands:?}"
        );
        let mut released_rights: Vec<Vec<String>> = ands
            .iter()
            .map(|r| match r {
                ControlDataReleases::And { on_left_false } => path_keys(on_left_false),
                other => panic!("expected And, got {other:?}"),
            })
            .collect();
        released_rights.sort();
        assert!(
            released_rights
                .iter()
                .any(|k: &Vec<String>| k.contains(&"b".to_string())),
            "one And must release b (exclusive to its right): {released_rights:?}"
        );
        assert!(
            released_rights
                .iter()
                .any(|k: &Vec<String>| k.contains(&"c".to_string())),
            "one And must release c: {released_rights:?}"
        );
    }

    #[test]
    fn data_releases_piecewise_not_taken_releases_arm_body() {
        let plan = plan_from_code(
            r#"
spec demo
data flag: boolean
data expensive: number
rule main: 1
  unless flag then expensive
"#,
            "demo",
        );
        match piecewise_releases(&plan, "main") {
            ControlDataReleases::Piecewise {
                on_arm_not_taken, ..
            } => {
                assert_eq!(on_arm_not_taken.len(), 2);
                let keys = path_keys(&on_arm_not_taken[1]);
                assert!(
                    keys.contains(&"expensive".to_string()),
                    "NotTaken must release expensive: {keys:?}"
                );
                assert!(
                    !keys.contains(&"flag".to_string()),
                    "flag was evaluated: {keys:?}"
                );
            }
            other => panic!("expected Piecewise, got {other:?}"),
        }
    }

    #[test]
    fn data_releases_piecewise_default_wins_releases_unless_bodies() {
        let plan = plan_from_code(
            r#"
spec demo
data flag: boolean
data expensive: number
rule main: 1
  unless flag then expensive
"#,
            "demo",
        );
        match piecewise_releases(&plan, "main") {
            ControlDataReleases::Piecewise {
                on_default_wins, ..
            } => {
                let keys = path_keys(on_default_wins);
                assert!(
                    keys.contains(&"expensive".to_string()),
                    "default_wins must release expensive: {keys:?}"
                );
            }
            other => panic!("expected Piecewise, got {other:?}"),
        }
    }

    #[test]
    fn data_releases_piecewise_taken_multi_edge_releases_shared_body() {
        let plan = plan_from_code(
            r#"
spec demo
data shared: number
data flag_a: boolean
data flag_b: boolean
rule main: shared
  unless flag_a then shared
  unless flag_b then 0
"#,
            "demo",
        );
        match piecewise_releases(&plan, "main") {
            ControlDataReleases::Piecewise { on_arm_taken, .. } => {
                assert_eq!(on_arm_taken.len(), 3);
                let keys = path_keys(&on_arm_taken[2]);
                assert!(
                    keys.contains(&"shared".to_string()),
                    "flag_b Taken must multi-edge-release shared: {keys:?}"
                );
            }
            other => panic!("expected Piecewise, got {other:?}"),
        }
    }

    fn wide_iso_style_spec(unless_count: usize) -> String {
        let mut code = String::from(
            r#"
spec wide
data code: text
"#,
        );
        for i in 0..unless_count {
            code.push_str(&format!("  -> option \"{i}\"\n"));
        }
        code.push_str("rule name: veto \"x\"\n");
        for i in 0..unless_count {
            code.push_str(&format!("  unless code is \"{i}\" then \"{i}\"\n"));
        }
        code
    }

    fn plan_wide_counting_visits(unless_count: usize) -> (ExecutionPlan, u64) {
        STRUCTURAL_DAG_VISIT_COUNT.with(|count| count.set(0));
        STRUCTURAL_DAG_VISIT_ENABLED.with(|enabled| enabled.set(true));
        let plan = plan_from_code(&wide_iso_style_spec(unless_count), "wide");
        STRUCTURAL_DAG_VISIT_ENABLED.with(|enabled| enabled.set(false));
        let visits = STRUCTURAL_DAG_VISIT_COUNT.with(|count| count.get());
        (plan, visits)
    }

    #[test]
    fn data_releases_wide_piecewise_fill_visits_scale_near_linear() {
        let (plan_16, visits_16) = plan_wide_counting_visits(16);
        let (plan_64, visits_64) = plan_wide_counting_visits(64);

        match piecewise_releases(&plan_16, "name") {
            ControlDataReleases::Piecewise {
                on_arm_not_taken,
                on_arm_taken,
                on_default_wins,
            } => {
                assert_eq!(on_arm_not_taken.len(), 17, "default + 16 unless arms");
                assert_eq!(on_arm_taken.len(), 17);
                assert!(
                    !path_keys(on_default_wins).contains(&"code".to_string()),
                    "iso-style: code must not release on default_wins"
                );
            }
            other => panic!("expected Piecewise, got {other:?}"),
        }
        match piecewise_releases(&plan_64, "name") {
            ControlDataReleases::Piecewise {
                on_arm_not_taken, ..
            } => {
                assert_eq!(on_arm_not_taken.len(), 65, "default + 64 unless arms");
            }
            other => panic!("expected Piecewise, got {other:?}"),
        }

        assert!(
            visits_16 > 0,
            "visit counter must observe fill walks, got 0"
        );
        let ratio = visits_64 as f64 / visits_16 as f64;
        assert!(
            ratio < 8.0,
            "exclusivity fill must scale near-linear in unless arms; visits(16)={visits_16} visits(64)={visits_64} ratio={ratio} (quadratic ≈ 16)"
        );
    }
}
