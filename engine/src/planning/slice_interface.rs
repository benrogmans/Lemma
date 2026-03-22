use crate::parsing::ast::{LemmaSpec, Span};
use crate::parsing::source::Source;
use crate::planning::execution_plan::ExecutionPlan;
use crate::planning::semantics::{ExpressionKind, FactData, LemmaType, PathSegment, RulePath};
use crate::planning::types::ResolvedSpecTypes;
use crate::Error;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

type ResolvedTypesMap = HashMap<Arc<LemmaSpec>, ResolvedSpecTypes>;

/// The resolved interface of a referenced spec within a single temporal slice.
///
/// Captures only what the caller actually uses: needed facts, referenced rules,
/// and type definitions. Two SliceInterfaces are equal iff the caller sees the
/// exact same contract from the referenced spec in both slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceInterface {
    pub facts: BTreeMap<String, FactKind>,
    pub rules: BTreeMap<String, LemmaType>,
    pub types: BTreeMap<String, LemmaType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactKind {
    Value(Box<LemmaType>),
    SpecRef { spec_name: String },
}

impl SliceInterface {
    /// Extract the interface of a referenced spec from a built execution plan.
    ///
    /// `segments` identifies the referenced spec (e.g. `[PathSegment { fact: "b", spec: "B" }]`).
    /// Uses the plan's precomputed `needs_facts` to determine which facts matter.
    pub(crate) fn from_plan(
        plan: &ExecutionPlan,
        segments: &[PathSegment],
        resolved_types: &ResolvedTypesMap,
        ref_spec: &Arc<LemmaSpec>,
    ) -> Self {
        let needed_at_segments = collect_needed_facts_at_segments(plan, segments);

        let mut facts = BTreeMap::new();
        for (path, data) in &plan.facts {
            if path.segments != *segments {
                continue;
            }
            if !needed_at_segments.contains(path.fact.as_str()) {
                continue;
            }
            let kind = match data {
                FactData::Value { value, .. } => {
                    FactKind::Value(Box::new(value.lemma_type.clone()))
                }
                FactData::TypeDeclaration { resolved_type, .. } => {
                    FactKind::Value(Box::new(resolved_type.clone()))
                }
                FactData::SpecRef { spec, .. } => FactKind::SpecRef {
                    spec_name: spec.name.clone(),
                },
            };
            facts.insert(path.fact.clone(), kind);
        }

        let referenced_rules = collect_referenced_rules_at_segments(plan, segments);
        let mut rules = BTreeMap::new();
        for rule in &plan.rules {
            if rule.path.segments != *segments {
                continue;
            }
            if !referenced_rules.contains(rule.name.as_str()) {
                continue;
            }
            rules.insert(rule.name.clone(), rule.rule_type.clone());
        }

        let mut types = BTreeMap::new();
        if let Some(spec_types) = resolved_types.get(ref_spec) {
            for (name, lemma_type) in &spec_types.named_types {
                types.insert(name.clone(), lemma_type.clone());
            }
        }

        SliceInterface {
            facts,
            rules,
            types,
        }
    }

    pub fn diff(&self, other: &SliceInterface) -> Vec<String> {
        let mut diffs = Vec::new();
        diff_map("fact", &self.facts, &other.facts, &mut diffs, |a, b| a != b);
        diff_map("rule", &self.rules, &other.rules, &mut diffs, |a, b| a != b);
        diff_map("type", &self.types, &other.types, &mut diffs, |a, b| a != b);
        diffs
    }
}

fn diff_map<V: std::fmt::Debug>(
    label: &str,
    a: &BTreeMap<String, V>,
    b: &BTreeMap<String, V>,
    diffs: &mut Vec<String>,
    changed: impl Fn(&V, &V) -> bool,
) {
    for key in a.keys() {
        if !b.contains_key(key) {
            diffs.push(format!("{} '{}' removed", label, key));
        }
    }
    for key in b.keys() {
        if !a.contains_key(key) {
            diffs.push(format!("{} '{}' added", label, key));
        }
    }
    for (key, val_a) in a {
        if let Some(val_b) = b.get(key) {
            if changed(val_a, val_b) {
                diffs.push(format!(
                    "{} '{}' changed: {:?} -> {:?}",
                    label, key, val_a, val_b
                ));
            }
        }
    }
}

