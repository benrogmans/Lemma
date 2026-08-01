//! Expression normalization: algebraic simplification for drift-free evaluation.
//!
//! Internal [`NormalForm`] IR used by planning; sealed into [`ExecutionPlan`] tables.

use crate::computation::comparison::comparison_operation;
use crate::computation::rational::{
    rational_is_zero, rational_new, rational_one, rational_operation, rational_zero,
    NumericFailure, NumericOperation, RationalInteger,
};
use crate::computation::UnitResolutionContext;
use crate::parsing::ast::{
    arithmetic_associativity, arithmetic_precedence, operand_needs_parentheses, Associativity,
    CalendarPeriodUnit, DateCalendarKind, DateRelativeKind, OperandSide, PrimitiveKind,
};
use crate::planning::semantics::{
    negated_comparison, primitive_number_arc, ArithmeticComputation, ComparisonComputation,
    DataDefinition, DataPath, Expression, ExpressionKind, LiteralValue, MathematicalComputation,
    ReferenceTarget, RulePath, SemanticConversionTarget, Source, TypeSpecification, ValueKind,
    VetoExpression,
};
use crate::Error;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn normalization_error(source: Option<Source>, failure: NumericFailure, context: &str) -> Error {
    Error::validation(format!("{context}: {failure}"), source, None::<String>)
}

/// Build the literal for an exactly folded plain-number rational.
///
/// Contract: callers must only fold operands admitted by
/// [`as_rational_literal`] (plain numbers). Unit-bearing operands must never
/// reach this function — measure folds must carry their unit signature (see
/// [`typed_product_zero`]).
fn literal_from_folded_rational(
    rational: RationalInteger,
    _source: Option<Source>,
) -> Result<LiteralValue, Error> {
    Ok(LiteralValue::number_with_type(
        rational,
        primitive_number_arc().clone(),
    ))
}

/// Build unless branches as a single expression (`Piecewise` or lone result).
///
/// Piecewise arms are stored in source order: default first, then each `unless` clause.
/// Compilation walks unless arms in reverse and returns on the first true condition
/// (last match in source order wins).
pub(crate) fn unless_branches_to_piecewise(
    branches: &[(Option<Expression>, Expression)],
) -> Expression {
    assert!(
        !branches.is_empty(),
        "BUG: rule must have at least one branch"
    );
    if branches.len() == 1 {
        return branches[0].1.clone();
    }

    let (_, default_result) = &branches[0];
    let source = default_result.source_location.clone();
    let mut arms: Vec<(Arc<Expression>, Arc<Expression>)> = Vec::with_capacity(branches.len());
    arms.push((
        Arc::new(literal_bool_expression(true, source.clone())),
        Arc::new(default_result.clone()),
    ));
    for (condition, result) in branches.iter().skip(1) {
        let unless_condition = condition
            .as_ref()
            .expect("BUG: non-default branch missing condition");
        arms.push((Arc::new(unless_condition.clone()), Arc::new(result.clone())));
    }

    Expression::with_source(ExpressionKind::Piecewise(arms), source)
}

/// One rule after unless→Piecewise, lower into the shared NormalForm graph, normalize.
pub(crate) struct NormalizedRule {
    pub(crate) body: NormalFormId,
}

/// Plan-level inputs to per-rule normalization that do not change across the
/// execution order. Per-rule inputs (branches, source) stay explicit.
pub(crate) struct NormalizeContext<'a> {
    pub(crate) data: &'a IndexMap<DataPath, DataDefinition>,
    pub(crate) unit_ctx: &'a UnitResolutionContext<'a>,
    pub(crate) max_normalized_expression_nodes: usize,
    pub(crate) max_normal_form_depth: usize,
}

/// Completed rule roots and rule-target data paths for Expression→NormalForm lower.
struct LowerCtx<'a> {
    completed_rules: &'a HashMap<RulePath, NormalFormId>,
    rule_target_data: &'a HashMap<DataPath, RulePath>,
}

pub(crate) fn build_normalized_rule(
    ctx: &NormalizeContext<'_>,
    completed_rules: &HashMap<RulePath, NormalFormId>,
    branches: &[(Option<Expression>, Expression)],
    source: Option<Source>,
    interner: &mut NormalFormInterner,
) -> Result<NormalizedRule, Error> {
    let data = ctx.data;
    let unit_ctx = ctx.unit_ctx;
    let max_normalized_expression_nodes = ctx.max_normalized_expression_nodes;
    let piecewise = unless_branches_to_piecewise(branches);
    let rule_target_data = build_in_plan_rule_target_data_map(data);
    let lower = LowerCtx {
        completed_rules,
        rule_target_data: &rule_target_data,
    };

    let root = to_normal_form(&piecewise, interner, &lower);
    let nf = normalize_once(root, interner, Some(unit_ctx), source.clone())?;

    if normal_form_exceeds_node_budget(interner, nf, max_normalized_expression_nodes) {
        return Err(expression_node_limit_error(
            max_normalized_expression_nodes,
            source,
        ));
    }

    let depth = normal_form_depth(interner, nf);
    if depth > ctx.max_normal_form_depth {
        return Err(Error::resource_limit_exceeded(
            "max_normal_form_depth",
            format!("{} levels", ctx.max_normal_form_depth),
            format!("{depth} levels in the normalized graph"),
            "Restructure the rule or reduce repeated references to other rules",
            source,
            None,
            None,
        ));
    }

    Ok(NormalizedRule { body: nf })
}

fn expression_node_limit_error(limit: usize, source: Option<Source>) -> Error {
    Error::resource_limit_exceeded(
        "max_normalized_expression_nodes",
        format!("{limit} expression nodes"),
        format!("more than {limit} unique normal-form cells reachable from the rule root"),
        "Restructure the rule or reduce repeated references to other rules",
        source,
        None,
        None,
    )
}

/// Whether the normal form, counted as unique DAG cells reachable from `root`,
/// exceeds the node budget (IR size = distinct cells, not tree expansion).
fn normal_form_exceeds_node_budget(
    interner: &NormalFormInterner,
    root: NormalFormId,
    budget: usize,
) -> bool {
    let mut visited = HashSet::new();
    let mut worklist = vec![root];
    while let Some(current) = worklist.pop() {
        if !visited.insert(current) {
            continue;
        }
        if visited.len() > budget {
            return true;
        }
        match &interner.get(current).kind {
            NormalFormKind::Leaf(_) | NormalFormKind::Veto(_) | NormalFormKind::Now => {}
            NormalFormKind::Sum(children)
            | NormalFormKind::Product(children)
            | NormalFormKind::And(children) => {
                worklist.extend(children.iter().copied());
            }
            NormalFormKind::Subtract(a, b)
            | NormalFormKind::Divide(a, b)
            | NormalFormKind::Power(a, b)
            | NormalFormKind::Modulo(a, b)
            | NormalFormKind::Comparison(a, _, b)
            | NormalFormKind::RangeLiteral(a, b)
            | NormalFormKind::RangeContainment(a, b) => {
                worklist.push(*a);
                worklist.push(*b);
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
                worklist.push(*x);
            }
            NormalFormKind::Piecewise(arms) => {
                for (condition, result) in arms.iter() {
                    worklist.push(*condition);
                    worklist.push(*result);
                }
            }
        }
        if let Some(origin) = interner.get(current).origin {
            worklist.push(origin);
        }
    }
    false
}

/// Maximum nesting depth of a NormalForm DAG (leaves have depth 1). Used by
/// planning to guarantee a recursive evaluator can never overflow the stack.
pub(crate) fn normal_form_depth(interner: &NormalFormInterner, root: NormalFormId) -> usize {
    fn child_depth(
        interner: &NormalFormInterner,
        id: NormalFormId,
        memo: &mut HashMap<NormalFormId, usize>,
    ) -> usize {
        if let Some(depth) = memo.get(&id) {
            return *depth;
        }
        let depth = match &interner.get(id).kind {
            NormalFormKind::Leaf(_) | NormalFormKind::Veto(_) | NormalFormKind::Now => 1,
            NormalFormKind::Sum(children)
            | NormalFormKind::Product(children)
            | NormalFormKind::And(children) => {
                1 + children
                    .iter()
                    .map(|child| child_depth(interner, *child, memo))
                    .max()
                    .unwrap_or(0)
            }
            NormalFormKind::Subtract(a, b)
            | NormalFormKind::Divide(a, b)
            | NormalFormKind::Power(a, b)
            | NormalFormKind::Modulo(a, b)
            | NormalFormKind::Comparison(a, _, b)
            | NormalFormKind::RangeLiteral(a, b)
            | NormalFormKind::RangeContainment(a, b) => {
                1 + child_depth(interner, *a, memo).max(child_depth(interner, *b, memo))
            }
            NormalFormKind::Negate(x)
            | NormalFormKind::Reciprocal(x)
            | NormalFormKind::Not(x)
            | NormalFormKind::MathOp(_, x)
            | NormalFormKind::UnitConversion(x, _)
            | NormalFormKind::DateRelative(_, x)
            | NormalFormKind::DateCalendar(_, _, x)
            | NormalFormKind::PastFutureRange(_, x)
            | NormalFormKind::ResultIsVeto(x) => 1 + child_depth(interner, *x, memo),
            NormalFormKind::Piecewise(arms) => {
                1 + arms
                    .iter()
                    .map(|(c, r)| {
                        child_depth(interner, *c, memo).max(child_depth(interner, *r, memo))
                    })
                    .max()
                    .unwrap_or(0)
            }
        };
        let depth = if let Some(origin) = interner.get(id).origin {
            depth.max(1 + child_depth(interner, origin, memo))
        } else {
            depth
        };
        memo.insert(id, depth);
        depth
    }
    let mut memo = HashMap::new();
    child_depth(interner, root, &mut memo)
}

pub(crate) fn follow_data_reference_to_rule_target(
    data: &IndexMap<DataPath, DataDefinition>,
    start: &DataPath,
) -> Option<RulePath> {
    let mut visited: HashSet<DataPath> = HashSet::new();
    let mut cursor = start.clone();
    loop {
        if !visited.insert(cursor.clone()) {
            return None;
        }
        let Some(DataDefinition::Reference { target, .. }) = data.get(&cursor) else {
            return None;
        };
        match target {
            ReferenceTarget::Data(next) => cursor = next.clone(),
            ReferenceTarget::Rule(rule_path) => return Some(rule_path.clone()),
        }
    }
}

fn build_in_plan_rule_target_data_map(
    data: &IndexMap<DataPath, DataDefinition>,
) -> HashMap<DataPath, RulePath> {
    let mut out: HashMap<DataPath, RulePath> = HashMap::new();
    for (path, definition) in data {
        if !matches!(definition, DataDefinition::Reference { .. }) {
            continue;
        }
        if let Some(rule_path) = follow_data_reference_to_rule_target(data, path) {
            out.insert(path.clone(), rule_path);
        }
    }
    out
}

fn literal_bool_expression(value: bool, source: Option<Source>) -> Expression {
    Expression::with_source(
        ExpressionKind::Literal(Box::new(LiteralValue::from_bool(value))),
        source,
    )
}

/// Leaves in the shipped equation DAG: literals and data only.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LeafKind {
    Literal(Arc<LiteralValue>),
    DataPath(crate::planning::semantics::DataPath),
}

/// Index into a [`NormalFormInterner`] / shipped [`crate::planning::execution_plan::ExecutionPlan::normal_forms`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NormalFormId(u32);

impl NormalFormId {
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// Algebraic structure only. Children are [`NormalFormId`]. Hash/Eq drive cons —
/// no source spans or rule-embed identity here (those live on [`NormalForm`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NormalFormKind {
    Leaf(LeafKind),
    Sum(Vec<NormalFormId>),
    Product(Vec<NormalFormId>),
    Subtract(NormalFormId, NormalFormId),
    Divide(NormalFormId, NormalFormId),
    Power(NormalFormId, NormalFormId),
    Modulo(NormalFormId, NormalFormId),
    Negate(NormalFormId),
    Reciprocal(NormalFormId),
    Comparison(NormalFormId, ComparisonComputation, NormalFormId),
    And(Vec<NormalFormId>),
    Not(NormalFormId),
    MathOp(MathematicalComputation, NormalFormId),
    UnitConversion(NormalFormId, SemanticConversionTarget),
    Veto(VetoExpression),
    DateRelative(DateRelativeKind, NormalFormId),
    DateCalendar(DateCalendarKind, CalendarPeriodUnit, NormalFormId),
    RangeLiteral(NormalFormId, NormalFormId),
    PastFutureRange(DateRelativeKind, NormalFormId),
    RangeContainment(NormalFormId, NormalFormId),
    ResultIsVeto(NormalFormId),
    Now,
    Piecewise(Vec<(NormalFormId, NormalFormId)>),
}

/// One node in THE DAG: algebraic Kind, optional conversion span, optional fold
/// origin, optional rule use-site identity.
///
/// A rule use-site shares the completed body's Kind (same child ids) and sets
/// `rule_embed`. That keeps algebra visible to normalize while naming the embed
/// for explanation. Cons keys on `(kind, rule_embed)` so an embed never collapses
/// into the bare body cell.
#[derive(Clone, Debug)]
pub(crate) struct NormalForm {
    pub(crate) kind: NormalFormKind,
    /// Span for unit-conversion nodes; does not affect sharing.
    pub(crate) source: Option<Source>,
    /// Pre-image destroyed by a fold; blocks sharing when present.
    pub(crate) origin: Option<NormalFormId>,
    /// Use-site of a named rule (`None` on ordinary cells).
    pub(crate) rule_embed: Option<RulePath>,
}

/// Cons key: algebraic Kind plus optional rule-embed identity.
type ConsKey = (NormalFormKind, Option<RulePath>);

/// Build-local table + origin-free cons. Ship via [`Self::into_reachable`].
pub(crate) struct NormalFormInterner {
    forms: Vec<NormalForm>,
    cons: HashMap<ConsKey, NormalFormId>,
}

impl NormalFormInterner {
    pub(crate) fn new() -> Self {
        Self {
            forms: Vec::new(),
            cons: HashMap::new(),
        }
    }

