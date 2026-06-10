//! Structural explanation trees for evaluated rules.
//!
//! Every runtime fact in an explanation (branch decisions, intermediate
//! values, results) comes from the recorded execution of the rule's
//! source-shaped instruction stream ([`RuleRecording`]) — explanations never
//! re-evaluate expressions.

use crate::computation::rational::checked_div;
use crate::computation::UnitResolutionContext;
use crate::evaluation::expression::resolve_data_path_value;
use crate::evaluation::operations::{OperationResult, VetoType};
use crate::evaluation::{BranchDecision, EvaluationContext, RuleRecording};
use crate::planning::execution_plan::{
    ArmRole, ExecutableRule, ExecutionPlan, Instruction, Instructions,
};
use crate::planning::semantics::{
    compare_semantic_dates, negated_comparison, ArithmeticComputation, ComparisonComputation,
    DataPath, Expression, ExpressionKind, LemmaType, LiteralValue, NegationType, RulePath,
    SemanticConversionTarget, TypeSpecification, ValueKind,
};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::cmp::Ordering;
use std::collections::HashMap;

fn serialize_rule_name<S>(path: &RulePath, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path.rule)
}

fn serialize_data_input_key<S>(path: &DataPath, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path.input_key())
}

#[derive(Debug, Clone)]
pub struct Explanation {
    pub rule: RulePath,
    pub result: OperationResult,
    pub body: String,
    pub causes: Vec<Cause>,
    pub children: Vec<ExplanationNode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExplanationNode {
    Rule {
        #[serde(serialize_with = "serialize_rule_name")]
        rule: RulePath,
        result: String,
        body: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        causes: Vec<Cause>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        children: Vec<ExplanationNode>,
    },
    Compose {
        expression: String,
        operands: Vec<ExplanationNode>,
    },
    DataInput {
        #[serde(serialize_with = "serialize_data_input_key")]
        data: DataPath,
        display: String,
    },
    Conversion {
        expression: String,
        steps: Vec<SerializedConversionTraceStep>,
        operands: Vec<ExplanationNode>,
    },
    Veto {
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// A unit reconciliation fact ("1 mile is 1.60934 kilometer") stated when
    /// an operator's operands carry different units of the same quantity
    /// family, so the implicit conversion inside the arithmetic is
    /// followable without external lookup tables. Derived from declared unit
    /// factors — static metadata, not evaluation.
    UnitEquivalence { text: String },
}

/// One evaluated unless condition, stated as a fact.
///
/// `condition` is the true-form expression text: a falsified comparison is
/// flipped to its complement (`distance < 5 mile` that failed becomes
/// `distance >= 5 mile`) so the explanation states what *held*. `value` is
/// `"true"` for stated facts, or `"false"` when the condition could not be
/// flipped into a positive statement. `children` show the inputs that drove
/// the condition (data values, embedded rule explanations).
#[derive(Debug, Clone, Serialize)]
pub struct Cause {
    pub condition: String,
    pub value: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ExplanationNode>,
}

#[derive(Debug, Clone)]
pub enum ConversionTraceRole {
    Outcome,
    Rule,
    Source,
}

#[derive(Debug, Clone)]
pub struct ConversionTraceStep {
    pub role: ConversionTraceRole,
    pub text: String,
    pub data_ref: Option<DataPath>,
}

fn build_conversion_steps(
    value: &LiteralValue,
    target: &SemanticConversionTarget,
    result: &LiteralValue,
    data_ref: Option<&DataPath>,
    resolution_context: UnitResolutionContext<'_>,
) -> Vec<ConversionTraceStep> {
    let mut steps = Vec::new();
    steps.push(ConversionTraceStep {
        role: ConversionTraceRole::Outcome,
        text: result.to_string(),
        data_ref: None,
    });

    if let Some(rule_text) = conversion_rule_step_text(value, target, result, resolution_context) {
        steps.push(ConversionTraceStep {
            role: ConversionTraceRole::Rule,
            text: rule_text,
            data_ref: None,
        });
    }

    // An identity conversion (operand already in the target unit) has no
    // source step: it would restate the outcome value verbatim.
    if value.to_string() != result.to_string() {
        steps.push(ConversionTraceStep {
            role: ConversionTraceRole::Source,
            text: conversion_source_step_text(value, data_ref),
            data_ref: data_ref.cloned(),
        });
    }

    steps
}

fn conversion_source_step_text(operand: &LiteralValue, data_ref: Option<&DataPath>) -> String {
    let type_name = type_specification_display_name(&operand.lemma_type);
    let value_display = operand.to_string();
    match data_ref {
        Some(path) => format!("The {type_name} of {path} is {value_display}"),
        None => format!("The {type_name} is {value_display}"),
    }
}

fn type_specification_display_name(lemma_type: &LemmaType) -> &'static str {
    match &lemma_type.specifications {
        TypeSpecification::Boolean { .. } => "boolean",
        TypeSpecification::Quantity { .. } => "quantity",
        TypeSpecification::QuantityRange { .. } => "quantity range",
        TypeSpecification::Number { .. } => "number",
        TypeSpecification::NumberRange { .. } => "number range",
        TypeSpecification::Text { .. } => "text",
        TypeSpecification::Date { .. } => "date",
        TypeSpecification::DateRange { .. } => "date range",
        TypeSpecification::TimeRange { .. } => "time range",
        TypeSpecification::Time { .. } => "time",
        TypeSpecification::Ratio { .. } => "ratio",
        TypeSpecification::RatioRange { .. } => "ratio range",
        TypeSpecification::Veto { .. } => "veto",
        TypeSpecification::Undetermined => "undetermined",
    }
}

fn conversion_rule_step_text(
    value: &LiteralValue,
    target: &SemanticConversionTarget,
    result: &LiteralValue,
    resolution_context: UnitResolutionContext<'_>,
) -> Option<String> {
    match &value.value {
        ValueKind::Range(left, right) => range_span_rule_step_text(left, right, result),
        ValueKind::Quantity(_, from_signature) if !value.lemma_type.is_calendar_like() => {
            match target {
                SemanticConversionTarget::Unit { unit_name } => {
                    quantity_unit_equivalence_step_text(
                        from_signature,
                        unit_name,
                        &value.lemma_type,
                        resolution_context,
                    )
                }
                _ => None,
            }
        }
        ValueKind::Number(_) => None,
        ValueKind::Ratio(_, _) => None,
        ValueKind::Quantity(_, _) if value.lemma_type.is_calendar_like() => None,
        _ => None,
    }
}

fn format_explanation_multiplier(
    rational: &crate::computation::rational::RationalInteger,
) -> String {
    use crate::computation::rational::{commit_rational_to_decimal, decimal_to_rational};
    let reduced = rational
        .clone()
        .try_reduce()
        .unwrap_or_else(|_| rational.clone());
    if reduced.denom() == &crate::computation::rational::BigInt::one() {
        return reduced.numer().to_string();
    }
    // Prefer a decimal display ("1.60934") when it is exact; fall back to
    // the fraction only when decimal would lose precision.
    if let Ok(decimal) = commit_rational_to_decimal(&reduced) {
        if decimal_to_rational(decimal).is_ok_and(|round_trip| round_trip == reduced) {
            return decimal.normalize().to_string();
        }
    }
    format!("{}/{}", reduced.numer(), reduced.denom())
}

fn quantity_unit_equivalence_step_text(
    from_signature: &[(String, i32)],
    to_unit: &str,
    lemma_type: &LemmaType,
    resolution_context: UnitResolutionContext<'_>,
) -> Option<String> {
    let from_unit = from_signature
        .first()
        .map(|(name, _)| name.as_str())
        .unwrap_or("");

    let both_units_in_lemma_type = match &lemma_type.specifications {
        TypeSpecification::Quantity { units, .. } => {
            !from_unit.is_empty()
                && from_signature.len() == 1
                && units.get(from_unit).is_ok()
                && units.get(to_unit).is_ok()
        }
        _ => false,
    };

    if both_units_in_lemma_type {
        let from_factor = lemma_type.quantity_unit_factor(from_unit);
        let to_factor = lemma_type.quantity_unit_factor(to_unit);
        let multiplier = checked_div(from_factor, to_factor).ok()?;
        let multiplier_display = format_explanation_multiplier(&multiplier);
        if multiplier_display == "1" {
            return None;
        }
        return Some(format!("1 {from_unit} is {multiplier_display} {to_unit}"));
    }

    let UnitResolutionContext::WithIndex(unit_index) = resolution_context else {
        return None;
    };
    let target_type = unit_index.get(to_unit)?;
    let to_factor = target_type.quantity_unit_factor(to_unit).clone();
    let from_factor =
        crate::planning::semantics::signature_factor(from_signature, unit_index, None);
    let multiplier = checked_div(&from_factor, &to_factor).ok()?;
    let multiplier_display = format_explanation_multiplier(&multiplier);
    if multiplier_display == "1" {
        return None;
    }
    let source_label = crate::planning::semantics::format_signature_operator_style(from_signature);
    Some(format!(
        "1 {source_label} is {multiplier_display} {to_unit}"
    ))
}

fn range_span_rule_step_text(
    left: &LiteralValue,
    right: &LiteralValue,
    result: &LiteralValue,
) -> Option<String> {
    match (&left.value, &right.value) {
        (ValueKind::Date(left_date), ValueKind::Date(right_date)) => {
            let (lower, upper) = ordered_date_pair(left_date, right_date);
            let lower_literal = LiteralValue::date(lower.clone());
            let upper_literal = LiteralValue::date(upper.clone());
            Some(format!("{upper_literal} − {lower_literal} = {result}"))
        }
        (ValueKind::Number(_), ValueKind::Number(_)) => {
            let (lower, upper) = ordered_number_pair(left, right);
            Some(format!("{upper} − {lower} = {result}"))
        }
        (ValueKind::Quantity(_, _), ValueKind::Quantity(_, _)) => {
            let (lower, upper) = ordered_quantity_pair(left, right);
            Some(format!("{upper} − {lower} = {result}"))
        }
        _ => None,
    }
}

fn ordered_date_pair<'a>(
    left: &'a crate::planning::semantics::SemanticDateTime,
    right: &'a crate::planning::semantics::SemanticDateTime,
) -> (
    &'a crate::planning::semantics::SemanticDateTime,
    &'a crate::planning::semantics::SemanticDateTime,
) {
    match compare_semantic_dates(left, right) {
        Ordering::Less | Ordering::Equal => (left, right),
        Ordering::Greater => (right, left),
    }
}

