//! Execution plan for evaluated specs
//!
//! Provides a complete self-contained execution plan ready for the evaluator.
//! The plan contains all data, rules flattened into executable branches,
//! and execution order - no spec structure needed during evaluation.
//!
//! Reliability model:
//! - `SpecSchema` is the IO contract surface for consumers (data and rule outputs).
//!   IO compatibility is the consumer-facing guarantee.

use crate::computation::UnitResolutionContext;
use crate::parsing::ast::{EffectiveDate, LemmaRepository, LemmaSpec, MetaValue};
use crate::parsing::source::Source;
use crate::planning::graph::Graph;
use crate::planning::graph::ResolvedSpecTypes;
use crate::planning::normalize::{build_unless_chain, inline_rule_refs, normalize_expression};
use crate::planning::semantics::{
    DataDefinition, DataPath, Expression, LemmaType, LiteralValue, RulePath, SemanticCalendarUnit,
    TypeSpecification, ValueKind,
};
use crate::Error;
use crate::ResourceLimits;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

/// One spec's contribution to an [`ExecutionPlan`], together with its
/// formatted AST source.
///
/// `repository` is `None` for workspace (root) specs. Including the
/// repository name means two specs with the same base name from different
/// repos are always distinct entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    pub name: String,
    pub effective_from: EffectiveDate,
    pub source: String,
}

pub type SpecSources = Vec<SpecSource>;

/// A complete execution plan ready for the evaluator
///
/// Contains the topologically sorted list of rules to execute, along with all data.
/// Self-contained structure - no spec lookups required during evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Main spec name
    pub spec_name: String,

    /// Per-data data in definition order: value, type-only, or spec reference.
    #[serde(serialize_with = "crate::serialization::serialize_resolved_data_value_map")]
    #[serde(deserialize_with = "crate::serialization::deserialize_resolved_data_value_map")]
    pub data: IndexMap<DataPath, DataDefinition>,

    /// Rules to execute in topological order (sorted by dependencies)
    pub rules: Vec<ExecutableRule>,

    /// Order in which [`DataDefinition::Reference`] entries must be resolved
    /// at evaluation time so that chained references (reference → reference →
    /// data) copy values in the correct sequence. Empty when the plan has no
    /// references.
    #[serde(default, alias = "alias_evaluation_order")]
    pub reference_evaluation_order: Vec<DataPath>,

    /// Spec metadata
    pub meta: HashMap<String, MetaValue>,

    /// Unit name → owning quantity/ratio type (same as planner [`ResolvedSpecTypes::unit_index`]:
    /// local types plus units from **direct** `uses` imports only; qualified re-exports skipped).
    #[serde(default)]
    pub unit_index: HashMap<String, LemmaType>,

    pub effective: EffectiveDate,

    /// Canonical source for all specs in this plan (one entry per spec, includes repository).
    /// Reconstructed from AST — not raw file content.
    #[serde(default)]
    pub sources: SpecSources,
}

/// All [`ExecutionPlan`]s for a spec name after dependency resolution.
/// Ordered by [`ExecutionPlan::effective`]. Slice end is derived from the next plan's `effective`.
#[derive(Debug, Clone)]
pub struct ExecutionPlanSet {
    pub spec_name: String,
    pub plans: Vec<ExecutionPlan>,
}

impl ExecutionPlanSet {
    /// Plan covering `[effective[i], effective[i+1])` (half-open).
    #[must_use]
    pub fn plan_at(&self, effective: &EffectiveDate) -> Option<&ExecutionPlan> {
        for (i, plan) in self.plans.iter().enumerate() {
            let from_ok = *effective >= plan.effective;
            let to_ok = self
                .plans
                .get(i + 1)
                .map(|next| *effective < next.effective)
                .unwrap_or(true);
            if from_ok && to_ok {
                return Some(plan);
            }
        }
        None
    }
}

/// An executable rule with flattened branches
///
/// Contains all information needed to evaluate a rule without spec lookups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableRule {
    /// Unique identifier for this rule
    pub path: RulePath,

    /// Rule name
    pub name: String,

    /// Branches evaluated in order (last matching wins)
    /// First branch has condition=None (default expression)
    /// Subsequent branches have condition=Some(...) (unless clauses)
    /// The evaluation is done in reverse order with the earliest matching branch returning (winning) the result.
    pub branches: Vec<Branch>,

    /// All data this rule needs (direct + inherited from rule dependencies)
    pub needs_data: BTreeSet<DataPath>,

    /// Source location for error messages (always present for rules from parsed specs)
    pub source: Source,

    /// Computed type of this rule's result
    /// Every rule MUST have a type (Lemma is strictly typed)
    pub rule_type: LemmaType,
}

/// A branch in an executable rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    /// Condition expression (None for default branch)
    pub condition: Option<Expression>,

    /// Unless condition after normalize (authoritative for evaluation when present)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_condition: Option<Expression>,

    /// Result expression as written (for explanation trace; `RulePath` refs preserved)
    pub result: Expression,

    /// Dependencies inlined and algebraically simplified; evaluated for authoritative result
    pub normalized_result: Expression,

    /// Source location for error messages (always present for branches from parsed specs)
    pub source: Source,
}

/// One expression for a rule's branch semantics (unless-chain), using normalized branch results
/// and normalized conditions. Used for rule inlining into downstream rules.
fn build_rule_normalized_result_expression(branches: &[Branch]) -> Expression {
    let pairs: Vec<(Option<Expression>, Expression)> = branches
        .iter()
        .map(|b| {
            let condition = b.condition.as_ref().map(|_| {
                b.normalized_condition
                    .clone()
                    .expect("BUG: normalized_condition must exist when condition exists")
            });
            (condition, b.normalized_result.clone())
        })
        .collect();
    build_unless_chain(&pairs)
}

