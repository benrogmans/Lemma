//! Explanation wire types built during evaluation.
//!
//! Planning ships THE DAG (`NormalForm` nodes with optional fold `origin`).
//! Evaluation walks that DAG — following non-piecewise origins for structure;
//! piecewise origins supply cause records without re-entering live arm control —
//! and fills these nodes for the response.
//!
//! Bound data narration is `Data` with a required `display` string.
//! Structural mentions of paths that were never looked up are `DataUnused`
//! (no display field) — distinct from a Missing-data veto on a live leaf walk.
//!
//! `Piecewise` is eval-internal only. It must be lowered to Rule causes +
//! winner children before any wire serialize.

use crate::planning::semantics::{DataPath, RulePath};
use serde::{Serialize, Serializer};

pub(crate) fn serialize_rule_path_as_name<S>(
    path: &RulePath,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path.rule)
}

pub(crate) fn serialize_data_path_as_name<S>(
    path: &DataPath,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path.input_key())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExplanationNode {
    Rule {
        #[serde(serialize_with = "serialize_rule_path_as_name")]
        name: RulePath,
        #[serde(serialize_with = "serialize_option_string")]
        result: Option<String>,
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
    /// Evaluated / bound data narration. `display` is always the looked-up value or veto text.
    Data {
        #[serde(serialize_with = "serialize_data_path_as_name")]
        name: DataPath,
        display: String,
    },
    /// Structural mention of a data path that was not looked up for this cause
    /// (short-circuit skip or static record narration without a binding).
    DataUnused {
        #[serde(serialize_with = "serialize_data_path_as_name")]
        name: DataPath,
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
    UnitEquivalence {
        text: String,
    },
    /// Planning/eval only. Never reaches wire — lowered to causes + winner first.
    Piecewise {
        #[serde(serialize_with = "forbid_piecewise_serialize")]
        arms: Vec<PiecewiseArm>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiecewiseArm {
    pub condition: ExplanationNode,
    pub result: ExplanationNode,
    /// Plan-time truth of the condition after constant-fold / unless-force.
    /// `None` means the condition still needs runtime evaluation.
    pub static_condition: Option<bool>,
    /// Cause.value when `static_condition` is `Some`. True for held arms and
    /// narrated flipped comparison facts; false for bare / and-false static miss.
    pub static_cause_value: Option<bool>,
}

fn forbid_piecewise_serialize<S: Serializer>(
    _: &Vec<PiecewiseArm>,
    _: S,
) -> Result<S::Ok, S::Error> {
    panic!("BUG: Piecewise must be lowered before serialize")
}

fn serialize_option_string<S>(value: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(s) => serializer.serialize_str(s),
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cause {
    pub condition: String,
    #[serde(serialize_with = "serialize_option_string")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ExplanationNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionTraceRole {
    Outcome,
    Rule,
    Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SerializedConversionTraceStep {
    pub(crate) role: ConversionTraceRole,
    pub(crate) text: String,
}