    pub(crate) fn intern(
        &mut self,
        kind: NormalFormKind,
        source: Option<Source>,
        origin: Option<NormalFormId>,
        rule_embed: Option<RulePath>,
    ) -> NormalFormId {
        let key = (kind, rule_embed);
        if origin.is_none() {
            if let Some(id) = self.cons.get(&key) {
                return *id;
            }
        }
        let id = NormalFormId(
            u32::try_from(self.forms.len()).expect("BUG: normal_forms table exceeds u32::MAX"),
        );
        if origin.is_none() {
            self.cons.insert(key.clone(), id);
        }
        self.forms.push(NormalForm {
            kind: key.0,
            source,
            origin,
            rule_embed: key.1,
        });
        id
    }

    pub(crate) fn get(&self, id: NormalFormId) -> &NormalForm {
        self.forms.get(id.index()).unwrap_or_else(|| {
            panic!(
                "BUG: NormalFormId {} out of range (table len {})",
                id.0,
                self.forms.len()
            )
        })
    }

    /// Ship only cells reachable from `roots`. Dense-remap ids in ascending
    /// old-index order. Walks shape children and origin. Remaps both.
    pub(crate) fn into_reachable(
        self,
        roots: &[NormalFormId],
    ) -> (Vec<NormalForm>, Vec<NormalFormId>) {
        let mut reachable: HashSet<u32> = HashSet::new();
        let mut worklist: Vec<NormalFormId> = roots.to_vec();
        while let Some(id) = worklist.pop() {
            if id.index() >= self.forms.len() {
                panic!(
                    "BUG: NormalFormId {} out of range during reachable extract (table len {})",
                    id.0,
                    self.forms.len()
                );
            }
            if !reachable.insert(id.0) {
                continue;
            }
            let cell = &self.forms[id.index()];
            push_child_ids(&cell.kind, &mut worklist);
            if let Some(origin) = cell.origin {
                worklist.push(origin);
            }
        }
        let mut old_ids: Vec<u32> = reachable.into_iter().collect();
        old_ids.sort_unstable();
        let mut remap: HashMap<u32, NormalFormId> = HashMap::with_capacity(old_ids.len());
        for (new_index, old) in old_ids.iter().copied().enumerate() {
            remap.insert(
                old,
                NormalFormId(
                    u32::try_from(new_index).expect("BUG: normal_forms table exceeds u32::MAX"),
                ),
            );
        }
        let mut table = Vec::with_capacity(old_ids.len());
        for old in &old_ids {
            let cell = &self.forms[*old as usize];
            table.push(NormalForm {
                kind: remap_kind(&cell.kind, &remap),
                source: cell.source.clone(),
                origin: cell.origin.map(|o| {
                    *remap.get(&o.0).unwrap_or_else(|| {
                        panic!("BUG: origin NormalFormId {} missing from remap", o.0)
                    })
                }),
                rule_embed: cell.rule_embed.clone(),
            });
        }
        let remapped_roots = roots
            .iter()
            .map(|r| {
                *remap.get(&r.0).unwrap_or_else(|| {
                    panic!("BUG: root NormalFormId {} not in reachable set", r.0)
                })
            })
            .collect();
        (table, remapped_roots)
    }
}

fn push_child_ids(kind: &NormalFormKind, worklist: &mut Vec<NormalFormId>) {
    match kind {
        NormalFormKind::Leaf(_) | NormalFormKind::Veto(_) | NormalFormKind::Now => {}
        NormalFormKind::Sum(children)
        | NormalFormKind::Product(children)
        | NormalFormKind::And(children) => worklist.extend(children.iter().copied()),
        NormalFormKind::Subtract(a, b)
        | NormalFormKind::Divide(a, b)
        | NormalFormKind::Power(a, b)
        | NormalFormKind::Modulo(a, b)
        | NormalFormKind::Comparison(a, _, b)
        | NormalFormKind::RangeLiteral(a, b)
        | NormalFormKind::RangeContainment(a, b) => {
            worklist.push(*a);
            worklist.push(*b);
        }
        NormalFormKind::Negate(x)
        | NormalFormKind::Reciprocal(x)
        | NormalFormKind::Not(x)
        | NormalFormKind::MathOp(_, x)
        | NormalFormKind::UnitConversion(x, _)
        | NormalFormKind::DateRelative(_, x)
        | NormalFormKind::DateCalendar(_, _, x)
        | NormalFormKind::PastFutureRange(_, x)
        | NormalFormKind::ResultIsVeto(x) => worklist.push(*x),
        NormalFormKind::Piecewise(arms) => {
            for (c, r) in arms {
                worklist.push(*c);
                worklist.push(*r);
            }
        }
    }
}

fn remap_kind(kind: &NormalFormKind, remap: &HashMap<u32, NormalFormId>) -> NormalFormKind {
    let map = |id: NormalFormId| {
        *remap
            .get(&id.0)
            .unwrap_or_else(|| panic!("BUG: child NormalFormId {} missing from remap", id.0))
    };
    match kind {
        NormalFormKind::Leaf(leaf) => NormalFormKind::Leaf(leaf.clone()),
        NormalFormKind::Sum(children) => {
            NormalFormKind::Sum(children.iter().copied().map(map).collect())
        }
        NormalFormKind::Product(children) => {
            NormalFormKind::Product(children.iter().copied().map(map).collect())
        }
        NormalFormKind::And(children) => {
            NormalFormKind::And(children.iter().copied().map(map).collect())
        }
        NormalFormKind::Subtract(a, b) => NormalFormKind::Subtract(map(*a), map(*b)),
        NormalFormKind::Divide(a, b) => NormalFormKind::Divide(map(*a), map(*b)),
        NormalFormKind::Power(a, b) => NormalFormKind::Power(map(*a), map(*b)),
        NormalFormKind::Modulo(a, b) => NormalFormKind::Modulo(map(*a), map(*b)),
        NormalFormKind::Comparison(a, op, b) => {
            NormalFormKind::Comparison(map(*a), op.clone(), map(*b))
        }
        NormalFormKind::RangeLiteral(a, b) => NormalFormKind::RangeLiteral(map(*a), map(*b)),
        NormalFormKind::RangeContainment(a, b) => {
            NormalFormKind::RangeContainment(map(*a), map(*b))
        }
        NormalFormKind::Negate(x) => NormalFormKind::Negate(map(*x)),
        NormalFormKind::Reciprocal(x) => NormalFormKind::Reciprocal(map(*x)),
        NormalFormKind::Not(x) => NormalFormKind::Not(map(*x)),
        NormalFormKind::MathOp(op, x) => NormalFormKind::MathOp(op.clone(), map(*x)),
        NormalFormKind::UnitConversion(x, target) => {
            NormalFormKind::UnitConversion(map(*x), target.clone())
        }
        NormalFormKind::Veto(v) => NormalFormKind::Veto(v.clone()),
        NormalFormKind::DateRelative(kind, x) => NormalFormKind::DateRelative(*kind, map(*x)),
        NormalFormKind::DateCalendar(kind, unit, x) => {
            NormalFormKind::DateCalendar(*kind, *unit, map(*x))
        }
        NormalFormKind::PastFutureRange(kind, x) => NormalFormKind::PastFutureRange(*kind, map(*x)),
        NormalFormKind::ResultIsVeto(x) => NormalFormKind::ResultIsVeto(map(*x)),
        NormalFormKind::Now => NormalFormKind::Now,
        NormalFormKind::Piecewise(arms) => {
            NormalFormKind::Piecewise(arms.iter().map(|(c, r)| (map(*c), map(*r))).collect())
        }
    }
}

fn intern_empty(interner: &mut NormalFormInterner, kind: NormalFormKind) -> NormalFormId {
    interner.intern(kind, None, None, None)
}

/// Use-site of a named rule: share the completed body's Kind (child ids), set rule_embed.
fn intern_rule_embed(
    interner: &mut NormalFormInterner,
    body: NormalFormId,
    rule_path: RulePath,
) -> NormalFormId {
    let kind = interner.get(body).kind.clone();
    interner.intern(kind, None, None, Some(rule_path))
}

fn rebuild(
    interner: &mut NormalFormInterner,
    kind: NormalFormKind,
    source: Option<Source>,
    origin: Option<NormalFormId>,
) -> NormalFormId {
    interner.intern(kind, source, origin, None)
}

fn rebuild_nary(
    interner: &mut NormalFormInterner,
    source: Option<Source>,
    origin: Option<NormalFormId>,
    children: Vec<NormalFormId>,
    wrap: fn(Vec<NormalFormId>) -> NormalFormKind,
) -> NormalFormId {
    interner.intern(wrap(children), source, origin, None)
}

fn node_source_origin(
    interner: &NormalFormInterner,
    id: NormalFormId,
) -> (Option<Source>, Option<NormalFormId>) {
    let cell = interner.get(id);
    (cell.source.clone(), cell.origin)
}

fn fold_into(
    interner: &mut NormalFormInterner,
    kind: NormalFormKind,
    destroyed_id: NormalFormId,
) -> NormalFormId {
    interner.intern(kind, None, Some(destroyed_id), None)
}

fn fold_into_survivor(
    interner: &mut NormalFormInterner,
    survivor_id: NormalFormId,
    destroyed_id: NormalFormId,
) -> NormalFormId {
    let survivor = interner.get(survivor_id);
    // Identity-elim unwrap onto a node that already records a fold must not
    // erase that fold (e.g. `(exp(log(5))) + 0` → keep exp/log origin).
    // Piecewise collapse always replaces — recorded arms are the contract.
    if survivor.origin.is_some()
        && !matches!(
            interner.get(destroyed_id).kind,
            NormalFormKind::Piecewise(_)
        )
    {
        return survivor_id;
    }
    interner.intern(
        survivor.kind.clone(),
        survivor.source.clone(),
        Some(destroyed_id),
        survivor.rule_embed.clone(),
    )
}

/// Embed cells expose Kind to *parents* for algebra. Rewrite passes must not
/// enter an embed as a root: `rebuild` would strip `rule_embed` and re-walk the
/// completed body (Θ(2ⁿ) on `r and not r` chains).
fn return_if_rule_embed(id: NormalFormId, interner: &NormalFormInterner) -> Option<NormalFormId> {
    if interner.get(id).rule_embed.is_some() {
        Some(id)
    } else {
        None
    }
}

fn normalize_once(
    id: NormalFormId,
    interner: &mut NormalFormInterner,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalFormId, Error> {
    // Body already normalized when that rule completed. Re-entering through a
    // use-site doubles work on chains like `r and not r` (Θ(2^n) call trees).
    if interner.get(id).rule_embed.is_some() {
        return Ok(id);
    }
    let mut wrappers = Vec::new();
    let mut current = id;
    while let NormalFormKind::UnitConversion(inner, target) = &interner.get(current).kind.clone() {
        let (provenance, origin) = node_source_origin(interner, current);
        wrappers.push((provenance, origin, target.clone()));
        current = *inner;
    }
    let mut normalized = {
        let nf = normalize_children(current, interner, unit_ctx, source.clone())?;
        simplify(nf, interner, unit_ctx, source)?
    };
    while let Some((provenance, origin, target)) = wrappers.pop() {
        if unit_ctx.is_some() {
            if let NormalFormKind::Leaf(LeafKind::Literal(literal)) =
                interner.get(normalized).kind.clone()
            {
                if let (
                    ValueKind::Number(number),
                    SemanticConversionTarget::Type(PrimitiveKind::Number),
                ) = (&literal.value, &target)
                {
                    let destroyed = rebuild(
                        interner,
                        NormalFormKind::UnitConversion(normalized, target.clone()),
                        provenance.clone(),
                        origin,
                    );
                    normalized = fold_into(
                        interner,
                        NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                            LiteralValue::number_with_type(
                                number.clone(),
                                primitive_number_arc().clone(),
                            ),
                        ))),
                        destroyed,
                    );
                    continue;
                }
            }
        }
        normalized = rebuild(
            interner,
            NormalFormKind::UnitConversion(normalized, target),
            provenance,
            origin,
        );
    }
    Ok(normalized)
}

fn normalize_children(
    id: NormalFormId,
    interner: &mut NormalFormInterner,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalFormId, Error> {
    let (carried, origin) = node_source_origin(interner, id);
    let kind = interner.get(id).kind.clone();
    let mut normalize_vec = |children: Vec<NormalFormId>| -> Result<Vec<NormalFormId>, Error> {
        children
            .into_iter()
            .map(|child| normalize_once(child, interner, unit_ctx, source.clone()))
            .collect()
    };
    match kind {
        NormalFormKind::Sum(children) => {
            let children = normalize_vec(children)?;
            Ok(rebuild(
                interner,
                NormalFormKind::Sum(children),
                carried,
                origin,
            ))
        }
        NormalFormKind::Product(children) => {
            let children = normalize_vec(children)?;
            Ok(rebuild(
                interner,
                NormalFormKind::Product(children),
                carried,
                origin,
            ))
        }
        NormalFormKind::Subtract(a, b) => {
            let a = normalize_once(a, interner, unit_ctx, source.clone())?;
            let b = normalize_once(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Subtract(a, b),
                carried,
                origin,
            ))
        }
        NormalFormKind::Divide(a, b) => {
            let a = normalize_once(a, interner, unit_ctx, source.clone())?;
            let b = normalize_once(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Divide(a, b),
                carried,
                origin,
            ))
        }
        NormalFormKind::Power(a, b) => {
            let a = normalize_once(a, interner, unit_ctx, source.clone())?;
            let b = normalize_once(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Power(a, b),
                carried,
                origin,
            ))
        }
        NormalFormKind::Modulo(a, b) => {
            let a = normalize_once(a, interner, unit_ctx, source.clone())?;
            let b = normalize_once(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Modulo(a, b),
                carried,
                origin,
            ))
        }
        NormalFormKind::Negate(x) => {
            let x = normalize_once(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Negate(x),
                carried,
                origin,
            ))
        }
        NormalFormKind::Reciprocal(x) => {
            let x = normalize_once(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Reciprocal(x),
                carried,
                origin,
            ))
        }
        NormalFormKind::Comparison(a, op, b) => {
            let a = normalize_once(a, interner, unit_ctx, source.clone())?;
            let b = normalize_once(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Comparison(a, op, b),
                carried,
                origin,
            ))
        }
        NormalFormKind::And(children) => {
            let children = normalize_vec(children)?;
            Ok(rebuild(
                interner,
                NormalFormKind::And(children),
                carried,
                origin,
            ))
        }
        NormalFormKind::Not(x) => {
            let x = normalize_once(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(interner, NormalFormKind::Not(x), carried, origin))
        }
        NormalFormKind::MathOp(op, x) => {
            let x = normalize_once(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::MathOp(op, x),
                carried,
                origin,
            ))
        }
        NormalFormKind::UnitConversion(_, _) => {
            unreachable!("BUG: UnitConversion peeled in normalize_once before normalize_children")
        }
        NormalFormKind::DateRelative(kind, x) => {
            let x = normalize_once(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::DateRelative(kind, x),
                carried,
                origin,
            ))
        }
        NormalFormKind::DateCalendar(kind, unit, x) => {
            let x = normalize_once(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::DateCalendar(kind, unit, x),
                carried,
                origin,
            ))
        }
        NormalFormKind::RangeLiteral(a, b) => {
            let a = normalize_once(a, interner, unit_ctx, source.clone())?;
            let b = normalize_once(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::RangeLiteral(a, b),
                carried,
                origin,
            ))
        }
        NormalFormKind::PastFutureRange(kind, x) => {
            let x = normalize_once(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::PastFutureRange(kind, x),
                carried,
                origin,
            ))
        }
        NormalFormKind::RangeContainment(a, b) => {
            let a = normalize_once(a, interner, unit_ctx, source.clone())?;
            let b = normalize_once(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::RangeContainment(a, b),
                carried,
                origin,
            ))
        }
        NormalFormKind::ResultIsVeto(operand) => {
            let operand = normalize_once(operand, interner, unit_ctx, source)?;
            Ok(rebuild(
                interner,
                NormalFormKind::ResultIsVeto(operand),
                carried,
                origin,
            ))
        }
        NormalFormKind::Piecewise(arms) => {
            let mut normalized_arms = Vec::with_capacity(arms.len());
            for (condition, result) in arms {
                let c = normalize_once(condition, interner, unit_ctx, source.clone())?;
                let r = normalize_once(result, interner, unit_ctx, source.clone())?;
                normalized_arms.push((c, r));
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Piecewise(normalized_arms),
                carried,
                origin,
            ))
        }
        leaf @ (NormalFormKind::Leaf(_) | NormalFormKind::Veto(_) | NormalFormKind::Now) => {
            Ok(rebuild(interner, leaf, carried, origin))
        }
    }
}

