//! Tree evaluator: walks drift-free [`NormalForm`] cells by [`NormalFormId`].
//!
//! When explanation is requested, the same walk builds Compose/Data nodes
//! from Kind. Values always come from Kind.

use crate::computation::arithmetic::expand_signature_to_base_units;
use crate::computation::{
    arithmetic_operation, comparison_operation, convert_unit_operand, OperationResult,
    UnitResolutionContext, VetoType,
};
use crate::evaluation::branch_semantics::{condition_outcome, BranchOutcome};
use crate::evaluation::explanations::{format_operation_result, Explanation};
use crate::evaluation::expression::{evaluate_mathematical_operator, resolve_data_path_value};
use crate::evaluation::EvaluationContext;
use crate::planning::execution_plan::{ExecutableRule, ExecutionPlan};
use crate::planning::explanation::{Cause, ExplanationNode};
use crate::planning::normalize::{explanation_display, LeafKind, NormalFormId, NormalFormKind};
use crate::planning::ordered_dispatch::{
    dispatch_probe_of, region_count, region_for_value, DispatchKey, DispatchProbeOutcome,
};
use crate::planning::semantics::{
    negated_comparison, ArithmeticComputation, DataPath, LemmaType, LiteralValue, RulePath,
    ValueKind,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn borrow_value<'a>(result: &'a OperationResult, operand: &str) -> &'a LiteralValue {
    match result {
        OperationResult::Value(v) => v,
        OperationResult::Veto(_) => panic!("BUG: {operand} passed veto check but has no value"),
    }
}

/// Promote anonymous / compound measure types via the plan signature index, or
/// via a unique named type in the plan unit index that shares the decomposition.
fn resolve_measure_type_for_magnitude_math(
    plan: &ExecutionPlan,
    operand_type: &Arc<LemmaType>,
) -> Arc<LemmaType> {
    if !operand_type.is_measure() {
        return Arc::clone(operand_type);
    }
    let signature = operand_type.measure_runtime_signature();
    if let Some((unit_name, named)) = plan.signature_index.get(&signature) {
        return Arc::new(
            named
                .as_ref()
                .clone()
                .with_measure_binding_unit(unit_name.clone()),
        );
    }
    let owners = [operand_type.as_ref()];
    let expanded =
        expand_signature_to_base_units(&signature, plan.expression_unit_index(), &owners);
    if let Some((unit_name, named)) = plan.signature_index.get(&expanded) {
        return Arc::new(
            named
                .as_ref()
                .clone()
                .with_measure_binding_unit(unit_name.clone()),
        );
    }
    if operand_type.is_anonymous_measure() {
        if let Some(decomp) = operand_type.measure_type_decomposition() {
            if !decomp.is_empty() {
                let mut unique: Option<Arc<LemmaType>> = None;
                for candidate in plan.resolved_types.unit_index.values() {
                    if !matches!(
                        candidate.specifications,
                        crate::planning::semantics::TypeSpecification::Measure { .. }
                    ) {
                        continue;
                    }
                    if candidate.measure_type_decomposition() != Some(decomp) {
                        continue;
                    }
                    match &unique {
                        None => unique = Some(Arc::clone(candidate)),
                        Some(existing) if existing.name() == candidate.name() => {}
                        Some(_) => {
                            unique = None;
                            break;
                        }
                    }
                }
                if let Some(named) = unique {
                    return named;
                }
            }
        }
    }
    Arc::clone(operand_type)
}

fn own_literal(result: OperationResult, operand: &str) -> LiteralValue {
    match result {
        OperationResult::Value(v) => v,
        OperationResult::Veto(_) => panic!("BUG: {operand} passed veto check but is vetoed"),
    }
}

fn now_date(ctx: &EvaluationContext) -> &crate::planning::semantics::SemanticDateTime {
    match &ctx.now().value {
        ValueKind::Date(dt) => dt,
        other => panic!("BUG: context.now() must be a date, got {other:?}"),
    }
}

/// Result of evaluating a subtree, optionally with explanation material.
pub(crate) struct Explained {
    pub result: OperationResult,
    pub body: String,
    pub causes: Vec<Cause>,
    pub children: Vec<ExplanationNode>,
    /// How this subtree appears as an operand of a parent Compose/Rule.
    pub as_operand: Option<ExplanationNode>,
}

impl Explained {
    fn value_only(result: OperationResult) -> Self {
        Self {
            result,
            body: String::new(),
            causes: Vec::new(),
            children: Vec::new(),
            as_operand: None,
        }
    }
}

