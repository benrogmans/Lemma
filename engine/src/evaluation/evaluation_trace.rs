use crate::evaluation::operations::{ComputationKind, OperationResult};
use crate::planning::semantics::{DataPath, LiteralValue, RulePath, Source};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct EvaluationTrace {
    pub rule_path: RulePath,
    pub source: Option<Source>,
    pub result: OperationResult,
    pub tree: Arc<TraceNode>,
}

impl Serialize for EvaluationTrace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut expanded = HashSet::new();
        SerializedEvaluationTrace {
            rule: self.rule_path.rule.clone(),
            result: match &self.result {
                OperationResult::Value(value) => value.display_value(),
                OperationResult::Veto(veto) => veto.to_string(),
            },
            tree: serialize_trace_node(&self.tree, &mut expanded),
        }
        .serialize(serializer)
    }
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

#[derive(Debug, Clone)]
pub enum TraceNode {
    Value {
        value: LiteralValue,
        source: TraceValueSource,
        source_location: Option<Source>,
    },
    RuleReference {
        rule_path: RulePath,
        result: OperationResult,
        source_location: Option<Source>,
        expansion: Arc<TraceNode>,
    },
    Computation {
        kind: ComputationKind,
        conversion_steps: Vec<ConversionTraceStep>,
        expression: String,
        result: LiteralValue,
        source_location: Option<Source>,
        operands: Vec<TraceNode>,
    },
    Branches {
        matched: Box<TraceBranch>,
        non_matched: Vec<TraceNonMatchedBranch>,
        source_location: Option<Source>,
    },
    Veto {
        message: Option<String>,
        source_location: Option<Source>,
    },
}

#[derive(Debug, Clone)]
pub enum TraceValueSource {
    Data { data_ref: DataPath },
    Literal,
    Computed,
}

#[derive(Debug, Clone)]
pub struct TraceBranch {
    pub condition: Option<Box<TraceNode>>,
    pub result: Box<TraceNode>,
    pub clause_index: Option<usize>,
    pub source_location: Option<Source>,
}

#[derive(Debug, Clone)]
pub struct TraceNonMatchedBranch {
    pub condition: Box<TraceNode>,
    pub result: Option<Box<TraceNode>>,
    pub clause_index: Option<usize>,
    pub source_location: Option<Source>,
}

/// Human-readable expression text for a trace node used when composing parent expressions.
pub fn trace_expression(node: &TraceNode) -> String {
    match node {
        TraceNode::Computation { expression, .. } => expression.clone(),
        TraceNode::Value { value, source, .. } => match source {
            TraceValueSource::Data { data_ref } => {
                format!("{} is {}", data_ref, value.display_value())
            }
            TraceValueSource::Literal | TraceValueSource::Computed => value.display_value(),
        },
        TraceNode::RuleReference { rule_path, .. } => rule_path.to_string(),
        TraceNode::Branches { .. } | TraceNode::Veto { .. } => {
            panic!(
                "BUG: trace expression must come from Computation, Value, or RuleReference, got {node:?}"
            )
        }
    }
}

#[derive(Serialize)]
struct SerializedEvaluationTrace {
    rule: String,
    result: String,
    tree: SerializedTraceNode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SerializedTraceNode {
    Value {
        display: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
    RuleReference {
        rule: String,
        result: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tree: Option<Box<SerializedTraceNode>>,
    },
    Computation {
        expression: String,
        result: String,
        operands: Vec<SerializedTraceNode>,
    },
    Branches {
        matched: SerializedTraceBranch,
        non_matched: Vec<SerializedTraceBranch>,
    },
    Conversion {
        steps: Vec<SerializedConversionStep>,
        operands: Vec<SerializedTraceNode>,
    },
    Veto {
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
struct SerializedTraceBranch {
    #[serde(skip_serializing_if = "Option::is_none")]
    condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tree: Option<Box<SerializedTraceNode>>,
}

#[derive(Debug, Clone, Serialize)]
struct SerializedConversionStep {
    role: String,
    text: String,
}

fn serialize_trace_node(node: &TraceNode, expanded: &mut HashSet<String>) -> SerializedTraceNode {
    match node {
        TraceNode::Value { value, source, .. } => {
            let data = match source {
                TraceValueSource::Data { data_ref } => Some(data_ref.to_string()),
                _ => None,
            };
            SerializedTraceNode::Value {
                display: value.display_value(),
                data,
            }
        }
        TraceNode::RuleReference {
            rule_path,
            result,
            expansion,
            ..
        } => {
            let rule_name = rule_path.to_string();
            let tree = if expanded.insert(rule_name.clone()) {
                Some(Box::new(serialize_trace_node(expansion, expanded)))
            } else {
                None
            };
            SerializedTraceNode::RuleReference {
                rule: rule_name,
                result: match result {
                    OperationResult::Value(value) => value.display_value(),
                    OperationResult::Veto(veto) => veto.to_string(),
                },
                tree,
            }
        }
        TraceNode::Computation {
            kind,
            conversion_steps,
            expression,
            result,
            operands,
            ..
        } => {
            let ops = operands
                .iter()
                .map(|operand| serialize_trace_node(operand, expanded))
                .collect();
            if matches!(kind, ComputationKind::UnitConversion { .. }) {
                SerializedTraceNode::Conversion {
                    steps: conversion_steps
                        .iter()
                        .map(|step| SerializedConversionStep {
                            role: match step.role {
                                ConversionTraceRole::Outcome => "outcome".to_string(),
                                ConversionTraceRole::Rule => "rule".to_string(),
                                ConversionTraceRole::Source => "source".to_string(),
                            },
                            text: step.text.clone(),
                        })
                        .collect(),
                    operands: ops,
                }
            } else {
                SerializedTraceNode::Computation {
                    expression: expression.clone(),
                    result: result.display_value(),
                    operands: ops,
                }
            }
        }
        TraceNode::Branches {
            matched,
            non_matched,
            ..
        } => SerializedTraceNode::Branches {
            matched: SerializedTraceBranch {
                condition: matched
                    .condition
                    .as_ref()
                    .map(|condition| branch_condition_expression(condition)),
                tree: Some(Box::new(serialize_trace_node(&matched.result, expanded))),
            },
            non_matched: non_matched
                .iter()
                .map(|branch| SerializedTraceBranch {
                    condition: Some(branch_condition_expression(&branch.condition)),
                    tree: branch
                        .result
                        .as_ref()
                        .map(|result| Box::new(serialize_trace_node(result, expanded))),
                })
                .collect(),
        },
        TraceNode::Veto { message, .. } => SerializedTraceNode::Veto {
            message: message.clone(),
        },
    }
}

fn branch_condition_expression(node: &TraceNode) -> String {
    match node {
        TraceNode::Computation { expression, .. } => expression.clone(),
        TraceNode::Value { value, .. } => value.display_value(),
        TraceNode::RuleReference { rule_path, .. } => rule_path.to_string(),
        TraceNode::Branches { .. } | TraceNode::Veto { .. } => {
            panic!(
                "BUG: branch condition must be Computation, Value, or RuleReference, got {node:?}"
            )
        }
    }
}
