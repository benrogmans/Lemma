use crate::evaluation::operations::{ComputationKind, OperationResult};
use crate::{FactPath, RulePath};
use crate::{LiteralValue, Source};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Proof {
    pub rule_path: RulePath,
    pub source: Option<Source>,
    pub result: OperationResult,
    pub tree: ProofNode,
}

#[derive(Debug, Clone, Serialize)]
pub enum ProofNode {
    Value {
        value: LiteralValue,
        origin: ValueOrigin,
        source: Option<Source>,
    },
    RuleReference {
        rule_path: RulePath,
        result: OperationResult,
        source: Option<Source>,
        expansion: Box<ProofNode>,
    },
    Computation {
        kind: ComputationKind,
        original_expression: String,
        expression: String,
        result: LiteralValue,
        source: Option<Source>,
        operands: Vec<ProofNode>,
    },
    Branches {
        matched: Box<Branch>,
        non_matched: Vec<NonMatchedBranch>,
        source: Option<Source>,
    },
    Condition {
        original_expression: String,
        expression: String,
        result: bool,
        source: Option<Source>,
        operands: Vec<ProofNode>,
    },
    Veto {
        message: Option<String>,
        source: Option<Source>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub enum ValueOrigin {
    Fact { fact_ref: FactPath },
    Literal,
    Computed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Branch {
    pub condition: Box<ProofNode>,
    pub result: Box<ProofNode>,
    pub clause_index: Option<usize>,
    pub source: Option<Source>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NonMatchedBranch {
    pub condition: Box<ProofNode>,
    pub result: Option<Box<ProofNode>>,
    pub clause_index: Option<usize>,
    pub source: Option<Source>,
}