fn simplify(
    id: NormalFormId,
    interner: &mut NormalFormInterner,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalFormId, Error> {
    let id = expand_numeric_subtract_divide(id, interner);
    let id = flatten_associative(id, interner);
    let id = power_laws(id, interner);
    let id = eliminate_identities(id, interner);
    let id = double_negate_reciprocal(id, interner);
    let id = math_identities(id, interner);
    let id = constant_fold(id, interner, source.clone())?;
    let id = fold_unit_literals(id, interner, unit_ctx, source.clone())?;
    let id = demorgan(id, interner);
    let id = logical_flatten(id, interner);
    let id = logical_short_circuit(id, interner);
    let id = logical_idempotency(id, interner);
    let id = collapse_piecewise(id, interner);
    let id = negated_comparisons(id, interner);
    Ok(canonical_order(id, interner))
}

fn fold_unit_literals(
    id: NormalFormId,
    interner: &mut NormalFormInterner,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalFormId, Error> {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return Ok(id);
    }
    let (provenance, origin) = node_source_origin(interner, id);
    let kind = interner.get(id).kind.clone();
    let mut fold_vec = |children: Vec<NormalFormId>| -> Result<Vec<NormalFormId>, Error> {
        children
            .into_iter()
            .map(|child| fold_unit_literals(child, interner, unit_ctx, source.clone()))
            .collect()
    };
    match kind {
        NormalFormKind::Sum(children) => {
            let children = fold_vec(children)?;
            Ok(rebuild_nary(
                interner,
                provenance,
                origin,
                children,
                NormalFormKind::Sum,
            ))
        }
        NormalFormKind::Product(children) => {
            let children = fold_vec(children)?;
            Ok(rebuild_nary(
                interner,
                provenance,
                origin,
                children,
                NormalFormKind::Product,
            ))
        }
        NormalFormKind::Subtract(a, b) => {
            let a_done = fold_unit_literals(a, interner, unit_ctx, source.clone())?;
            let b_done = fold_unit_literals(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Subtract(a_done, b_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Divide(a, b) => {
            let a_done = fold_unit_literals(a, interner, unit_ctx, source.clone())?;
            let b_done = fold_unit_literals(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Divide(a_done, b_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Power(a, b) => {
            let a_done = fold_unit_literals(a, interner, unit_ctx, source.clone())?;
            let b_done = fold_unit_literals(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Power(a_done, b_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Modulo(a, b) => {
            let a_done = fold_unit_literals(a, interner, unit_ctx, source.clone())?;
            let b_done = fold_unit_literals(b, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Modulo(a_done, b_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Negate(x) => {
            let x_done = fold_unit_literals(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Negate(x_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Reciprocal(x) => {
            let x_done = fold_unit_literals(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Reciprocal(x_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Comparison(a, op, b) => {
            let a_done = fold_unit_literals(a, interner, unit_ctx, source.clone())?;
            let b_done = fold_unit_literals(b, interner, unit_ctx, source)?;
            Ok(rebuild(
                interner,
                NormalFormKind::Comparison(a_done, op, b_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::And(children) => {
            let children = fold_vec(children)?;
            Ok(rebuild_nary(
                interner,
                provenance,
                origin,
                children,
                NormalFormKind::And,
            ))
        }
        NormalFormKind::Not(x) => {
            let x_done = fold_unit_literals(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::Not(x_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::MathOp(op, x) => {
            let x_done = fold_unit_literals(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::MathOp(op, x_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::UnitConversion(inner, target) => {
            let inner_done = fold_unit_literals(inner, interner, unit_ctx, source.clone())?;
            if let (Some(_unit_context), NormalFormKind::Leaf(LeafKind::Literal(literal))) =
                (unit_ctx, &interner.get(inner_done).kind)
            {
                if let (
                    ValueKind::Number(number),
                    SemanticConversionTarget::Type(PrimitiveKind::Number),
                ) = (&literal.value, &target)
                {
                    return Ok(fold_into(
                        interner,
                        NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                            LiteralValue::number_with_type(
                                number.clone(),
                                primitive_number_arc().clone(),
                            ),
                        ))),
                        id,
                    ));
                }
            }
            Ok(rebuild(
                interner,
                NormalFormKind::UnitConversion(inner_done, target),
                provenance,
                origin,
            ))
        }
        NormalFormKind::DateRelative(kind, x) => {
            let x_done = fold_unit_literals(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::DateRelative(kind, x_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::DateCalendar(kind, unit, x) => {
            let x_done = fold_unit_literals(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::DateCalendar(kind, unit, x_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::RangeLiteral(a, b) => {
            let a_done = fold_unit_literals(a, interner, unit_ctx, source.clone())?;
            let b_done = fold_unit_literals(b, interner, unit_ctx, source)?;
            Ok(rebuild(
                interner,
                NormalFormKind::RangeLiteral(a_done, b_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::PastFutureRange(kind, x) => {
            let x_done = fold_unit_literals(x, interner, unit_ctx, source.clone())?;
            Ok(rebuild(
                interner,
                NormalFormKind::PastFutureRange(kind, x_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::RangeContainment(a, b) => {
            let a_done = fold_unit_literals(a, interner, unit_ctx, source.clone())?;
            let b_done = fold_unit_literals(b, interner, unit_ctx, source)?;
            Ok(rebuild(
                interner,
                NormalFormKind::RangeContainment(a_done, b_done),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Piecewise(arms) => {
            let mut folded_arms = Vec::with_capacity(arms.len());
            for (condition, result) in arms {
                folded_arms.push((
                    fold_unit_literals(condition, interner, unit_ctx, source.clone())?,
                    fold_unit_literals(result, interner, unit_ctx, source.clone())?,
                ));
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Piecewise(folded_arms),
                provenance.clone(),
                origin,
            ))
        }
        NormalFormKind::Leaf(LeafKind::Literal(literal)) => {
            if let Some(expanded) = expand_named_measure_literal(literal.as_ref(), unit_ctx) {
                Ok(fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(expanded))),
                    id,
                ))
            } else {
                Ok(rebuild(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(literal)),
                    provenance,
                    origin,
                ))
            }
        }
        leaf @ (NormalFormKind::Leaf(_)
        | NormalFormKind::Veto(_)
        | NormalFormKind::ResultIsVeto(_)
        | NormalFormKind::Now) => Ok(rebuild(interner, leaf, provenance, origin)),
    }
}

pub(crate) fn expand_named_measure_literal(
    literal: &LiteralValue,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
) -> Option<LiteralValue> {
    let UnitResolutionContext::WithIndex(unit_index) = unit_ctx? else {
        return None;
    };
    let ValueKind::Measure(magnitude, signature) = &literal.value else {
        return None;
    };
    if signature.len() != 1 || signature[0].1 != 1 {
        return None;
    }
    let unit_name = &signature[0].0;
    let owning_type = unit_index.get(unit_name)?;
    let TypeSpecification::Measure { units, .. } = &owning_type.specifications else {
        return None;
    };
    let unit = units.get(unit_name).ok()?;
    if unit.derived_measure_factors.is_empty() {
        return None;
    }
    let expanded =
        crate::computation::arithmetic::expand_signature_to_base_units(signature, unit_index);
    Some(LiteralValue::measure_with_signature(
        magnitude.clone(),
        expanded,
        Arc::clone(&literal.lemma_type),
    ))
}

fn to_normal_form(
    expr: &Expression,
    interner: &mut NormalFormInterner,
    lower: &LowerCtx<'_>,
) -> NormalFormId {
    to_normal_form_node(expr, interner, lower)
}

fn to_normal_form_node(
    expr: &Expression,
    interner: &mut NormalFormInterner,
    lower: &LowerCtx<'_>,
) -> NormalFormId {
    match &expr.kind {
        ExpressionKind::Literal(lit) => intern_empty(
            interner,
            NormalFormKind::Leaf(LeafKind::Literal(Arc::new((**lit).clone()))),
        ),
        ExpressionKind::DataPath(p) => {
            if let Some(rule_path) = lower.rule_target_data.get(p) {
                let body = *lower.completed_rules.get(rule_path).unwrap_or_else(|| {
                    panic!(
                        "BUG: rule-target data '{}' maps to rule '{}' with no completed NormalFormId",
                        p, rule_path.rule
                    )
                });
                intern_rule_embed(interner, body, rule_path.clone())
            } else {
                intern_empty(
                    interner,
                    NormalFormKind::Leaf(LeafKind::DataPath(p.clone())),
                )
            }
        }
        ExpressionKind::RulePath(path) => {
            let body = *lower.completed_rules.get(path).unwrap_or_else(|| {
                panic!(
                    "BUG: in-plan rule reference '{}' has no completed NormalFormId (topo order broken)",
                    path.rule
                )
            });
            intern_rule_embed(interner, body, path.clone())
        }
        ExpressionKind::LogicalAnd(left, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(interner, NormalFormKind::And(vec![left_id, right_id]))
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Subtract, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(interner, NormalFormKind::Subtract(left_id, right_id))
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Divide, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(interner, NormalFormKind::Divide(left_id, right_id))
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Add, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(interner, NormalFormKind::Sum(vec![left_id, right_id]))
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Multiply, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(interner, NormalFormKind::Product(vec![left_id, right_id]))
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Power, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(interner, NormalFormKind::Power(left_id, right_id))
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Modulo, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(interner, NormalFormKind::Modulo(left_id, right_id))
        }
        ExpressionKind::Comparison(left, op, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(
                interner,
                NormalFormKind::Comparison(left_id, op.clone(), right_id),
            )
        }
        ExpressionKind::UnitConversion(inner, target) => {
            let inner_id = to_normal_form(inner, interner, lower);
            interner.intern(
                NormalFormKind::UnitConversion(inner_id, target.clone()),
                expr.source_location.clone(),
                None,
                None,
            )
        }
        ExpressionKind::LogicalNegation(inner, _) => {
            let inner_id = to_normal_form(inner, interner, lower);
            intern_empty(interner, NormalFormKind::Not(inner_id))
        }
        ExpressionKind::MathematicalComputation(op, inner) => {
            let inner_id = to_normal_form(inner, interner, lower);
            intern_empty(interner, NormalFormKind::MathOp(op.clone(), inner_id))
        }
        ExpressionKind::Veto(v) => intern_empty(interner, NormalFormKind::Veto(v.clone())),
        ExpressionKind::Now => intern_empty(interner, NormalFormKind::Now),
        ExpressionKind::DateRelative(kind, inner) => {
            let inner_id = to_normal_form(inner, interner, lower);
            intern_empty(interner, NormalFormKind::DateRelative(*kind, inner_id))
        }
        ExpressionKind::DateCalendar(kind, unit, inner) => {
            let inner_id = to_normal_form(inner, interner, lower);
            intern_empty(
                interner,
                NormalFormKind::DateCalendar(*kind, *unit, inner_id),
            )
        }
        ExpressionKind::RangeLiteral(left, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(interner, NormalFormKind::RangeLiteral(left_id, right_id))
        }
        ExpressionKind::PastFutureRange(kind, inner) => {
            let inner_id = to_normal_form(inner, interner, lower);
            intern_empty(interner, NormalFormKind::PastFutureRange(*kind, inner_id))
        }
        ExpressionKind::RangeContainment(left, right) => {
            let left_id = to_normal_form(left, interner, lower);
            let right_id = to_normal_form(right, interner, lower);
            intern_empty(
                interner,
                NormalFormKind::RangeContainment(left_id, right_id),
            )
        }
        ExpressionKind::ResultIsVeto(operand) => {
            let operand_id = to_normal_form(operand, interner, lower);
            intern_empty(interner, NormalFormKind::ResultIsVeto(operand_id))
        }
        ExpressionKind::Piecewise(arms) => {
            let mut mapped = Vec::with_capacity(arms.len());
            for (condition, result) in arms {
                mapped.push((
                    to_normal_form(condition, interner, lower),
                    to_normal_form(result, interner, lower),
                ));
            }
            intern_empty(interner, NormalFormKind::Piecewise(mapped))
        }
    }
}

fn expand_numeric_subtract_divide(
    id: NormalFormId,
    interner: &mut NormalFormInterner,
) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::Subtract(a, b)
            if is_numeric_only(interner, a) && is_numeric_only(interner, b) =>
        {
            let negate_b = rebuild(interner, NormalFormKind::Negate(b), None, None);
            fold_into(interner, NormalFormKind::Sum(vec![a, negate_b]), id)
        }
        NormalFormKind::Divide(a, b)
            if is_numeric_only(interner, a) && is_numeric_only(interner, b) =>
        {
            let reciprocal_b = rebuild(interner, NormalFormKind::Reciprocal(b), None, None);
            fold_into(interner, NormalFormKind::Product(vec![a, reciprocal_b]), id)
        }
        NormalFormKind::Sum(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|c| expand_numeric_subtract_divide(c, interner))
                .collect();
            rebuild_nary(interner, provenance, origin, children, NormalFormKind::Sum)
        }
        NormalFormKind::Product(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|c| expand_numeric_subtract_divide(c, interner))
                .collect();
            rebuild_nary(
                interner,
                provenance,
                origin,
                children,
                NormalFormKind::Product,
            )
        }
        NormalFormKind::Subtract(a, b) => {
            let a_done = expand_numeric_subtract_divide(a, interner);
            let b_done = expand_numeric_subtract_divide(b, interner);
            rebuild(
                interner,
                NormalFormKind::Subtract(a_done, b_done),
                provenance,
                origin,
            )
        }
        NormalFormKind::Divide(a, b) => {
            let a_done = expand_numeric_subtract_divide(a, interner);
            let b_done = expand_numeric_subtract_divide(b, interner);
            rebuild(
                interner,
                NormalFormKind::Divide(a_done, b_done),
                provenance,
                origin,
            )
        }
        NormalFormKind::Power(a, b) => {
            let a_done = expand_numeric_subtract_divide(a, interner);
            let b_done = expand_numeric_subtract_divide(b, interner);
            rebuild(
                interner,
                NormalFormKind::Power(a_done, b_done),
                provenance,
                origin,
            )
        }
        NormalFormKind::Negate(x) => {
            let x_done = expand_numeric_subtract_divide(x, interner);
            rebuild(interner, NormalFormKind::Negate(x_done), provenance, origin)
        }
        NormalFormKind::Reciprocal(x) => {
            let x_done = expand_numeric_subtract_divide(x, interner);
            rebuild(
                interner,
                NormalFormKind::Reciprocal(x_done),
                provenance,
                origin,
            )
        }
        NormalFormKind::And(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|c| expand_numeric_subtract_divide(c, interner))
                .collect();
            rebuild_nary(interner, provenance, origin, children, NormalFormKind::And)
        }
        NormalFormKind::Not(x) => {
            let x_done = expand_numeric_subtract_divide(x, interner);
            rebuild(interner, NormalFormKind::Not(x_done), provenance, origin)
        }
        NormalFormKind::Comparison(a, op, b) => {
            let a_done = expand_numeric_subtract_divide(a, interner);
            let b_done = expand_numeric_subtract_divide(b, interner);
            rebuild(
                interner,
                NormalFormKind::Comparison(a_done, op, b_done),
                provenance,
                origin,
            )
        }
        NormalFormKind::MathOp(op, x) => {
            let x_done = expand_numeric_subtract_divide(x, interner);
            rebuild(
                interner,
                NormalFormKind::MathOp(op, x_done),
                provenance,
                origin,
            )
        }
        NormalFormKind::UnitConversion(x, target) => {
            let x_done = expand_numeric_subtract_divide(x, interner);
            rebuild(
                interner,
                NormalFormKind::UnitConversion(x_done, target),
                provenance,
                origin,
            )
        }
        other => rebuild(interner, other, provenance, origin),
    }
}

fn is_numeric_only(interner: &NormalFormInterner, id: NormalFormId) -> bool {
    match &interner.get(id).kind {
        NormalFormKind::Leaf(LeafKind::Literal(lit)) => {
            matches!(lit.value, crate::planning::semantics::ValueKind::Number(_))
        }
        NormalFormKind::Sum(children) | NormalFormKind::Product(children) => {
            children.iter().all(|c| is_numeric_only(interner, *c))
        }
        NormalFormKind::Subtract(a, b) | NormalFormKind::Divide(a, b) => {
            is_numeric_only(interner, *a) && is_numeric_only(interner, *b)
        }
        NormalFormKind::Power(a, b) => {
            is_numeric_only(interner, *a) && is_numeric_only(interner, *b)
        }
        NormalFormKind::Negate(x) | NormalFormKind::Reciprocal(x) => is_numeric_only(interner, *x),
        _ => false,
    }
}

fn flatten_associative(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::Sum(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|c| flatten_associative(c, interner))
                .collect();
            let needs_flatten = children.iter().any(|child| {
                interner.get(*child).rule_embed.is_none()
                    && matches!(interner.get(*child).kind, NormalFormKind::Sum(_))
            });
            if !needs_flatten {
                return rebuild_nary(interner, provenance, origin, children, NormalFormKind::Sum);
            }
            let mut flat = Vec::new();
            for child in children {
                match &interner.get(child).kind {
                    NormalFormKind::Sum(inner) if interner.get(child).rule_embed.is_none() => {
                        flat.extend(inner.iter().copied())
                    }
                    _ => flat.push(child),
                }
            }
            fold_into(interner, NormalFormKind::Sum(flat), id)
        }
        NormalFormKind::Product(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|c| flatten_associative(c, interner))
                .collect();
            let needs_flatten = children.iter().any(|child| {
                interner.get(*child).rule_embed.is_none()
                    && matches!(interner.get(*child).kind, NormalFormKind::Product(_))
            });
            if !needs_flatten {
                return rebuild_nary(
                    interner,
                    provenance,
                    origin,
                    children,
                    NormalFormKind::Product,
                );
            }
            let mut flat = Vec::new();
            for child in children {
                match &interner.get(child).kind {
                    NormalFormKind::Product(inner) if interner.get(child).rule_embed.is_none() => {
                        flat.extend(inner.iter().copied())
                    }
                    _ => flat.push(child),
                }
            }
            fold_into(interner, NormalFormKind::Product(flat), id)
        }
        // Do not flatten nested And: guard chains are `And(cond, val)`; merging would yield
        // `And(And(cond, val), …)` rebuilds as left-assoc and leaves a non-boolean on the
        // left of an outer `LogicalAnd` (invalid for evaluator / unless semantics).
        NormalFormKind::And(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|c| flatten_associative(c, interner))
                .collect();
            rebuild_nary(interner, provenance, origin, children, NormalFormKind::And)
        }
        other => rebuild(interner, other, provenance, origin),
    }
}

fn eliminate_identities(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::Sum(children) => {
            let original_len = children.len();
            let kept: Vec<NormalFormId> = children
                .into_iter()
                .filter_map(|c| {
                    if is_numeric_zero(interner, c) {
                        None
                    } else {
                        Some(eliminate_identities(c, interner))
                    }
                })
                .collect();
            match kept.len() {
                0 => fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                        rational_zero(),
                    )))),
                    id,
                ),
                1 => {
                    let survivor = kept.into_iter().next().expect("BUG: len 1");
                    fold_into_survivor(interner, survivor, id)
                }
                _ if kept.len() == original_len => {
                    rebuild_nary(interner, provenance, origin, kept, NormalFormKind::Sum)
                }
                _ => fold_into(interner, NormalFormKind::Sum(kept), id),
            }
        }
        NormalFormKind::Product(children) => {
            if children.iter().any(|c| is_numeric_zero(interner, *c)) {
                if let Some(zero) = typed_product_zero(interner, &children) {
                    let zero_kind = interner.get(zero).kind.clone();
                    return fold_into(interner, zero_kind, id);
                }
            }
            let original_len = children.len();
            let kept: Vec<NormalFormId> = children
                .into_iter()
                .filter_map(|c| {
                    if is_numeric_one(interner, c) {
                        None
                    } else {
                        Some(eliminate_identities(c, interner))
                    }
                })
                .collect();
            match kept.len() {
                0 => fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                        rational_one(),
                    )))),
                    id,
                ),
                1 => {
                    let survivor = kept.into_iter().next().expect("BUG: len 1");
                    fold_into_survivor(interner, survivor, id)
                }
                _ if kept.len() == original_len => {
                    rebuild_nary(interner, provenance, origin, kept, NormalFormKind::Product)
                }
                _ => fold_into(interner, NormalFormKind::Product(kept), id),
            }
        }
        NormalFormKind::Power(base, exp) => {
            let base_done = eliminate_identities(base, interner);
            let exp_done = eliminate_identities(exp, interner);
            if is_numeric_one(interner, exp_done) {
                return fold_into_survivor(interner, base_done, id);
            }
            if is_numeric_zero(interner, exp_done)
                && as_rational_literal(interner, base_done)
                    .is_some_and(|rational| !rational_is_zero(&rational))
            {
                return fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                        rational_one(),
                    )))),
                    id,
                );
            }
            rebuild(
                interner,
                NormalFormKind::Power(base_done, exp_done),
                provenance,
                origin,
            )
        }
        NormalFormKind::Negate(x) => {
            let inner = eliminate_identities(x, interner);
            if is_numeric_zero(interner, inner) {
                return fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                        rational_zero(),
                    )))),
                    id,
                );
            }
            rebuild(interner, NormalFormKind::Negate(inner), provenance, origin)
        }
        NormalFormKind::Subtract(a, b) => {
            let a_done = eliminate_identities(a, interner);
            let b_done = eliminate_identities(b, interner);
            if is_numeric_zero(interner, a_done) {
                return fold_into(interner, NormalFormKind::Negate(b_done), id);
            }
            if is_numeric_zero(interner, b_done) {
                return fold_into_survivor(interner, a_done, id);
            }
            rebuild(
                interner,
                NormalFormKind::Subtract(a_done, b_done),
                provenance,
                origin,
            )
        }
        NormalFormKind::Divide(a, b) => {
            let a_done = eliminate_identities(a, interner);
            let b_done = eliminate_identities(b, interner);
            if is_numeric_one(interner, b_done) {
                return fold_into_survivor(interner, a_done, id);
            }
            if is_numeric_one(interner, a_done) {
                return fold_into(interner, NormalFormKind::Reciprocal(b_done), id);
            }
            rebuild(
                interner,
                NormalFormKind::Divide(a_done, b_done),
                provenance,
                origin,
            )
        }
        NormalFormKind::And(children) => {
            if children
                .iter()
                .any(|child| is_literal_bool(interner, *child, false))
                && children.iter().all(|child| is_total(interner, *child))
            {
                return fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                        false,
                    )))),
                    id,
                );
            }
            let original_len = children.len();
            let kept: Vec<NormalFormId> = children
                .into_iter()
                .filter_map(|child| {
                    if is_literal_bool(interner, child, true) {
                        None
                    } else {
                        Some(eliminate_identities(child, interner))
                    }
                })
                .collect();
            match kept.len() {
                0 => fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                        true,
                    )))),
                    id,
                ),
                1 => {
                    let survivor = kept.into_iter().next().expect("BUG: len 1");
                    fold_into_survivor(interner, survivor, id)
                }
                _ if kept.len() == original_len => {
                    rebuild_nary(interner, provenance, origin, kept, NormalFormKind::And)
                }
                _ => fold_into(interner, NormalFormKind::And(kept), id),
            }
        }
        NormalFormKind::Not(inner) => {
            let inner_done = eliminate_identities(inner, interner);
            if is_literal_bool(interner, inner_done, true) {
                return fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                        false,
                    )))),
                    id,
                );
            }
            if is_literal_bool(interner, inner_done, false) {
                return fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                        true,
                    )))),
                    id,
                );
            }
            rebuild(
                interner,
                NormalFormKind::Not(inner_done),
                provenance,
                origin,
            )
        }
        other => rebuild(interner, other, provenance, origin),
    }
}

