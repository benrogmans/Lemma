//! Execution plan for evaluated specs
//!
//! Provides a complete self-contained execution plan ready for the evaluator.
//! The plan contains all data, rules flattened into executable branches,
//! and execution order - no spec structure needed during evaluation.
//!
//! Reliability model:
//! - `SpecSchema` is the IO contract surface for consumers (data and rule outputs).
//!   IO compatibility is the consumer-facing guarantee.

use crate::parsing::ast::{EffectiveDate, LemmaSpec, MetaValue};
use crate::planning::graph::Graph;
use crate::planning::graph::ResolvedSpecTypes;
use crate::planning::semantics;
use crate::planning::semantics::{
    DataDefinition, DataPath, Expression, LemmaType, LiteralValue, RulePath, TypeSpecification,
    ValueKind,
};
use crate::Error;
use crate::ResourceLimits;
use crate::Source;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

/// Spec sources keyed by (name, effective_from).
pub type SpecSources = IndexMap<(String, EffectiveDate), String>;

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

    /// Named types defined in or imported by this spec, in deterministic order.
    pub named_types: BTreeMap<String, LemmaType>,

    pub effective: EffectiveDate,

    /// Canonical source for all specs in this plan, keyed by (name, effective_from).
    /// Reconstructed from AST — not raw file content.
    #[serde(default)]
    #[serde(
        serialize_with = "serialize_sources",
        deserialize_with = "deserialize_sources"
    )]
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

fn serialize_sources<S>(sources: &SpecSources, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeSeq;
    let mut seq = serializer.serialize_seq(Some(sources.len()))?;
    for ((name, effective_from), source) in sources {
        seq.serialize_element(&SpecSourceEntry {
            name,
            effective_from,
            source,
        })?;
    }
    seq.end()
}

fn deserialize_sources<'de, D>(deserializer: D) -> Result<SpecSources, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let entries: Vec<SpecSourceEntryOwned> = Vec::deserialize(deserializer)?;
    let mut map = IndexMap::with_capacity(entries.len());
    for e in entries {
        map.insert((e.name, e.effective_from), e.source);
    }
    Ok(map)
}

#[derive(Serialize)]
struct SpecSourceEntry<'a> {
    name: &'a str,
    effective_from: &'a EffectiveDate,
    source: &'a str,
}

#[derive(Deserialize)]
struct SpecSourceEntryOwned {
    name: String,
    effective_from: EffectiveDate,
    source: String,
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

    /// Result expression
    pub result: Expression,

    /// Source location for error messages (always present for branches from parsed specs)
    pub source: Source,
}

/// Builds an execution plan from a Graph for one temporal slice.
/// Internal implementation detail - only called by plan()
pub(crate) fn build_execution_plan(
    graph: &Graph,
    resolved_types: &HashMap<Arc<LemmaSpec>, ResolvedSpecTypes>,
    effective: &EffectiveDate,
) -> ExecutionPlan {
    let data = graph.build_data();
    let execution_order = graph.execution_order();

    let mut executable_rules: Vec<ExecutableRule> = Vec::new();
    let mut path_to_index: HashMap<RulePath, usize> = HashMap::new();

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
        for (condition, result) in &rule_node.branches {
            executable_branches.push(Branch {
                condition: condition.clone(),
                result: result.clone(),
                source: rule_node.source.clone(),
            });
        }

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

    let main_spec = graph.main_spec();
    let named_types = build_type_tables(main_spec, resolved_types);

    let mut sources: SpecSources = IndexMap::new();
    for spec in resolved_types.keys() {
        let key = (spec.name.clone(), spec.effective_from.clone());
        sources
            .entry(key)
            .or_insert_with(|| crate::formatting::format_spec(spec, crate::formatting::MAX_COLS));
    }

    ExecutionPlan {
        spec_name: main_spec.name.clone(),
        data,
        rules: executable_rules,
        reference_evaluation_order: graph.reference_evaluation_order().to_vec(),
        meta: main_spec
            .meta_fields
            .iter()
            .map(|f| (f.key.clone(), f.value.clone()))
            .collect(),
        named_types,
        effective: effective.clone(),
        sources,
    }
}

