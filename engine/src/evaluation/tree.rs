//! Tree evaluator: walks drift-free [`NormalForm`] cells by [`NormalFormId`].
//!
//! When explanation is requested, the same walk builds Compose/Data nodes
//! from Kind. Values always come from Kind.

use crate::computation::{
    arithmetic_operation, comparison_operation, convert_unit_operand, UnitResolutionContext,
};
use crate::evaluation::branch_semantics::{condition_outcome, BranchOutcome};
use crate::evaluation::explanations::{format_operation_result, Explanation};
use crate::evaluation::expression::{evaluate_mathematical_operator, resolve_data_path_value};
use crate::evaluation::operations::{OperationResult, VetoType};
use crate::evaluation::EvaluationContext;
use crate::planning::execution_plan::{ExecutableRule, ExecutionPlan};
use crate::planning::explanation::{Cause, ExplanationNode};
use crate::planning::normalize::{explanation_display, LeafKind, NormalFormId, NormalFormKind};
use crate::planning::semantics::{
    negated_comparison, ArithmeticComputation, DataPath, LiteralValue, RulePath, ValueKind,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn borrow_value<'a>(result: &'a OperationResult, operand: &str) -> &'a LiteralValue {
    match result {
        OperationResult::Value(arc) => arc.as_ref(),
        OperationResult::Veto(_) => panic!("BUG: {operand} passed veto check but has no value"),
    }
}

