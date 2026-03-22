use crate::evaluation::operations::{ComputationKind, OperationResult};
use crate::planning::semantics::{FactPath, LiteralValue, RulePath, Source};
use serde::{Serialize, Serializer};
use std::sync::Arc;

fn serialize_arc<T, S>(value: &Arc<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    value.as_ref().serialize(serializer)
}

#[derive(Debug, Clone, Serialize)]
pub struct Explanation {
    pub rule_path: RulePath,
    pub source: Option<Source>,
    pub result: OperationResult,
    #[serde(serialize_with = "serialize_arc")]
    pub tree: Arc<ExplanationNode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplanationNode {
    Value {
        value: LiteralValue,
        source: ValueSource,
        source_location: Option<Source>,
    },
    RuleReference {
        rule_path: RulePath,
        result: OperationResult,
        source_location: Option<Source>,
        #[serde(serialize_with = "serialize_arc")]
        expansion: Arc<ExplanationNode>,
    },
    Computation {
        kind: ComputationKind,
        original_expression: String,
        expression: String,
        result: LiteralValue,
        source_location: Option<Source>,
        operands: Vec<ExplanationNode>,
    },
    Branches {
        matched: Box<Branch>,
        non_matched: Vec<NonMatchedBranch>,
        source_location: Option<Source>,
    },
    Condition {
        original_expression: String,
        expression: String,
        result: bool,
        source_location: Option<Source>,
        operands: Vec<ExplanationNode>,
    },
    Veto {
        message: Option<String>,
        source_location: Option<Source>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueSource {
    Fact { fact_ref: FactPath },
    Literal,
    Computed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Branch {
    pub condition: Option<Box<ExplanationNode>>,
    pub result: Box<ExplanationNode>,
    pub clause_index: Option<usize>,
    pub source_location: Option<Source>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NonMatchedBranch {
    pub condition: Box<ExplanationNode>,
    pub result: Option<Box<ExplanationNode>>,
    pub clause_index: Option<usize>,
    pub source_location: Option<Source>,
}