/// Builds an execution plan from a Graph for one temporal slice.
/// Internal implementation detail - only called by plan()
pub(crate) fn build_execution_plan(
    graph: &Graph,
    resolved_types: &[(Arc<LemmaRepository>, Arc<LemmaSpec>, ResolvedSpecTypes)],
    effective: &EffectiveDate,
) -> Result<ExecutionPlan, Vec<Error>> {
    let data = graph.build_data();
    let execution_order = graph.execution_order();

    let main_spec = graph.main_spec();
    let unit_index = resolved_types
        .iter()
        .find(|(_, spec, _)| Arc::ptr_eq(spec, main_spec))
        .map(|(_, _, types)| types.unit_index.clone())
        .unwrap_or_default();

    let mut executable_rules: Vec<ExecutableRule> = Vec::new();
    let mut path_to_index: HashMap<RulePath, usize> = HashMap::new();
    let mut normalized_rule_results: HashMap<RulePath, Expression> = HashMap::new();

    for rule_path in execution_order {
        let rule_node = graph.rules().get(rule_path).expect(
            "bug: rule from topological sort not in graph - validation should have caught this",
        );

        let mut direct_data = HashSet::new();
        for (condition, result) in &rule_node.branches {
            if let Some(cond) = condition {
                cond.collect_data_paths(&mut direct_data);
            }
            result.collect_data_paths(&mut direct_data);
        }
        let mut needs_data: BTreeSet<DataPath> = direct_data.into_iter().collect();

        for dep in &rule_node.depends_on_rules {
            if let Some(&dep_idx) = path_to_index.get(dep) {
                needs_data.extend(executable_rules[dep_idx].needs_data.iter().cloned());
            }
        }

        let mut executable_branches = Vec::new();
        let unit_ctx = UnitResolutionContext::WithIndex(&unit_index);
        for (condition, result) in &rule_node.branches {
            let inlined = inline_rule_refs(result, &normalized_rule_results);
            let normalized_result =
                normalize_expression(&inlined, Some(&unit_ctx)).map_err(|error| {
                    vec![Error::validation(
                        format!("failed to normalize rule result: {error}"),
                        Some(rule_node.source.clone()),
                        None::<String>,
                    )]
                })?;
            let normalized_condition = match condition {
                Some(condition) => Some(normalize_expression(condition, Some(&unit_ctx)).map_err(
                    |error| {
                        vec![Error::validation(
                            format!("failed to normalize unless condition: {error}"),
                            Some(rule_node.source.clone()),
                            None::<String>,
                        )]
                    },
                )?),
                None => None,
            };
            executable_branches.push(Branch {
                condition: condition.clone(),
                normalized_condition,
                result: result.clone(),
                normalized_result,
                source: rule_node.source.clone(),
            });
        }

        normalized_rule_results.insert(
            rule_path.clone(),
            build_rule_normalized_result_expression(&executable_branches),
        );

        path_to_index.insert(rule_path.clone(), executable_rules.len());
        executable_rules.push(ExecutableRule {
            path: rule_path.clone(),
            name: rule_path.rule.clone(),
            branches: executable_branches,
            source: rule_node.source.clone(),
            needs_data,
            rule_type: rule_node.rule_type.clone(),
        });
    }

    let mut sources: SpecSources = Vec::new();
    for (repo, spec, _) in resolved_types.iter() {
        if !sources.iter().any(|e| {
            e.repository == repo.name
                && e.name == spec.name
                && e.effective_from == spec.effective_from
        }) {
            sources.push(SpecSource {
                repository: repo.name.clone(),
                name: spec.name.clone(),
                effective_from: spec.effective_from.clone(),
                source: crate::formatting::format_specs(&[spec.as_ref().clone()]),
            });
        }
    }

    Ok(ExecutionPlan {
        spec_name: main_spec.name.clone(),
        data,
        rules: executable_rules,
        reference_evaluation_order: graph.reference_evaluation_order().to_vec(),
        meta: main_spec
            .meta_fields
            .iter()
            .map(|f| (f.key.clone(), f.value.clone()))
            .collect(),
        unit_index,
        effective: effective.clone(),
        sources,
    })
}

/// A spec's public interface: its data (inputs) and rules (outputs) with
/// full structured type information.
///
/// Built from an [`ExecutionPlan`] via [`ExecutionPlan::schema`] (all data and
/// rules) or [`ExecutionPlan::schema_for_rules`] (scoped to specific rules and
/// only the data they need).
///
/// Shared by the HTTP server, the CLI, the MCP server, WASM, and any other
/// consumer. Carries the real [`LemmaType`] and [`LiteralValue`] so consumers
/// can work at whatever fidelity they need — structured types for input forms,
/// or `Display` for plain text.
///
/// This is the IO contract consumers can rely on:
/// - `data`: required/provided inputs with full type constraints
/// - `rules`: produced outputs with full result types
///
/// For cross-spec composition, planning validates that referenced specs satisfy
/// this contract. Plan hashes are complementary: they lock full behavior.
/// One data input in a [`SpecSchema`].
///
/// A named struct instead of a `(type, bound, default)` tuple so JSON-native consumers
/// (TypeScript, Python, ...) get stable field names. `bound_value` holds a spec or
/// caller-fixed literal; `default` is only a `-> default ...` suggestion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataEntry {
    #[serde(rename = "type")]
    pub lemma_type: LemmaType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bound_value: Option<LiteralValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default: Option<LiteralValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecSchema {
    /// Resolved spec id (logical name including path segments).
    pub spec: String,
    /// Data (inputs) keyed by name.
    pub data: indexmap::IndexMap<String, DataEntry>,
    /// Rules (outputs) keyed by name, with their computed result types
    pub rules: indexmap::IndexMap<String, LemmaType>,
    /// Spec metadata
    pub meta: HashMap<String, MetaValue>,
}