fn ordered_number_pair<'a>(
    left: &'a LiteralValue,
    right: &'a LiteralValue,
) -> (&'a LiteralValue, &'a LiteralValue) {
    let ValueKind::Number(left_number) = &left.value else {
        unreachable!("BUG: ordered_number_pair called with non-number operand");
    };
    let ValueKind::Number(right_number) = &right.value else {
        unreachable!("BUG: ordered_number_pair called with non-number operand");
    };
    if left_number <= right_number {
        (left, right)
    } else {
        (right, left)
    }
}

fn ordered_quantity_pair<'a>(
    left: &'a LiteralValue,
    right: &'a LiteralValue,
) -> (&'a LiteralValue, &'a LiteralValue) {
    let ValueKind::Quantity(left_magnitude, _) = &left.value else {
        unreachable!("BUG: ordered_quantity_pair called with non-quantity operand");
    };
    let ValueKind::Quantity(right_magnitude, _) = &right.value else {
        unreachable!("BUG: ordered_quantity_pair called with non-quantity operand");
    };
    if *left_magnitude <= *right_magnitude {
        (left, right)
    } else {
        (right, left)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SerializedConversionTraceStep {
    role: String,
    text: String,
}

impl Explanation {
    fn as_rule_node(&self) -> ExplanationNode {
        ExplanationNode::Rule {
            rule: self.rule.clone(),
            result: format_operation_result(&self.result),
            body: self.body.clone(),
            causes: self.causes.clone(),
            children: self.children.clone(),
        }
    }
}

impl Serialize for Explanation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("rule", &self.rule.rule)?;
        map.serialize_entry("result", &format_operation_result(&self.result))?;
        map.serialize_entry("body", &self.body)?;
        if !self.causes.is_empty() {
            map.serialize_entry("causes", &self.causes)?;
        }
        map.serialize_entry("children", &self.children)?;
        map.end()
    }
}

fn format_operation_result(result: &OperationResult) -> String {
    match result {
        OperationResult::Value(value) => value.display_value(),
        OperationResult::Veto(VetoType::UserDefined { message: None }) => String::new(),
        OperationResult::Veto(veto) => veto.to_string(),
    }
}

/// Which source expression explains the rule's result.
enum WinningSourceBranch<'a> {
    /// A branch was selected: its result expression is the rule's body.
    BranchResult {
        result_expression: &'a Expression,
        causes: Vec<Cause>,
    },
    /// An unless condition vetoed before any branch was selected: the rule's
    /// result is that veto and no branch body applies. The condition
    /// expression explains where the veto arose.
    ConditionVeto {
        condition_expression: &'a Expression,
        causes: Vec<Cause>,
    },
}

/// Everything the explanation builder reads while walking one rule's source
/// expressions: the evaluation context (data lookups, rule results), the
/// already-built dependency explanations, and the recorded execution of this
/// rule's source instruction stream.
struct ExplainCtx<'a, 'plan> {
    context: &'a EvaluationContext<'plan>,
    plan: &'a ExecutionPlan,
    built: &'a HashMap<RulePath, Explanation>,
    instructions: &'a Instructions,
    recording: &'a RuleRecording,
}

