use crate::evaluation::operations::{ComputationKind, OperationResult};
use crate::planning::semantics::{FactPath, LiteralValue, RulePath, Source};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Proof {
    pub rule_path: RulePath,
    pub source: Option<Source>,
    pub result: OperationResult,
    pub tree: ProofNode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofNode {
    Value {
        value: LiteralValue,
        source: ValueSource,
        source_location: Option<Source>,
    },
    RuleReference {
        rule_path: RulePath,
        result: OperationResult,
        source_location: Option<Source>,
        expansion: Box<ProofNode>,
    },
    Computation {
        kind: ComputationKind,
        original_expression: String,
        expression: String,
        result: LiteralValue,
        source_location: Option<Source>,
        operands: Vec<ProofNode>,
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
        operands: Vec<ProofNode>,
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
    pub condition: Option<Box<ProofNode>>,
    pub result: Box<ProofNode>,
    pub clause_index: Option<usize>,
    pub source_location: Option<Source>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NonMatchedBranch {
    pub condition: Box<ProofNode>,
    pub result: Option<Box<ProofNode>>,
    pub clause_index: Option<usize>,
    pub source_location: Option<Source>,
}