/// Evaluate one rule by walking its root [`NormalFormId`] (values only).
///
/// Stores the result in [`EvaluationContext::rule_values`] at this rule's
/// plan index so later embeds read it instead of re-entering the body.
pub(crate) fn evaluate_rule(
    rule: &ExecutableRule,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> OperationResult {
    let result = evaluate_id(rule.normal_form, plan, ctx);
    let index = plan.rules.get_index_of(&rule.path).unwrap_or_else(|| {
        panic!(
            "BUG: rule '{}' missing from execution plan after evaluate_rule",
            rule.path.rule
        )
    });
    ctx.rule_values[index] = Some(result.clone());
    result
}

/// Evaluate one rule while building its explanation (single walk).
///
/// Dependency values and explanations must already be in
/// [`EvaluationContext::rule_values`] / `rule_explanations`. Asserts that the
/// explain walk agrees with the stored value result.
pub(crate) fn evaluate_rule_explained(
    rule: &ExecutableRule,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> (OperationResult, Explanation) {
    let stored = ctx.rule_value(plan, &rule.path).clone();
    let explained = evaluate_explained(rule.normal_form, plan, ctx);
    assert_eq!(
        &explained.result, &stored,
        "BUG: explain walk for '{}' disagreed with stored rule value",
        rule.path.rule
    );
    let result = explained.result.clone();
    let children = explained.children;
    let result_type = ctx.rule_result_type(plan, rule);

    let node = ExplanationNode::Rule {
        name: rule.path.clone(),
        result: Some(format_operation_result(&result, result_type.as_ref())),
        body: explained.body.clone(),
        causes: explained.causes.clone(),
        children: children.clone(),
    };
    ctx.rule_explanations.insert(rule.path.clone(), node);

    (
        result.clone(),
        Explanation {
            name: rule.path.clone(),
            result,
            result_type,
            body: explained.body,
            causes: explained.causes,
            children,
        },
    )
}

/// Ensure `rule_path` (and its plan dependencies) are in `rule_explanations`.
///
/// Uses an explicit heap stack so a long rule chain does not grow the Rust
/// call stack. Pending deps come from [`ExecutableRule::depends_on_rules`].
pub(crate) fn ensure_rule_explained(
    rule_path: &RulePath,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) {
    if ctx.rule_explanations.contains_key(rule_path) {
        return;
    }

    let mut stack = vec![rule_path.clone()];
    while let Some(current) = stack.last().cloned() {
        if ctx.rule_explanations.contains_key(&current) {
            stack.pop();
            continue;
        }

        let rule = plan.rules.get(&current).unwrap_or_else(|| {
            panic!(
                "BUG: rule embed path '{}' missing from execution plan",
                current.rule
            )
        });

        let mut pending = None;
        for dep in &rule.depends_on_rules {
            if !ctx.rule_explanations.contains_key(dep) {
                pending = Some(dep.clone());
                break;
            }
        }

        if let Some(dep) = pending {
            if stack.iter().any(|p| p == &dep) {
                panic!(
                    "BUG: cyclic rule embed while ensuring explain for '{}'",
                    current.rule
                );
            }
            stack.push(dep);
            continue;
        }

        let explanation = evaluate_rule_explained(rule, plan, ctx).1;
        if explanation.name != current {
            panic!(
                "BUG: on-demand explain for '{}' stored explanation for '{}'",
                current.rule, explanation.name.rule
            );
        }
        stack.pop();
    }
}

/// Explain-mode Rule embed: stored rule value, narration from cached Rule node.
fn embed_rule_explained(
    rule_path: &RulePath,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> Explained {
    let result = ctx.rule_value(plan, rule_path).clone();
    let node = ctx
        .rule_explanations
        .get(rule_path)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "BUG: dependency '{}' not explained before its use-site",
                rule_path.rule
            )
        });
    Explained {
        result,
        body: rule_path.rule.clone(),
        causes: Vec::new(),
        children: vec![node.clone()],
        as_operand: Some(node),
    }
}