fn typed_product_zero(
    interner: &mut NormalFormInterner,
    children: &[NormalFormId],
) -> Option<NormalFormId> {
    let mut measure_literal: Option<&LiteralValue> = None;
    for &child in children {
        let NormalFormKind::Leaf(LeafKind::Literal(literal)) = &interner.get(child).kind else {
            return None;
        };
        match &literal.value {
            ValueKind::Number(_) => {}
            ValueKind::Measure(_, _) => {
                if measure_literal.is_some() {
                    return None;
                }
                measure_literal = Some(literal.as_ref());
            }
            _ => return None,
        }
    }
    match measure_literal {
        None => Some(intern_empty(
            interner,
            NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                rational_zero(),
            )))),
        )),
        Some(literal) => {
            let ValueKind::Measure(_, signature) = &literal.value else {
                unreachable!("BUG: measure literal collected above must carry a measure value");
            };
            Some(intern_empty(
                interner,
                NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                    LiteralValue::measure_with_signature(
                        rational_zero(),
                        signature.clone(),
                        literal.lemma_type.clone(),
                    ),
                ))),
            ))
        }
    }
}

fn is_literal_bool(interner: &NormalFormInterner, id: NormalFormId, expected: bool) -> bool {
    match &interner.get(id).kind {
        NormalFormKind::Leaf(LeafKind::Literal(literal)) => {
            matches!(literal.value, ValueKind::Boolean(value) if value == expected)
        }
        _ => false,
    }
}

fn double_negate_reciprocal(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::Negate(x) => match interner.get(x).kind {
            NormalFormKind::Negate(y) => fold_into_survivor(interner, y, id),
            _ => {
                let inner = double_negate_reciprocal(x, interner);
                rebuild(interner, NormalFormKind::Negate(inner), provenance, origin)
            }
        },
        NormalFormKind::Reciprocal(x) => match interner.get(x).kind {
            NormalFormKind::Reciprocal(y) if is_total(interner, y) => {
                fold_into_survivor(interner, y, id)
            }
            _ => {
                let inner = double_negate_reciprocal(x, interner);
                rebuild(
                    interner,
                    NormalFormKind::Reciprocal(inner),
                    provenance,
                    origin,
                )
            }
        },
        NormalFormKind::Not(x) => match interner.get(x).kind {
            NormalFormKind::Not(y) => fold_into_survivor(interner, y, id),
            _ => {
                let inner = double_negate_reciprocal(x, interner);
                rebuild(interner, NormalFormKind::Not(inner), provenance, origin)
            }
        },
        other => rebuild(interner, other, provenance, origin),
    }
}