/// Read the winning branch and evaluated-condition causes from the recorded
/// execution: arm tags identify which `JumpIfFalse`/`Return` belongs to which
/// source branch, and the recording says what each decided.
fn winning_source_branch_and_causes<'a>(
    exec_rule: &'a ExecutableRule,
    ctx: &ExplainCtx<'_, '_>,
) -> WinningSourceBranch<'a> {
    if exec_rule.branches.len() == 1 {
        return WinningSourceBranch::BranchResult {
            result_expression: &exec_rule.branches[0].result,
            causes: Vec::new(),
        };
    }

    let condition_arm: HashMap<u32, u16> = ctx
        .instructions
        .arm_tags
        .iter()
        .filter(|tag| tag.role == ArmRole::Condition)
        .map(|tag| (tag.pc, tag.arm))
        .collect();
    let result_arm: HashMap<u32, u16> = ctx
        .instructions
        .arm_tags
        .iter()
        .filter(|tag| tag.role == ArmRole::Result)
        .map(|tag| (tag.pc, tag.arm))
        .collect();

    // Decisions of this rule's own arms (nested piecewise jumps from inlined
    // rules carry no arm tag and are filtered out), in execution order.
    let mut decisions: Vec<(u16, BranchDecision)> = ctx
        .recording
        .branch_decisions
        .iter()
        .filter_map(|(pc, decision)| condition_arm.get(pc).map(|arm| (*arm, *decision)))
        .collect();

    if let Some(returned_pc) = ctx.recording.returned_pc {
        let winning_arm = *result_arm
            .get(&returned_pc)
            .expect("BUG: executed Return must carry an arm tag");
        // Conditions are evaluated in reverse source order; state causes in
        // source order.
        decisions.sort_by_key(|(arm, _)| *arm);
        // When a branch was taken, only the matching condition explains the
        // result — listing every non-matching condition is noise (especially
        // for lookup tables with hundreds of entries). When no branch was
        // taken (default), the non-matching conditions explain why the
        // default applies.
        let has_taken = decisions
            .iter()
            .any(|(_, d)| matches!(d, BranchDecision::Taken));
        let causes = decisions
            .iter()
            .filter(|(_, decision)| {
                if has_taken {
                    matches!(decision, BranchDecision::Taken)
                } else {
                    true
                }
            })
            .map(|(arm, decision)| {
                let condition = exec_rule.branches[*arm as usize]
                    .condition
                    .as_ref()
                    .expect("BUG: unless branch missing condition");
                let held = matches!(decision, BranchDecision::Taken);
                build_cause(condition, held, ctx)
            })
            .collect();
        return WinningSourceBranch::BranchResult {
            result_expression: &exec_rule.branches[winning_arm as usize].result,
            causes,
        };
    }

    // No Return executed: a veto propagated from a JumpIfFalse. The vetoing
    // jump may be this rule's own (tagged) condition jump, or an untagged
    // jump inside an inlined sub-expression. Either way the next tagged
    // instruction after the veto pc identifies where execution was: an arm's
    // condition (Condition tag) or an arm's selected result (Result tag).
    let (veto_pc, _) = *ctx
        .recording
        .branch_decisions
        .iter()
        .rev()
        .find(|(_, decision)| matches!(decision, BranchDecision::Veto))
        .expect("BUG: execution ended without Return but no veto was recorded");
    let enclosing_tag = ctx
        .instructions
        .arm_tags
        .iter()
        .filter(|tag| tag.pc >= veto_pc)
        .min_by_key(|tag| tag.pc)
        .expect("BUG: veto pc past the final tagged Return");

    // Causes: this rule's own evaluated conditions, except the vetoing one
    // (which becomes the body and would otherwise be stated twice).
    let mut causes_decisions: Vec<(u16, BranchDecision)> = ctx
        .recording
        .branch_decisions
        .iter()
        .filter(|(pc, _)| *pc != veto_pc)
        .filter_map(|(pc, decision)| condition_arm.get(pc).map(|arm| (*arm, *decision)))
        .collect();
    causes_decisions.sort_by_key(|(arm, _)| *arm);
    let causes = causes_decisions
        .iter()
        .map(|(arm, decision)| {
            let condition = exec_rule.branches[*arm as usize]
                .condition
                .as_ref()
                .expect("BUG: unless branch missing condition");
            let held = matches!(decision, BranchDecision::Taken);
            build_cause(condition, held, ctx)
        })
        .collect();

    match enclosing_tag.role {
        ArmRole::Condition => {
            let condition_expression = exec_rule.branches[enclosing_tag.arm as usize]
                .condition
                .as_ref()
                .expect("BUG: unless branch missing condition");
            WinningSourceBranch::ConditionVeto {
                condition_expression,
                causes,
            }
        }
        // The veto arose while computing the selected arm's result: that arm
        // won, and the veto is the rule's result.
        ArmRole::Result => WinningSourceBranch::BranchResult {
            result_expression: &exec_rule.branches[enclosing_tag.arm as usize].result,
            causes,
        },
    }
}

fn build_cause(condition: &Expression, held: bool, ctx: &ExplainCtx<'_, '_>) -> Cause {
    let (text, value) = stated_fact(condition, held);
    // A bare data reference is fully stated by the fact text itself
    // ("is_rush is true"); a child would restate the same value. The same
    // holds for an equality against a literal that held ("code is L17"):
    // the data value appears verbatim in the fact.
    let children = match &condition.kind {
        ExpressionKind::DataPath(_) => Vec::new(),
        ExpressionKind::Comparison(left, ComparisonComputation::Is, right)
            if held
                && matches!(left.kind, ExpressionKind::DataPath(_))
                && matches!(right.kind, ExpressionKind::Literal(_)) =>
        {
            Vec::new()
        }
        _ => build_expression_children(condition, ctx),
    };
    Cause {
        condition: text,
        value,
        children,
    }
}

/// State an evaluated condition as a fact.
///
/// Returns `(condition_text, value)`. When the fact can be stated positively
/// (the condition held, or its falsification flips to a complement operator),
/// the text is the true statement and `value` is `"true"`. Conditions that
/// cannot be flipped keep their source text with `value` `"false"`.
fn stated_fact(condition: &Expression, held: bool) -> (String, String) {
    if held {
        return match &condition.kind {
            // A bare reference is not a readable statement on its own.
            ExpressionKind::DataPath(_) | ExpressionKind::RulePath(_) => (
                format!("{} is true", format_expression(condition)),
                "true".to_string(),
            ),
            _ => (format_expression(condition), "true".to_string()),
        };
    }
    match &condition.kind {
        ExpressionKind::Comparison(left, op, right) => {
            let flipped = Expression::with_source(
                ExpressionKind::Comparison(
                    std::sync::Arc::clone(left),
                    negated_comparison(op.clone()),
                    std::sync::Arc::clone(right),
                ),
                condition.source_location.clone(),
            );
            (format_expression(&flipped), "true".to_string())
        }
        // `not x` being false means `x` held.
        ExpressionKind::LogicalNegation(inner, _) => stated_fact(inner, true),
        ExpressionKind::ResultIsVeto(operand) => (
            format!("{} is not veto", format_expression(operand)),
            "true".to_string(),
        ),
        ExpressionKind::DataPath(_) | ExpressionKind::RulePath(_) => (
            format!("{} is false", format_expression(condition)),
            "true".to_string(),
        ),
        _ => (format_expression(condition), "false".to_string()),
    }
}

pub fn build_explanation(
    exec_rule: &ExecutableRule,
    context: &EvaluationContext<'_>,
    plan: &ExecutionPlan,
    built: &HashMap<RulePath, Explanation>,
) -> Explanation {
    let authoritative_result = context
        .rule_results
        .get(&exec_rule.path)
        .expect("BUG: rule evaluated before explain")
        .clone();
    let recording = context
        .recordings
        .get(&exec_rule.path)
        .expect("BUG: recording must exist when explanations are requested");
    let ctx = ExplainCtx {
        context,
        plan,
        built,
        instructions: &exec_rule.source_instructions,
        recording,
    };

    let (body, causes, children) = match winning_source_branch_and_causes(exec_rule, &ctx) {
        WinningSourceBranch::BranchResult {
            result_expression,
            causes,
        } => (
            format_expression(result_expression),
            causes,
            build_expression_children(result_expression, &ctx),
        ),
        WinningSourceBranch::ConditionVeto {
            condition_expression,
            causes,
        } => (
            // No branch was selected: the rule result is the condition's
            // veto, so the body names the vetoing condition and the
            // children show its operands.
            format_expression(condition_expression),
            causes,
            build_expression_children(condition_expression, &ctx),
        ),
    };

    Explanation {
        rule: exec_rule.path.clone(),
        result: authoritative_result,
        body,
        causes,
        children,
    }
}