pub(crate) fn evaluate_id(
    id: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> OperationResult {
    eval(id, plan, ctx, false).result
}

/// Evaluate a normal-form cell while building explanation material.
pub(crate) fn evaluate_explained(
    id: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> Explained {
    eval(id, plan, ctx, true)
}

fn eval(
    id: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
) -> Explained {
    if !explain {
        if let Some(cached) = ctx.values[id.index()].as_ref() {
            return Explained::value_only(cached.clone());
        }
    }
    let explained = eval_uncached(id, plan, ctx, explain);
    if !explain {
        // Unbound data leaves must stay None so missing_data walks still see
        // them as unbound. MissingData from a DataPath leaf is not a binding.
        let store = !matches!(
            (&plan.normal_form(id).kind, &explained.result),
            (
                NormalFormKind::Leaf(LeafKind::DataPath(_)),
                OperationResult::Veto(VetoType::MissingData { .. }),
            )
        );
        if store {
            ctx.values[id.index()] = Some(explained.result.clone());
        }
    }
    explained
}

fn eval_uncached(
    id: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
) -> Explained {
    if explain {
        if let Some(origin) = plan.normal_form(id).origin {
            // Match on the *current* cell. OrderedDispatch narrates by replaying its
            // Piecewise pre-image. Any other cell whose origin is still a Piecewise
            // (including a literal left after collapse_piecewise) uses the record path
            // so dead arms become data_unused without re-evaluating them.
            match &plan.normal_form(id).kind {
                NormalFormKind::OrderedDispatch {
                    scrutinee,
                    boundaries,
                    regions,
                } => {
                    return explain_ordered_dispatch(
                        id, origin, *scrutinee, boundaries, regions, plan, ctx,
                    );
                }
                _ if matches!(&plan.normal_form(origin).kind, NormalFormKind::Piecewise(_)) => {
                    return explain_with_piecewise_origin(id, origin, plan, ctx);
                }
                _ => {
                    let value = eval(id, plan, ctx, false);
                    let mut explained = eval(origin, plan, ctx, true);
                    explained.result = value.result;
                    return explained;
                }
            }
        }
    }

    if let Some(path) = plan.normal_form(id).rule_embed.clone() {
        if explain {
            return embed_rule_explained(&path, plan, ctx);
        }
        return Explained::value_only(ctx.rule_value(plan, &path).clone());
    }

    eval_cell(id, plan, ctx, explain)
}

fn eval_cell(
    id: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
) -> Explained {
    if let NormalFormKind::Piecewise(arms) = &plan.normal_form(id).kind {
        return evaluate_piecewise(arms, plan, ctx, explain);
    }
    eval_kind(id, plan, ctx, explain)
}

/// Value and body from current shape; causes from the piecewise origin record.
fn explain_with_piecewise_origin(
    current_id: NormalFormId,
    origin_id: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> Explained {
    let value = eval(current_id, plan, ctx, false);
    let NormalFormKind::Piecewise(recorded) = &plan.normal_form(origin_id).kind else {
        panic!("BUG: explain_with_piecewise_origin requires Piecewise origin");
    };
    let recorded = recorded.clone();
    let causes = piecewise_causes_from_record(&recorded, plan, ctx);

    let mut body = match &plan.normal_form(current_id).kind {
        NormalFormKind::Piecewise(kept) => evaluate_piecewise(kept, plan, ctx, true),
        _ => eval_kind(current_id, plan, ctx, true),
    };
    body.result = value.result;
    body.causes = causes;
    body
}

fn is_bool_leaf(plan: &ExecutionPlan, id: NormalFormId) -> Option<bool> {
    match &plan.normal_form(id).kind {
        NormalFormKind::Leaf(LeafKind::Literal(lit)) => match &lit.value {
            ValueKind::Boolean(b) => Some(*b),
            _ => None,
        },
        _ => None,
    }
}

/// Peel bool-leaf origins until Comparison, Not, And, or a non-bool-leaf.
fn peel_bool_leaf_origins(condition: NormalFormId, plan: &ExecutionPlan) -> NormalFormId {
    let mut id = condition;
    loop {
        if is_bool_leaf(plan, id).is_none() {
            return id;
        }
        let Some(origin) = plan.normal_form(id).origin else {
            return id;
        };
        match &plan.normal_form(origin).kind {
            NormalFormKind::Comparison(_, _, _)
            | NormalFormKind::Not(_)
            | NormalFormKind::And(_) => return origin,
            NormalFormKind::Leaf(LeafKind::Literal(lit))
                if matches!(lit.value, ValueKind::Boolean(_)) =>
            {
                id = origin;
            }
            _ => return origin,
        }
    }
}

/// Collect DataPath leaves under a condition graph (structural narration sources).
fn structural_data_paths_from(id: NormalFormId, plan: &ExecutionPlan) -> Vec<DataPath> {
    let mut out = Vec::new();
    let mut stack = vec![id];
    let mut seen = HashSet::new();
    while let Some(current) = stack.pop() {
        if !seen.insert(current.index()) {
            continue;
        }
        match &plan.normal_form(current).kind {
            NormalFormKind::Leaf(LeafKind::DataPath(path)) => {
                out.push(path.clone());
            }
            NormalFormKind::And(children)
            | NormalFormKind::Sum(children)
            | NormalFormKind::Product(children) => {
                stack.extend(children.iter().copied());
            }
            NormalFormKind::Not(x)
            | NormalFormKind::Negate(x)
            | NormalFormKind::Reciprocal(x)
            | NormalFormKind::MathOp(_, x)
            | NormalFormKind::ResultIsVeto(x)
            | NormalFormKind::UnitConversion(x, _)
            | NormalFormKind::DateRelative(_, x)
            | NormalFormKind::DateCalendar(_, _, x)
            | NormalFormKind::PastFutureRange(_, x) => stack.push(*x),
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
            NormalFormKind::Piecewise(arms) => {
                for (c, r) in arms {
                    stack.push(*c);
                    stack.push(*r);
                }
            }
            NormalFormKind::OrderedDispatch {
                scrutinee, regions, ..
            } => {
                stack.push(*scrutinee);
                stack.extend(regions.iter().copied());
            }
            NormalFormKind::Leaf(_) | NormalFormKind::Veto(_) | NormalFormKind::Now => {}
        }
        if let Some(origin) = plan.normal_form(current).origin {
            stack.push(origin);
        }
    }
    out
}

/// Bound `Data` nodes already present in an evaluated operand tree, keyed by path.
fn collect_evaluated_data(node: &ExplanationNode, out: &mut HashMap<DataPath, ExplanationNode>) {
    match node {
        ExplanationNode::Data { name, display } => {
            out.entry(name.clone())
                .or_insert_with(|| ExplanationNode::Data {
                    name: name.clone(),
                    display: display.clone(),
                });
        }
        ExplanationNode::Compose { operands, .. }
        | ExplanationNode::Conversion { operands, .. } => {
            for operand in operands {
                collect_evaluated_data(operand, out);
            }
        }
        ExplanationNode::Rule {
            causes, children, ..
        } => {
            for cause in causes {
                for child in &cause.children {
                    collect_evaluated_data(child, out);
                }
            }
            for child in children {
                collect_evaluated_data(child, out);
            }
        }
        ExplanationNode::DataUnused { .. }
        | ExplanationNode::Veto { .. }
        | ExplanationNode::Piecewise { .. } => {}
    }
}

/// Bound display for a path only when the context already has a value or veto.
/// Absent paths stay data_unused — do not invent Missing-data text for structural mentions.
fn bound_data_from_context(
    path: &DataPath,
    plan: &ExecutionPlan,
    ctx: &EvaluationContext,
) -> Option<ExplanationNode> {
    ctx.data_slot(plan, path)?;
    let result = resolve_data_path_value(path, plan, ctx);
    let data_type = plan
        .data
        .get(path)
        .and_then(|def| def.schema_type())
        .expect("BUG: bound data path missing schema type");
    let display = match &result {
        OperationResult::Value(v) => v.display_value_with_type(data_type),
        OperationResult::Veto(_) => format_operation_result(&result, data_type),
    };
    Some(ExplanationNode::Data {
        name: path.clone(),
        display,
    })
}

/// And-cause children: evaluated bound nodes first, then context bindings, else data_unused.
fn and_cause_children(
    focus: NormalFormId,
    cond_operand: Option<&ExplanationNode>,
    plan: &ExecutionPlan,
    ctx: &EvaluationContext,
) -> Vec<ExplanationNode> {
    let mut evaluated = HashMap::new();
    if let Some(operand) = cond_operand {
        collect_evaluated_data(operand, &mut evaluated);
    }

    let mut children = Vec::new();
    let mut seen = HashSet::new();
    for path in structural_data_paths_from(focus, plan) {
        if !seen.insert(path.clone()) {
            continue;
        }
        if let Some(node) = evaluated.remove(&path) {
            children.push(node);
        } else if let Some(node) = bound_data_from_context(&path, plan, ctx) {
            children.push(node);
        } else {
            children.push(ExplanationNode::DataUnused { name: path });
        }
    }
    children
}

fn cause_from_record_condition(
    condition: NormalFormId,
    held: bool,
    plan: &ExecutionPlan,
    ctx: &EvaluationContext,
) -> Cause {
    let focus = peel_bool_leaf_origins(condition, plan);
    let (condition_text, value) = condition_statement_from_id(focus, held, plan);
    let children = match &plan.normal_form(focus).kind {
        NormalFormKind::And(_) => and_cause_children(focus, None, plan, ctx),
        _ => {
            if let Some(origin) = plan.normal_form(condition).origin {
                if matches!(plan.normal_form(origin).kind, NormalFormKind::And(_)) {
                    and_cause_children(origin, None, plan, ctx)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        }
    };
    Cause {
        condition: condition_text,
        value,
        children,
    }
}

/// Last-match-wins causes from a piecewise record without live-eval of static bool leaves.
///
/// Winner is the last Taken unless-arm (reverse scan), or the default. Dead causes are
/// NotTaken arms with index strictly below the winner (earlier in source). Shadowed
/// earlier Taken arms are omitted. When the winner is a static `true` leaf, earlier
/// NotTaken arms are omitted too — collapse already pruned from that arm onward.
fn piecewise_causes_from_record(
    arms: &[(NormalFormId, NormalFormId)],
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> Vec<Cause> {
    assert!(!arms.is_empty(), "BUG: empty piecewise record");

    let mut winner: Option<usize> = None;
    for i in (1..arms.len()).rev() {
        match record_condition_outcome(arms[i].0, plan, ctx) {
            BranchOutcome::Taken => {
                winner = Some(i);
                break;
            }
            BranchOutcome::NotTaken | BranchOutcome::Propagate(_) => {}
        }
    }

    let mut causes = Vec::new();
    let winner_is_static_true = winner.is_some_and(|i| is_bool_leaf(plan, arms[i].0) == Some(true));
    if !winner_is_static_true {
        let before_winner = winner.unwrap_or(arms.len());
        for (condition, _) in arms.iter().take(before_winner).skip(1) {
            match record_condition_outcome(*condition, plan, ctx) {
                BranchOutcome::NotTaken => {
                    causes.push(cause_from_record_condition(*condition, false, plan, ctx));
                }
                BranchOutcome::Taken | BranchOutcome::Propagate(_) => {}
            }
        }
    }
    if let Some(i) = winner {
        causes.push(cause_from_record_condition(arms[i].0, true, plan, ctx));
    }
    causes
}

fn record_condition_outcome(
    condition: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> BranchOutcome {
    if let Some(boolean) = is_bool_leaf(plan, condition) {
        if boolean {
            BranchOutcome::Taken
        } else {
            BranchOutcome::NotTaken
        }
    } else {
        let cond_result = eval(condition, plan, ctx, false).result;
        condition_outcome(&cond_result)
    }
}

fn evaluate_piecewise(
    arms: &[(NormalFormId, NormalFormId)],
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
) -> Explained {
    assert!(!arms.is_empty(), "BUG: empty piecewise");

    let mut not_taken: Vec<(usize, Cause)> = Vec::new();

    for i in (1..arms.len()).rev() {
        let (condition, body) = arms[i];
        let cond_e = eval(condition, plan, ctx, explain);
        match condition_outcome(&cond_e.result) {
            BranchOutcome::Propagate(result) => {
                if !explain {
                    return Explained::value_only(result);
                }
                let node = ExplanationNode::Veto {
                    message: Some(format_operation_result(
                        &result,
                        plan.result_type(condition).as_ref(),
                    )),
                };
                return Explained {
                    result,
                    body: explanation_display(plan.normal_forms.as_slice(), condition),
                    causes: Vec::new(),
                    children: vec![node],
                    as_operand: None,
                };
            }
            BranchOutcome::Taken => {
                let body_e = eval(body, plan, ctx, explain);
                if !explain {
                    return Explained::value_only(body_e.result);
                }
                let taken = cause_from_condition_id(condition, true, cond_e.as_operand, plan, ctx);
                let mut causes: Vec<Cause> = not_taken
                    .into_iter()
                    .filter(|(idx, _)| *idx < i)
                    .map(|(_, c)| c)
                    .collect();
                causes.reverse();
                causes.push(taken);
                return finish_piecewise(body_e, causes, true);
            }
            BranchOutcome::NotTaken => {
                if explain {
                    not_taken.push((
                        i,
                        cause_from_condition_id(condition, false, cond_e.as_operand, plan, ctx),
                    ));
                }
            }
        }
    }

    let body_e = eval(arms[0].1, plan, ctx, explain);
    if !explain {
        return Explained::value_only(body_e.result);
    }
    not_taken.reverse();
    let causes = not_taken.into_iter().map(|(_, c)| c).collect();
    finish_piecewise(body_e, causes, true)
}

/// Explain an [`NormalFormKind::OrderedDispatch`]: value from the table, narration from
/// the Piecewise origin. Before narrating, assert the Piecewise winner body is the
/// same cell the table selected — two decision procedures must not disagree.
fn explain_ordered_dispatch(
    id: NormalFormId,
    origin: NormalFormId,
    scrutinee: NormalFormId,
    boundaries: &[DispatchKey],
    regions: &[NormalFormId],
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> Explained {
    let value = eval(id, plan, ctx, false);
    if !value.result.vetoed() {
        let selected = dispatch_selected_body(scrutinee, boundaries, regions, plan, ctx);
        let NormalFormKind::Piecewise(arms) = &plan.normal_form(origin).kind else {
            panic!("BUG: OrderedDispatch origin must be Piecewise");
        };
        let winner_body = piecewise_winner_body(arms, plan, ctx);
        assert_eq!(
            selected, winner_body,
            "BUG: OrderedDispatch region disagrees with Piecewise origin"
        );
    }
    let mut explained = eval(origin, plan, ctx, true);
    explained.result = value.result;
    explained
}

/// Body id the dispatch table selects for the current scrutinee value.
///
/// Scrutinee must already have evaluated to a non-veto value (caller checked).
fn dispatch_selected_body(
    scrutinee: NormalFormId,
    boundaries: &[DispatchKey],
    regions: &[NormalFormId],
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> NormalFormId {
    assert_eq!(
        regions.len(),
        region_count(boundaries.len()),
        "BUG: OrderedDispatch region table does not match its boundary list"
    );
    let scrutinee_e = eval(scrutinee, plan, ctx, false);
    let value = borrow_value(&scrutinee_e.result, "dispatch scrutinee for region check");
    let probe = match dispatch_probe_of(&value.value) {
        DispatchProbeOutcome::Probe(probe) => probe,
        DispatchProbeOutcome::CalendarFailure(message) => {
            panic!(
                "BUG: OrderedDispatch region check saw calendar failure after non-veto value: {message}"
            );
        }
        DispatchProbeOutcome::Unsupported => panic!(
            "BUG: OrderedDispatch scrutinee evaluated to {:?}, a kind the fold excludes",
            value.value
        ),
    };
    let region = region_for_value(boundaries, &probe).unwrap_or_else(|failure| {
        panic!(
            "BUG: OrderedDispatch region check saw numeric failure after non-veto value: {failure}"
        );
    });
    regions[region]
}

/// Body id the Piecewise reverse scan selects (default when no unless arm is Taken).
fn piecewise_winner_body(
    arms: &[(NormalFormId, NormalFormId)],
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> NormalFormId {
    assert!(!arms.is_empty(), "BUG: empty piecewise record");
    for i in (1..arms.len()).rev() {
        match record_condition_outcome(arms[i].0, plan, ctx) {
            BranchOutcome::Taken => return arms[i].1,
            BranchOutcome::NotTaken | BranchOutcome::Propagate(_) => {}
        }
    }
    arms[0].1
}

/// Value of an [`NormalFormKind::OrderedDispatch`] cell: evaluate the scrutinee once,
/// binary-search its region, evaluate that region's result.
///
/// Value mode only. Explanation routes through [`explain_ordered_dispatch`].
fn evaluate_ordered_dispatch(
    scrutinee: NormalFormId,
    boundaries: &[DispatchKey],
    regions: &[NormalFormId],
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
) -> Explained {
    assert!(
        !explain,
        "BUG: OrderedDispatch narrated directly; explain mode must route through explain_ordered_dispatch"
    );
    assert_eq!(
        regions.len(),
        region_count(boundaries.len()),
        "BUG: OrderedDispatch region table does not match its boundary list"
    );

    let scrutinee_e = eval(scrutinee, plan, ctx, false);
    if scrutinee_e.result.vetoed() {
        return scrutinee_e;
    }
    let value = borrow_value(&scrutinee_e.result, "dispatch scrutinee");
    let probe = match dispatch_probe_of(&value.value) {
        DispatchProbeOutcome::Probe(probe) => probe,
        DispatchProbeOutcome::CalendarFailure(message) => {
            return Explained::value_only(OperationResult::Veto(VetoType::computation(message)));
        }
        DispatchProbeOutcome::Unsupported => panic!(
            "BUG: OrderedDispatch scrutinee evaluated to {:?}, a kind the fold excludes",
            value.value
        ),
    };
    let region = match region_for_value(boundaries, &probe) {
        Ok(region) => region,
        Err(failure) => {
            return Explained::value_only(OperationResult::Veto(VetoType::computation(
                failure.to_string(),
            )));
        }
    };

    let selected = regions[region];
    Explained::value_only(eval(selected, plan, ctx, false).result)
}

fn cause_children(as_operand: Option<ExplanationNode>) -> Vec<ExplanationNode> {
    match as_operand {
        // Bare data ref is fully stated in the condition text.
        Some(ExplanationNode::Data { .. } | ExplanationNode::DataUnused { .. }) => Vec::new(),
        Some(ExplanationNode::Compose { operands, .. }) => operands
            .into_iter()
            .filter(|n| {
                matches!(
                    n,
                    ExplanationNode::Data { .. }
                        | ExplanationNode::DataUnused { .. }
                        | ExplanationNode::Rule { .. }
                        | ExplanationNode::Conversion { .. }
                )
            })
            .collect(),
        Some(node @ ExplanationNode::Rule { .. }) => vec![node],
        Some(node @ ExplanationNode::Conversion { .. }) => vec![node],
        _ => Vec::new(),
    }
}

fn finish_piecewise(body: Explained, causes: Vec<Cause>, explain: bool) -> Explained {
    if !explain {
        return Explained::value_only(body.result);
    }
    Explained {
        result: body.result,
        body: body.body,
        causes,
        children: significant_children(body.children),
        as_operand: body.as_operand.map(|n| match n {
            ExplanationNode::Compose {
                expression,
                operands,
            } => ExplanationNode::Compose {
                expression,
                operands: significant_children(operands),
            },
            other => other,
        }),
    }
}

/// Drop bare literal composes (e.g. `25%`) from rule/product children — the
/// body expression already names them.
fn significant_children(nodes: Vec<ExplanationNode>) -> Vec<ExplanationNode> {
    nodes
        .into_iter()
        .filter(|n| {
            !matches!(
                n,
                ExplanationNode::Compose { operands, .. } if operands.is_empty()
            )
        })
        .collect()
}

fn cause_from_condition_id(
    condition: NormalFormId,
    held: bool,
    cond_operand: Option<ExplanationNode>,
    plan: &ExecutionPlan,
    ctx: &EvaluationContext,
) -> Cause {
    let focus = peel_bool_leaf_origins(condition, plan);
    let (condition_text, value) = condition_statement_from_id(focus, held, plan);
    let children = if matches!(plan.normal_form(focus).kind, NormalFormKind::And(_)) {
        and_cause_children(focus, cond_operand.as_ref(), plan, ctx)
    } else {
        cause_children(cond_operand)
    };
    Cause {
        condition: condition_text,
        value,
        children,
    }
}

fn condition_statement_from_id(
    condition: NormalFormId,
    held: bool,
    plan: &ExecutionPlan,
) -> (String, String) {
    let forms = plan.normal_forms.as_slice();
    match &plan.normal_form(condition).kind {
        NormalFormKind::Comparison(a, op, b) => {
            let op = if held {
                op.clone()
            } else {
                negated_comparison(op.clone())
            };
            (
                format!(
                    "{} {op} {}",
                    explanation_display(forms, *a),
                    explanation_display(forms, *b)
                ),
                "true".to_string(),
            )
        }
        NormalFormKind::Not(inner) => condition_statement_from_id(*inner, !held, plan),
        NormalFormKind::Leaf(LeafKind::DataPath(path)) => (
            format!(
                "{} is {}",
                path.input_key(),
                if held { "true" } else { "false" }
            ),
            "true".to_string(),
        ),
        _ => {
            let text = explanation_display(forms, condition);
            (
                text,
                if held {
                    "true".to_string()
                } else {
                    "false".to_string()
                },
            )
        }
    }
}

fn eval_kind(
    id: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
) -> Explained {
    match &plan.normal_form(id).kind {
        NormalFormKind::Leaf(LeafKind::Literal(literal)) => {
            let result = OperationResult::from_literal(literal.clone());
            if !explain {
                return Explained::value_only(result);
            }
            let expression = literal.display_value_with_type(plan.result_type(id).as_ref());
            // Empty-operand compose so parents (e.g. `sqrt(4)`) have a
            // non-empty operand list and survive `significant_children`.
            // Bare literal composes are still dropped at product/rule level.
            let node = ExplanationNode::Compose {
                expression: expression.clone(),
                operands: Vec::new(),
            };
            Explained {
                result,
                body: expression,
                causes: Vec::new(),
                children: Vec::new(),
                as_operand: Some(node),
            }
        }
        NormalFormKind::Leaf(LeafKind::DataPath(path)) => {
            let result = resolve_data_path_value(path, plan, ctx);
            if !explain {
                return Explained::value_only(result);
            }
            let display = match &result {
                OperationResult::Value(v) => {
                    let data_type = ctx.data_display_type(plan, path);
                    v.display_value_with_type(data_type.as_ref())
                }
                OperationResult::Veto(_) => {
                    let data_type = ctx.data_display_type(plan, path);
                    format_operation_result(&result, data_type.as_ref())
                }
            };
            let node = ExplanationNode::Data {
                name: path.clone(),
                display,
            };
            Explained {
                result,
                body: path.input_key(),
                causes: Vec::new(),
                children: vec![node.clone()],
                as_operand: Some(node),
            }
        }
        NormalFormKind::Now => {
            let result = OperationResult::from_literal(ctx.now().clone());
            Explained::value_only(result)
        }
        NormalFormKind::Veto(veto) => {
            let result = OperationResult::Veto(VetoType::UserDefined {
                message: veto.message.clone().filter(|m| !m.is_empty()),
            });
            if !explain {
                return Explained::value_only(result);
            }
            let node = ExplanationNode::Veto {
                message: veto.message.clone(),
            };
            Explained {
                result,
                body: "veto".to_string(),
                causes: Vec::new(),
                children: vec![node.clone()],
                as_operand: Some(node),
            }
        }
        NormalFormKind::Sum(children) => {
            fold_nary_arithmetic(children, ArithmeticComputation::Add, plan, ctx, explain, id)
        }
        NormalFormKind::Product(children) => fold_nary_arithmetic(
            children,
            ArithmeticComputation::Multiply,
            plan,
            ctx,
            explain,
            id,
        ),
        NormalFormKind::Subtract(left, right) => binary_arithmetic(
            *left,
            *right,
            ArithmeticComputation::Subtract,
            plan,
            ctx,
            explain,
            id,
        ),
        NormalFormKind::Divide(left, right) => binary_arithmetic(
            *left,
            *right,
            ArithmeticComputation::Divide,
            plan,
            ctx,
            explain,
            id,
        ),
        NormalFormKind::Power(left, right) => binary_arithmetic(
            *left,
            *right,
            ArithmeticComputation::Power,
            plan,
            ctx,
            explain,
            id,
        ),
        NormalFormKind::Modulo(left, right) => binary_arithmetic(
            *left,
            *right,
            ArithmeticComputation::Modulo,
            plan,
            ctx,
            explain,
            id,
        ),
        NormalFormKind::Negate(inner) => {
            let zero = OperationResult::from_literal(LiteralValue::number(
                crate::computation::rational::rational_zero(),
            ));
            let value = eval(*inner, plan, ctx, explain);
            let number_ty = crate::planning::semantics::primitive_number_arc();
            let result = binary_arithmetic_result(
                &zero,
                number_ty,
                value.result.clone(),
                plan.result_type(*inner),
                ArithmeticComputation::Subtract,
                plan,
            );
            compose_unary(id, result, value, explain, plan)
        }
        NormalFormKind::Reciprocal(inner) => {
            let one = OperationResult::from_literal(LiteralValue::number(
                crate::computation::rational::rational_one(),
            ));
            let value = eval(*inner, plan, ctx, explain);
            let number_ty = crate::planning::semantics::primitive_number_arc();
            let result = binary_arithmetic_result(
                &one,
                number_ty,
                value.result.clone(),
                plan.result_type(*inner),
                ArithmeticComputation::Divide,
                plan,
            );
            compose_unary(id, result, value, explain, plan)
        }
        NormalFormKind::Comparison(left, op, right) => {
            let unit_ctx = UnitResolutionContext::WithIndex(&plan.resolved_types.unit_index);
            evaluate_binary(
                *left,
                *right,
                plan,
                ctx,
                explain,
                id,
                |left_result, right_result| {
                    comparison_operation(
                        borrow_value(left_result, "left operand"),
                        plan.result_type(*left),
                        op,
                        borrow_value(&right_result, "right operand"),
                        plan.result_type(*right),
                        unit_ctx,
                    )
                },
            )
        }
        NormalFormKind::And(children) => evaluate_and(children, plan, ctx, explain, id),
        NormalFormKind::Not(inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let false_lit = OperationResult::from_literal(LiteralValue::from_bool(false));
            let unit_ctx = UnitResolutionContext::WithIndex(&plan.resolved_types.unit_index);
            let bool_ty = crate::planning::semantics::primitive_boolean_arc();
            let result = comparison_operation(
                borrow_value(&inner_e.result, "not operand"),
                plan.result_type(*inner),
                &crate::planning::semantics::ComparisonComputation::Is,
                borrow_value(&false_lit, "not operand"),
                bool_ty,
                unit_ctx,
            );
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::MathOp(op, inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let math_type = plan
                .rules
                .values()
                .find(|rule| rule.normal_form == id)
                .map(|rule| Arc::clone(&rule.rule_type))
                .unwrap_or_else(|| {
                    resolve_measure_type_for_magnitude_math(plan, plan.result_type(*inner))
                });
            let result = evaluate_mathematical_operator(
                op,
                borrow_value(&inner_e.result, "operand"),
                &math_type,
            );
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::UnitConversion(inner, target) => {
            let conversion_source = plan.normal_form(id).source.clone();
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let source_value = match &inner_e.result {
                OperationResult::Value(v) => v,
                OperationResult::Veto(_) => {
                    panic!(
                        "BUG: UnitConversion operand passed veto check but is vetoed (source={conversion_source:?})"
                    )
                }
            };
            let result =
                convert_unit_operand(source_value, plan.result_type(*inner).as_ref(), target);
            if !explain {
                return Explained::value_only(result);
            }
            let expression = explanation_display(plan.normal_forms.as_slice(), id);
            let result_lit = match &result {
                OperationResult::Value(v) => v,
                OperationResult::Veto(_) => {
                    let node = ExplanationNode::Veto {
                        message: Some(format_operation_result(
                            &result,
                            plan.result_type(id).as_ref(),
                        )),
                    };
                    return Explained {
                        result,
                        body: expression,
                        causes: Vec::new(),
                        children: vec![node],
                        as_operand: None,
                    };
                }
            };
            let data_ref = match &inner_e.as_operand {
                Some(ExplanationNode::Data { name, .. }) => Some(name),
                _ => None,
            };
            let steps = crate::evaluation::conversion_trace::build_conversion_steps(
                source_value,
                plan.result_type(*inner),
                target,
                result_lit,
                plan.result_type(id),
                data_ref,
            );
            let operands = inner_e.as_operand.into_iter().collect::<Vec<_>>();
            let node = ExplanationNode::Conversion {
                expression: expression.clone(),
                steps,
                operands: operands.clone(),
            };
            Explained {
                result,
                body: expression,
                causes: Vec::new(),
                children: vec![node.clone()],
                as_operand: Some(node),
            }
        }
        NormalFormKind::DateRelative(kind, inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let date = match &borrow_value(&inner_e.result, "date operand").value {
                ValueKind::Date(dt) => dt,
                other => panic!("BUG: date-relative operand expected date, got {other:?}"),
            };
            let result =
                crate::computation::datetime::compute_date_relative(kind, date, now_date(ctx));
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::DateCalendar(kind, unit, inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let date = match &borrow_value(&inner_e.result, "date operand").value {
                ValueKind::Date(dt) => dt,
                other => panic!("BUG: date-calendar operand expected date, got {other:?}"),
            };
            let result = crate::computation::datetime::compute_date_calendar(
                kind,
                unit,
                date,
                now_date(ctx),
            );
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::RangeLiteral(left, right) => evaluate_binary(
            *left,
            *right,
            plan,
            ctx,
            explain,
            id,
            |left_result, right_result| {
                let range = LiteralValue::range(
                    own_literal(left_result.clone(), "left endpoint"),
                    own_literal(right_result, "right endpoint"),
                );
                OperationResult::from_literal(range)
            },
        ),
        NormalFormKind::PastFutureRange(kind, inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let result = crate::computation::datetime::evaluate_past_future_range(
                kind,
                borrow_value(&inner_e.result, "offset operand"),
                plan.result_type(*inner),
                now_date(ctx),
            );
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::RangeContainment(value, range) => evaluate_binary(
            *value,
            *range,
            plan,
            ctx,
            explain,
            id,
            |value_result, range_result| {
                let range_literal = borrow_value(&range_result, "range operand");
                match &range_literal.value {
                    ValueKind::Range(range_left, range_right) => {
                        let endpoint_type = plan
                            .result_type(*range)
                            .specifications
                            .element_from_range()
                            .map(|element| {
                                std::sync::Arc::new(
                                    crate::planning::semantics::LemmaType::primitive(element),
                                )
                            })
                            .expect("BUG: range containment requires a range result type");
                        crate::computation::range::check_containment(
                            borrow_value(value_result, "value operand"),
                            plan.result_type(*value),
                            range_left.as_ref(),
                            range_right.as_ref(),
                            &endpoint_type,
                        )
                    }
                    other => {
                        panic!("BUG: range containment expected range operand, got {other:?}")
                    }
                }
            },
        ),
        NormalFormKind::ResultIsVeto(inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            let result =
                OperationResult::from_literal(LiteralValue::from_bool(inner_e.result.vetoed()));
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::Piecewise(arms) => evaluate_piecewise(arms, plan, ctx, explain),
        NormalFormKind::OrderedDispatch {
            scrutinee,
            boundaries,
            regions,
        } => evaluate_ordered_dispatch(*scrutinee, boundaries, regions, plan, ctx, explain),
    }
}

fn compose_unary(
    id: NormalFormId,
    result: OperationResult,
    inner: Explained,
    explain: bool,
    plan: &ExecutionPlan,
) -> Explained {
    if !explain {
        return Explained::value_only(result);
    }
    let expression = explanation_display(plan.normal_forms.as_slice(), id);
    let operands: Vec<_> = inner.as_operand.into_iter().collect();
    let node = ExplanationNode::Compose {
        expression: expression.clone(),
        operands: operands.clone(),
    };
    Explained {
        result,
        body: expression,
        causes: Vec::new(),
        children: operands,
        as_operand: Some(node),
    }
}

fn compose_binary(
    id: NormalFormId,
    result: OperationResult,
    left: Explained,
    right: Explained,
    explain: bool,
    plan: &ExecutionPlan,
) -> Explained {
    if !explain {
        return Explained::value_only(result);
    }
    let expression = explanation_display(plan.normal_forms.as_slice(), id);
    let mut operands = Vec::new();
    if let Some(n) = left.as_operand {
        operands.push(n);
    }
    if let Some(n) = right.as_operand {
        operands.push(n);
    }
    let node = ExplanationNode::Compose {
        expression: expression.clone(),
        operands: operands.clone(),
    };
    Explained {
        result,
        body: expression,
        causes: Vec::new(),
        children: operands,
        as_operand: Some(node),
    }
}

fn fold_nary_arithmetic(
    children: &[NormalFormId],
    op: ArithmeticComputation,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
    id: NormalFormId,
) -> Explained {
    assert!(!children.is_empty(), "BUG: empty n-ary arithmetic");
    let mut operands = Vec::new();
    // MissingData: continue siblings for prune recording. If a later sibling is a
    // definitive veto, that answer wins over the earlier MissingData.
    let mut first_veto: Option<OperationResult> = None;
    let mut continue_for_recording = false;
    let mut acc: Option<OperationResult> = None;
    let mut acc_type: Option<std::sync::Arc<crate::planning::semantics::LemmaType>> = None;

    for child in children {
        if first_veto.is_some() && !continue_for_recording {
            break;
        }
        let explained = eval(*child, plan, ctx, explain);
        if let Some(n) = explained.as_operand {
            operands.push(n);
        }
        if first_veto.is_some() {
            if explained.result.vetoed() && !explained.result.is_missing_data() {
                first_veto = Some(explained.result);
                continue_for_recording = false;
            }
            continue;
        }
        if explained.result.vetoed() {
            continue_for_recording = explained.result.is_missing_data();
            first_veto = Some(explained.result);
            continue;
        }
        let child_type = std::sync::Arc::clone(plan.result_type(*child));
        match acc.take() {
            None => {
                acc = Some(explained.result);
                acc_type = Some(child_type);
            }
            Some(left) => {
                let left_type = acc_type.take().expect("BUG: acc without type");
                let combined = binary_arithmetic_result(
                    &left,
                    &left_type,
                    explained.result,
                    &child_type,
                    op.clone(),
                    plan,
                );
                if combined.vetoed() {
                    continue_for_recording = combined.is_missing_data();
                    first_veto = Some(combined);
                } else {
                    let next_type = crate::planning::graph::compute_arithmetic_result_type(
                        left_type, &op, child_type,
                    );
                    acc = Some(combined);
                    acc_type = Some(next_type);
                }
            }
        }
    }

    let result = first_veto
        .or(acc)
        .expect("BUG: n-ary arithmetic produced neither value nor veto after evaluating children");
    finish_nary(id, result, operands, explain, plan)
}

fn finish_nary(
    id: NormalFormId,
    result: OperationResult,
    operands: Vec<ExplanationNode>,
    explain: bool,
    plan: &ExecutionPlan,
) -> Explained {
    if !explain {
        return Explained::value_only(result);
    }
    let operands = significant_children(operands);
    let expression = explanation_display(plan.normal_forms.as_slice(), id);
    let node = ExplanationNode::Compose {
        expression: expression.clone(),
        operands: operands.clone(),
    };
    Explained {
        result,
        body: expression,
        causes: Vec::new(),
        children: operands,
        as_operand: Some(node),
    }
}

fn evaluate_binary<F>(
    left: NormalFormId,
    right: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
    id: NormalFormId,
    combine: F,
) -> Explained
where
    F: FnOnce(&OperationResult, OperationResult) -> OperationResult,
{
    let left_e = eval(left, plan, ctx, explain);
    if left_e.result.vetoed() {
        if !left_e.result.is_missing_data() {
            return left_e;
        }
        let right_e = eval(right, plan, ctx, explain);
        if right_e.result.vetoed() && !right_e.result.is_missing_data() {
            return compose_binary(id, right_e.result.clone(), left_e, right_e, explain, plan);
        }
        return compose_binary(id, left_e.result.clone(), left_e, right_e, explain, plan);
    }
    let right_e = eval(right, plan, ctx, explain);
    if right_e.result.vetoed() {
        return compose_binary(id, right_e.result.clone(), left_e, right_e, explain, plan);
    }
    let result = combine(&left_e.result, right_e.result.clone());
    compose_binary(id, result, left_e, right_e, explain, plan)
}

fn binary_arithmetic(
    left: NormalFormId,
    right: NormalFormId,
    op: ArithmeticComputation,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
    id: NormalFormId,
) -> Explained {
    evaluate_binary(
        left,
        right,
        plan,
        ctx,
        explain,
        id,
        |left_result, right_result| {
            binary_arithmetic_result(
                left_result,
                plan.result_type(left),
                right_result,
                plan.result_type(right),
                op,
                plan,
            )
        },
    )
}

fn binary_arithmetic_result(
    left: &OperationResult,
    left_type: &std::sync::Arc<crate::planning::semantics::LemmaType>,
    right: OperationResult,
    right_type: &std::sync::Arc<crate::planning::semantics::LemmaType>,
    op: ArithmeticComputation,
    plan: &ExecutionPlan,
) -> OperationResult {
    arithmetic_operation(
        borrow_value(left, "left operand"),
        left_type,
        &op,
        borrow_value(&right, "right operand"),
        right_type,
        &plan.resolved_types.unit_index,
        &plan.signature_index,
    )
}

fn evaluate_and(
    children: &[NormalFormId],
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
    id: NormalFormId,
) -> Explained {
    assert!(!children.is_empty(), "BUG: empty And");
    let mut operands = Vec::new();
    let last = children.len() - 1;
    for (i, child) in children.iter().enumerate() {
        if i < last {
            let conjunct = eval(*child, plan, ctx, explain);
            if let Some(n) = conjunct.as_operand {
                operands.push(n);
            }
            match condition_outcome(&conjunct.result) {
                BranchOutcome::Propagate(result) => {
                    // Keep walking later conjuncts for explain / nested control prune.
                    // Intake: MissingData left stays MissingData (a false binding can still
                    // answer). Definitive left stays that veto; later MissingData must not
                    // reopen intake (veto and _ is still veto).
                    let answer = result;
                    for later in children.iter().skip(i + 1) {
                        let later_e = eval(*later, plan, ctx, explain);
                        if let Some(n) = later_e.as_operand {
                            operands.push(n);
                        }
                    }
                    return finish_nary(id, answer, operands, explain, plan);
                }
                BranchOutcome::NotTaken => {
                    assert!(
                        i == 0,
                        "BUG: And short-circuit applies only to left conjunct (index 0); got index {i}"
                    );
                    assert!(
                        children.len() == 2,
                        "BUG: And must be binary after lowering, got {} children",
                        children.len()
                    );
                    let result = OperationResult::from_literal(LiteralValue::from_bool(false));
                    return finish_nary(id, result, operands, explain, plan);
                }
                BranchOutcome::Taken => {}
            }
        } else {
            let last_e = eval(*child, plan, ctx, explain);
            if let Some(n) = last_e.as_operand {
                operands.push(n);
            }
            return finish_nary(id, last_e.result, operands, explain, plan);
        }
    }
    unreachable!("BUG: and loop exhausted without reaching the last conjunct")
}