fn power_laws(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::Power(base, exp) => {
            let base_simplified = power_laws(base, interner);
            let exp_simplified = power_laws(exp, interner);
            if let NormalFormKind::Power(inner_base, inner_exp) =
                interner.get(base_simplified).kind.clone()
            {
                let merge_preserves_domain = is_total(interner, inner_base)
                    || nested_power_merge_preserves_domain(interner, inner_exp, exp_simplified);
                if merge_preserves_domain {
                    if let Some(new_exp) =
                        try_multiply_nf_rational(interner, inner_exp, exp_simplified)
                    {
                        return fold_into(interner, NormalFormKind::Power(inner_base, new_exp), id);
                    }
                }
            }
            rebuild(
                interner,
                NormalFormKind::Power(base_simplified, exp_simplified),
                provenance,
                origin,
            )
        }
        NormalFormKind::Product(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|c| power_laws(c, interner))
                .collect();
            collect_like_base_powers(interner, children, provenance, id)
        }
        other => rebuild(interner, other, provenance, origin),
    }
}

fn nested_power_merge_preserves_domain(
    interner: &NormalFormInterner,
    inner_exponent: NormalFormId,
    outer_exponent: NormalFormId,
) -> bool {
    let Some(inner) = as_integer_literal(interner, inner_exponent) else {
        return false;
    };
    let Some(outer) = as_integer_literal(interner, outer_exponent) else {
        return false;
    };
    inner.numer().is_positive() && outer.numer().is_positive()
}

fn like_base_merge_preserves_domain(
    interner: &NormalFormInterner,
    left_exponent: NormalFormId,
    right_exponent: NormalFormId,
) -> bool {
    let Some(left) = as_integer_literal(interner, left_exponent) else {
        return false;
    };
    let Some(right) = as_integer_literal(interner, right_exponent) else {
        return false;
    };
    (left.numer().is_positive() && right.numer().is_positive())
        || (left.numer().is_negative() && right.numer().is_negative())
}

fn collect_like_base_powers(
    interner: &mut NormalFormInterner,
    children: Vec<NormalFormId>,
    outer_source: Option<Source>,
    destroyed_id: NormalFormId,
) -> NormalFormId {
    let original_len = children.len();
    let mut powers: Vec<(NormalFormId, NormalFormId, Option<Source>)> = Vec::new();
    let mut other = Vec::new();
    let mut merged = false;
    for c in children {
        let power_shape = match &interner.get(c).kind {
            NormalFormKind::Power(base, exp) => Some((*base, *exp)),
            _ => None,
        };
        if let Some((base_val, exp_val)) = power_shape {
            if let Some((_, stored_exp, _)) = powers.iter_mut().find(|(b, _, _)| *b == base_val) {
                let merge_preserves_domain = is_total(interner, base_val)
                    || like_base_merge_preserves_domain(interner, *stored_exp, exp_val);
                if merge_preserves_domain {
                    if let Some(sum_exp) = try_add_nf_rational(interner, *stored_exp, exp_val) {
                        *stored_exp = sum_exp;
                        merged = true;
                        continue;
                    }
                }
            }
            powers.push((base_val, exp_val, interner.get(c).source.clone()));
        } else {
            other.push(c);
        }
    }
    for (base, exp, factor_source) in powers {
        other.push(rebuild(
            interner,
            NormalFormKind::Power(base, exp),
            factor_source,
            None,
        ));
    }
    let changed = merged || other.len() != original_len;
    if !changed {
        rebuild_nary(interner, outer_source, None, other, NormalFormKind::Product)
    } else {
        match other.len() {
            0 => fold_into(
                interner,
                NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_one(),
                )))),
                destroyed_id,
            ),
            1 => {
                let survivor = other.into_iter().next().expect("BUG: single product term");
                fold_into_survivor(interner, survivor, destroyed_id)
            }
            _ => fold_into(interner, NormalFormKind::Product(other), destroyed_id),
        }
    }
}

fn try_multiply_nf_rational(
    interner: &mut NormalFormInterner,
    a: NormalFormId,
    b: NormalFormId,
) -> Option<NormalFormId> {
    let ra = as_rational_literal(interner, a)?;
    let rb = as_rational_literal(interner, b)?;
    let rational = rational_operation(&ra, NumericOperation::Multiply, &rb).ok()?;
    let literal = literal_from_folded_rational(rational, None).ok()?;
    Some(intern_empty(
        interner,
        NormalFormKind::Leaf(LeafKind::Literal(Arc::new(literal))),
    ))
}

fn try_add_nf_rational(
    interner: &mut NormalFormInterner,
    a: NormalFormId,
    b: NormalFormId,
) -> Option<NormalFormId> {
    let ra = as_rational_literal(interner, a)?;
    let rb = as_rational_literal(interner, b)?;
    let rational = rational_operation(&ra, NumericOperation::Add, &rb).ok()?;
    let literal = literal_from_folded_rational(rational, None).ok()?;
    Some(intern_empty(
        interner,
        NormalFormKind::Leaf(LeafKind::Literal(Arc::new(literal))),
    ))
}

fn constant_fold(
    id: NormalFormId,
    interner: &mut NormalFormInterner,
    source: Option<Source>,
) -> Result<NormalFormId, Error> {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return Ok(id);
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::Sum(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|child| constant_fold(child, interner, source.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            if children
                .iter()
                .all(|c| as_rational_literal(interner, *c).is_some())
            {
                let mut sorted = children;
                sorted.sort_by(|a, b| canonical_numeric_cmp(interner, *a, *b));
                let mut acc = rational_new(0, 1);
                for child in &sorted {
                    let rational = as_rational_literal(interner, *child).expect("BUG: all numeric");
                    acc = rational_operation(&acc, NumericOperation::Add, &rational).map_err(
                        |failure| normalization_error(source.clone(), failure, "constant fold sum"),
                    )?;
                }
                let destroyed_id = rebuild(
                    interner,
                    NormalFormKind::Sum(sorted),
                    provenance.clone(),
                    origin,
                );
                return Ok(fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(acc, source.clone())?,
                    ))),
                    destroyed_id,
                ));
            }
            Ok(rebuild_nary(
                interner,
                provenance,
                origin,
                children,
                NormalFormKind::Sum,
            ))
        }
        NormalFormKind::Product(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|child| constant_fold(child, interner, source.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            if children
                .iter()
                .all(|c| as_rational_literal(interner, *c).is_some())
            {
                let mut acc = rational_new(1, 1);
                for child in &children {
                    let rational = as_rational_literal(interner, *child).expect("BUG: all numeric");
                    acc = rational_operation(&acc, NumericOperation::Multiply, &rational).map_err(
                        |failure| {
                            normalization_error(source.clone(), failure, "constant fold product")
                        },
                    )?;
                }
                return Ok(fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(acc, source.clone())?,
                    ))),
                    id,
                ));
            }
            Ok(rebuild_nary(
                interner,
                provenance,
                origin,
                children,
                NormalFormKind::Product,
            ))
        }
        NormalFormKind::Power(base, exp) => {
            let base = constant_fold(base, interner, source.clone())?;
            let exp = constant_fold(exp, interner, source.clone())?;
            if let (Some(base_rational), Some(exp_rational)) = (
                as_rational_literal(interner, base),
                as_rational_literal(interner, exp),
            ) {
                if let Ok(rational) =
                    rational_operation(&base_rational, NumericOperation::Power, &exp_rational)
                {
                    return Ok(fold_into(
                        interner,
                        NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                            literal_from_folded_rational(rational, source.clone())?,
                        ))),
                        id,
                    ));
                }
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Power(base, exp),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Negate(x) => {
            let inner = constant_fold(x, interner, source.clone())?;
            if let Some(rational) = as_rational_literal(interner, inner) {
                let zero = rational_new(0, 1);
                let negated = rational_operation(&zero, NumericOperation::Subtract, &rational)
                    .map_err(|failure| {
                        normalization_error(source.clone(), failure, "constant fold negate")
                    })?;
                return Ok(fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(negated, source.clone())?,
                    ))),
                    id,
                ));
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Negate(inner),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Reciprocal(x) => {
            let inner = constant_fold(x, interner, source.clone())?;
            if let Some(rational) = as_rational_literal(interner, inner) {
                let one = rational_one();
                let reciprocal = rational_operation(&one, NumericOperation::Divide, &rational)
                    .map_err(|failure| {
                        normalization_error(source.clone(), failure, "constant fold reciprocal")
                    })?;
                return Ok(fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(reciprocal, source.clone())?,
                    ))),
                    id,
                ));
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Reciprocal(inner),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Divide(left, right) => {
            let left = constant_fold(left, interner, source.clone())?;
            let right = constant_fold(right, interner, source.clone())?;
            if let (Some(left_rational), Some(right_rational)) = (
                as_rational_literal(interner, left),
                as_rational_literal(interner, right),
            ) {
                let quotient =
                    rational_operation(&left_rational, NumericOperation::Divide, &right_rational)
                        .map_err(|failure| {
                        normalization_error(source.clone(), failure, "constant fold divide")
                    })?;
                return Ok(fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(quotient, source.clone())?,
                    ))),
                    id,
                ));
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Divide(left, right),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Subtract(left, right) => {
            let left = constant_fold(left, interner, source.clone())?;
            let right = constant_fold(right, interner, source.clone())?;
            if let (Some(left_rational), Some(right_rational)) = (
                as_rational_literal(interner, left),
                as_rational_literal(interner, right),
            ) {
                let difference =
                    rational_operation(&left_rational, NumericOperation::Subtract, &right_rational)
                        .map_err(|failure| {
                            normalization_error(source.clone(), failure, "constant fold subtract")
                        })?;
                return Ok(fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(difference, source.clone())?,
                    ))),
                    id,
                ));
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Subtract(left, right),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Modulo(left, right) => {
            let left = constant_fold(left, interner, source.clone())?;
            let right = constant_fold(right, interner, source.clone())?;
            if let (Some(left_rational), Some(right_rational)) = (
                as_rational_literal(interner, left),
                as_rational_literal(interner, right),
            ) {
                let remainder =
                    rational_operation(&left_rational, NumericOperation::Modulo, &right_rational)
                        .map_err(|failure| {
                        normalization_error(source.clone(), failure, "constant fold modulo")
                    })?;
                return Ok(fold_into(
                    interner,
                    NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(remainder, source.clone())?,
                    ))),
                    id,
                ));
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Modulo(left, right),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Not(x) => {
            let inner = constant_fold(x, interner, source.clone())?;
            if let NormalFormKind::Leaf(LeafKind::Literal(literal)) = &interner.get(inner).kind {
                if let ValueKind::Boolean(boolean) = &literal.value {
                    return Ok(fold_into(
                        interner,
                        NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                            !boolean,
                        )))),
                        id,
                    ));
                }
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Not(inner),
                provenance,
                origin,
            ))
        }
        NormalFormKind::Comparison(left, op, right) => {
            let left = constant_fold(left, interner, source.clone())?;
            let right = constant_fold(right, interner, source.clone())?;
            if let (
                NormalFormKind::Leaf(LeafKind::Literal(left_literal)),
                NormalFormKind::Leaf(LeafKind::Literal(right_literal)),
            ) = (&interner.get(left).kind, &interner.get(right).kind)
            {
                let folded = comparison_operation(
                    left_literal,
                    &op,
                    right_literal,
                    UnitResolutionContext::NamedMeasureOnly,
                );
                if let Some(literal) = folded.value() {
                    if let ValueKind::Boolean(held) = literal.value {
                        return Ok(fold_comparison_to_bool(interner, left, op, right, held));
                    }
                }
            }
            Ok(rebuild(
                interner,
                NormalFormKind::Comparison(left, op, right),
                provenance,
                origin,
            ))
        }
        NormalFormKind::And(children) => {
            let folded = children
                .into_iter()
                .map(|child| constant_fold(child, interner, source.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rebuild_nary(
                interner,
                provenance,
                origin,
                folded,
                NormalFormKind::And,
            ))
        }
        NormalFormKind::MathOp(op, x) => {
            let inner = constant_fold(x, interner, source.clone())?;
            if let Some(rational) = as_rational_literal(interner, inner) {
                if let Some(folded) = fold_math_op(&op, &rational) {
                    return Ok(fold_into(
                        interner,
                        NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                            literal_from_folded_rational(folded, source.clone())?,
                        ))),
                        id,
                    ));
                }
            }
            Ok(rebuild(
                interner,
                NormalFormKind::MathOp(op, inner),
                provenance,
                origin,
            ))
        }
        other => Ok(rebuild(interner, other, provenance, origin)),
    }
}

/// Fold a decidable Comparison to a bool leaf, retaining the comparison as origin.
fn fold_comparison_to_bool(
    interner: &mut NormalFormInterner,
    left: NormalFormId,
    op: ComparisonComputation,
    right: NormalFormId,
    held: bool,
) -> NormalFormId {
    let destroyed = rebuild(
        interner,
        NormalFormKind::Comparison(left, op, right),
        None,
        None,
    );
    fold_into(
        interner,
        NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(held)))),
        destroyed,
    )
}

/// Last-match-wins collapse of Piecewise arms with statically decided conditions.
///
/// - Drop arms whose condition is statically false (including `flag and false`
///   after a conjunct folds — only for unless conditions, not rule-body And).
/// - Keep only arms from the last statically-true condition onward.
/// - Unwrap a single remaining arm to its result.
fn collapse_piecewise(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let NormalFormKind::Piecewise(arms) = interner.get(id).kind.clone() else {
        return id;
    };

    let mut recorded: Vec<(NormalFormId, NormalFormId)> = Vec::with_capacity(arms.len());
    for (condition, result) in arms {
        let forced = force_unless_condition_static_false(condition, interner);
        recorded.push((forced, result));
    }
    assert!(!recorded.is_empty(), "BUG: Piecewise collapse has no arms");

    let mut kept: Vec<(NormalFormId, NormalFormId)> = recorded
        .iter()
        .filter(|(condition, _)| !is_literal_bool(interner, *condition, false))
        .copied()
        .collect();
    assert!(
        !kept.is_empty(),
        "BUG: Piecewise collapse removed every arm"
    );
    let mut last_true_index = None;
    for (index, (condition, _)) in kept.iter().enumerate() {
        if is_literal_bool(interner, *condition, true) {
            last_true_index = Some(index);
        }
    }
    if let Some(index) = last_true_index {
        kept = kept.split_off(index);
    }

    let dropped = kept.len() != recorded.len();
    if !dropped && kept.len() > 1 {
        return rebuild(interner, NormalFormKind::Piecewise(kept), None, None);
    }

    let destroyed_id = rebuild(interner, NormalFormKind::Piecewise(recorded), None, None);

    if kept.len() == 1 {
        let survivor = kept[0].1;
        return fold_into_survivor(interner, survivor, destroyed_id);
    }
    let survivor = rebuild(interner, NormalFormKind::Piecewise(kept), None, None);
    fold_into_survivor(interner, survivor, destroyed_id)
}

