//! Structural explanation trees for evaluated rules.

use crate::computation::rational::checked_div;
use crate::computation::UnitResolutionContext;
use crate::evaluation::expression::{resolve_data_path_value, resolve_source_expression_values};
use crate::evaluation::operations::{OperationResult, VetoType};
use crate::evaluation::EvaluationContext;
use crate::planning::execution_plan::{ExecutableRule, ExecutionPlan};
use crate::planning::semantics::{
    compare_semantic_dates, ArithmeticComputation, DataPath, Expression, ExpressionKind, LemmaType,
    LiteralValue, NegationType, RulePath, SemanticConversionTarget, TypeSpecification, ValueKind,
};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cause {
    pub condition: String,
    pub value: String,
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

    steps.push(ConversionTraceStep {
        role: ConversionTraceRole::Source,
        text: conversion_source_step_text(value, data_ref),
        data_ref: data_ref.cloned(),
    });

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
    let reduced = rational
        .clone()
        .try_reduce()
        .unwrap_or_else(|_| rational.clone());
    if reduced.denom() == &crate::computation::rational::BigInt::one() {
        reduced.numer().to_string()
    } else {
        format!("{}/{}", reduced.numer(), reduced.denom())
    }
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
pub(crate) enum WinningSourceBranch<'a> {
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

/// Causes for one evaluated unless condition: per referenced data path, or a
/// single condition-level cause when the condition references no data.
fn unless_condition_causes<'plan>(
    condition: &Expression,
    condition_result: &OperationResult,
    context: &mut EvaluationContext<'plan>,
) -> Vec<Cause> {
    let mut data_paths = std::collections::HashSet::new();
    condition.collect_data_paths(&mut data_paths);
    let mut paths: Vec<DataPath> = data_paths.into_iter().collect();
    paths.sort();
    if paths.is_empty() {
        let value = match condition_result {
            OperationResult::Veto(veto) => veto.to_string(),
            OperationResult::Value(literal) => match &literal.value {
                ValueKind::Boolean(value) => LiteralValue::from_bool(*value).display_value(),
                _ => {
                    unreachable!("BUG: unless condition non-boolean after type validation")
                }
            },
        };
        return vec![Cause {
            condition: format_expression(condition),
            value,
        }];
    }
    paths
        .into_iter()
        .map(|data_path| {
            let path_expr = Expression::with_source(
                ExpressionKind::DataPath(data_path.clone()),
                condition.source_location.clone(),
            );
            let value = match resolve_source_expression_values(&path_expr, context) {
                OperationResult::Value(literal) => literal.display_value(),
                OperationResult::Veto(veto) => context
                    .unique_data_value_by_name(&data_path.data)
                    .map(|value| value.display_value())
                    .unwrap_or_else(|| veto.to_string()),
            };
            Cause {
                condition: data_path.data.clone(),
                value,
            }
        })
        .collect()
}

pub(crate) fn winning_source_branch_and_causes<'a, 'plan>(
    exec_rule: &'a ExecutableRule,
    context: &mut EvaluationContext<'plan>,
) -> WinningSourceBranch<'a> {
    use crate::evaluation::branch_semantics::{unless_condition_outcome, BranchOutcome};
    use crate::planning::execution_plan::JumpVetoSemantics;

    if exec_rule.branches.len() == 1 {
        return WinningSourceBranch::BranchResult {
            result_expression: &exec_rule.branches[0].result,
            causes: Vec::new(),
        };
    }

    // Mirror the compiled piecewise exactly (`compile_piecewise_rule`):
    // unless conditions are evaluated in reverse source order (last match
    // wins) under rule-reference veto semantics. The first condition that
    // vetoes propagates as the rule result; the first that is true selects
    // its branch; only the conditions actually evaluated produce causes.
    let mut evaluated_in_reverse_order: Vec<Vec<Cause>> = Vec::new();
    for branch in exec_rule.branches[1..].iter().rev() {
        let condition = branch
            .condition
            .as_ref()
            .expect("BUG: unless branch missing condition");
        let condition_result = resolve_source_expression_values(condition, context);
        let causes = unless_condition_causes(condition, &condition_result, context);
        match unless_condition_outcome(&condition_result, JumpVetoSemantics::UnlessRuleReference) {
            BranchOutcome::Propagate(_) => {
                evaluated_in_reverse_order.push(causes);
                return WinningSourceBranch::ConditionVeto {
                    condition_expression: condition,
                    causes: causes_in_source_order(evaluated_in_reverse_order, false),
                };
            }
            BranchOutcome::Taken => {
                evaluated_in_reverse_order.push(causes);
                return WinningSourceBranch::BranchResult {
                    result_expression: &branch.result,
                    causes: causes_in_source_order(evaluated_in_reverse_order, false),
                };
            }
            BranchOutcome::NotTaken => {
                evaluated_in_reverse_order.push(causes);
            }
        }
    }

    WinningSourceBranch::BranchResult {
        result_expression: &exec_rule.branches[0].result,
        causes: causes_in_source_order(evaluated_in_reverse_order, true),
    }
}