impl std::fmt::Display for SpecSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Spec: {}", self.spec)?;

        if !self.meta.is_empty() {
            write!(f, "\n\nMeta:")?;
            // Sort keys for deterministic output
            let mut entries: Vec<(&String, &MetaValue)> = self.meta.iter().collect();
            entries.sort_by_key(|(k, _)| *k);
            for (key, value) in entries {
                write!(f, "\n  {}: {}", key, value)?;
            }
        }

        if !self.data.is_empty() {
            write!(f, "\n\nData:")?;
            for (name, entry) in &self.data {
                write!(f, "\n  {} ({}", name, entry.lemma_type.name())?;
                if let Some(constraints) = format_type_constraints(&entry.lemma_type.specifications)
                {
                    write!(f, ", {}", constraints)?;
                }
                if let Some(val) = &entry.bound_value {
                    write!(f, ", value: {}", val)?;
                }
                if let Some(val) = &entry.default {
                    write!(f, ", default: {}", val)?;
                }
                write!(f, ")")?;
            }
        }

        if !self.rules.is_empty() {
            write!(f, "\n\nRules:")?;
            for (name, rule_type) in &self.rules {
                write!(f, "\n  {} ({})", name, rule_type.name())?;
            }
        }

        if self.data.is_empty() && self.rules.is_empty() {
            write!(f, "\n  (no data or rules)")?;
        }

        Ok(())
    }
}

impl SpecSchema {
    /// Type-structural compatibility: every data/rule present in BOTH schemas
    /// must have the same `LemmaType`. New additions (present in one but not
    /// the other) are allowed. Ignores literal default values on data,
    /// spec name, and meta fields.
    pub(crate) fn is_type_compatible(&self, other: &SpecSchema) -> bool {
        for (name, entry) in &self.data {
            if let Some(other_entry) = other.data.get(name) {
                if entry.lemma_type != other_entry.lemma_type {
                    return false;
                }
            }
        }
        for (name, lt) in &self.rules {
            if let Some(other_lt) = other.rules.get(name) {
                if lt != other_lt {
                    return false;
                }
            }
        }
        true
    }
}