fn embed_rule(rule_path: &RulePath, built: &HashMap<RulePath, Explanation>) -> ExplanationNode {
    built
        .get(rule_path)
        .expect("BUG: rule explanation must be built before dependents")
        .as_rule_node()
}

fn is_literal(expr: &Expression) -> bool {
    matches!(expr.kind, ExpressionKind::Literal(_))
}

/// Flatten an associative same-operator arithmetic chain into its operands.
fn flatten_arithmetic_chain<'e>(
    expr: &'e Expression,
    op: &ArithmeticComputation,
    out: &mut Vec<&'e Expression>,
) {
    match &expr.kind {
        ExpressionKind::Arithmetic(left, inner_op, right) if inner_op == op => {
            flatten_arithmetic_chain(left, op, out);
            flatten_arithmetic_chain(right, op, out);
        }
        _ => out.push(expr),
    }
}

fn build_operand_nodes(operands: &[&Expression], ctx: &ExplainCtx<'_, '_>) -> Vec<ExplanationNode> {
    // Cross-unit facts first: when operands mix units of one quantity
    // family, the arithmetic reconciles them implicitly; the factor is
    // stated so the numbers are followable.
    let mut nodes = unit_equivalence_nodes(operands, ctx);
    nodes.extend(
        operands
            .iter()
            // Literal operands are already visible in the expression line; a
            // separate child restates them without adding information.
            .filter(|operand| !is_literal(operand))
            .map(|operand| build_expression_node(operand, ctx)),
    );
    nodes
}

/// The operand's value, when it is a leaf whose value is known without
/// evaluation: a literal, a data lookup, or a recorded rule result.
fn operand_leaf_value(expr: &Expression, ctx: &ExplainCtx<'_, '_>) -> Option<LiteralValue> {
    match &expr.kind {
        ExpressionKind::Literal(lit) => Some((**lit).clone()),
        ExpressionKind::DataPath(path) => match resolve_data_path_value(path, ctx.context) {
            OperationResult::Value(value) => Some(value),
            OperationResult::Veto(_) => None,
        },
        ExpressionKind::RulePath(path) => ctx
            .context
            .rule_results
            .get(path)
            .and_then(|result| result.value().cloned()),
        _ => None,
    }
}

/// Does the type owning `unit` also declare `other` (same quantity family)?
fn same_quantity_family(
    unit: &str,
    other: &str,
    unit_index: &HashMap<String, std::sync::Arc<LemmaType>>,
) -> bool {
    unit_index.get(unit).is_some_and(|owning| {
        matches!(
            &owning.specifications,
            TypeSpecification::Quantity { units, .. } if units.get(other).is_ok()
        )
    })
}

/// Unit reconciliation facts for one operator's operands, in operand order:
/// each unit that shares a quantity family with an earlier-seen different
/// unit yields one equivalence line. Pure unit-index metadata; no values are
/// computed.
fn unit_equivalence_nodes(
    operands: &[&Expression],
    ctx: &ExplainCtx<'_, '_>,
) -> Vec<ExplanationNode> {
    let unit_index = ctx.plan.expression_unit_index();
    let resolution = UnitResolutionContext::WithIndex(unit_index);
    let mut seen: Vec<String> = Vec::new();
    let mut nodes = Vec::new();
    for operand in operands {
        let Some(value) = operand_leaf_value(operand, ctx) else {
            continue;
        };
        if !matches!(value.value, ValueKind::Quantity(_, _)) {
            continue;
        }
        // Expand named compound units (eur_per_km → eur/kilometer) so family
        // members are visible.
        let expanded =
            crate::planning::normalize::expand_named_quantity_literal(&value, Some(&resolution))
                .unwrap_or(value);
        let ValueKind::Quantity(_, signature) = &expanded.value else {
            continue;
        };
        for (unit, _) in signature {
            if seen.iter().any(|known| known == unit) {
                continue;
            }
            if let Some(earlier) = seen
                .iter()
                .find(|earlier| same_quantity_family(unit, earlier, unit_index))
            {
                if let Some(owning) = unit_index.get(unit.as_str()) {
                    if let Some(text) = quantity_unit_equivalence_step_text(
                        &[(unit.clone(), 1)],
                        earlier,
                        owning,
                        UnitResolutionContext::WithIndex(unit_index),
                    ) {
                        nodes.push(ExplanationNode::UnitEquivalence { text });
                    }
                }
            }
            seen.push(unit.clone());
        }
    }
    nodes
}

fn build_expression_children(expr: &Expression, ctx: &ExplainCtx<'_, '_>) -> Vec<ExplanationNode> {
    match &expr.kind {
        ExpressionKind::RulePath(rule_path) => vec![embed_rule(rule_path, ctx.built)],
        ExpressionKind::DataPath(data_path) => vec![build_data_input_node(data_path, ctx)],
        ExpressionKind::Literal(_) => Vec::new(),
        ExpressionKind::Arithmetic(_, op, _)
            if matches!(
                op,
                ArithmeticComputation::Add | ArithmeticComputation::Multiply
            ) =>
        {
            let mut operands = Vec::new();
            flatten_arithmetic_chain(expr, op, &mut operands);
            build_operand_nodes(&operands, ctx)
        }
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right)
        | ExpressionKind::RangeLiteral(left, right)
        | ExpressionKind::RangeContainment(left, right) => {
            build_operand_nodes(&[left.as_ref(), right.as_ref()], ctx)
        }
        ExpressionKind::LogicalNegation(operand, _)
        | ExpressionKind::MathematicalComputation(_, operand)
        | ExpressionKind::ResultIsVeto(operand)
        | ExpressionKind::PastFutureRange(_, operand)
        | ExpressionKind::DateRelative(_, operand)
        | ExpressionKind::DateCalendar(_, _, operand) => {
            build_operand_nodes(&[operand.as_ref()], ctx)
        }
        ExpressionKind::Veto(veto_expr) => {
            if veto_expr.message.is_none() {
                Vec::new()
            } else {
                vec![ExplanationNode::Veto {
                    message: veto_expr.message.clone(),
                }]
            }
        }
        ExpressionKind::UnitConversion(value_expr, target) => {
            vec![build_conversion_node(value_expr, target, expr, ctx)]
        }
        ExpressionKind::Now => Vec::new(),
        ExpressionKind::Piecewise(_) => {
            unreachable!("BUG: Piecewise in source expression for explanation")
        }
    }
}