/// Collect fact names at `segments` depth that any root-level rule needs.
///
/// Uses the plan's precomputed `needs_facts` (transitive closure) and also
/// extracts intermediate SpecRef traversal facts: if a needed FactPath or
/// a referenced RulePath passes through a deeper segment, the linking fact at
/// `segments` depth is itself a needed interface fact.
fn collect_needed_facts_at_segments<'a>(
    plan: &'a ExecutionPlan,
    segments: &[PathSegment],
) -> HashSet<&'a str> {
    let mut needed = HashSet::new();

    for rule in &plan.rules {
        if !rule.path.segments.is_empty() {
            continue;
        }
        for fp in &rule.needs_facts {
            if fp.segments == *segments {
                needed.insert(fp.fact.as_str());
            }
            if fp.segments.len() > segments.len() && fp.segments[..segments.len()] == *segments {
                needed.insert(fp.segments[segments.len()].fact.as_str());
            }
        }
    }

    // RulePath references at deeper segments also imply an intermediate
    // SpecRef fact at our level (e.g. `b.nested.val` means `nested` is needed).
    let referenced_rules = collect_root_rule_paths(plan);
    for rp in &referenced_rules {
        if rp.segments.len() > segments.len() && rp.segments[..segments.len()] == *segments {
            needed.insert(rp.segments[segments.len()].fact.as_str());
        }
    }

    needed
}

/// Collect rule names at `segments` depth that root-level rules directly reference.
///
/// Walks root-level rule expressions for RulePath references at the dep's depth.
/// Internal dep rules (only reachable transitively within the dep) are excluded —
/// the caller only cares about the dep rules it explicitly uses.
fn collect_referenced_rules_at_segments<'a>(
    plan: &'a ExecutionPlan,
    segments: &[PathSegment],
) -> HashSet<&'a str> {
    let mut referenced = HashSet::new();
    let all_rule_paths = collect_root_rule_paths(plan);
    for rp in &all_rule_paths {
        if rp.segments == *segments {
            referenced.insert(rp.rule.as_str());
        }
    }
    referenced
}

/// Collect all RulePath references from root-level rule expressions.
fn collect_root_rule_paths(plan: &ExecutionPlan) -> Vec<&RulePath> {
    let mut paths = Vec::new();
    for rule in &plan.rules {
        if !rule.path.segments.is_empty() {
            continue;
        }
        for branch in &rule.branches {
            collect_rule_paths_from_expr(&branch.result, &mut paths);
            if let Some(cond) = &branch.condition {
                collect_rule_paths_from_expr(cond, &mut paths);
            }
        }
    }
    paths
}