fn force_unless_condition_static_false(
    id: NormalFormId,
    interner: &mut NormalFormInterner,
) -> NormalFormId {
    if unless_condition_contains_literal_false(interner, id) {
        return fold_into(
            interner,
            NormalFormKind::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(false)))),
            id,
        );
    }
    id
}

fn unless_condition_contains_literal_false(
    interner: &NormalFormInterner,
    id: NormalFormId,
) -> bool {
    if is_literal_bool(interner, id, false) {
        return true;
    }
    match &interner.get(id).kind {
        NormalFormKind::And(children) => children
            .iter()
            .any(|child| unless_condition_contains_literal_false(interner, *child)),
        _ => false,
    }
}

fn fold_math_op(
    op: &MathematicalComputation,
    operand: &RationalInteger,
) -> Option<RationalInteger> {
    let zero = rational_new(0, 1);
    let one = rational_new(1, 1);
    match op {
        MathematicalComputation::Sin if *operand == zero => Some(zero),
        MathematicalComputation::Cos if *operand == zero => Some(one),
        MathematicalComputation::Tan if *operand == zero => Some(zero),
        MathematicalComputation::Log if *operand == one => Some(zero),
        MathematicalComputation::Exp if *operand == zero => Some(one),
        _ => None,
    }
}

fn demorgan(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::Not(x) => match &interner.get(x).kind {
            NormalFormKind::Comparison(a, op, b) => fold_into(
                interner,
                NormalFormKind::Comparison(*a, negated_comparison(op.clone()), *b),
                id,
            ),
            _ => {
                let inner = demorgan(x, interner);
                rebuild(interner, NormalFormKind::Not(inner), provenance, origin)
            }
        },
        other => rebuild(interner, other, provenance, origin),
    }
}

fn logical_flatten(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    flatten_associative(id, interner)
}

fn logical_short_circuit(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::And(children) => {
            let all_total = children.iter().all(|c| is_total(interner, *c));
            if all_total {
                for c in &children {
                    if matches!(
                        &interner.get(*c).kind,
                        NormalFormKind::Leaf(LeafKind::Literal(l))
                            if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(false))
                    ) {
                        return fold_into(
                            interner,
                            NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                                LiteralValue::from_bool(false),
                            ))),
                            id,
                        );
                    }
                }
            }
            rebuild_nary(interner, provenance, origin, children, NormalFormKind::And)
        }
        other => rebuild(interner, other, provenance, origin),
    }
}

fn logical_idempotency(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::And(children) => {
            let original_len = children.len();
            let mut unique = Vec::new();
            for c in &children {
                // Id equality only: Kind equality would collapse distinct rule
                // embeds that share body Kind after cons.
                if !unique.contains(c) {
                    unique.push(*c);
                }
            }
            if unique.len() == original_len {
                rebuild_nary(interner, provenance, origin, children, NormalFormKind::And)
            } else {
                fold_into(interner, NormalFormKind::And(unique), id)
            }
        }
        other => rebuild(interner, other, provenance, origin),
    }
}

fn negated_comparisons(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::Not(x) => match &interner.get(x).kind {
            NormalFormKind::Comparison(a, op, b) => fold_into(
                interner,
                NormalFormKind::Comparison(*a, negated_comparison(op.clone()), *b),
                id,
            ),
            _ => {
                let inner = negated_comparisons(x, interner);
                rebuild(interner, NormalFormKind::Not(inner), provenance, origin)
            }
        },
        other => rebuild(interner, other, provenance, origin),
    }
}

fn math_identities(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::MathOp(MathematicalComputation::Abs, x) => match &interner.get(x).kind {
            NormalFormKind::MathOp(MathematicalComputation::Abs, inner) => fold_into(
                interner,
                NormalFormKind::MathOp(MathematicalComputation::Abs, *inner),
                id,
            ),
            _ => {
                let inner = math_identities(x, interner);
                rebuild(
                    interner,
                    NormalFormKind::MathOp(MathematicalComputation::Abs, inner),
                    provenance,
                    origin,
                )
            }
        },
        NormalFormKind::MathOp(MathematicalComputation::Exp, x) => match &interner.get(x).kind {
            NormalFormKind::MathOp(MathematicalComputation::Log, inner)
                if is_total(interner, *inner) =>
            {
                fold_into_survivor(interner, *inner, id)
            }
            _ => {
                let inner = math_identities(x, interner);
                rebuild(
                    interner,
                    NormalFormKind::MathOp(MathematicalComputation::Exp, inner),
                    provenance,
                    origin,
                )
            }
        },
        NormalFormKind::MathOp(MathematicalComputation::Log, x) => match &interner.get(x).kind {
            NormalFormKind::MathOp(MathematicalComputation::Exp, inner)
                if is_total(interner, *inner) =>
            {
                fold_into_survivor(interner, *inner, id)
            }
            _ => {
                let inner = math_identities(x, interner);
                rebuild(
                    interner,
                    NormalFormKind::MathOp(MathematicalComputation::Log, inner),
                    provenance,
                    origin,
                )
            }
        },
        NormalFormKind::MathOp(MathematicalComputation::Sqrt, x) => {
            let base = math_identities(x, interner);
            let half = intern_empty(
                interner,
                NormalFormKind::Leaf(LeafKind::Literal(Arc::new(
                    literal_from_folded_rational(rational_new(1, 2), None)
                        .expect("BUG: literal 1/2 must commit at normalize"),
                ))),
            );
            fold_into(interner, NormalFormKind::Power(base, half), id)
        }
        other => rebuild(interner, other, provenance, origin),
    }
}

fn canonical_order(id: NormalFormId, interner: &mut NormalFormInterner) -> NormalFormId {
    if let Some(id) = return_if_rule_embed(id, interner) {
        return id;
    }
    let (provenance, origin) = node_source_origin(interner, id);
    match interner.get(id).kind.clone() {
        NormalFormKind::Sum(mut children) => {
            if children.iter().all(|c| is_numeric_only(interner, *c)) {
                let before = children.clone();
                children.sort_by(|a, b| canonical_numeric_cmp(interner, *a, *b));
                if children != before {
                    return fold_into(interner, NormalFormKind::Sum(children), id);
                }
            }
            rebuild_nary(interner, provenance, origin, children, NormalFormKind::Sum)
        }
        NormalFormKind::Product(mut children) => {
            if children.iter().all(|c| is_numeric_only(interner, *c)) {
                let before = children.clone();
                children.sort_by(|a, b| canonical_numeric_cmp(interner, *a, *b));
                if children != before {
                    return fold_into(interner, NormalFormKind::Product(children), id);
                }
            }
            rebuild_nary(
                interner,
                provenance,
                origin,
                children,
                NormalFormKind::Product,
            )
        }
        NormalFormKind::And(children) => {
            let children: Vec<NormalFormId> = children
                .into_iter()
                .map(|c| canonical_order(c, interner))
                .collect();
            rebuild_nary(interner, provenance, origin, children, NormalFormKind::And)
        }
        other => rebuild(interner, other, provenance, origin),
    }
}

fn canonical_numeric_cmp(
    interner: &NormalFormInterner,
    a: NormalFormId,
    b: NormalFormId,
) -> std::cmp::Ordering {
    match (sort_key(interner, a), sort_key(interner, b)) {
        (ak, bk) if ak != bk => ak.cmp(&bk),
        _ => match (
            as_rational_literal(interner, a),
            as_rational_literal(interner, b),
        ) {
            (Some(ra), Some(rb)) => ra.cmp(&rb),
            _ => std::cmp::Ordering::Equal,
        },
    }
}

fn sort_key(interner: &NormalFormInterner, id: NormalFormId) -> u8 {
    match &interner.get(id).kind {
        NormalFormKind::Leaf(LeafKind::Literal(_)) => 0,
        _ => 1,
    }
}

fn is_numeric_zero(interner: &NormalFormInterner, id: NormalFormId) -> bool {
    as_rational_literal(interner, id).is_some_and(|r| rational_is_zero(&r))
}

fn is_numeric_one(interner: &NormalFormInterner, id: NormalFormId) -> bool {
    as_rational_literal(interner, id).is_some_and(|r| r == rational_new(1, 1))
}