fn build_expression_node(expr: &Expression, ctx: &ExplainCtx<'_, '_>) -> ExplanationNode {
    match &expr.kind {
        ExpressionKind::RulePath(rule_path) => embed_rule(rule_path, ctx.built),
        ExpressionKind::DataPath(data_path) => build_data_input_node(data_path, ctx),
        ExpressionKind::Literal(_) => {
            unreachable!("BUG: literal operands are filtered before node construction")
        }
        ExpressionKind::UnitConversion(value_expr, target) => {
            build_conversion_node(value_expr, target, expr, ctx)
        }
        ExpressionKind::Veto(veto_expr) => ExplanationNode::Veto {
            message: veto_expr.message.clone(),
        },
        ExpressionKind::Now => ExplanationNode::DataInput {
            data: DataPath::local(String::new()),
            display: ctx.context.now().display_value(),
        },
        ExpressionKind::Piecewise(_) => {
            unreachable!("BUG: Piecewise in source expression for explanation")
        }
        _ => ExplanationNode::Compose {
            expression: format_expression(expr),
            operands: build_expression_children(expr, ctx),
        },
    }
}

/// Recorded operand/result values for a conversion expression, looked up via
/// the conversion provenance tag matching the expression's source location.
/// `None` when the conversion was not executed (untaken branch) or carries no
/// tag (synthetic expression without a source location).
fn recorded_conversion_values(
    expr: &Expression,
    ctx: &ExplainCtx<'_, '_>,
) -> Option<(OperationResult, OperationResult)> {
    let source = expr.source_location.as_ref()?;
    for tag in &ctx.instructions.conversion_tags {
        if &tag.source != source {
            continue;
        }
        let Instruction::UnitConversion {
            destination_register,
            source_register,
            ..
        } = &ctx.instructions.code[tag.pc as usize]
        else {
            unreachable!("BUG: conversion tag must reference a UnitConversion instruction");
        };
        let operand = ctx
            .recording
            .registers
            .get(*source_register as usize)
            .cloned()
            .flatten();
        let result = ctx
            .recording
            .registers
            .get(*destination_register as usize)
            .cloned()
            .flatten();
        if let (Some(operand), Some(result)) = (operand, result) {
            return Some((operand, result));
        }
    }
    None
}

fn build_conversion_node(
    value_expr: &Expression,
    target: &SemanticConversionTarget,
    expr: &Expression,
    ctx: &ExplainCtx<'_, '_>,
) -> ExplanationNode {
    let steps = match recorded_conversion_values(expr, ctx) {
        Some((OperationResult::Veto(veto), _)) => {
            return ExplanationNode::Veto {
                message: Some(veto.to_string()),
            };
        }
        Some((_, OperationResult::Veto(veto))) => {
            return ExplanationNode::Veto {
                message: Some(veto.to_string()),
            };
        }
        Some((OperationResult::Value(operand_value), OperationResult::Value(converted_value))) => {
            let data_ref = data_path_in_expression(value_expr);
            build_conversion_steps(
                &operand_value,
                target,
                &converted_value,
                data_ref.as_ref(),
                UnitResolutionContext::WithIndex(ctx.plan.expression_unit_index()),
            )
        }
        // Not executed (or untagged): the conversion's structure still
        // renders; only value-bearing steps are omitted.
        None => Vec::new(),
    };

    // The source step may already name the operand data leaf and its value
    // ("The quantity of mass is 2 kilogram"); a child would restate it.
    let operand_named_in_steps = data_path_in_expression(value_expr)
        .map(|path| {
            steps
                .iter()
                .any(|step| step.data_ref.as_ref() == Some(&path))
        })
        .unwrap_or(false);
    let operands = if is_literal(value_expr) || operand_named_in_steps {
        Vec::new()
    } else {
        vec![build_expression_node(value_expr, ctx)]
    };
    ExplanationNode::Conversion {
        expression: format_expression(expr),
        steps: steps
            .iter()
            .map(SerializedConversionTraceStep::from)
            .collect(),
        operands,
    }
}

impl From<&ConversionTraceStep> for SerializedConversionTraceStep {
    fn from(step: &ConversionTraceStep) -> Self {
        Self {
            role: match step.role {
                ConversionTraceRole::Outcome => "outcome".to_string(),
                ConversionTraceRole::Rule => "rule".to_string(),
                ConversionTraceRole::Source => "source".to_string(),
            },
            text: step.text.clone(),
        }
    }
}

fn build_data_input_node(data_path: &DataPath, ctx: &ExplainCtx<'_, '_>) -> ExplanationNode {
    match resolve_data_path_value(data_path, ctx.context) {
        OperationResult::Value(value) => ExplanationNode::DataInput {
            data: data_path.clone(),
            display: value.display_value(),
        },
        OperationResult::Veto(veto) => ExplanationNode::Veto {
            message: Some(veto.to_string()),
        },
    }
}

fn data_path_in_expression(value_expr: &Expression) -> Option<DataPath> {
    if let ExpressionKind::DataPath(data_path) = &value_expr.kind {
        Some(data_path.clone())
    } else {
        None
    }
}

pub fn format_explanation(explanation: &Explanation) -> String {
    let mut lines = Vec::new();
    let result_display = format_operation_result(&explanation.result);
    lines.push(format!("{}: {}", explanation.rule.rule, result_display));
    let mut ctx = FormatContext {
        lines: &mut lines,
        indent: String::new(),
    };
    ctx.render_rule_contents(
        &result_display,
        &explanation.body,
        &explanation.causes,
        &explanation.children,
    );
    lines.join("\n")
}

#[derive(Copy, Clone)]
enum Connector {
    Branch,
    Last,
}

struct FormatContext<'a> {
    lines: &'a mut Vec<String>,
    indent: String,
}

impl<'a> FormatContext<'a> {
    fn push_line(&mut self, connector: Connector, text: &str) {
        self.lines.push(format!(
            "{}{} {text}",
            self.indent,
            connector_str(connector)
        ));
    }

    fn child_indent(&self, connector: Connector) -> String {
        match connector {
            Connector::Branch => format!("{}│  ", self.indent),
            Connector::Last => format!("{}   ", self.indent),
        }
    }

    /// Render a rule's contents under its headline: causes first (branch
    /// selection facts), then the body expression with its operand subtree.
    /// A body that restates the result verbatim (literal branch results) is
    /// omitted.
    fn render_rule_contents(
        &mut self,
        result_display: &str,
        body: &str,
        causes: &[Cause],
        children: &[ExplanationNode],
    ) {
        let body_shown = !body.is_empty() && body != result_display;
        let total = causes.len() + usize::from(body_shown);
        let mut index = 0;

        for cause in causes {
            index += 1;
            let connector = if index == total {
                Connector::Last
            } else {
                Connector::Branch
            };
            let line = if cause.value == "true" {
                cause.condition.clone()
            } else {
                format!("{} is {}", cause.condition, cause.value)
            };
            self.push_line(connector, &line);
            let child_indent = self.child_indent(connector);
            let mut child_ctx = FormatContext {
                lines: self.lines,
                indent: child_indent,
            };
            child_ctx.render_nodes(&cause.children, None);
        }

        if body_shown {
            self.push_line(Connector::Last, body);
            let child_indent = self.child_indent(Connector::Last);
            let mut child_ctx = FormatContext {
                lines: self.lines,
                indent: child_indent,
            };
            child_ctx.render_nodes(children, Some(body));
        } else if !children.is_empty() {
            self.render_nodes(children, None);
        }
    }