/// Build the named types table from the main spec's resolved types.
fn build_type_tables(
    main_spec: &Arc<LemmaSpec>,
    resolved_types: &HashMap<Arc<LemmaSpec>, ResolvedSpecTypes>,
) -> BTreeMap<String, LemmaType> {
    let mut named_types = BTreeMap::new();

    let main_resolved = resolved_types
        .iter()
        .find(|(spec, _)| Arc::ptr_eq(spec, main_spec))
        .map(|(_, types)| types);

    if let Some(resolved) = main_resolved {
        for (type_name, lemma_type) in &resolved.named_types {
            named_types.insert(type_name.clone(), lemma_type.clone());
        }
    }

    named_types
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
/// A named struct instead of a `(type, default)` tuple so JSON-native consumers
/// (TypeScript, Python, ...) get stable field names. `default` is `None` unless
/// the spec (or a typedef it references) declared one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataEntry {
    #[serde(rename = "type")]
    pub lemma_type: LemmaType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default: Option<LiteralValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecSchema {
    /// Spec name
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
            let mut keys: Vec<&String> = self.meta.keys().collect();
            keys.sort();
            for key in keys {
                write!(f, "\n  {}: {}", key, self.meta.get(key).unwrap())?;
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
        TypeSpecification::Scale {
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
            if let Some(v) = minimum {
                parts.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                parts.push(format!("maximum: {}", v));
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
        | TypeSpecification::Duration { .. }
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
                let default = data.schema_default();
                (
                    data.source().span.start,
                    path.input_key(),
                    DataEntry {
                        lemma_type,
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
            .filter(|(_, data)| data.schema_type().is_some())
            .map(|(path, data)| {
                let lemma_type = data.schema_type().unwrap().clone();
                let default = data.schema_default();
                (
                    data.source().span.start,
                    path.input_key(),
                    DataEntry {
                        lemma_type,
                        default,
                    },
                )
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

    /// Provide string values for data.
    ///
    /// Parses each string to its expected type, validates constraints, and applies to the plan.
    pub fn with_data_values(
        mut self,
        values: HashMap<String, String>,
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

            let parsed_value = crate::planning::semantics::parse_value_from_string(
                &raw_value,
                &expected_type.specifications,
                &data_source,
            )
            .map_err(|e| e.with_related_data(&name))?;
            let semantic_value = semantics::value_to_semantic(&parsed_value).map_err(|msg| {
                Error::validation(msg, Some(data_source.clone()), None::<String>)
                    .with_related_data(&name)
            })?;
            let literal_value = LiteralValue {
                value: semantic_value,
                lemma_type: expected_type.clone(),
            };

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
}

pub(crate) fn validate_value_against_type(
    expected_type: &LemmaType,
    value: &LiteralValue,
) -> Result<(), String> {
    use crate::planning::semantics::TypeSpecification;

    let effective_decimals = |n: rust_decimal::Decimal| n.scale();

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
            if let Some(min) = minimum {
                if n < min {
                    return Err(format!("{} is below minimum {}", n, min));
                }
            }
            if let Some(max) = maximum {
                if n > max {
                    return Err(format!("{} is above maximum {}", n, max));
                }
            }
            if let Some(d) = decimals {
                if effective_decimals(*n) > u32::from(*d) {
                    return Err(format!("{} has more than {} decimals", n, d));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Scale {
                minimum,
                maximum,
                decimals,
                ..
            },
            ValueKind::Scale(n, _unit),
        ) => {
            if let Some(min) = minimum {
                if n < min {
                    return Err(format!("{} is below minimum {}", n, min));
                }
            }
            if let Some(max) = maximum {
                if n > max {
                    return Err(format!("{} is above maximum {}", n, max));
                }
            }
            if let Some(d) = decimals {
                if effective_decimals(*n) > u32::from(*d) {
                    return Err(format!("{} has more than {} decimals", n, d));
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
                ..
            },
            ValueKind::Ratio(r, _unit),
        ) => {
            if let Some(min) = minimum {
                if r < min {
                    return Err(format!("{} is below minimum {}", r, min));
                }
            }
            if let Some(max) = maximum {
                if r > max {
                    return Err(format!("{} is above maximum {}", r, max));
                }
            }
            if let Some(d) = decimals {
                if effective_decimals(*r) > u32::from(*d) {
                    return Err(format!("{} has more than {} decimals", r, d));
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
            TypeSpecification::Duration {
                minimum, maximum, ..
            },
            ValueKind::Duration(value, unit),
        ) => {
            use crate::computation::units::duration_to_seconds;
            let value_secs = duration_to_seconds(*value, unit);
            if let Some((min_v, min_u)) = minimum {
                let min_secs = duration_to_seconds(*min_v, min_u);
                if value_secs < min_secs {
                    return Err(format!(
                        "{} {} is below minimum {} {}",
                        value, unit, min_v, min_u
                    ));
                }
            }
            if let Some((max_v, max_u)) = maximum {
                let max_secs = duration_to_seconds(*max_v, max_u);
                if value_secs > max_secs {
                    return Err(format!(
                        "{} {} is above maximum {} {}",
                        value, unit, max_v, max_u
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
            | DataDefinition::SpecRef { .. }
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

    #[test]
    fn test_with_raw_values() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec test
                data age: number -> default 25
                "#,
                crate::SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan("test", Some(&now)).unwrap().clone();
        let data_path = DataPath::new(vec![], "age".to_string());

        let mut values = HashMap::new();
        values.insert("age".to_string(), "30".to_string());

        let updated_plan = plan.with_data_values(values, &default_limits()).unwrap();
        let updated_value = updated_plan.get_data_value(&data_path).unwrap();
        match &updated_value.value {
            crate::planning::semantics::ValueKind::Number(n) => {
                assert_eq!(n, &rust_decimal::Decimal::from(30))
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
                crate::SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan("test", Some(&now)).unwrap().clone();

        let mut values = HashMap::new();
        values.insert("age".to_string(), "thirty".to_string());

        assert!(plan.with_data_values(values, &default_limits()).is_err());
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
                crate::SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan("test", Some(&now)).unwrap().clone();

        let mut values = HashMap::new();
        values.insert("unknown".to_string(), "30".to_string());

        assert!(plan.with_data_values(values, &default_limits()).is_err());
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
                with rules: private
                "#,
                crate::SourceType::Labeled("test.lemma"),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan("test", Some(&now)).unwrap().clone();

        let mut values = HashMap::new();
        values.insert("rules.base_price".to_string(), "100".to_string());

        let updated_plan = plan.with_data_values(values, &default_limits()).unwrap();
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
                assert_eq!(n, &rust_decimal::Decimal::from(100))
            }
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    fn test_source() -> crate::Source {
        use crate::parsing::ast::Span;
        crate::Source::new(
            "<test>",
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
        LiteralValue::number(n)
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
                maximum: Some(rust_decimal::Decimal::from_str("10").unwrap()),
                decimals: None,
                precision: None,
                help: String::new(),
            },
        );
        let source = Source::new(
            "<test>",
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("x".to_string(), "11".to_string());

        assert!(
            plan.with_data_values(values, &default_limits()).is_err(),
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
            "<test>",
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("tier".to_string(), "platinum".to_string());

        assert!(
            plan.with_data_values(values, &default_limits()).is_err(),
            "Invalid enum value should be rejected (tier='platinum')"
        );
    }

    #[test]
    fn with_values_should_enforce_scale_decimals() {
        // Higher-standard requirement: decimals should be enforced on scale inputs,
        // unless the language explicitly defines rounding semantics.
        let data_path = DataPath::new(vec![], "price".to_string());

        let money = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Scale {
                minimum: None,
                maximum: None,
                decimals: Some(2),
                precision: None,
                units: crate::planning::semantics::ScaleUnits::from(vec![
                    crate::planning::semantics::ScaleUnit {
                        name: "eur".to_string(),
                        value: rust_decimal::Decimal::from_str("1.0").unwrap(),
                    },
                ]),
                help: String::new(),
            },
        );
        let source = Source::new(
            "<test>",
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
                value: crate::planning::semantics::LiteralValue::scale_with_type(
                    rust_decimal::Decimal::from_str("0").unwrap(),
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("price".to_string(), "1.234 eur".to_string());

        assert!(
            plan.with_data_values(values, &default_limits()).is_err(),
            "Scale decimals=2 should reject 1.234 eur"
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
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
            TypeSpecification::scale(),
            crate::planning::semantics::TypeExtends::Custom {
                parent: "money".to_string(),
                family: "money".to_string(),
                defining_spec: crate::planning::semantics::TypeDefiningSpec::Import {
                    spec: Arc::clone(&dep_spec),
                },
            },
        );

        let mut named_types = BTreeMap::new();
        named_types.insert("salary".to_string(), imported_type);

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            data: IndexMap::new(),
            rules: Vec::new(),
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            named_types,
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        let recovered = deserialized
            .named_types
            .get("salary")
            .expect("salary type should be present");
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "can_drive".to_string()),
            name: "can_drive".to_string(),
            branches: vec![Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Arc::new(create_data_path_expr(age_path.clone())),
                        crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                        Arc::new(create_literal_expr(create_number_literal(18.into()))),
                    ),
                    test_source(),
                )),
                result: create_literal_expr(create_boolean_literal(true)),
                source: test_source(),
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "tier".to_string()),
            name: "tier".to_string(),
            branches: vec![
                Branch {
                    condition: None,
                    result: create_literal_expr(create_text_literal("bronze".to_string())),
                    source: test_source(),
                },
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Arc::new(create_data_path_expr(points_path.clone())),
                            crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                            Arc::new(create_literal_expr(create_number_literal(100.into()))),
                        ),
                        test_source(),
                    )),
                    result: create_literal_expr(create_text_literal("silver".to_string())),
                    source: test_source(),
                },
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Arc::new(create_data_path_expr(points_path.clone())),
                            crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                            Arc::new(create_literal_expr(create_number_literal(500.into()))),
                        ),
                        test_source(),
                    )),
                    result: create_literal_expr(create_text_literal("gold".to_string())),
                    source: test_source(),
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "doubled".to_string()),
            name: "doubled".to_string(),
            branches: vec![Branch {
                condition: None,
                result: Expression::new(
                    ExpressionKind::Arithmetic(
                        Arc::new(create_data_path_expr(x_path.clone())),
                        crate::parsing::ast::ArithmeticComputation::Multiply,
                        Arc::new(create_literal_expr(create_number_literal(2.into()))),
                    ),
                    test_source(),
                ),
                source: test_source(),
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
            named_types: BTreeMap::new(),
            effective: EffectiveDate::Origin,
            sources: IndexMap::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "is_adult".to_string()),
            name: "is_adult".to_string(),
            branches: vec![Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Arc::new(create_data_path_expr(age_path.clone())),
                        crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                        Arc::new(create_literal_expr(create_number_literal(18.into()))),
                    ),
                    test_source(),
                )),
                result: create_literal_expr(create_boolean_literal(true)),
                source: test_source(),
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
            named_types: BTreeMap::new(),
            effective,
            sources: IndexMap::new(),
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
                data bridge_height: scale
                  -> unit meter 1
                  -> default 100 meter
                data quantity: number -> minimum 0
                rule cost: bridge_height * quantity
                "#,
                crate::SourceType::Labeled("test.lemma"),
            )
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine.get_plan("pricing", Some(&now)).unwrap().schema();

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
            "bridge_height has a promoted default"
        );

        let ty = &bh["type"];
        assert_eq!(
            ty["kind"], "scale",
            "kind tag sits on the type object itself"
        );
        assert!(
            ty["units"].is_array(),
            "scale-only fields flatten up to top level"
        );
        assert!(
            ty.get("options").is_none(),
            "text-only fields must not leak"
        );

        let qty = &value["data"]["quantity"];
        assert_eq!(qty["type"]["kind"], "number");
        assert!(
            qty.get("default").is_none(),
            "no declared default means no field"
        );

        let cost = &value["rules"]["cost"];
        assert_eq!(cost["kind"], "scale", "rule types use the same flat shape");
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
                crate::SourceType::Labeled("s.lemma"),
            )
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine.get_plan("s", Some(&now)).unwrap().schema();

        let json = serde_json::to_string(&schema).unwrap();
        let round_tripped: SpecSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(schema, round_tripped);
    }
}

// ---------------------------------------------------------------------------
// ExecutionPlanSet (formerly plan_set.rs)
// ---------------------------------------------------------------------------