fn collect_rule_paths_from_expr<'a>(
    expr: &'a crate::planning::semantics::Expression,
    out: &mut Vec<&'a RulePath>,
) {
    match &expr.kind {
        ExpressionKind::RulePath(rp) => out.push(rp),
        ExpressionKind::LogicalAnd(l, r)
        | ExpressionKind::Arithmetic(l, _, r)
        | ExpressionKind::Comparison(l, _, r) => {
            collect_rule_paths_from_expr(l, out);
            collect_rule_paths_from_expr(r, out);
        }
        ExpressionKind::UnitConversion(inner, _)
        | ExpressionKind::LogicalNegation(inner, _)
        | ExpressionKind::MathematicalComputation(_, inner) => {
            collect_rule_paths_from_expr(inner, out);
        }
        ExpressionKind::DateRelative(_, date_expr, tolerance) => {
            collect_rule_paths_from_expr(date_expr, out);
            if let Some(tol) = tolerance {
                collect_rule_paths_from_expr(tol, out);
            }
        }
        ExpressionKind::DateCalendar(_, _, date_expr) => {
            collect_rule_paths_from_expr(date_expr, out);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::FactPath(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Now => {}
    }
}

/// Validate that all temporal slices of a spec see the same interface
/// from each referenced spec.
pub(crate) fn validate_slice_interfaces(
    spec: &LemmaSpec,
    slice_plans: &[ExecutionPlan],
    resolved_types_per_slice: &[ResolvedTypesMap],
) -> Vec<Error> {
    let spec_name = &spec.name;
    if slice_plans.len() <= 1 {
        return Vec::new();
    }

    let attr = spec.attribute.as_deref().unwrap_or(spec_name.as_str());
    let span = Span {
        start: 0,
        end: 0,
        line: spec.start_line.max(1),
        col: 0,
    };
    let source_loc = Source::new(attr, span);

    let ref_segments = collect_ref_spec_segments(&slice_plans[0]);

    let mut errors = Vec::new();

    for (segments, ref_spec_arc) in &ref_segments {
        let first_interface = SliceInterface::from_plan(
            &slice_plans[0],
            segments,
            &resolved_types_per_slice[0],
            ref_spec_arc,
        );

        for (i, plan) in slice_plans.iter().enumerate().skip(1) {
            let ref_spec_in_slice = find_ref_spec_in_plan(plan, segments);
            let ref_spec = ref_spec_in_slice.as_ref().unwrap_or(ref_spec_arc);
            let slice_interface =
                SliceInterface::from_plan(plan, segments, &resolved_types_per_slice[i], ref_spec);

            if first_interface != slice_interface {
                let diffs = first_interface.diff(&slice_interface);
                let diff_detail = if diffs.is_empty() {
                    String::new()
                } else {
                    format!(": {}", diffs.join(", "))
                };
                errors.push(Error::validation(
                    format!(
                        "Referenced spec '{}' changed its interface between temporal slices of '{}'{}\n\
                         Create a new temporal version of '{}' to handle the interface change.",
                        ref_spec_arc.name, spec_name, diff_detail, spec_name
                    ),
                    Some(source_loc.clone()),
                    None::<String>,
                ));
                break;
            }
        }
    }

    errors
}

/// Find all first-level referenced spec segments, plus nested ones reachable
/// through the plan's facts/rules.
fn collect_ref_spec_segments(plan: &ExecutionPlan) -> Vec<(Vec<PathSegment>, Arc<LemmaSpec>)> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for (path, data) in &plan.facts {
        if let FactData::SpecRef { spec, .. } = data {
            let mut seg = path.segments.clone();
            seg.push(PathSegment {
                fact: path.fact.clone(),
                spec: spec.name.clone(),
            });
            let key = seg
                .iter()
                .map(|s| format!("{}.{}", s.fact, s.spec))
                .collect::<Vec<_>>()
                .join("/");
            if seen.insert(key) {
                result.push((seg, Arc::clone(spec)));
            }
        }
    }

    result
}

/// Find the Arc<LemmaSpec> for a referenced spec in a plan by matching segments.
fn find_ref_spec_in_plan(plan: &ExecutionPlan, segments: &[PathSegment]) -> Option<Arc<LemmaSpec>> {
    if segments.is_empty() {
        return None;
    }
    let parent_segments = &segments[..segments.len() - 1];
    let target_seg = &segments[segments.len() - 1];

    for (path, data) in &plan.facts {
        if let FactData::SpecRef { spec, .. } = data {
            if path.segments == *parent_segments
                && path.fact == target_seg.fact
                && spec.name == target_seg.spec
            {
                return Some(Arc::clone(spec));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::semantics::primitive_number;

    #[test]
    fn identical_interfaces_are_equal() {
        let mut facts = BTreeMap::new();
        facts.insert(
            "x".to_string(),
            FactKind::Value(Box::new(primitive_number().clone())),
        );

        let mut rules = BTreeMap::new();
        rules.insert("z".to_string(), primitive_number().clone());

        let a = SliceInterface {
            facts: facts.clone(),
            rules: rules.clone(),
            types: BTreeMap::new(),
        };
        let b = SliceInterface {
            facts,
            rules,
            types: BTreeMap::new(),
        };
        assert_eq!(a, b);
        assert!(a.diff(&b).is_empty());
    }

    #[test]
    fn added_fact_detected() {
        let a = SliceInterface {
            facts: BTreeMap::new(),
            rules: BTreeMap::new(),
            types: BTreeMap::new(),
        };

        let mut facts_b = BTreeMap::new();
        facts_b.insert(
            "y".to_string(),
            FactKind::Value(Box::new(primitive_number().clone())),
        );
        let b = SliceInterface {
            facts: facts_b,
            rules: BTreeMap::new(),
            types: BTreeMap::new(),
        };

        assert_ne!(a, b);
        let diffs = a.diff(&b);
        assert!(diffs.iter().any(|d| d.contains("'y' added")));
    }

    #[test]
    fn removed_rule_detected() {
        let mut rules_a = BTreeMap::new();
        rules_a.insert("z".to_string(), primitive_number().clone());
        let a = SliceInterface {
            facts: BTreeMap::new(),
            rules: rules_a,
            types: BTreeMap::new(),
        };
        let b = SliceInterface {
            facts: BTreeMap::new(),
            rules: BTreeMap::new(),
            types: BTreeMap::new(),
        };

        assert_ne!(a, b);
        let diffs = a.diff(&b);
        assert!(diffs.iter().any(|d| d.contains("'z' removed")));
    }
}