    fn render_nodes(&mut self, nodes: &[ExplanationNode], parent_body: Option<&str>) {
        let len = nodes.len();
        for (i, node) in nodes.iter().enumerate() {
            let connector = if i + 1 == len {
                Connector::Last
            } else {
                Connector::Branch
            };
            self.render_node(node, connector, parent_body);
        }
    }

    /// Conversion trace steps followed by operand subtrees, as one sibling
    /// sequence with correct connectors.
    fn render_conversion_contents(
        &mut self,
        steps: &[SerializedConversionTraceStep],
        operands: &[ExplanationNode],
    ) {
        let total = steps.len() + operands.len();
        let mut index = 0;
        for step in steps {
            index += 1;
            let connector = if index == total {
                Connector::Last
            } else {
                Connector::Branch
            };
            self.push_line(connector, &step.text);
        }
        for operand in operands {
            index += 1;
            let connector = if index == total {
                Connector::Last
            } else {
                Connector::Branch
            };
            self.render_node(operand, connector, None);
        }
    }

    fn render_node(
        &mut self,
        node: &ExplanationNode,
        connector: Connector,
        parent_body: Option<&str>,
    ) {
        match node {
            ExplanationNode::Rule {
                rule,
                result,
                body,
                causes,
                children,
            } => {
                self.push_line(connector, &format!("{}: {result}", rule.rule));
                let child_indent = self.child_indent(connector);
                let mut child_ctx = FormatContext {
                    lines: self.lines,
                    indent: child_indent,
                };
                child_ctx.render_rule_contents(result, body, causes, children);
            }
            ExplanationNode::Compose {
                expression,
                operands,
            } => {
                self.push_line(connector, expression);
                let child_indent = self.child_indent(connector);
                let mut child_ctx = FormatContext {
                    lines: self.lines,
                    indent: child_indent,
                };
                child_ctx.render_nodes(operands, None);
            }
            ExplanationNode::DataInput { data, display } => {
                if data.data.is_empty() {
                    self.push_line(connector, display);
                } else {
                    self.push_line(connector, &format!("{data}: {display}"));
                }
            }
            ExplanationNode::Conversion {
                expression,
                steps,
                operands,
            } => {
                // When the parent body line already names this conversion,
                // its steps and operands render directly at this level. The
                // outcome step is dropped there: the conversion's result is
                // the rule result already shown in the headline.
                let expression_is_parent_body = parent_body.is_some_and(|body| body == expression);
                if expression_is_parent_body {
                    let steps_without_outcome: Vec<SerializedConversionTraceStep> = steps
                        .iter()
                        .filter(|step| step.role != "outcome")
                        .cloned()
                        .collect();
                    self.render_conversion_contents(&steps_without_outcome, operands);
                } else {
                    self.push_line(connector, expression);
                    let child_indent = self.child_indent(connector);
                    let mut child_ctx = FormatContext {
                        lines: self.lines,
                        indent: child_indent,
                    };
                    child_ctx.render_conversion_contents(steps, operands);
                }
            }
            ExplanationNode::Veto { message } => {
                self.push_line(
                    connector,
                    message
                        .as_deref()
                        .expect("BUG: veto explanation must carry message"),
                );
            }
            ExplanationNode::UnitEquivalence { text } => {
                self.push_line(connector, text);
            }
        }
    }
}

fn connector_str(connector: Connector) -> &'static str {
    match connector {
        Connector::Branch => "├─",
        Connector::Last => "└─",
    }
}

fn expression_precedence(kind: &ExpressionKind) -> u8 {
    match kind {
        ExpressionKind::LogicalAnd(..) | ExpressionKind::LogicalOr(..) => 2,
        ExpressionKind::LogicalNegation(..) => 3,
        ExpressionKind::Comparison(..) | ExpressionKind::ResultIsVeto(..) => 4,
        ExpressionKind::RangeContainment(..) => 4,
        ExpressionKind::DateRelative(..) | ExpressionKind::DateCalendar(..) => 4,
        ExpressionKind::Arithmetic(_, op, _) => match op {
            ArithmeticComputation::Add | ArithmeticComputation::Subtract => 5,
            ArithmeticComputation::Multiply
            | ArithmeticComputation::Divide
            | ArithmeticComputation::Modulo => 6,
            ArithmeticComputation::Power => 7,
        },
        ExpressionKind::UnitConversion(..) => 8,
        ExpressionKind::RangeLiteral(..) => 9,
        ExpressionKind::MathematicalComputation(..) | ExpressionKind::PastFutureRange(..) => 10,
        ExpressionKind::Literal(_)
        | ExpressionKind::DataPath(_)
        | ExpressionKind::RulePath(_)
        | ExpressionKind::Now
        | ExpressionKind::Veto(_)
        | ExpressionKind::Piecewise(_) => 10,
    }
}

fn write_expression_child(out: &mut String, child: &Expression, parent_prec: u8) {
    let child_prec = expression_precedence(&child.kind);
    if child_prec < parent_prec {
        out.push('(');
        out.push_str(&format_expression(child));
        out.push(')');
    } else {
        out.push_str(&format_expression(child));
    }
}