/// Flatten per-condition causes (collected in reverse evaluation order) back
/// into source order. When the default branch wins every condition was
/// evaluated; duplicate condition texts are deduplicated, keeping the first
/// occurrence in source order (preserving the established output shape).
fn causes_in_source_order(
    evaluated_in_reverse_order: Vec<Vec<Cause>>,
    deduplicate: bool,
) -> Vec<Cause> {
    let mut causes: Vec<Cause> = evaluated_in_reverse_order
        .into_iter()
        .rev()
        .flatten()
        .collect();
    if deduplicate {
        let mut seen = std::collections::HashSet::new();
        causes.retain(|cause| seen.insert(cause.condition.clone()));
    }
    causes
}

pub fn build_explanation<'plan>(
    exec_rule: &ExecutableRule,
    context: &mut EvaluationContext<'plan>,
    plan: &ExecutionPlan,
    built: &HashMap<RulePath, Explanation>,
) -> Explanation {
    let authoritative_result = context
        .rule_results
        .get(&exec_rule.path)
        .expect("BUG: rule evaluated before explain")
        .clone();

    let (body, causes, children) = match winning_source_branch_and_causes(exec_rule, context) {
        WinningSourceBranch::BranchResult {
            result_expression,
            causes,
        } => (
            format_expression(result_expression),
            causes,
            build_expression_children(result_expression, context, plan, built),
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
            build_expression_children(condition_expression, context, plan, built),
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

fn build_expression_children<'plan>(
    expr: &Expression,
    context: &mut EvaluationContext<'plan>,
    plan: &ExecutionPlan,
    built: &HashMap<RulePath, Explanation>,
) -> Vec<ExplanationNode> {
    if let Some(rule_paths) = direct_in_spec_rule_children(expr) {
        return rule_paths
            .into_iter()
            .map(|rule_path| embed_rule(&rule_path, built))
            .collect();
    }

    if let Some(data_paths) = direct_data_children(expr) {
        return data_paths
            .into_iter()
            .map(|data_path| build_data_input_node(&data_path, context))
            .collect();
    }

    if matches!(expr.kind, ExpressionKind::Literal(_)) {
        return Vec::new();
    }

    match &expr.kind {
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right)
        | ExpressionKind::RangeLiteral(left, right)
        | ExpressionKind::RangeContainment(left, right) => {
            let operands = vec![
                build_expression_node(left, context, plan, built),
                build_expression_node(right, context, plan, built),
            ];
            let mut rule_paths = HashSet::new();
            expr.collect_rule_paths(&mut rule_paths);
            if !rule_paths.is_empty() {
                return vec![ExplanationNode::Compose {
                    expression: format_expression(expr),
                    operands,
                }];
            }
            // Keep the operand nodes: users need the values that drove the
            // comparison or computation, not only its outcome.
            operands
        }
        ExpressionKind::LogicalNegation(operand, _)
        | ExpressionKind::MathematicalComputation(_, operand)
        | ExpressionKind::ResultIsVeto(operand)
        | ExpressionKind::PastFutureRange(_, operand) => {
            let operands = vec![build_expression_node(operand, context, plan, built)];
            let mut rule_paths = HashSet::new();
            expr.collect_rule_paths(&mut rule_paths);
            if !rule_paths.is_empty() {
                return vec![ExplanationNode::Compose {
                    expression: format_expression(expr),
                    operands,
                }];
            }
            operands
        }
        ExpressionKind::DateRelative(_, date_expr)
        | ExpressionKind::DateCalendar(_, _, date_expr) => {
            let operands = vec![build_expression_node(date_expr, context, plan, built)];
            let mut rule_paths = HashSet::new();
            expr.collect_rule_paths(&mut rule_paths);
            if !rule_paths.is_empty() {
                return vec![ExplanationNode::Compose {
                    expression: format_expression(expr),
                    operands,
                }];
            }
            operands
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
            vec![build_conversion_node(
                value_expr, target, expr, context, plan, built,
            )]
        }
        ExpressionKind::Now => Vec::new(),
        ExpressionKind::Piecewise(_) => {
            unreachable!("BUG: Piecewise in source expression for explanation")
        }
        ExpressionKind::RulePath(_) | ExpressionKind::DataPath(_) | ExpressionKind::Literal(_) => {
            unreachable!(
                "BUG: expression kind must be handled by build_expression_children fast path"
            )
        }
    }
}

fn build_expression_node<'plan>(
    expr: &Expression,
    context: &mut EvaluationContext<'plan>,
    plan: &ExecutionPlan,
    built: &HashMap<RulePath, Explanation>,
) -> ExplanationNode {
    match &expr.kind {
        ExpressionKind::RulePath(rule_path) => embed_rule(rule_path, built),
        ExpressionKind::DataPath(data_path) => build_data_input_node(data_path, context),
        ExpressionKind::Literal(lit) => ExplanationNode::DataInput {
            data: DataPath::local(String::new()),
            display: lit.display_value(),
        },
        ExpressionKind::UnitConversion(value_expr, target) => {
            build_conversion_node(value_expr, target, expr, context, plan, built)
        }
        ExpressionKind::Veto(veto_expr) => ExplanationNode::Veto {
            message: veto_expr.message.clone(),
        },
        ExpressionKind::Arithmetic(left, _, right)
        | ExpressionKind::Comparison(left, _, right)
        | ExpressionKind::LogicalAnd(left, right)
        | ExpressionKind::LogicalOr(left, right)
        | ExpressionKind::RangeLiteral(left, right)
        | ExpressionKind::RangeContainment(left, right) => {
            let operands = vec![
                build_expression_node(left, context, plan, built),
                build_expression_node(right, context, plan, built),
            ];
            ExplanationNode::Compose {
                expression: format_expression(expr),
                operands,
            }
        }
        ExpressionKind::LogicalNegation(operand, _)
        | ExpressionKind::MathematicalComputation(_, operand)
        | ExpressionKind::ResultIsVeto(operand)
        | ExpressionKind::PastFutureRange(_, operand) => {
            let operands = vec![build_expression_node(operand, context, plan, built)];
            ExplanationNode::Compose {
                expression: format_expression(expr),
                operands,
            }
        }
        ExpressionKind::DateRelative(_, date_expr)
        | ExpressionKind::DateCalendar(_, _, date_expr) => {
            let operands = vec![build_expression_node(date_expr, context, plan, built)];
            ExplanationNode::Compose {
                expression: format_expression(expr),
                operands,
            }
        }
        ExpressionKind::Now => ExplanationNode::DataInput {
            data: DataPath::local(String::new()),
            display: context.now().display_value(),
        },
        ExpressionKind::Piecewise(_) => {
            unreachable!("BUG: Piecewise in source expression for explanation")
        }
    }
}