fn as_rational_literal(interner: &NormalFormInterner, id: NormalFormId) -> Option<RationalInteger> {
    match &interner.get(id).kind {
        NormalFormKind::Leaf(LeafKind::Literal(literal)) => match &literal.value {
            ValueKind::Number(number) => Some(number.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn is_total(interner: &NormalFormInterner, id: NormalFormId) -> bool {
    matches!(
        &interner.get(id).kind,
        NormalFormKind::Leaf(LeafKind::Literal(_))
    )
}

fn as_integer_literal(interner: &NormalFormInterner, id: NormalFormId) -> Option<RationalInteger> {
    let rational = as_rational_literal(interner, id)?;
    if rational.denom() == &crate::computation::bigint::BigInt::one() {
        Some(rational)
    } else {
        None
    }
}

/// Precedence for a normal-form kind. Levels match [`crate::parsing::ast::expression_precedence`].
fn normal_form_precedence(kind: &NormalFormKind) -> u8 {
    match kind {
        NormalFormKind::And(_) => 2,
        NormalFormKind::Not(_) | NormalFormKind::Negate(_) => 3,
        NormalFormKind::Comparison(..)
        | NormalFormKind::ResultIsVeto(_)
        | NormalFormKind::RangeContainment(..)
        | NormalFormKind::DateRelative(..)
        | NormalFormKind::DateCalendar(..) => 4,
        NormalFormKind::Sum(_) | NormalFormKind::Subtract(..) => {
            arithmetic_precedence(&ArithmeticComputation::Add)
        }
        NormalFormKind::Product(_)
        | NormalFormKind::Divide(..)
        | NormalFormKind::Modulo(..)
        | NormalFormKind::Reciprocal(_) => arithmetic_precedence(&ArithmeticComputation::Multiply),
        NormalFormKind::Power(..) => arithmetic_precedence(&ArithmeticComputation::Power),
        NormalFormKind::UnitConversion(..) => 8,
        NormalFormKind::RangeLiteral(..) => 9,
        NormalFormKind::Leaf(_)
        | NormalFormKind::MathOp(..)
        | NormalFormKind::Veto(_)
        | NormalFormKind::Now
        | NormalFormKind::PastFutureRange(..)
        | NormalFormKind::Piecewise(_) => 10,
    }
}

fn explain_child(
    forms: &[NormalForm],
    id: NormalFormId,
    parent_prec: u8,
    side: OperandSide,
    parent_assoc: Option<Associativity>,
) -> String {
    let text = explanation_display_inner(forms, id);
    let child_prec = {
        let mut focus = id;
        loop {
            let nf = cell(forms, focus);
            if let Some(origin) = nf.origin {
                focus = origin;
                continue;
            }
            if nf.rule_embed.is_some() {
                break 10;
            }
            break normal_form_precedence(&nf.kind);
        }
    };
    if operand_needs_parentheses(child_prec, parent_prec, side, parent_assoc) {
        format!("({text})")
    } else {
        text
    }
}

fn explain_nary(
    forms: &[NormalForm],
    children: &[NormalFormId],
    sep: &str,
    prec: u8,
    assoc: Associativity,
) -> String {
    children
        .iter()
        .enumerate()
        .map(|(i, child)| {
            let side = if i == 0 {
                OperandSide::Left
            } else {
                OperandSide::Right
            };
            explain_child(forms, *child, prec, side, Some(assoc))
        })
        .collect::<Vec<_>>()
        .join(sep)
}

fn explain_binary(
    forms: &[NormalForm],
    left: NormalFormId,
    sep: &str,
    right: NormalFormId,
    prec: u8,
    assoc: Associativity,
) -> String {
    format!(
        "{}{}{}",
        explain_child(forms, left, prec, OperandSide::Left, Some(assoc)),
        sep,
        explain_child(forms, right, prec, OperandSide::Right, Some(assoc))
    )
}

fn cell(forms: &[NormalForm], id: NormalFormId) -> &NormalForm {
    forms.get(id.index()).unwrap_or_else(|| {
        panic!(
            "BUG: NormalFormId {} out of range (table len {})",
            id.0,
            forms.len()
        )
    })
}

/// Expression text for a DAG node. Follows origin when present so folded
/// nodes display their pre-image.
pub(crate) fn explanation_display(forms: &[NormalForm], id: NormalFormId) -> String {
    explanation_display_inner(forms, id)
}

fn explanation_display_inner(forms: &[NormalForm], id: NormalFormId) -> String {
    let nf = cell(forms, id);
    if let Some(origin) = nf.origin {
        return explanation_display_inner(forms, origin);
    }
    if let Some(path) = &nf.rule_embed {
        return path.rule.clone();
    }
    match &nf.kind {
        NormalFormKind::Leaf(LeafKind::Literal(lit)) => lit.display_value(),
        NormalFormKind::Leaf(LeafKind::DataPath(p)) => p.input_key(),
        NormalFormKind::Comparison(a, op, b) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "{} {op} {}",
                explain_child(forms, *a, prec, OperandSide::Left, None),
                explain_child(forms, *b, prec, OperandSide::Right, None)
            )
        }
        NormalFormKind::And(children) => explain_nary(
            forms,
            children,
            " and ",
            normal_form_precedence(&nf.kind),
            Associativity::Left,
        ),
        NormalFormKind::Not(x) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "not {}",
                explain_child(forms, *x, prec, OperandSide::Right, None)
            )
        }
        NormalFormKind::ResultIsVeto(x) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "is veto {}",
                explain_child(forms, *x, prec, OperandSide::Right, None)
            )
        }
        NormalFormKind::Sum(children) => explain_nary(
            forms,
            children,
            " + ",
            normal_form_precedence(&nf.kind),
            Associativity::Left,
        ),
        NormalFormKind::Product(children) => explain_nary(
            forms,
            children,
            " * ",
            normal_form_precedence(&nf.kind),
            Associativity::Left,
        ),
        NormalFormKind::MathOp(op, x) => {
            format!("{op}({})", explanation_display_inner(forms, *x))
        }
        NormalFormKind::UnitConversion(x, target) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "{} as {target}",
                explain_child(forms, *x, prec, OperandSide::Left, None)
            )
        }
        NormalFormKind::Veto(_) => "veto".to_string(),
        NormalFormKind::Piecewise(arms) => {
            let mut parts = Vec::with_capacity(arms.len());
            if let Some((_, default)) = arms.first() {
                parts.push(explanation_display_inner(forms, *default));
            }
            for (cond, result) in arms.iter().skip(1) {
                parts.push(format!(
                    "unless {}: {}",
                    explanation_display_inner(forms, *cond),
                    explanation_display_inner(forms, *result)
                ));
            }
            parts.join(" ")
        }
        NormalFormKind::Subtract(a, b) => explain_binary(
            forms,
            *a,
            " - ",
            *b,
            normal_form_precedence(&nf.kind),
            arithmetic_associativity(&ArithmeticComputation::Subtract),
        ),
        NormalFormKind::Divide(a, b) => explain_binary(
            forms,
            *a,
            " / ",
            *b,
            normal_form_precedence(&nf.kind),
            arithmetic_associativity(&ArithmeticComputation::Divide),
        ),
        NormalFormKind::Power(a, b) => explain_binary(
            forms,
            *a,
            " ^ ",
            *b,
            normal_form_precedence(&nf.kind),
            arithmetic_associativity(&ArithmeticComputation::Power),
        ),
        NormalFormKind::Modulo(a, b) => explain_binary(
            forms,
            *a,
            " % ",
            *b,
            normal_form_precedence(&nf.kind),
            arithmetic_associativity(&ArithmeticComputation::Modulo),
        ),
        NormalFormKind::Negate(x) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "-{}",
                explain_child(forms, *x, prec, OperandSide::Right, None)
            )
        }
        NormalFormKind::Reciprocal(x) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "1/{}",
                explain_child(
                    forms,
                    *x,
                    prec,
                    OperandSide::Right,
                    Some(Associativity::Left)
                )
            )
        }
        NormalFormKind::DateRelative(kind, x) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "{kind} {}",
                explain_child(forms, *x, prec, OperandSide::Right, None)
            )
        }
        NormalFormKind::DateCalendar(kind, unit, x) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "{kind} {unit} {}",
                explain_child(forms, *x, prec, OperandSide::Right, None)
            )
        }
        NormalFormKind::RangeLiteral(a, b) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "{} to {}",
                explain_child(forms, *a, prec, OperandSide::Left, None),
                explain_child(forms, *b, prec, OperandSide::Right, None)
            )
        }
        NormalFormKind::PastFutureRange(kind, x) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "{kind} {}",
                explain_child(forms, *x, prec, OperandSide::Right, None)
            )
        }
        NormalFormKind::RangeContainment(value, range) => {
            let prec = normal_form_precedence(&nf.kind);
            format!(
                "{} in {}",
                explain_child(forms, *value, prec, OperandSide::Left, None),
                explain_child(forms, *range, prec, OperandSide::Right, None)
            )
        }
        NormalFormKind::Now => "now".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::rational_new;
    use crate::computation::UnitResolutionContext;
    use crate::planning::semantics::{
        ComparisonComputation, DataPath, ExpressionKind, NegationType, SemanticConversionTarget,
        ValueKind,
    };

    fn interner_forms(interner: &NormalFormInterner) -> &[NormalForm] {
        &interner.forms
    }

    /// Normalize a semantic expression to canonical form (drift-free, single pass).
    fn normalize_expression(
        expr: &Expression,
        unit_ctx: Option<&UnitResolutionContext<'_>>,
    ) -> Result<Expression, Error> {
        let source = expr.source_location.clone();
        let mut interner = NormalFormInterner::new();
        let completed_rules = HashMap::new();
        let rule_target_data = HashMap::new();
        let lower = LowerCtx {
            completed_rules: &completed_rules,
            rule_target_data: &rule_target_data,
        };
        let root = to_normal_form(expr, &mut interner, &lower);
        let nf = normalize_once(root, &mut interner, unit_ctx, source.clone())?;
        Ok(to_expression(interner_forms(&interner), nf, source))
    }

    fn to_normal_form_empty(expr: &Expression, interner: &mut NormalFormInterner) -> NormalFormId {
        let completed_rules = HashMap::new();
        let rule_target_data = HashMap::new();
        let lower = LowerCtx {
            completed_rules: &completed_rules,
            rule_target_data: &rule_target_data,
        };
        to_normal_form(expr, interner, &lower)
    }

    fn to_expression(forms: &[NormalForm], id: NormalFormId, source: Option<Source>) -> Expression {
        let kind = nf_to_kind(forms, id, source.clone());
        Expression::with_source(kind, source)
    }

    fn nf_to_kind(
        forms: &[NormalForm],
        id: NormalFormId,
        source: Option<Source>,
    ) -> ExpressionKind {
        let nf = cell(forms, id);
        if let Some(path) = &nf.rule_embed {
            return ExpressionKind::RulePath(path.clone());
        }
        match &nf.kind {
            NormalFormKind::Leaf(LeafKind::Literal(lit)) => {
                ExpressionKind::Literal(Box::new((**lit).clone()))
            }
            NormalFormKind::Leaf(LeafKind::DataPath(p)) => ExpressionKind::DataPath(p.clone()),
            NormalFormKind::Sum(children) => sum_to_kind(forms, children, source),
            NormalFormKind::Product(children) => product_to_kind(forms, children, source),
            NormalFormKind::Subtract(a, b) => ExpressionKind::Arithmetic(
                Arc::new(to_expression(forms, *a, source.clone())),
                ArithmeticComputation::Subtract,
                Arc::new(to_expression(forms, *b, source)),
            ),
            NormalFormKind::Divide(a, b) => ExpressionKind::Arithmetic(
                Arc::new(to_expression(forms, *a, source.clone())),
                ArithmeticComputation::Divide,
                Arc::new(to_expression(forms, *b, source)),
            ),
            NormalFormKind::Power(base, exp) => ExpressionKind::Arithmetic(
                Arc::new(to_expression(forms, *base, source.clone())),
                ArithmeticComputation::Power,
                Arc::new(to_expression(forms, *exp, source.clone())),
            ),
            NormalFormKind::Modulo(a, b) => ExpressionKind::Arithmetic(
                Arc::new(to_expression(forms, *a, source.clone())),
                ArithmeticComputation::Modulo,
                Arc::new(to_expression(forms, *b, source.clone())),
            ),
            NormalFormKind::Negate(x) => {
                let zero = Expression::with_source(
                    ExpressionKind::Literal(Box::new(LiteralValue::number(rational_zero()))),
                    source.clone(),
                );
                ExpressionKind::Arithmetic(
                    Arc::new(zero),
                    ArithmeticComputation::Subtract,
                    Arc::new(to_expression(forms, *x, source)),
                )
            }
            NormalFormKind::Reciprocal(x) => {
                let one = Expression::with_source(
                    ExpressionKind::Literal(Box::new(LiteralValue::number(rational_one()))),
                    source.clone(),
                );
                ExpressionKind::Arithmetic(
                    Arc::new(one),
                    ArithmeticComputation::Divide,
                    Arc::new(to_expression(forms, *x, source)),
                )
            }
            NormalFormKind::Comparison(a, op, b) => ExpressionKind::Comparison(
                Arc::new(to_expression(forms, *a, source.clone())),
                op.clone(),
                Arc::new(to_expression(forms, *b, source)),
            ),
            NormalFormKind::And(children) => and_to_kind(forms, children, source),
            NormalFormKind::Not(x) => ExpressionKind::LogicalNegation(
                Arc::new(to_expression(forms, *x, source)),
                NegationType::Not,
            ),
            NormalFormKind::MathOp(op, x) => ExpressionKind::MathematicalComputation(
                op.clone(),
                Arc::new(to_expression(forms, *x, source)),
            ),
            NormalFormKind::UnitConversion(x, target) => ExpressionKind::UnitConversion(
                Arc::new(to_expression(forms, *x, source)),
                target.clone(),
            ),
            NormalFormKind::Veto(v) => ExpressionKind::Veto(v.clone()),
            NormalFormKind::Now => ExpressionKind::Now,
            NormalFormKind::DateRelative(kind, x) => {
                ExpressionKind::DateRelative(*kind, Arc::new(to_expression(forms, *x, source)))
            }
            NormalFormKind::DateCalendar(kind, unit, x) => ExpressionKind::DateCalendar(
                *kind,
                *unit,
                Arc::new(to_expression(forms, *x, source)),
            ),
            NormalFormKind::RangeLiteral(a, b) => ExpressionKind::RangeLiteral(
                Arc::new(to_expression(forms, *a, source.clone())),
                Arc::new(to_expression(forms, *b, source)),
            ),
            NormalFormKind::PastFutureRange(kind, x) => {
                ExpressionKind::PastFutureRange(*kind, Arc::new(to_expression(forms, *x, source)))
            }
            NormalFormKind::RangeContainment(a, b) => ExpressionKind::RangeContainment(
                Arc::new(to_expression(forms, *a, source.clone())),
                Arc::new(to_expression(forms, *b, source)),
            ),
            NormalFormKind::ResultIsVeto(operand) => {
                ExpressionKind::ResultIsVeto(Arc::new(to_expression(forms, *operand, source)))
            }
            NormalFormKind::Piecewise(arms) => ExpressionKind::Piecewise(
                arms.iter()
                    .map(|(condition, result)| {
                        (
                            Arc::new(to_expression(forms, *condition, source.clone())),
                            Arc::new(to_expression(forms, *result, source.clone())),
                        )
                    })
                    .collect(),
            ),
        }
    }

    fn sum_to_kind(
        forms: &[NormalForm],
        children: &[NormalFormId],
        source: Option<Source>,
    ) -> ExpressionKind {
        match children {
            [] => ExpressionKind::Literal(Box::new(LiteralValue::number(rational_zero()))),
            [one] => nf_to_kind(forms, *one, source),
            [first, rest @ ..] => {
                let mut acc = Expression::with_source(
                    nf_to_kind(forms, *first, source.clone()),
                    source.clone(),
                );
                for c in rest {
                    acc = Expression::with_source(
                        ExpressionKind::Arithmetic(
                            Arc::new(acc),
                            ArithmeticComputation::Add,
                            Arc::new(to_expression(forms, *c, source.clone())),
                        ),
                        source.clone(),
                    );
                }
                acc.kind
            }
        }
    }

    fn product_to_kind(
        forms: &[NormalForm],
        children: &[NormalFormId],
        source: Option<Source>,
    ) -> ExpressionKind {
        match children {
            [] => ExpressionKind::Literal(Box::new(LiteralValue::number(rational_one()))),
            [one] => nf_to_kind(forms, *one, source),
            [first, rest @ ..] => {
                let mut acc = Expression::with_source(
                    nf_to_kind(forms, *first, source.clone()),
                    source.clone(),
                );
                for c in rest {
                    acc = Expression::with_source(
                        ExpressionKind::Arithmetic(
                            Arc::new(acc),
                            ArithmeticComputation::Multiply,
                            Arc::new(to_expression(forms, *c, source.clone())),
                        ),
                        source.clone(),
                    );
                }
                acc.kind
            }
        }
    }

    fn and_to_kind(
        forms: &[NormalForm],
        children: &[NormalFormId],
        source: Option<Source>,
    ) -> ExpressionKind {
        match children {
            [] => ExpressionKind::Literal(Box::new(LiteralValue::from_bool(true))),
            [one] => nf_to_kind(forms, *one, source),
            [first, rest @ ..] => {
                let mut acc = Expression::with_source(
                    nf_to_kind(forms, *first, source.clone()),
                    source.clone(),
                );
                for c in rest {
                    acc = Expression::with_source(
                        ExpressionKind::LogicalAnd(
                            Arc::new(acc),
                            Arc::new(to_expression(forms, *c, source.clone())),
                        ),
                        source.clone(),
                    );
                }
                acc.kind
            }
        }
    }

    fn num_expr(n: i64) -> Expression {
        Expression::with_source(
            ExpressionKind::Literal(Box::new(LiteralValue::number(rational_new(n, 1)))),
            None,
        )
    }

    fn bool_expr(b: bool) -> Expression {
        Expression::with_source(
            ExpressionKind::Literal(Box::new(LiteralValue::from_bool(b))),
            None,
        )
    }

    fn dx() -> Expression {
        Expression::with_source(
            ExpressionKind::DataPath(DataPath::new(vec![], "x".into())),
            None,
        )
    }

    fn add_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Add, Arc::new(b)),
            None,
        )
    }

    fn mul_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Multiply, Arc::new(b)),
            None,
        )
    }

    fn div_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Divide, Arc::new(b)),
            None,
        )
    }

    fn pow_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Power, Arc::new(b)),
            None,
        )
    }

    fn and_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(ExpressionKind::LogicalAnd(Arc::new(a), Arc::new(b)), None)
    }

    fn not_expr(inner: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::LogicalNegation(Arc::new(inner), NegationType::Not),
            None,
        )
    }

    fn lt_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Comparison(Arc::new(a), ComparisonComputation::LessThan, Arc::new(b)),
            None,
        )
    }

    #[test]
    fn flatten_associative_sum_nested() {
        let inner = add_expr(num_expr(1), num_expr(2));
        let expr = add_expr(inner, num_expr(3));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(6, 1))
        ));
    }

    #[test]
    fn flatten_associative_product_nested() {
        let inner = mul_expr(num_expr(2), num_expr(3));
        let expr = mul_expr(inner, num_expr(4));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(24, 1))
        ));
    }

    #[test]
    fn nested_and_is_not_associative_flattened() {
        // Guard-shaped inner And must stay grouped; flattening inner clauses into one n-ary And
        // would break LogicalAnd evaluation (non-boolean left).
        let inner = and_expr(bool_expr(true), num_expr(7));
        let outer = and_expr(inner, bool_expr(false));
        let mut interner = NormalFormInterner::new();
        let id = to_normal_form_empty(&outer, &mut interner);
        let NormalFormKind::And(top) = &interner.get(id).kind else {
            panic!("expected And(..), got {:?}", interner.get(id).kind);
        };
        assert_eq!(
            top.len(),
            2,
            "nested And must remain 2 children at NF level, got {top:?}"
        );
        assert!(
            matches!(&interner.get(top[0]).kind, NormalFormKind::And(inner) if inner.len() == 2)
        );
    }

    #[test]
    fn identity_add_zero() {
        let expr = add_expr(dx(), num_expr(0));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn identity_mul_one() {
        let expr = mul_expr(dx(), num_expr(1));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn identity_mul_zero_with_data_path_is_not_folded() {
        // `x * 0` must stay a runtime multiply: folding it to `0` would erase
        // a missing-data veto for `x` and drop `x` from the data manifest.
        let expr = mul_expr(dx(), num_expr(0));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::Arithmetic(left, ArithmeticComputation::Multiply, right) = norm.kind
        else {
            panic!("expected preserved multiply, got {:?}", norm.kind);
        };
        assert!(matches!(left.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        assert!(matches!(
            right.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(0, 1))
        ));
    }

    #[test]
    fn identity_mul_zero_with_literal_folds() {
        // All-literal products still fold: `7 * 0 → 0`.
        let expr = mul_expr(num_expr(7), num_expr(0));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(0, 1))
        ));
    }

    #[test]
    fn identity_pow_one_and_zero() {
        let p1 = normalize_expression(&pow_expr(dx(), num_expr(1)), None).expect("normalize");
        assert!(matches!(p1.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        // `x ^ 0` must stay a runtime power: folding it to `1` would erase a
        // missing-data veto for `x`.
        let p0 = normalize_expression(&pow_expr(dx(), num_expr(0)), None).expect("normalize");
        let ExpressionKind::Arithmetic(base, ArithmeticComputation::Power, exp) = p0.kind else {
            panic!("expected preserved power, got {:?}", p0.kind);
        };
        assert!(matches!(base.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        assert!(matches!(
            exp.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(0, 1))
        ));
        // A total, nonzero literal base still folds: `7 ^ 0 → 1`.
        let literal_pow_zero =
            normalize_expression(&pow_expr(num_expr(7), num_expr(0)), None).expect("normalize");
        assert!(matches!(
            literal_pow_zero.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(1, 1))
        ));
    }

    #[test]
    fn double_logical_negation() {
        let norm =
            normalize_expression(&not_expr(not_expr(bool_expr(true))), None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(true))
        ));
    }

    #[test]
    fn double_numeric_negation() {
        let zero = num_expr(0);
        let inner = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(zero.clone()),
                ArithmeticComputation::Subtract,
                Arc::new(dx()),
            ),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(zero),
                ArithmeticComputation::Subtract,
                Arc::new(inner),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn double_reciprocal_with_data_path_is_not_collapsed() {
        // `1 / (1 / x)` must stay nested: collapsing it to `x` would erase
        // the inner division-by-zero veto for `x = 0`.
        let one = num_expr(1);
        let inner = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(one.clone()),
                ArithmeticComputation::Divide,
                Arc::new(dx()),
            ),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(one),
                ArithmeticComputation::Divide,
                Arc::new(inner),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::Arithmetic(_, ArithmeticComputation::Divide, outer_denominator) =
            norm.kind
        else {
            panic!("expected preserved outer division, got {:?}", norm.kind);
        };
        let ExpressionKind::Arithmetic(_, ArithmeticComputation::Divide, inner_denominator) =
            &outer_denominator.kind
        else {
            panic!(
                "expected preserved inner division, got {:?}",
                outer_denominator.kind
            );
        };
        assert!(matches!(inner_denominator.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn sqrt_squared_numeric_folds_to_exact_rational() {
        let sqrt2 = Expression::with_source(
            ExpressionKind::MathematicalComputation(
                MathematicalComputation::Sqrt,
                Arc::new(num_expr(2)),
            ),
            None,
        );
        let expr = pow_expr(sqrt2, num_expr(2));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(2, 1))
        ));
    }

    /// `2^(1/2)` has no exact rational result: constant_fold must not substitute a decimal.
    #[test]
    fn literal_power_with_irrational_result_stays_power() {
        let half = Expression::with_source(
            ExpressionKind::Literal(Box::new(
                literal_from_folded_rational(rational_new(1, 2), None)
                    .expect("BUG: literal 1/2 must commit at normalize"),
            )),
            None,
        );
        let expr = pow_expr(num_expr(2), half);
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(
            matches!(
                norm.kind,
                ExpressionKind::Arithmetic(_, ArithmeticComputation::Power, _)
            ),
            "expected symbolic Power, got {:?}",
            norm.kind
        );
    }

    #[test]
    fn negated_conjunction_preserved_for_data_paths() {
        // Non-total conjunctions under negation must not be rewritten:
        // AND propagates every conjunct's veto, so algebraic rewrites change
        // veto observability.
        fn dy() -> Expression {
            Expression::with_source(
                ExpressionKind::DataPath(DataPath::new(vec![], "y".into())),
                None,
            )
        }
        let expr = not_expr(and_expr(dx(), dy()));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::LogicalNegation(inner, _) = norm.kind else {
            panic!("expected preserved negation, got {:?}", norm.kind);
        };
        assert!(
            matches!(inner.kind, ExpressionKind::LogicalAnd(_, _)),
            "expected preserved conjunction under the negation, got {:?}",
            inner.kind
        );
    }

    #[test]
    fn power_law_nested_power() {
        let expr = pow_expr(pow_expr(dx(), num_expr(2)), num_expr(3));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Arithmetic(_, ArithmeticComputation::Power, _)
        ));
        let ExpressionKind::Arithmetic(base, ArithmeticComputation::Power, exp) = norm.kind else {
            unreachable!();
        };
        assert!(matches!(
            base.kind,
            ExpressionKind::DataPath(ref p) if p.data == "x"
        ));
        assert!(matches!(
            exp.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(6, 1))
        ));
    }

    #[test]
    fn power_law_like_base_product() {
        let expr = mul_expr(pow_expr(dx(), num_expr(2)), pow_expr(dx(), num_expr(3)));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::Arithmetic(base, ArithmeticComputation::Power, exp) = norm.kind else {
            panic!("expected single power, got {:?}", norm.kind);
        };
        assert!(matches!(base.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        assert!(matches!(
            exp.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(5, 1))
        ));
    }

    #[test]
    fn negated_comparison_not_less_than() {
        let cmp = lt_expr(dx(), num_expr(0));
        let norm = normalize_expression(&not_expr(cmp), None).expect("normalize");
        let ExpressionKind::Comparison(_, op, _) = norm.kind else {
            panic!("expected Comparison, got {:?}", norm.kind);
        };
        assert_eq!(op, ComparisonComputation::GreaterThanOrEqual);
    }

    #[test]
    fn logical_short_circuit_and_false_with_data_path_is_not_folded() {
        // `false and x` must stay a conjunction: folding it to `false` would
        // erase a missing-data veto for `x`.
        let expr = and_expr(bool_expr(false), dx());
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::LogicalAnd(left, right) = norm.kind else {
            panic!("expected preserved conjunction, got {:?}", norm.kind);
        };
        assert!(matches!(
            left.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(false))
        ));
        assert!(matches!(right.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn logical_short_circuit_and_false_all_literal_folds() {
        let expr = and_expr(bool_expr(false), bool_expr(true));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(false))
        ));
    }

    #[test]
    fn logical_idempotency_and_duplicate_paths() {
        let a = dx();
        let expr = and_expr(a.clone(), a);
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn unit_conversion_fold_number_to_number() {
        let inner = num_expr(42);
        let expr = Expression::with_source(
            ExpressionKind::UnitConversion(
                Arc::new(inner),
                SemanticConversionTarget::Type(PrimitiveKind::Number),
            ),
            None,
        );
        let ctx = UnitResolutionContext::NamedMeasureOnly;
        let norm = normalize_expression(&expr, Some(&ctx)).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(42, 1))
        ));
    }

    #[test]
    fn piecewise_single_branch() {
        let branches = vec![(None as Option<Expression>, num_expr(9))];
        let expr = unless_branches_to_piecewise(&branches);
        assert!(matches!(expr.kind, ExpressionKind::Literal(_)));
    }

    #[test]
    fn piecewise_unless_arms_in_source_order() {
        let c_big = lt_expr(num_expr(10), dx());
        let c_small = lt_expr(num_expr(5), dx());
        let branches = vec![
            (None, num_expr(0)),
            (Some(c_big.clone()), num_expr(100)),
            (Some(c_small.clone()), num_expr(50)),
        ];
        let ExpressionKind::Piecewise(arms) = unless_branches_to_piecewise(&branches).kind else {
            panic!("expected Piecewise for unless chain");
        };
        assert_eq!(arms.len(), 3);
        assert!(matches!(
            arms[0].0.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Boolean(true))
        ));
        assert!(
            matches!(arms[0].1.kind, ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Number(ref n) if n == &rational_new(0, 1)))
        );
        assert!(matches!(
            arms[1].0.kind,
            ExpressionKind::Comparison(_, _, _)
        ));
        assert!(
            matches!(arms[1].1.kind, ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Number(ref n) if n == &rational_new(100, 1)))
        );
        assert!(matches!(
            arms[2].0.kind,
            ExpressionKind::Comparison(_, _, _)
        ));
        assert!(
            matches!(arms[2].1.kind, ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Number(ref n) if n == &rational_new(50, 1)))
        );
    }

    #[test]
    fn piecewise_collapse_last_true_unless_keeps_only_that_result() {
        let branches = vec![(None, num_expr(0)), (Some(bool_expr(true)), num_expr(42))];
        let piecewise = unless_branches_to_piecewise(&branches);
        let norm = normalize_expression(&piecewise, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, ValueKind::Number(ref n) if n == &rational_new(42, 1))
        ));
    }

    #[test]
    fn collapsed_unless_false_retains_piecewise_origin() {
        let branches = vec![(None, num_expr(1)), (Some(bool_expr(false)), num_expr(2))];
        let piecewise = unless_branches_to_piecewise(&branches);
        let mut interner = NormalFormInterner::new();
        let completed_rules = HashMap::new();
        let rule_target_data = HashMap::new();
        let lower = LowerCtx {
            completed_rules: &completed_rules,
            rule_target_data: &rule_target_data,
        };
        let root = to_normal_form(&piecewise, &mut interner, &lower);
        let nf = normalize_once(root, &mut interner, None, None).expect("normalize");
        let cell = interner.get(nf);
        assert!(
            cell.origin.is_some(),
            "collapsed unless must keep piecewise origin, kind={:?}",
            cell.kind
        );
        let origin = cell.origin.expect("origin");
        assert!(
            matches!(interner.get(origin).kind, NormalFormKind::Piecewise(_)),
            "origin must be piecewise, got {:?}",
            interner.get(origin).kind
        );
    }

    #[test]
    fn piecewise_collapse_drops_arm_with_statically_false_condition() {
        let branches = vec![
            (None, num_expr(7)),
            (Some(lt_expr(num_expr(1), num_expr(0))), num_expr(99)),
        ];
        let piecewise = unless_branches_to_piecewise(&branches);
        let norm = normalize_expression(&piecewise, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, ValueKind::Number(ref n) if n == &rational_new(7, 1))
        ));
    }

    #[test]
    fn constant_fold_divide_of_literals_to_number() {
        let expr = div_expr(num_expr(5), num_expr(2));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, ValueKind::Number(ref n) if n == &rational_new(5, 2))
        ));
    }

    #[test]
    fn constant_fold_divide_comparison_to_true() {
        let expr = lt_expr(div_expr(num_expr(5), num_expr(2)), num_expr(4));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Boolean(true))
        ));
    }

    #[test]
    fn constant_fold_nested_arithmetic_comparison_to_true() {
        // (1 + 2) * 3 / 4 < 10
        let expr = lt_expr(
            div_expr(
                mul_expr(add_expr(num_expr(1), num_expr(2)), num_expr(3)),
                num_expr(4),
            ),
            num_expr(10),
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Boolean(true))
        ));
    }

    #[test]
    fn power_laws_sqrt_squared_is_not_merged_for_data_paths() {
        let x = Expression::with_source(
            ExpressionKind::DataPath(DataPath::new(vec![], "x".into())),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(Expression::with_source(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Sqrt,
                        Arc::new(x.clone()),
                    ),
                    None,
                )),
                ArithmeticComputation::Multiply,
                Arc::new(Expression::with_source(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Sqrt,
                        Arc::new(x),
                    ),
                    None,
                )),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        let mut interner = NormalFormInterner::new();
        let id = to_normal_form_empty(&norm, &mut interner);
        assert!(
            matches!(&interner.get(id).kind, NormalFormKind::Product(ref children) if children.len() == 2),
            "sqrt(x)*sqrt(x) must stay a product of two powers, got {:?}",
            interner.get(id).kind
        );
    }

    /// `sqrt_two * sqrt_two` (each factor inlined with `rule_origin = sqrt_two`)
    /// folds to `Leaf(2)` and records the full fold chain as provenance:
    #[test]
    fn exp_log_not_folded_for_data_path() {
        // `exp (log x)` must stay nested: folding it to `x` would erase the
        // `log` domain veto for `x <= 0`.
        let inner = Expression::with_source(
            ExpressionKind::MathematicalComputation(MathematicalComputation::Log, Arc::new(dx())),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::MathematicalComputation(MathematicalComputation::Exp, Arc::new(inner)),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::MathematicalComputation(MathematicalComputation::Exp, inner) =
            norm.kind
        else {
            panic!("expected preserved exp, got {:?}", norm.kind);
        };
        assert!(
            matches!(
                inner.kind,
                ExpressionKind::MathematicalComputation(MathematicalComputation::Log, _)
            ),
            "expected preserved log under exp, got {:?}",
            inner.kind
        );
    }

    #[test]

    fn constant_fold_add() {
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(num_expr(2)),
                ArithmeticComputation::Add,
                Arc::new(num_expr(3)),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(5, 1))
        ));
    }
}