pub fn format_expression(expr: &Expression) -> String {
    match &expr.kind {
        ExpressionKind::Literal(lit) => lit.display_value(),
        ExpressionKind::DataPath(path) => path.to_string(),
        ExpressionKind::RulePath(path) => path.to_string(),
        ExpressionKind::Arithmetic(left, op, right) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = String::new();
            write_expression_child(&mut out, left, my_prec);
            out.push(' ');
            out.push_str(&op.to_string());
            out.push(' ');
            write_expression_child(&mut out, right, my_prec);
            out
        }
        ExpressionKind::Comparison(left, op, right) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = String::new();
            write_expression_child(&mut out, left, my_prec);
            out.push(' ');
            out.push_str(&op.to_string());
            out.push(' ');
            write_expression_child(&mut out, right, my_prec);
            out
        }
        ExpressionKind::UnitConversion(value, target) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = String::new();
            write_expression_child(&mut out, value, my_prec);
            out.push_str(" as ");
            out.push_str(&target.to_string());
            out
        }
        ExpressionKind::LogicalNegation(inner, negation) => {
            if let (NegationType::Not, ExpressionKind::ResultIsVeto(operand)) =
                (negation, &inner.kind)
            {
                let my_prec = expression_precedence(&expr.kind);
                let mut out = String::new();
                write_expression_child(&mut out, operand, my_prec);
                out.push_str(" is not veto");
                out
            } else {
                let my_prec = expression_precedence(&expr.kind);
                let mut out = String::from("not ");
                write_expression_child(&mut out, inner, my_prec);
                out
            }
        }
        ExpressionKind::ResultIsVeto(operand) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = String::new();
            write_expression_child(&mut out, operand, my_prec);
            out.push_str(" is veto");
            out
        }
        ExpressionKind::LogicalAnd(left, right) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = String::new();
            write_expression_child(&mut out, left, my_prec);
            out.push_str(" and ");
            write_expression_child(&mut out, right, my_prec);
            out
        }
        ExpressionKind::LogicalOr(left, right) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = String::new();
            write_expression_child(&mut out, left, my_prec);
            out.push_str(" or ");
            write_expression_child(&mut out, right, my_prec);
            out
        }
        ExpressionKind::MathematicalComputation(op, operand) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = format!("{op} ");
            write_expression_child(&mut out, operand, my_prec);
            out
        }
        ExpressionKind::Veto(veto) => match &veto.message {
            Some(msg) => format!("veto \"{msg}\""),
            None => "veto".to_string(),
        },
        ExpressionKind::Now => "now".to_string(),
        ExpressionKind::DateRelative(kind, date_expr) => {
            format!("{} {}", format_expression(date_expr), kind)
        }
        ExpressionKind::DateCalendar(kind, unit, date_expr) => {
            format!("{} {} {}", format_expression(date_expr), kind, unit)
        }
        ExpressionKind::RangeLiteral(left, right) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = String::new();
            write_expression_child(&mut out, left, my_prec);
            out.push_str("...");
            write_expression_child(&mut out, right, my_prec);
            out
        }
        ExpressionKind::PastFutureRange(kind, offset_expr) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = format!("{} ", kind);
            write_expression_child(&mut out, offset_expr, my_prec);
            out
        }
        ExpressionKind::RangeContainment(value, range) => {
            let my_prec = expression_precedence(&expr.kind);
            let mut out = String::new();
            write_expression_child(&mut out, value, my_prec);
            out.push_str(" in ");
            write_expression_child(&mut out, range, my_prec);
            out
        }
        ExpressionKind::Piecewise(_) => {
            unreachable!("BUG: Piecewise in source expression for explanation formatting")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::rational_new;
    use crate::computation::UnitResolutionContext;
    use crate::literals::DateGranularity;
    use crate::literals::QuantityUnit;
    use crate::parsing::ast::DateTimeValue;
    use crate::parsing::source::SourceType;
    use crate::planning::semantics::{
        date_time_to_semantic, DataPath, LemmaType, LiteralValue, QuantityUnits, RulePath,
        SemanticConversionTarget, TypeSpecification, ValueKind,
    };
    use crate::Engine;
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    const CALC_SPEC: &str = r#"
spec calc

data money: quantity
  -> decimals 2
  -> unit eur 1

data hourly_rate: 85.00 eur
data hours_worked: 37.5
data is_rush: boolean
data is_super_rush: boolean

rule labor: hourly_rate * hours_worked
rule rush_surcharge: 0 eur
  unless is_rush then labor * 25%
  unless is_super_rush then labor * 50%
rule subtotal: labor + rush_surcharge
rule vat: subtotal * 21%
rule total: subtotal + vat
"#;

    const CALC_TOTAL_IS_RUSH_ONLY_GOLDEN_JSON: &str = r#"{
  "rule": "total",
  "result": "4821.09 eur",
  "body": "subtotal + vat",
  "children": [
    {
      "type": "rule",
      "rule": "subtotal",
      "result": "3984.38 eur",
      "body": "labor + rush_surcharge",
      "children": [
        {
          "type": "rule",
          "rule": "labor",
          "result": "3187.50 eur",
          "body": "hourly_rate * hours_worked",
          "children": [
            {
              "type": "data_input",
              "data": "hourly_rate",
              "display": "85.00 eur"
            },
            {
              "type": "data_input",
              "data": "hours_worked",
              "display": "37.5"
            }
          ]
        },
        {
          "type": "rule",
          "rule": "rush_surcharge",
          "result": "796.88 eur",
          "body": "labor * 25%",
          "causes": [
            {
              "condition": "is_rush is true",
              "value": "true"
            }
          ],
          "children": [
            {
              "type": "rule",
              "rule": "labor",
              "result": "3187.50 eur",
              "body": "hourly_rate * hours_worked",
              "children": [
                {
                  "type": "data_input",
                  "data": "hourly_rate",
                  "display": "85.00 eur"
                },
                {
                  "type": "data_input",
                  "data": "hours_worked",
                  "display": "37.5"
                }
              ]
            }
          ]
        }
      ]
    },
    {
      "type": "rule",
      "rule": "vat",
      "result": "836.72 eur",
      "body": "subtotal * 21%",
      "children": [
        {
          "type": "rule",
          "rule": "subtotal",
          "result": "3984.38 eur",
          "body": "labor + rush_surcharge",
          "children": [
            {
              "type": "rule",
              "rule": "labor",
              "result": "3187.50 eur",
              "body": "hourly_rate * hours_worked",
              "children": [
                {
                  "type": "data_input",
                  "data": "hourly_rate",
                  "display": "85.00 eur"
                },
                {
                  "type": "data_input",
                  "data": "hours_worked",
                  "display": "37.5"
                }
              ]
            },
            {
              "type": "rule",
              "rule": "rush_surcharge",
              "result": "796.88 eur",
              "body": "labor * 25%",
              "causes": [
                {
                  "condition": "is_rush is true",
                  "value": "true"
                }
              ],
              "children": [
                {
                  "type": "rule",
                  "rule": "labor",
                  "result": "3187.50 eur",
                  "body": "hourly_rate * hours_worked",
                  "children": [
                    {
                      "type": "data_input",
                      "data": "hourly_rate",
                      "display": "85.00 eur"
                    },
                    {
                      "type": "data_input",
                      "data": "hours_worked",
                      "display": "37.5"
                    }
                  ]
                }
              ]
            }
          ]
        }
      ]
    }
  ]
}"#;

    fn rush_surcharge_causes(data: HashMap<String, String>) -> serde_json::Value {
        let mut engine = Engine::new();
        engine
            .load(CALC_SPEC, crate::SourceType::Volatile)
            .expect("calc spec loads");
        let now = DateTimeValue::now();
        let response = engine
            .run(None, "calc", Some(&now), data, true, None)
            .expect("calc eval succeeds");
        let explanation = response
            .results
            .get("rush_surcharge")
            .expect("rush_surcharge rule evaluated")
            .explanation
            .as_ref()
            .expect("explanation always built");
        serde_json::to_value(&explanation.causes).expect("causes serialize")
    }

    #[test]
    fn unless_causes_neither_matches() {
        let mut data = HashMap::new();
        data.insert("is_rush".into(), "false".into());
        data.insert("is_super_rush".into(), "false".into());
        let causes = rush_surcharge_causes(data);
        assert_eq!(
            causes,
            serde_json::json!([
                { "condition": "is_rush is false", "value": "true" },
                { "condition": "is_super_rush is false", "value": "true" },
            ])
        );
    }

    #[test]
    fn calc_total_is_rush_only_serializes_to_golden_json() {
        let mut data = HashMap::new();
        data.insert("is_rush".into(), "true".into());
        data.insert("is_super_rush".into(), "false".into());

        let mut engine = Engine::new();
        engine
            .load(CALC_SPEC, crate::SourceType::Volatile)
            .expect("calc spec loads");
        let now = DateTimeValue::now();
        let response = engine
            .run(None, "calc", Some(&now), data, true, None)
            .expect("calc eval succeeds");
        let explanation = response
            .results
            .get("total")
            .expect("total rule evaluated")
            .explanation
            .as_ref()
            .expect("explanation always built");

        let actual: serde_json::Value =
            serde_json::to_value(explanation).expect("explanation serializes");
        let expected: serde_json::Value =
            serde_json::from_str(CALC_TOTAL_IS_RUSH_ONLY_GOLDEN_JSON).expect("golden json parses");
        assert_eq!(actual, expected);
    }

    #[test]
    fn unless_causes_is_rush_only() {
        let mut data = HashMap::new();
        data.insert("is_rush".into(), "true".into());
        data.insert("is_super_rush".into(), "false".into());
        let causes = rush_surcharge_causes(data);
        assert_eq!(
            causes,
            serde_json::json!([
                { "condition": "is_rush is true", "value": "true" },
            ])
        );
    }

    #[test]
    fn unless_causes_is_super_rush() {
        let mut data = HashMap::new();
        data.insert("is_rush".into(), "true".into());
        data.insert("is_super_rush".into(), "true".into());
        let causes = rush_surcharge_causes(data);
        assert_eq!(
            causes,
            serde_json::json!([
                { "condition": "is_super_rush is true", "value": "true" },
            ])
        );
    }

    #[test]
    fn conversion_source_step_text_with_data_reference() {
        let operand = LiteralValue::quantity_with_type(
            rational_new(2, 1),
            "kilogram".to_string(),
            Arc::new(LemmaType::primitive(TypeSpecification::quantity())),
        );
        let path = DataPath::local("mass".to_string());
        let text = conversion_source_step_text(&operand, Some(&path));
        assert_eq!(text, "The quantity of mass is 2 kilogram");
    }

    #[test]
    fn build_conversion_steps_scalar_quantity() {
        let mut units = QuantityUnits::new();
        units.0.push(
            QuantityUnit::from_decimal_factor("kilogram".to_string(), Decimal::ONE, vec![])
                .unwrap(),
        );
        units.0.push(
            QuantityUnit::from_decimal_factor("gram".to_string(), Decimal::new(1, 3), vec![])
                .unwrap(),
        );
        let lemma_type = Arc::new(LemmaType::primitive(TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units,
            traits: vec![],
            decomposition: Default::default(),
            help: String::new(),
        }));
        let operand = LiteralValue::quantity_with_type(
            rational_new(2, 1),
            "kilogram".to_string(),
            Arc::clone(&lemma_type),
        );
        let result =
            LiteralValue::quantity_with_type(rational_new(2, 1), "gram".to_string(), lemma_type);
        let path = DataPath::local("mass".to_string());
        let steps = build_conversion_steps(
            &operand,
            &SemanticConversionTarget::Unit {
                unit_name: "gram".to_string(),
            },
            &result,
            Some(&path),
            UnitResolutionContext::NamedQuantityOnly,
        );
        assert_eq!(steps.len(), 3);
        assert!(matches!(steps[0].role, ConversionTraceRole::Outcome));
        assert_eq!(steps[0].text, "2000 gram");
        assert!(matches!(steps[1].role, ConversionTraceRole::Rule));
        assert_eq!(steps[1].text, "1 kilogram is 1000 gram");
        assert!(matches!(steps[2].role, ConversionTraceRole::Source));
        assert_eq!(steps[2].text, "The quantity of mass is 2 kilogram");
        assert_eq!(steps[2].data_ref, Some(path));
    }

    #[test]
    fn build_conversion_steps_date_range() {
        let left = LiteralValue::date(date_time_to_semantic(&DateTimeValue {
            year: 2024,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        }));
        let right = LiteralValue::date(date_time_to_semantic(&DateTimeValue {
            year: 2024,
            month: 6,
            day: 15,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        }));
        let range = LiteralValue {
            value: ValueKind::Range(Box::new(left), Box::new(right)),
            lemma_type: Arc::new(LemmaType::primitive(TypeSpecification::date_range())),
        };
        let result = LiteralValue::quantity_with_type(
            rational_new(14, 1),
            "days".to_string(),
            Arc::new(LemmaType::primitive(TypeSpecification::quantity())),
        );
        let path = DataPath::local("age".to_string());
        let steps = build_conversion_steps(
            &range,
            &SemanticConversionTarget::Unit {
                unit_name: "days".to_string(),
            },
            &result,
            Some(&path),
            UnitResolutionContext::WithIndex(&HashMap::new()),
        );
        assert_eq!(steps.len(), 3);
        assert!(steps[1].text.contains('−'));
        assert!(steps[1].text.contains("2024-06-15"));
        assert!(steps[1].text.contains("2024-06-01"));
        assert!(steps[1].text.contains("14"));
        assert!(steps[2].text.contains("The date range of age is"));
    }

    #[test]
    fn build_conversion_steps_identity_omits_rule_and_source() {
        let mut units = QuantityUnits::new();
        units.0.push(
            QuantityUnit::from_decimal_factor("kilogram".to_string(), Decimal::ONE, vec![])
                .unwrap(),
        );
        let lemma_type = Arc::new(LemmaType::primitive(TypeSpecification::Quantity {
            minimum: None,
            maximum: None,
            decimals: None,
            units,
            traits: vec![],
            decomposition: Default::default(),
            help: String::new(),
        }));
        let operand = LiteralValue::quantity_with_type(
            rational_new(2, 1),
            "kilogram".to_string(),
            Arc::clone(&lemma_type),
        );
        let result = LiteralValue::quantity_with_type(
            rational_new(2, 1),
            "kilogram".to_string(),
            lemma_type,
        );
        let steps = build_conversion_steps(
            &operand,
            &SemanticConversionTarget::Unit {
                unit_name: "kilogram".to_string(),
            },
            &result,
            None,
            UnitResolutionContext::NamedQuantityOnly,
        );
        // Identity conversion: no factor to show, and a source step would
        // restate the outcome value verbatim.
        assert_eq!(steps.len(), 1);
        assert!(matches!(steps[0].role, ConversionTraceRole::Outcome));
    }

    #[test]
    fn conversion_trace_step_roundtrip() {
        let step = ConversionTraceStep {
            role: ConversionTraceRole::Rule,
            text: "1 kilogram is 1000 gram".to_string(),
            data_ref: Some(DataPath::local("mass".to_string())),
        };
        assert_eq!(step.text, "1 kilogram is 1000 gram");
        assert!(matches!(step.role, ConversionTraceRole::Rule));
    }

    #[test]
    fn explanation_for_compound_signature_uses_signature_factor() {
        let code = r#"spec t
uses lemma units
data money: quantity
  -> unit eur 1
data rate: quantity
  -> unit eur_per_minute eur/minute
data r: 40 eur_per_minute
data h: 2 hour
rule cost: (r * h) as eur
"#;
        let mut engine = Engine::new();
        engine
            .load(code, SourceType::Path(Arc::new(PathBuf::from("t.lemma"))))
            .expect("must load");
        let response = engine
            .run(None, "t", None, HashMap::new(), true, None)
            .expect("must eval");
        let cost_result = response.results.get("cost").expect("rule must exist");
        let display = cost_result
            .display
            .as_deref()
            .expect("must have display value");
        assert!(
            display.contains("4800") && display.contains("eur"),
            "expected 4800 eur, got: {display}"
        );
    }

    #[test]
    fn render_veto_with_none_message_must_not_use_placeholder_text() {
        use crate::evaluation::operations::{OperationResult, VetoType};

        let explanation = Explanation {
            rule: RulePath::new(vec![], "r".into()),
            result: OperationResult::Veto(VetoType::computation("test")),
            body: "expr".into(),
            causes: vec![],
            children: vec![ExplanationNode::Veto { message: None }],
        };
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            format_explanation(&explanation);
        }));
        assert!(
            panic.is_err(),
            "veto node without message must crash, not render placeholder"
        );
    }
}