fn build_conversion_node<'plan>(
    value_expr: &Expression,
    target: &SemanticConversionTarget,
    expr: &Expression,
    context: &mut EvaluationContext<'plan>,
    plan: &ExecutionPlan,
    built: &HashMap<RulePath, Explanation>,
) -> ExplanationNode {
    let operand_result = resolve_source_expression_values(value_expr, context);
    let OperationResult::Value(operand_value) = operand_result else {
        if let OperationResult::Veto(veto) = operand_result {
            return ExplanationNode::Veto {
                message: Some(veto.to_string()),
            };
        }
        unreachable!("BUG: conversion operand missing value");
    };

    let converted_result = resolve_source_expression_values(expr, context);
    let OperationResult::Value(converted_value) = converted_result else {
        if let OperationResult::Veto(veto) = converted_result {
            return ExplanationNode::Veto {
                message: Some(veto.to_string()),
            };
        }
        unreachable!("BUG: conversion result missing value");
    };

    let data_ref = data_path_in_expression(value_expr);
    let steps = build_conversion_steps(
        &operand_value,
        target,
        &converted_value,
        data_ref.as_ref(),
        UnitResolutionContext::WithIndex(plan.expression_unit_index()),
    );
    assert!(
        !steps.is_empty(),
        "BUG: unit conversion succeeded but explanation steps are empty"
    );

    ExplanationNode::Conversion {
        expression: format_expression(expr),
        steps: steps
            .iter()
            .map(SerializedConversionTraceStep::from)
            .collect(),
        operands: vec![build_expression_node(value_expr, context, plan, built)],
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

fn build_data_input_node(
    data_path: &DataPath,
    context: &mut EvaluationContext<'_>,
) -> ExplanationNode {
    match resolve_data_path_value(data_path, context) {
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

fn direct_in_spec_rule_children(expr: &Expression) -> Option<Vec<RulePath>> {
    collect_flat_in_spec_add_rule_paths(expr)
}

fn collect_flat_in_spec_add_rule_paths(expr: &Expression) -> Option<Vec<RulePath>> {
    match &expr.kind {
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Add, right) => {
            let mut paths = collect_flat_in_spec_add_rule_paths(left)?;
            paths.extend(collect_flat_in_spec_add_rule_paths(right)?);
            Some(paths)
        }
        _ => Some(vec![single_in_spec_rule(expr)?]),
    }
}

fn single_in_spec_rule(expr: &Expression) -> Option<RulePath> {
    match &expr.kind {
        ExpressionKind::RulePath(path) => Some(path.clone()),
        _ => None,
    }
}

fn direct_data_children(expr: &Expression) -> Option<Vec<DataPath>> {
    let mut leaves = Vec::new();
    if !collect_data_leaves(expr, &mut leaves) {
        return None;
    }
    if leaves.is_empty() {
        return None;
    }
    Some(leaves)
}

fn collect_data_leaves(expr: &Expression, out: &mut Vec<DataPath>) -> bool {
    match &expr.kind {
        ExpressionKind::DataPath(path) => {
            out.push(path.clone());
            true
        }
        ExpressionKind::Arithmetic(left, _, right) => {
            collect_data_leaves(left, out) && collect_data_leaves(right, out)
        }
        _ => false,
    }
}

pub fn format_explanation(explanation: &Explanation) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "{}: {}",
        explanation.rule.rule,
        format_operation_result(&explanation.result)
    ));
    let mut ctx = FormatContext {
        lines: &mut lines,
        indent: String::new(),
    };
    if !explanation.body.is_empty() {
        ctx.push_line(Connector::Last, &explanation.body);
    }
    let child_indent = ctx.child_indent(Connector::Last);
    let mut child_ctx = FormatContext {
        lines: ctx.lines,
        indent: child_indent,
    };
    child_ctx.render_causes_and_children(
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

    fn render_causes_and_children(
        &mut self,
        parent_body: &str,
        causes: &[Cause],
        children: &[ExplanationNode],
    ) {
        let visible: Vec<_> = children
            .iter()
            .filter(|child| !should_skip_in_text(child, parent_body))
            .collect();
        let total = causes.len() + visible.len();
        let mut index = 0;

        for cause in causes {
            index += 1;
            let connector = if index == total {
                Connector::Last
            } else {
                Connector::Branch
            };
            self.push_line(
                connector,
                &format!("{} is {}", cause.condition, cause.value),
            );
        }

        for child in visible {
            index += 1;
            let connector = if index == total {
                Connector::Last
            } else {
                Connector::Branch
            };
            self.render_node(child, connector, Some(parent_body));
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
                body,
                causes,
                children,
            } => {
                let style = rule_line_style(body, children);
                match style {
                    RuleLineStyle::NameOnly => {
                        self.push_line(connector, &rule.rule);
                        let child_indent = self.child_indent(connector);
                        let mut child_ctx = FormatContext {
                            lines: self.lines,
                            indent: child_indent,
                        };
                        if !body.is_empty() {
                            child_ctx.push_line(Connector::Last, body);
                            let body_child_indent = child_ctx.child_indent(Connector::Last);
                            let mut nested = FormatContext {
                                lines: child_ctx.lines,
                                indent: body_child_indent,
                            };
                            nested.render_causes_and_children(body, causes, children);
                        } else {
                            child_ctx.render_causes_and_children(body, causes, children);
                        }
                    }
                    RuleLineStyle::NameWithBody => {
                        self.push_line(connector, &format!("{}: {body}", rule.rule));
                        let child_indent = self.child_indent(connector);
                        let mut child_ctx = FormatContext {
                            lines: self.lines,
                            indent: child_indent,
                        };
                        child_ctx.render_causes_and_children(body, causes, children);
                    }
                }
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
                let len = operands.len();
                for (i, operand) in operands.iter().enumerate() {
                    let child_connector = if i + 1 == len {
                        Connector::Last
                    } else {
                        Connector::Branch
                    };
                    child_ctx.render_node(operand, child_connector, None);
                }
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
                ..
            } => {
                let expression_is_parent_body = parent_body.is_some_and(|body| body == expression);
                if !expression_is_parent_body {
                    self.push_line(connector, expression);
                }
                render_conversion_steps(self, connector, steps);
                let child_indent = self.child_indent(connector);
                let mut child_ctx = FormatContext {
                    lines: self.lines,
                    indent: child_indent,
                };
                let len = operands.len();
                for (i, operand) in operands.iter().enumerate() {
                    let child_connector = if i + 1 == len {
                        Connector::Last
                    } else {
                        Connector::Branch
                    };
                    child_ctx.render_node(operand, child_connector, None);
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
        }
    }
}

fn connector_str(connector: Connector) -> &'static str {
    match connector {
        Connector::Branch => "├─",
        Connector::Last => "└─",
    }
}

fn should_skip_in_text(node: &ExplanationNode, parent_body: &str) -> bool {
    match node {
        ExplanationNode::Compose { expression, .. } => expression == parent_body,
        _ => false,
    }
}

fn render_conversion_steps(
    ctx: &mut FormatContext<'_>,
    connector: Connector,
    steps: &[SerializedConversionTraceStep],
) {
    if steps.is_empty() {
        return;
    }
    let child_indent = ctx.child_indent(connector);
    let mut step_ctx = FormatContext {
        lines: ctx.lines,
        indent: child_indent,
    };
    for (index, step) in steps.iter().enumerate() {
        let step_connector = if index + 1 == steps.len() {
            Connector::Last
        } else {
            Connector::Branch
        };
        step_ctx.push_line(step_connector, &step.text);
    }
}

enum RuleLineStyle {
    NameOnly,
    NameWithBody,
}

fn rule_line_style(body: &str, children: &[ExplanationNode]) -> RuleLineStyle {
    if children
        .iter()
        .all(|child| matches!(child, ExplanationNode::Rule { .. }))
        && children.len() >= 2
    {
        return RuleLineStyle::NameOnly;
    }
    if children.len() == 1 {
        if let ExplanationNode::Compose { expression, .. } = &children[0] {
            if expression == body {
                return RuleLineStyle::NameWithBody;
            }
        }
    }
    RuleLineStyle::NameWithBody
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
    use crate::limits::ResourceLimits;
    use crate::literals::DateGranularity;
    use crate::literals::QuantityUnit;
    use crate::parsing::ast::DateTimeValue;
    use crate::parsing::source::SourceType;
    use crate::planning::data_input::DataValueInput;
    use crate::planning::execution_plan::DataOverlay;
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
      "body": "labor + rush_surcharge",
      "children": [
        {
          "type": "rule",
          "rule": "labor",
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
          "body": "labor * 25%",
          "causes": [
            { "condition": "is_rush", "value": "true" },
            { "condition": "is_super_rush", "value": "false" }
          ],
          "children": [
            {
              "type": "compose",
              "expression": "labor * 25%",
              "operands": [
                {
                  "type": "rule",
                  "rule": "labor",
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
                  "type": "data_input",
                  "data": "",
                  "display": "25%"
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
      "body": "subtotal * 21%",
      "children": [
        {
          "type": "compose",
          "expression": "subtotal * 21%",
          "operands": [
            {
              "type": "rule",
              "rule": "subtotal",
              "body": "labor + rush_surcharge",
              "children": [
                {
                  "type": "rule",
                  "rule": "labor",
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
                  "body": "labor * 25%",
                  "causes": [
                    { "condition": "is_rush", "value": "true" },
                    { "condition": "is_super_rush", "value": "false" }
                  ],
                  "children": [
                    {
                      "type": "compose",
                      "expression": "labor * 25%",
                      "operands": [
                        {
                          "type": "rule",
                          "rule": "labor",
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
                          "type": "data_input",
                          "data": "",
                          "display": "25%"
                        }
                      ]
                    }
                  ]
                }
              ]
            },
            {
              "type": "data_input",
              "data": "",
              "display": "21%"
            }
          ]
        }
      ]
    }
  ]
}"#;

    fn rush_surcharge_causes(data: HashMap<String, String>) -> Vec<Cause> {
        let mut engine = Engine::new();
        engine
            .load(CALC_SPEC, crate::SourceType::Volatile)
            .expect("calc spec loads");
        let now = DateTimeValue::now();
        let plan = engine
            .get_plan(None, "calc", Some(&now))
            .expect("calc plan");
        let overlay = DataOverlay::resolve(
            plan,
            data.into_iter()
                .map(|(k, v)| (k, DataValueInput::convenience(v)))
                .collect(),
            &ResourceLimits::default(),
        )
        .expect("overlay");
        let now_lit = LiteralValue {
            value: ValueKind::Date(crate::planning::semantics::date_time_to_semantic(&now)),
            lemma_type: crate::planning::semantics::primitive_date_arc().clone(),
        };
        let mut context = EvaluationContext::new(plan, &overlay, now_lit);
        let rush_rule = plan
            .get_rule("rush_surcharge")
            .expect("rush_surcharge rule");
        match winning_source_branch_and_causes(rush_rule, &mut context) {
            WinningSourceBranch::BranchResult { causes, .. } => causes,
            WinningSourceBranch::ConditionVeto { causes, .. } => causes,
        }
    }

    #[test]
    fn unless_causes_neither_matches() {
        let mut data = HashMap::new();
        data.insert("is_rush".into(), "false".into());
        data.insert("is_super_rush".into(), "false".into());
        let causes = rush_surcharge_causes(data);
        assert_eq!(
            causes,
            vec![
                Cause {
                    condition: "is_rush".to_string(),
                    value: "false".to_string(),
                },
                Cause {
                    condition: "is_super_rush".to_string(),
                    value: "false".to_string(),
                },
            ]
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
            vec![
                Cause {
                    condition: "is_rush".to_string(),
                    value: "true".to_string(),
                },
                Cause {
                    condition: "is_super_rush".to_string(),
                    value: "false".to_string(),
                },
            ]
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
            vec![Cause {
                condition: "is_super_rush".to_string(),
                value: "true".to_string(),
            }]
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
    fn build_conversion_steps_identity_omits_rule() {
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
        assert_eq!(steps.len(), 2);
        assert!(matches!(steps[0].role, ConversionTraceRole::Outcome));
        assert!(matches!(steps[1].role, ConversionTraceRole::Source));
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