/// Produce a human-readable summary of type constraints, or `None` when there
/// are no constraints worth showing (e.g. bare `boolean`).
fn format_type_constraints(spec: &TypeSpecification) -> Option<String> {
    let mut parts = Vec::new();

    match spec {
        TypeSpecification::Number {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Quantity {
            minimum,
            maximum,
            decimals,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                parts.push(format!("units: {}", unit_names.join(", ")));
            }
            if let Some((magnitude, unit_name)) = minimum {
                parts.push(format!("minimum: {} {}", magnitude, unit_name));
            }
            if let Some((magnitude, unit_name)) = maximum {
                parts.push(format!("maximum: {} {}", magnitude, unit_name));
            }
            if let Some(d) = decimals {
                parts.push(format!("decimals: {}", d));
            }
        }
        TypeSpecification::Ratio {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Text { options, .. } => {
            if !options.is_empty() {
                let quoted: Vec<String> = options.iter().map(|o| format!("\"{}\"", o)).collect();
                parts.push(format!("options: {}", quoted.join(", ")));
            }
        }
        TypeSpecification::Date {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Time {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Boolean { .. }
        | TypeSpecification::NumberRange { .. }
        | TypeSpecification::QuantityRange { .. }
        | TypeSpecification::DateRange { .. }
        | TypeSpecification::RatioRange { .. }
        | TypeSpecification::CalendarRange { .. }
        | TypeSpecification::Calendar { .. }
        | TypeSpecification::Veto { .. }
        | TypeSpecification::Undetermined => {}
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

impl ExecutionPlan {
    /// Build a [`SpecSchema`] describing this plan's public IO contract.
    ///
    /// Only data transitively reachable from at least one local rule (via
    /// `needs_data`) are included. Spec-reference data (which have no schema
    /// type) are also excluded. Only local rules (no cross-spec segments) are
    /// included. Data and rules are sorted by source position (definition
    /// order).
    pub fn schema(&self) -> SpecSchema {
        let all_local_rules: Vec<String> = self
            .rules
            .iter()
            .filter(|r| r.path.segments.is_empty())
            .map(|r| r.name.clone())
            .collect();
        self.schema_for_rules(&all_local_rules)
            .expect("BUG: all_local_rules sourced from self.rules")
    }

    /// Every typed data and every local rule — the surface other specs can address.
    pub(crate) fn interface_schema(&self) -> SpecSchema {
        let mut data_entries: Vec<(usize, String, DataEntry)> = self
            .data
            .iter()
            .filter(|(_, data)| data.schema_type().is_some())
            .map(|(path, data)| {
                let lemma_type = data
                    .schema_type()
                    .expect("BUG: filter above ensured schema_type is Some")
                    .clone();
                let bound_value = data.bound_value().cloned();
                let default = data.default_suggestion();
                (
                    data.source().span.start,
                    path.input_key(),
                    DataEntry {
                        lemma_type,
                        bound_value,
                        default,
                    },
                )
            })
            .collect();
        data_entries.sort_by_key(|(pos, _, _)| *pos);

        let rule_entries: Vec<(String, LemmaType)> = self
            .rules
            .iter()
            .filter(|r| r.path.segments.is_empty())
            .map(|r| (r.name.clone(), r.rule_type.clone()))
            .collect();

        SpecSchema {
            spec: self.spec_name.clone(),
            data: data_entries
                .into_iter()
                .map(|(_, name, data)| (name, data))
                .collect(),
            rules: rule_entries.into_iter().collect(),
            meta: self.meta.clone(),
        }
    }

    /// Build a [`SpecSchema`] scoped to specific rules.
    ///
    /// The returned schema contains only the data **needed** by the given rules
    /// (transitively, via `needs_data`) and only those rules. This is the
    /// "what do I need to evaluate these rules?" view.
    /// Data are sorted by source position (definition order).
    ///
    /// Returns `Err` if any rule name is not found in the plan.
    pub fn schema_for_rules(&self, rule_names: &[String]) -> Result<SpecSchema, Error> {
        let mut needed_data = HashSet::new();
        let mut rule_entries: Vec<(String, LemmaType)> = Vec::new();

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
            needed_data.extend(rule.needs_data.iter().cloned());
            rule_entries.push((rule.name.clone(), rule.rule_type.clone()));
        }

        let mut data_entries: Vec<(usize, String, DataEntry)> = self
            .data
            .iter()
            .filter(|(path, _)| needed_data.contains(path))
            .filter_map(|(path, data)| {
                let lemma_type = data.schema_type()?.clone();
                let bound_value = data.bound_value().cloned();
                let default = data.default_suggestion();
                Some((
                    data.source().span.start,
                    path.input_key(),
                    DataEntry {
                        lemma_type,
                        bound_value,
                        default,
                    },
                ))
            })
            .collect();
        data_entries.sort_by_key(|(pos, _, _)| *pos);
        let data_entries: Vec<(String, DataEntry)> = data_entries
            .into_iter()
            .map(|(_, name, data)| (name, data))
            .collect();

        Ok(SpecSchema {
            spec: self.spec_name.clone(),
            data: data_entries.into_iter().collect(),
            rules: rule_entries.into_iter().collect(),
            meta: self.meta.clone(),
        })
    }

    /// Look up a data by its input key (e.g., "age" or "rules.base_price").
    pub fn get_data_path_by_str(&self, name: &str) -> Option<&DataPath> {
        self.data.keys().find(|path| path.input_key() == name)
    }

    /// Look up a local rule by its name (rule in the main spec).
    pub fn get_rule(&self, name: &str) -> Option<&ExecutableRule> {
        self.rules
            .iter()
            .find(|r| r.name == name && r.path.segments.is_empty())
    }

    /// Look up a rule by its full path.
    pub fn get_rule_by_path(&self, rule_path: &RulePath) -> Option<&ExecutableRule> {
        self.rules.iter().find(|r| &r.path == rule_path)
    }

    /// Get the literal value for a data path, if it exists and has a literal value.
    pub fn get_data_value(&self, path: &DataPath) -> Option<&LiteralValue> {
        self.data.get(path).and_then(|d| d.value())
    }

    /// Provide data values as JSON (convenience strings or serialized objects).
    ///
    /// Parses each value to its expected type, validates constraints, and applies to the plan.
    pub fn set_data_values(
        mut self,
        values: std::collections::HashMap<String, serde_json::Value>,
        limits: &ResourceLimits,
    ) -> Result<Self, Error> {
        for (name, raw_value) in values {
            let data_path = self.get_data_path_by_str(&name).ok_or_else(|| {
                let available: Vec<String> = self.data.keys().map(|p| p.input_key()).collect();
                Error::request(
                    format!(
                        "Data '{}' not found. Available data: {}",
                        name,
                        available.join(", ")
                    ),
                    None::<String>,
                )
            })?;
            let data_path = data_path.clone();

            let data_definition = self
                .data
                .get(&data_path)
                .expect("BUG: data_path was just resolved from self.data, must exist");

            let data_source = data_definition.source().clone();
            let expected_type = data_definition.schema_type().cloned().ok_or_else(|| {
                Error::request(
                    format!(
                        "Data '{}' is a spec reference; cannot provide a value.",
                        name
                    ),
                    None::<String>,
                )
            })?;

            let literal_value = crate::planning::semantics::parse_data_value_from_json(
                &raw_value,
                &expected_type.specifications,
                &expected_type,
                &data_source,
            )
            .map_err(|e| e.with_related_data(&name))?;

            let size = literal_value.byte_size();
            if size > limits.max_data_value_bytes {
                return Err(Error::resource_limit_exceeded(
                    "max_data_value_bytes",
                    limits.max_data_value_bytes.to_string(),
                    size.to_string(),
                    format!(
                        "Reduce the size of data values to {} bytes or less",
                        limits.max_data_value_bytes
                    ),
                    Some(data_source.clone()),
                    None,
                    None,
                )
                .with_related_data(&name));
            }

            validate_value_against_type(&expected_type, &literal_value).map_err(|msg| {
                Error::validation(msg, Some(data_source.clone()), None::<String>)
                    .with_related_data(&name)
            })?;

            self.data.insert(
                data_path,
                DataDefinition::Value {
                    value: literal_value,
                    source: data_source,
                },
            );
        }

        Ok(self)
    }

    /// Promote declared defaults on type declarations into concrete [`DataDefinition::Value`] entries.
    /// Call BEFORE [`Self::set_data_values`] so user-provided values override defaults.
    /// Reference resolution is handled by the evaluator at runtime.
    #[must_use]
    pub fn with_defaults(mut self) -> Self {
        let promotions: Vec<(DataPath, DataDefinition)> = self
            .data
            .iter()
            .filter_map(|(path, def)| {
                if let DataDefinition::TypeDeclaration {
                    declared_default: Some(dv),
                    resolved_type,
                    source,
                } = def
                {
                    Some((
                        path.clone(),
                        DataDefinition::Value {
                            value: LiteralValue {
                                value: dv.clone(),
                                lemma_type: resolved_type.clone(),
                            },
                            source: source.clone(),
                        },
                    ))
                } else {
                    None
                }
            })
            .collect();

        for (path, def) in promotions {
            self.data.insert(path, def);
        }
        self
    }
}

pub(crate) fn validate_value_against_type(
    expected_type: &LemmaType,
    value: &LiteralValue,
) -> Result<(), String> {
    use crate::computation::rational::{commit_rational_to_decimal, RationalInteger};
    use crate::planning::semantics::TypeSpecification;

    fn exceeds_decimal_places(magnitude: &RationalInteger, max_decimals: u8) -> bool {
        match commit_rational_to_decimal(magnitude) {
            Ok(decimal) => decimal.scale() > u32::from(max_decimals),
            Err(_) => true,
        }
    }

    fn format_rational(r: &RationalInteger, decimals: Option<u8>) -> String {
        use crate::computation::rational::rational_to_display_str;
        match commit_rational_to_decimal(r) {
            Ok(decimal) => match decimals {
                Some(dp) => {
                    let rounded = decimal.round_dp(u32::from(dp));
                    format!("{:.prec$}", rounded, prec = dp as usize)
                }
                None => decimal.normalize().to_string(),
            },
            Err(_) => rational_to_display_str(r),
        }
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
                        format_rational(n, *decimals)
                    ));
                }
            }
            if let Some(min) = minimum {
                if n < min {
                    return Err(format!(
                        "{} is below minimum {}",
                        format_rational(n, *decimals),
                        format_rational(min, *decimals)
                    ));
                }
            }
            if let Some(max) = maximum {
                if n > max {
                    return Err(format!(
                        "{} is above maximum {}",
                        format_rational(n, *decimals),
                        format_rational(max, *decimals)
                    ));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Quantity {
                minimum,
                maximum,
                decimals,
                units,
                ..
            },
            ValueKind::Quantity(magnitude, unit, _),
        ) => {
            if let Some(d) = decimals {
                if exceeds_decimal_places(magnitude, *d) {
                    return Err(format!(
                        "{} {unit} exceeds decimals constraint {d}",
                        format_rational(magnitude, *decimals)
                    ));
                }
            }
            let quantity_unit = units.get(unit)?;
            if minimum.is_some() {
                let unit_minimum = quantity_unit.minimum.expect(
                    "BUG: QuantityUnit.minimum missing after type minimum set by sync_quantity_units_from_canonical",
                );
                if magnitude < &unit_minimum {
                    let value_display =
                        format!("{} {}", format_rational(magnitude, *decimals), unit);
                    let bound_display = format!(
                        "{} {}",
                        format_rational(&unit_minimum, *decimals),
                        quantity_unit.name
                    );
                    return Err(format!("{value_display} is below minimum {bound_display}"));
                }
            }
            if maximum.is_some() {
                let unit_maximum = quantity_unit.maximum.expect(
                    "BUG: QuantityUnit.maximum missing after type maximum set by sync_quantity_units_from_canonical",
                );
                if magnitude > &unit_maximum {
                    let value_display =
                        format!("{} {}", format_rational(magnitude, *decimals), unit);
                    let bound_display = format!(
                        "{} {}",
                        format_rational(&unit_maximum, *decimals),
                        quantity_unit.name
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
                if exceeds_decimal_places(r, *d) {
                    return Err(format!(
                        "{} exceeds decimals constraint {d}",
                        format_rational(r, *decimals)
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
                            let bound_per_unit = ratio_unit.minimum.expect(
                                "BUG: RatioUnit.minimum missing after type minimum set by sync_ratio_units_from_canonical",
                            );
                            format!(
                                "{} {unit} is below minimum {} {unit}",
                                format_rational(&value_per_unit, *decimals),
                                format_rational(&bound_per_unit, *decimals),
                            )
                        }
                        None => format!(
                            "{} is below minimum {}",
                            format_rational(r, *decimals),
                            format_rational(type_minimum, *decimals),
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
                            let bound_per_unit = ratio_unit.maximum.expect(
                                "BUG: RatioUnit.maximum missing after type maximum set by sync_ratio_units_from_canonical",
                            );
                            format!(
                                "{} {unit} is above maximum {} {unit}",
                                format_rational(&value_per_unit, *decimals),
                                format_rational(&bound_per_unit, *decimals),
                            )
                        }
                        None => format!(
                            "{} is above maximum {}",
                            format_rational(r, *decimals),
                            format_rational(type_maximum, *decimals),
                        ),
                    };
                    return Err(message);
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
            TypeSpecification::Calendar {
                minimum, maximum, ..
            },
            ValueKind::Calendar(value, unit),
        ) => {
            let value_months = crate::computation::units::convert_calendar_magnitude(
                *value,
                unit,
                &SemanticCalendarUnit::Month,
            );
            if let Some((min_val, min_unit)) = minimum {
                let min_months = crate::computation::units::convert_calendar_magnitude(
                    *min_val,
                    min_unit,
                    &SemanticCalendarUnit::Month,
                );
                if value_months < min_months {
                    return Err(format!(
                        "{value} {unit} is below minimum {min_val} {min_unit}"
                    ));
                }
            }
            if let Some((max_val, max_unit)) = maximum {
                let max_months = crate::computation::units::convert_calendar_magnitude(
                    *max_val,
                    max_unit,
                    &SemanticCalendarUnit::Month,
                );
                if value_months > max_months {
                    return Err(format!(
                        "{value} {unit} is above maximum {max_val} {max_unit}"
                    ));
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
        (TypeSpecification::Boolean { .. }, ValueKind::Boolean(_))
        | (TypeSpecification::NumberRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::DateRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::QuantityRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::RatioRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::CalendarRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::Veto { .. }, _)
        | (TypeSpecification::Undetermined, _) => Ok(()),
        (spec, value_kind) => unreachable!(
            "BUG: validate_value_against_type called with mismatched type/value: \
             spec={:?}, value={:?} — typing must be enforced before validation",
            spec, value_kind
        ),
    }
}

pub(crate) fn validate_literal_data_against_types(plan: &ExecutionPlan) -> Vec<Error> {
    let mut errors = Vec::new();

    for (data_path, data_definition) in &plan.data {
        let (expected_type, lit) = match data_definition {
            DataDefinition::Value { value, .. } => (&value.lemma_type, value),
            DataDefinition::TypeDeclaration { .. }
            | DataDefinition::Import { .. }
            | DataDefinition::Reference { .. } => continue,
        };

        if let Err(msg) = validate_value_against_type(expected_type, lit) {
            let source = data_definition.source().clone();
            errors.push(Error::validation(
                format!(
                    "Invalid value for data {} (expected {}): {}",
                    data_path,
                    expected_type.name(),
                    msg
                ),
                Some(source),
                None::<String>,
            ));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::{rational_zero, RationalInteger};
    use crate::parsing::ast::DateTimeValue;
    use crate::planning::semantics::{
        primitive_boolean, primitive_text, DataPath, LiteralValue, PathSegment, RulePath,
    };
    use crate::Engine;
    use serde_json;
    use std::str::FromStr;
    use std::sync::Arc;

    fn default_limits() -> ResourceLimits {
        ResourceLimits::default()
    }

    fn json_data(pairs: &[(&str, &str)]) -> HashMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::String((*v).to_string())))
            .collect()
    }

    #[test]
    fn test_with_raw_values() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec test
                data age: number -> default 25
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan(None, "test", Some(&now)).unwrap().clone();
        let data_path = DataPath::new(vec![], "age".to_string());

        let values = json_data(&[("age", "30")]);

        let updated_plan = plan.set_data_values(values, &default_limits()).unwrap();
        let updated_value = updated_plan.get_data_value(&data_path).unwrap();
        match &updated_value.value {
            crate::planning::semantics::ValueKind::Number(n) => {
                assert_eq!(*n, RationalInteger::new(30, 1));
            }
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    #[test]
    fn test_with_raw_values_type_mismatch() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec test
                data age: number
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan(None, "test", Some(&now)).unwrap().clone();

        let values = json_data(&[("age", "thirty")]);

        assert!(plan.set_data_values(values, &default_limits()).is_err());
    }

    #[test]
    fn test_with_raw_values_unknown_data() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec test
                data known: number
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan(None, "test", Some(&now)).unwrap().clone();

        let values = json_data(&[("unknown", "30")]);

        assert!(plan.set_data_values(values, &default_limits()).is_err());
    }

    #[test]
    fn test_with_raw_values_nested() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec private
                data base_price: number

                spec test
                uses rules: private
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan(None, "test", Some(&now)).unwrap().clone();

        let values = json_data(&[("rules.base_price", "100")]);

        let updated_plan = plan.set_data_values(values, &default_limits()).unwrap();
        let data_path = DataPath {
            segments: vec![PathSegment {
                data: "rules".to_string(),
                spec: "private".to_string(),
            }],
            data: "base_price".to_string(),
        };
        let updated_value = updated_plan.get_data_value(&data_path).unwrap();
        match &updated_value.value {
            crate::planning::semantics::ValueKind::Number(n) => {
                assert_eq!(*n, RationalInteger::new(100, 1));
            }
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    fn test_source() -> Source {
        use crate::parsing::ast::Span;
        Source::new(
            crate::parsing::source::SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        )
    }

    fn create_literal_expr(value: LiteralValue) -> Expression {
        Expression::new(
            crate::planning::semantics::ExpressionKind::Literal(Box::new(value)),
            test_source(),
        )
    }

    fn create_data_path_expr(path: DataPath) -> Expression {
        Expression::new(
            crate::planning::semantics::ExpressionKind::DataPath(path),
            test_source(),
        )
    }

    fn create_number_literal(n: rust_decimal::Decimal) -> LiteralValue {
        LiteralValue::number_from_decimal(n)
    }

    fn create_boolean_literal(b: bool) -> LiteralValue {
        LiteralValue::from_bool(b)
    }

    fn create_text_literal(s: String) -> LiteralValue {
        LiteralValue::text(s)
    }

    #[test]
    fn with_values_should_enforce_number_maximum_constraint() {
        // Higher-standard requirement: user input must be validated against type constraints.
        // If this test fails, Lemma accepts invalid values and gives false reassurance.
        let data_path = DataPath::new(vec![], "x".to_string());

        let max10 = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Number {
                minimum: None,
                maximum: Some(RationalInteger::new(10, 1)),
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
                    0.into(),
                    max10.clone(),
                ),
                source: source.clone(),
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let values = json_data(&[("x", "11")]);

        assert!(
            plan.set_data_values(values, &default_limits()).is_err(),
            "Providing x=11 should fail due to maximum 10"
        );
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
                    tier.clone(),
                ),
                source,
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let values = json_data(&[("tier", "platinum")]);

        assert!(
            plan.set_data_values(values, &default_limits()).is_err(),
            "Invalid enum value should be rejected (tier='platinum')"
        );
    }

    #[test]
    fn with_values_should_enforce_quantity_decimals() {
        // Higher-standard requirement: decimals should be enforced on quantity inputs,
        // unless the language explicitly defines rounding semantics.
        let data_path = DataPath::new(vec![], "price".to_string());

        let money = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Quantity {
                minimum: None,
                maximum: None,
                decimals: Some(2),
                units: crate::planning::semantics::QuantityUnits::from(vec![
                    crate::planning::semantics::QuantityUnit::from_decimal_factor(
                        "eur".to_string(),
                        rust_decimal::Decimal::from_str("1.0").unwrap(),
                        Vec::new(),
                    )
                    .expect("eur unit factor must be exact decimal"),
                ]),
                traits: Vec::new(),
                decomposition: crate::literals::BaseQuantityVector::new(),
                canonical_unit: "eur".to_string(),
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
                value: crate::planning::semantics::LiteralValue::quantity_with_type(
                    rational_zero(),
                    "eur".to_string(),
                    money.clone(),
                ),
                source,
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let values = json_data(&[("price", "1.234 eur")]);

        assert!(
            plan.set_data_values(values, &default_limits()).is_err(),
            "Quantity decimals=2 should reject 1.234 eur"
        );
    }

    #[test]
    fn test_serialize_deserialize_execution_plan() {
        let data_path = DataPath {
            segments: vec![],
            data: "age".to_string(),
        };
        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.spec_name, plan.spec_name);
        assert_eq!(deserialized.data.len(), plan.data.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
    }

    #[test]
    fn test_serialize_deserialize_plan_with_imported_named_type_defining_spec() {
        let dep_spec = Arc::new(crate::parsing::ast::LemmaSpec::new("examples".to_string()));
        let imported_type = crate::planning::semantics::LemmaType::new(
            "salary".to_string(),
            TypeSpecification::quantity(),
            crate::planning::semantics::TypeExtends::Custom {
                parent: "money".to_string(),
                family: "money".to_string(),
                defining_spec: crate::planning::semantics::TypeDefiningSpec::Import {
                    spec: Arc::clone(&dep_spec),
                },
            },
        );

        let salary_path = DataPath::new(vec![], "salary".to_string());
        let mut data = IndexMap::new();
        data.insert(
            salary_path,
            crate::planning::semantics::DataDefinition::TypeDeclaration {
                resolved_type: imported_type,
                declared_default: None,
                source: test_source(),
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        let recovered = deserialized
            .data
            .get(&DataPath::new(vec![], "salary".to_string()))
            .and_then(|d| d.schema_type())
            .expect("salary type should be present in plan.data");
        match &recovered.extends {
            crate::planning::semantics::TypeExtends::Custom {
                defining_spec: crate::planning::semantics::TypeDefiningSpec::Import { spec },
                ..
            } => {
                assert_eq!(spec.name, "examples");
            }
            other => panic!(
                "Expected imported defining_spec after round-trip, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_serialize_deserialize_plan_with_rules() {
        use crate::planning::semantics::ExpressionKind;

        let age_path = DataPath::new(vec![], "age".to_string());
        let mut data = IndexMap::new();
        data.insert(
            age_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "can_drive".to_string()),
            name: "can_drive".to_string(),
            branches: vec![{
                let result = create_literal_expr(create_boolean_literal(true));
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Arc::new(create_data_path_expr(age_path.clone())),
                            crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                            Arc::new(create_literal_expr(create_number_literal(18.into()))),
                        ),
                        test_source(),
                    )),
                    normalized_condition: None,
                    result: result.clone(),
                    normalized_result: result,
                    source: test_source(),
                }
            }],
            needs_data: BTreeSet::from([age_path]),
            source: test_source(),
            rule_type: primitive_boolean().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.spec_name, plan.spec_name);
        assert_eq!(deserialized.data.len(), plan.data.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
        assert_eq!(deserialized.rules[0].name, "can_drive");
        assert_eq!(deserialized.rules[0].branches.len(), 1);
        assert_eq!(deserialized.rules[0].needs_data.len(), 1);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_nested_data_paths() {
        use crate::planning::semantics::PathSegment;
        let data_path = DataPath {
            segments: vec![PathSegment {
                data: "employee".to_string(),
                spec: "private".to_string(),
            }],
            data: "salary".to_string(),
        };

        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.data.len(), 1);
        let (deserialized_path, _) = deserialized.data.iter().next().unwrap();
        assert_eq!(deserialized_path.segments.len(), 1);
        assert_eq!(deserialized_path.segments[0].data, "employee");
        assert_eq!(deserialized_path.data, "salary");
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_data_types() {
        let name_path = DataPath::new(vec![], "name".to_string());
        let age_path = DataPath::new(vec![], "age".to_string());
        let active_path = DataPath::new(vec![], "active".to_string());

        let mut data = IndexMap::new();
        data.insert(
            name_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_text_literal("Alice".to_string()),
                source: test_source(),
            },
        );
        data.insert(
            age_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(30.into()),
                source: test_source(),
            },
        );
        data.insert(
            active_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_boolean_literal(true),
                source: test_source(),
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.data.len(), 3);

        assert_eq!(
            deserialized.get_data_value(&name_path).unwrap().value,
            crate::planning::semantics::ValueKind::Text("Alice".to_string())
        );
        assert_eq!(
            deserialized.get_data_value(&age_path).unwrap().value,
            crate::planning::semantics::ValueKind::Number(30.into())
        );
        assert_eq!(
            deserialized.get_data_value(&active_path).unwrap().value,
            crate::planning::semantics::ValueKind::Boolean(true)
        );
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_branches() {
        use crate::planning::semantics::ExpressionKind;

        let points_path = DataPath::new(vec![], "points".to_string());
        let mut data = IndexMap::new();
        data.insert(
            points_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "tier".to_string()),
            name: "tier".to_string(),
            branches: vec![
                {
                    let result = create_literal_expr(create_text_literal("bronze".to_string()));
                    Branch {
                        condition: None,
                        normalized_condition: None,
                        result: result.clone(),
                        normalized_result: result,
                        source: test_source(),
                    }
                },
                {
                    let result = create_literal_expr(create_text_literal("silver".to_string()));
                    Branch {
                        condition: Some(Expression::new(
                            ExpressionKind::Comparison(
                                Arc::new(create_data_path_expr(points_path.clone())),
                                crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                                Arc::new(create_literal_expr(create_number_literal(100.into()))),
                            ),
                            test_source(),
                        )),
                        normalized_condition: None,
                        result: result.clone(),
                        normalized_result: result,
                        source: test_source(),
                    }
                },
                {
                    let result = create_literal_expr(create_text_literal("gold".to_string()));
                    Branch {
                        condition: Some(Expression::new(
                            ExpressionKind::Comparison(
                                Arc::new(create_data_path_expr(points_path.clone())),
                                crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                                Arc::new(create_literal_expr(create_number_literal(500.into()))),
                            ),
                            test_source(),
                        )),
                        normalized_condition: None,
                        result: result.clone(),
                        normalized_result: result,
                        source: test_source(),
                    }
                },
            ],
            needs_data: BTreeSet::from([points_path]),
            source: test_source(),
            rule_type: primitive_text().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.rules.len(), 1);
        assert_eq!(deserialized.rules[0].branches.len(), 3);
        assert!(deserialized.rules[0].branches[0].condition.is_none());
        assert!(deserialized.rules[0].branches[1].condition.is_some());
        assert!(deserialized.rules[0].branches[2].condition.is_some());
    }

    #[test]
    fn test_serialize_deserialize_empty_plan() {
        let plan = ExecutionPlan {
            spec_name: "empty".to_string(),
            data: IndexMap::new(),
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.spec_name, "empty");
        assert_eq!(deserialized.data.len(), 0);
        assert_eq!(deserialized.rules.len(), 0);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_arithmetic_expressions() {
        use crate::planning::semantics::ExpressionKind;

        let x_path = DataPath::new(vec![], "x".to_string());
        let mut data = IndexMap::new();
        data.insert(
            x_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "doubled".to_string()),
            name: "doubled".to_string(),
            branches: vec![{
                let result = Expression::new(
                    ExpressionKind::Arithmetic(
                        Arc::new(create_data_path_expr(x_path.clone())),
                        crate::parsing::ast::ArithmeticComputation::Multiply,
                        Arc::new(create_literal_expr(create_number_literal(2.into()))),
                    ),
                    test_source(),
                );
                Branch {
                    condition: None,
                    normalized_condition: None,
                    result: result.clone(),
                    normalized_result: result,
                    source: test_source(),
                }
            }],
            needs_data: BTreeSet::from([x_path]),
            source: test_source(),
            rule_type: crate::planning::semantics::primitive_number().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.rules.len(), 1);
        match &deserialized.rules[0].branches[0].result.kind {
            ExpressionKind::Arithmetic(left, op, right) => {
                assert_eq!(*op, crate::parsing::ast::ArithmeticComputation::Multiply);
                match &left.kind {
                    ExpressionKind::DataPath(_) => {}
                    _ => panic!("Expected DataPath in left operand"),
                }
                match &right.kind {
                    ExpressionKind::Literal(_) => {}
                    _ => panic!("Expected Literal in right operand"),
                }
            }
            _ => panic!("Expected Arithmetic expression"),
        }
    }

    #[test]
    fn test_serialize_deserialize_round_trip_equality() {
        use crate::planning::semantics::ExpressionKind;

        let age_path = DataPath::new(vec![], "age".to_string());
        let mut data = IndexMap::new();
        data.insert(
            age_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data,
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "is_adult".to_string()),
            name: "is_adult".to_string(),
            branches: vec![{
                let result = create_literal_expr(create_boolean_literal(true));
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Arc::new(create_data_path_expr(age_path.clone())),
                            crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                            Arc::new(create_literal_expr(create_number_literal(18.into()))),
                        ),
                        test_source(),
                    )),
                    normalized_condition: None,
                    result: result.clone(),
                    normalized_result: result,
                    source: test_source(),
                }
            }],
            needs_data: BTreeSet::from([age_path]),
            source: test_source(),
            rule_type: primitive_boolean().clone(),
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        let json2 = serde_json::to_string(&deserialized).expect("Should serialize again");
        let deserialized2: ExecutionPlan =
            serde_json::from_str(&json2).expect("Should deserialize again");

        assert_eq!(deserialized2.spec_name, plan.spec_name);
        assert_eq!(deserialized2.data.len(), plan.data.len());
        assert_eq!(deserialized2.rules.len(), plan.rules.len());
        assert_eq!(deserialized2.rules[0].name, plan.rules[0].name);
        assert_eq!(
            deserialized2.rules[0].branches.len(),
            plan.rules[0].branches.len()
        );
    }

    fn empty_plan(effective: crate::parsing::ast::EffectiveDate) -> ExecutionPlan {
        ExecutionPlan {
            spec_name: "s".into(),
            data: IndexMap::new(),
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            unit_index: HashMap::new(),
            effective,
            sources: Vec::new(),
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
        };

        let set = ExecutionPlanSet {
            spec_name: "s".into(),
            plans: vec![
                empty_plan(EffectiveDate::Origin),
                empty_plan(EffectiveDate::DateTimeValue(june.clone())),
                empty_plan(EffectiveDate::DateTimeValue(dec.clone())),
            ],
        };

        assert!(std::ptr::eq(
            set.plan_at(&EffectiveDate::DateTimeValue(june.clone()))
                .expect("boundary instant"),
            &set.plans[1]
        ));
        assert!(std::ptr::eq(
            set.plan_at(&EffectiveDate::DateTimeValue(dec.clone()))
                .expect("dec boundary"),
            &set.plans[2]
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
        };

        let set = ExecutionPlanSet {
            spec_name: "s".into(),
            plans: vec![
                empty_plan(EffectiveDate::Origin),
                empty_plan(EffectiveDate::DateTimeValue(june)),
            ],
        };

        assert!(std::ptr::eq(
            set.plan_at(&EffectiveDate::DateTimeValue(may_end))
                .expect("may 31"),
            &set.plans[0]
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
        };
        let set = ExecutionPlanSet {
            spec_name: "s".into(),
            plans: vec![empty_plan(EffectiveDate::DateTimeValue(DateTimeValue {
                year: 2025,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                microsecond: 0,
                timezone: None,
            }))],
        };
        assert!(std::ptr::eq(
            set.plan_at(&EffectiveDate::DateTimeValue(t))
                .expect("inside single slice"),
            &set.plans[0]
        ));
    }

    /// The schema JSON shape is the IO contract for every non-Rust consumer
    /// (WASM playground, Hex, HTTP, TypeScript). Nail the exact envelope.
    #[test]
    fn schema_json_shape_contract() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec pricing
                data bridge_height: quantity
                  -> unit meter 1
                  -> default 100 meter
                data quantity: number -> minimum 0
                rule cost: bridge_height * quantity
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine
            .get_plan(None, "pricing", Some(&now))
            .unwrap()
            .schema();

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
            bh.get("default").is_some(),
            "bridge_height exposes `-> default` as schema default suggestion"
        );
        assert!(
            bh.get("bound_value").is_none(),
            "bridge_height is not a spec-bound literal"
        );

        let ty = &bh["type"];
        assert_eq!(
            ty["kind"], "quantity",
            "kind tag sits on the type object itself"
        );
        assert!(
            ty["units"].is_array(),
            "quantity-only fields flatten up to top level"
        );
        assert!(
            ty.get("options").is_none(),
            "text-only fields must not leak"
        );

        let qty = &value["data"]["quantity"];
        assert_eq!(qty["type"]["kind"], "number");
        assert!(
            qty.get("default").is_none(),
            "quantity has no default suggestion"
        );
        assert!(
            qty.get("bound_value").is_none(),
            "quantity has no bound literal"
        );

        let cost = &value["rules"]["cost"];
        assert_eq!(
            cost["kind"], "quantity",
            "rule types use the same flat shape"
        );
        assert!(
            cost["units"].is_array() && !cost["units"].as_array().unwrap().is_empty(),
            "quantity rule result types expose declared units"
        );
        assert!(
            cost["units"][0].get("factor").is_some(),
            "quantity rule units use factor field"
        );
    }

    #[test]
    fn schema_rule_result_units_contract() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec units_contract
                data money: quantity
                  -> unit eur 1
                  -> unit usd 0.91
                data rate: ratio
                  -> unit basis_points 10000
                  -> unit percent 100
                  -> default 500 basis_points
                rule total: money
                rule rate_out: rate
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "units_contract.lemma",
                ))),
            )
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine
            .get_plan(None, "units_contract", Some(&now))
            .unwrap()
            .schema();
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
    fn schema_json_round_trip_preserves_shape() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec s
                data age: number -> minimum 0 -> default 18
                data grade: text -> options "A" "B" "C"
                rule adult: age >= 18
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("s.lemma"))),
            )
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine.get_plan(None, "s", Some(&now)).unwrap().schema();

        let json = serde_json::to_string(&schema).unwrap();
        let round_tripped: SpecSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(schema, round_tripped);
    }
}

// ---------------------------------------------------------------------------
// ExecutionPlanSet (formerly plan_set.rs)
// ---------------------------------------------------------------------------