fn own_literal(result: OperationResult, operand: &str) -> LiteralValue {
    match result {
        OperationResult::Value(arc) => {
            Arc::try_unwrap(arc).unwrap_or_else(|shared| shared.as_ref().clone())
        }
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
pub(crate) fn evaluate_rule(
    rule: &ExecutableRule,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> OperationResult {
    evaluate_id(rule.normal_form, plan, ctx)
}

/// Evaluate one rule while building its explanation (single walk).
pub(crate) fn evaluate_rule_explained(
    rule: &ExecutableRule,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> (OperationResult, Explanation) {
    let explained = evaluate_explained(rule.normal_form, plan, ctx);
    let result = explained.result.clone();
    let children = explained.children;

    let node = ExplanationNode::Rule {
        name: rule.path.clone(),
        result: Some(format_operation_result(&result)),
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
            body: explained.body,
            causes: explained.causes,
            children,
        },
    )
}

/// Direct rule-embed paths in `root`'s Kind DAG.
///
/// Cells with `rule_embed` are recorded; their Kind children are not walked
/// (the referenced rule's body is ensured separately via that path).
fn collect_direct_rule_embed_paths(
    plan: &ExecutionPlan,
    root: NormalFormId,
    out: &mut Vec<RulePath>,
) {
    let mut visited = HashSet::new();
    let mut worklist = vec![root];
    while let Some(id) = worklist.pop() {
        if !visited.insert(id) {
            continue;
        }
        let cell = plan.normal_form(id);
        if let Some(path) = &cell.rule_embed {
            out.push(path.clone());
            continue;
        }
        match &cell.kind {
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
                for (c, r) in arms {
                    worklist.push(*c);
                    worklist.push(*r);
                }
            }
        }
    }
}

/// Ensure `rule_path` (and its rule-embed deps) are in `rule_explanations`.
///
/// Uses an explicit heap stack so a long rule chain does not grow the Rust
/// call stack. Only calls [`evaluate_rule_explained`] when every direct
/// rule-embed dep is already cached (body walk then hits cache embeds).
fn ensure_rule_explained(rule_path: &RulePath, plan: &ExecutionPlan, ctx: &mut EvaluationContext) {
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

        let mut deps = Vec::new();
        collect_direct_rule_embed_paths(plan, rule.normal_form, &mut deps);

        let mut pending = None;
        for dep in deps {
            if !ctx.rule_explanations.contains_key(&dep) {
                pending = Some(dep);
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

        let (result, explanation) = {
            let previous = ctx.record_releases;
            ctx.record_releases = false;
            let out = evaluate_rule_explained(rule, plan, ctx);
            ctx.record_releases = previous;
            out
        };
        if explanation.name != current {
            panic!(
                "BUG: on-demand explain for '{}' stored explanation for '{}'",
                current.rule, explanation.name.rule
            );
        }
        // Prefer use-site value already cached for releases; keep explanation node.
        ctx.rule_results.entry(current).or_insert(result);
        stack.pop();
    }
}

/// Explain-mode Rule embed: value+releases at the use-site, narration without re-release.
fn embed_rule_explained(
    rule_path: &RulePath,
    use_site_id: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
) -> Explained {
    if !ctx.rule_results.contains_key(rule_path) {
        let explained = eval_use_site_algebra(use_site_id, plan, ctx, false);
        ctx.rule_results
            .insert(rule_path.clone(), explained.result.clone());
    }
    ensure_rule_explained(rule_path, plan, ctx);
    let result = ctx.rule_results.get(rule_path).cloned().unwrap_or_else(|| {
        panic!(
            "BUG: Rule embed '{}' missing rule_results after on-demand explain",
            rule_path.rule
        )
    });
    let node = ctx
        .rule_explanations
        .get(rule_path)
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "BUG: Rule embed '{}' missing rule_explanations after on-demand explain",
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
        if let Some(cached) = ctx.value_memo.get(&id) {
            return Explained::value_only(cached.clone());
        }
    }

    let explained = eval_uncached(id, plan, ctx, explain);
    if !explain {
        ctx.value_memo.insert(id, explained.result.clone());
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
            if matches!(&plan.normal_form(origin).kind, NormalFormKind::Piecewise(_)) {
                return explain_with_piecewise_origin(id, origin, plan, ctx);
            }
            let value = eval(id, plan, ctx, false);
            let previous = ctx.record_releases;
            ctx.record_releases = false;
            let mut explained = eval(origin, plan, ctx, true);
            ctx.record_releases = previous;
            explained.result = value.result;
            return explained;
        }
    }

    if let Some(path) = plan.normal_form(id).rule_embed.clone() {
        if explain {
            return embed_rule_explained(&path, id, plan, ctx);
        }
        if let Some(cached) = ctx.rule_results.get(&path) {
            return Explained::value_only(cached.clone());
        }
        // Evaluate this use-site cell (same Kind as bare body). Control ids for
        // data_releases are the use-site ids fill_data_releases recorded under
        // the outer release_rule — not the bare rule.normal_form id.
        let explained = eval_use_site_algebra(id, plan, ctx, false);
        ctx.rule_results.insert(path, explained.result.clone());
        return explained;
    }

    eval_use_site_algebra(id, plan, ctx, explain)
}

fn eval_use_site_algebra(
    id: NormalFormId,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
) -> Explained {
    if let NormalFormKind::Piecewise(arms) = &plan.normal_form(id).kind {
        return evaluate_piecewise(id, arms, plan, ctx, explain);
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
    let previous = ctx.record_releases;
    ctx.record_releases = false;
    let causes = piecewise_causes_from_record(&recorded, plan, ctx);

    let mut body = match &plan.normal_form(current_id).kind {
        NormalFormKind::Piecewise(kept) => evaluate_piecewise(current_id, kept, plan, ctx, true),
        _ => eval_kind(current_id, plan, ctx, true),
    };
    ctx.record_releases = previous;
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
        | ExplanationNode::UnitEquivalence { .. }
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
    if ctx.get_veto(path).is_none() && ctx.get_data_value(path).is_none() {
        return None;
    }
    let result = resolve_data_path_value(path, plan, ctx);
    let display = match &result {
        OperationResult::Value(v) => v.display_value(),
        OperationResult::Veto(_) => format_operation_result(&result),
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
        value: Some(value),
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
    id: NormalFormId,
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
                    message: Some(format_operation_result(&result)),
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
                ctx.apply_releases(plan, id, |releases| match releases {
                    crate::planning::execution_plan::ControlDataReleases::Piecewise {
                        on_arm_taken,
                        ..
                    } => on_arm_taken
                        .get(i)
                        .unwrap_or_else(|| {
                            panic!("BUG: on_arm_taken missing index {i} for piecewise {id:?}")
                        })
                        .as_slice(),
                    other => panic!(
                        "BUG: expected Piecewise ControlDataReleases for {id:?}, got {other:?}"
                    ),
                });
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
                ctx.apply_releases(plan, id, |releases| match releases {
                    crate::planning::execution_plan::ControlDataReleases::Piecewise {
                        on_arm_not_taken,
                        ..
                    } => on_arm_not_taken
                        .get(i)
                        .unwrap_or_else(|| {
                            panic!("BUG: on_arm_not_taken missing index {i} for piecewise {id:?}")
                        })
                        .as_slice(),
                    other => panic!(
                        "BUG: expected Piecewise ControlDataReleases for {id:?}, got {other:?}"
                    ),
                });
                if explain {
                    not_taken.push((
                        i,
                        cause_from_condition_id(condition, false, cond_e.as_operand, plan, ctx),
                    ));
                }
            }
        }
    }

    ctx.apply_releases(plan, id, |releases| match releases {
        crate::planning::execution_plan::ControlDataReleases::Piecewise {
            on_default_wins, ..
        } => on_default_wins.as_slice(),
        other => panic!("BUG: expected Piecewise ControlDataReleases for {id:?}, got {other:?}"),
    });
    let body_e = eval(arms[0].1, plan, ctx, explain);
    if !explain {
        return Explained::value_only(body_e.result);
    }
    not_taken.reverse();
    let causes = not_taken.into_iter().map(|(_, c)| c).collect();
    finish_piecewise(body_e, causes, true)
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
        value: Some(value),
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
            let result = OperationResult::from_literal_arc(Arc::clone(literal));
            if !explain {
                return Explained::value_only(result);
            }
            let expression = literal.display_value();
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
                OperationResult::Value(v) => v.display_value(),
                OperationResult::Veto(_) => format_operation_result(&result),
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
            let result = OperationResult::from_literal_arc(Arc::clone(&ctx.now));
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
            let result = binary_arithmetic_result(
                &zero,
                value.result.clone(),
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
            let result = binary_arithmetic_result(
                &one,
                value.result.clone(),
                ArithmeticComputation::Divide,
                plan,
            );
            compose_unary(id, result, value, explain, plan)
        }
        NormalFormKind::Comparison(left, op, right) => {
            let left_e = eval(*left, plan, ctx, explain);
            if left_e.result.vetoed() {
                return left_e;
            }
            let right_e = eval(*right, plan, ctx, explain);
            if right_e.result.vetoed() {
                return right_e;
            }
            let unit_ctx = UnitResolutionContext::WithIndex(&plan.resolved_types.unit_index);
            let result = comparison_operation(
                borrow_value(&left_e.result, "left operand"),
                op,
                borrow_value(&right_e.result, "right operand"),
                unit_ctx,
            );
            compose_binary(id, result, left_e, right_e, explain, plan)
        }
        NormalFormKind::And(children) => evaluate_and(children, plan, ctx, explain, id),
        NormalFormKind::Not(inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let false_lit = OperationResult::from_literal(LiteralValue::from_bool(false));
            let unit_ctx = UnitResolutionContext::WithIndex(&plan.resolved_types.unit_index);
            let result = comparison_operation(
                borrow_value(&inner_e.result, "not operand"),
                &crate::planning::semantics::ComparisonComputation::Is,
                borrow_value(&false_lit, "not operand"),
                unit_ctx,
            );
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::MathOp(op, inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let result =
                evaluate_mathematical_operator(op, borrow_value(&inner_e.result, "operand"));
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::UnitConversion(inner, target) => {
            let conversion_source = plan.normal_form(id).source.clone();
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let source_value = match &inner_e.result {
                OperationResult::Value(arc) => Arc::clone(arc),
                OperationResult::Veto(_) => {
                    panic!(
                        "BUG: UnitConversion operand passed veto check but is vetoed (source={conversion_source:?})"
                    )
                }
            };
            let result = convert_unit_operand(Arc::clone(&source_value), target);
            if !explain {
                return Explained::value_only(result);
            }
            let expression = explanation_display(plan.normal_forms.as_slice(), id);
            let result_lit = match &result {
                OperationResult::Value(arc) => arc.as_ref(),
                OperationResult::Veto(_) => {
                    let node = ExplanationNode::Veto {
                        message: Some(format_operation_result(&result)),
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
                source_value.as_ref(),
                target,
                result_lit,
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
        NormalFormKind::RangeLiteral(left, right) => {
            let left_e = eval(*left, plan, ctx, explain);
            if left_e.result.vetoed() {
                return left_e;
            }
            let right_e = eval(*right, plan, ctx, explain);
            if right_e.result.vetoed() {
                return right_e;
            }
            let range = LiteralValue::range(
                own_literal(left_e.result.clone(), "left endpoint"),
                own_literal(right_e.result.clone(), "right endpoint"),
            );
            let result = OperationResult::from_literal(range);
            compose_binary(id, result, left_e, right_e, explain, plan)
        }
        NormalFormKind::PastFutureRange(kind, inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            if inner_e.result.vetoed() {
                return inner_e;
            }
            let result = crate::computation::datetime::evaluate_past_future_range(
                kind,
                borrow_value(&inner_e.result, "offset operand"),
                now_date(ctx),
            );
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::RangeContainment(value, range) => {
            let value_e = eval(*value, plan, ctx, explain);
            if value_e.result.vetoed() {
                return value_e;
            }
            let range_e = eval(*range, plan, ctx, explain);
            if range_e.result.vetoed() {
                return range_e;
            }
            let range_literal = borrow_value(&range_e.result, "range operand");
            let result = match &range_literal.value {
                ValueKind::Range(range_left, range_right) => {
                    crate::computation::range::check_containment(
                        borrow_value(&value_e.result, "value operand"),
                        range_left.as_ref(),
                        range_right.as_ref(),
                    )
                }
                other => panic!("BUG: range containment expected range operand, got {other:?}"),
            };
            compose_binary(id, result, value_e, range_e, explain, plan)
        }
        NormalFormKind::ResultIsVeto(inner) => {
            let inner_e = eval(*inner, plan, ctx, explain);
            let result =
                OperationResult::from_literal(LiteralValue::from_bool(inner_e.result.vetoed()));
            compose_unary(id, result, inner_e, explain, plan)
        }
        NormalFormKind::Piecewise(arms) => evaluate_piecewise(id, arms, plan, ctx, explain),
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
    let mut acc = eval(children[0], plan, ctx, explain);
    if let Some(n) = acc.as_operand.take() {
        operands.push(n);
    }
    if acc.result.vetoed() {
        let veto = acc.result;
        return finish_nary(id, veto, operands, explain, plan);
    }
    for child in children.iter().skip(1) {
        let right = eval(*child, plan, ctx, explain);
        if let Some(n) = right.as_operand {
            operands.push(n);
        }
        if right.result.vetoed() {
            let veto = right.result;
            return finish_nary(id, veto, operands, explain, plan);
        }
        acc.result = binary_arithmetic_result(&acc.result, right.result, op.clone(), plan);
        if acc.result.vetoed() {
            let veto = acc.result;
            return finish_nary(id, veto, operands, explain, plan);
        }
    }
    finish_nary(id, acc.result, operands, explain, plan)
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

fn binary_arithmetic(
    left: NormalFormId,
    right: NormalFormId,
    op: ArithmeticComputation,
    plan: &ExecutionPlan,
    ctx: &mut EvaluationContext,
    explain: bool,
    id: NormalFormId,
) -> Explained {
    let left_e = eval(left, plan, ctx, explain);
    if left_e.result.vetoed() {
        return left_e;
    }
    let right_e = eval(right, plan, ctx, explain);
    if right_e.result.vetoed() {
        return right_e;
    }
    let result = binary_arithmetic_result(&left_e.result, right_e.result.clone(), op, plan);
    compose_binary(id, result, left_e, right_e, explain, plan)
}

fn binary_arithmetic_result(
    left: &OperationResult,
    right: OperationResult,
    op: ArithmeticComputation,
    plan: &ExecutionPlan,
) -> OperationResult {
    arithmetic_operation(
        borrow_value(left, "left operand"),
        &op,
        borrow_value(&right, "right operand"),
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
                    return finish_nary(id, result, operands, explain, plan);
                }
                BranchOutcome::NotTaken => {
                    assert!(
                        i == 0,
                        "BUG: And on_left_false applies only to left conjunct (index 0); got index {i}"
                    );
                    ctx.apply_releases(plan, id, |releases| match releases {
                        crate::planning::execution_plan::ControlDataReleases::And {
                            on_left_false,
                        } => on_left_false.as_slice(),
                        other => panic!(
                            "BUG: expected And ControlDataReleases for {id:?}, got {other:?}"
                        ),
                    });
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
