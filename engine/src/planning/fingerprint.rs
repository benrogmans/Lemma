//! Semantic plan fingerprint for content-addressable hashing.
//!
//! Projects ExecutionPlan onto a representation that contains only what the plan actually does.
//! Uses dedicated fingerprint types (no LemmaType, LiteralValue, Arc<LemmaSpec>) so the hash
//! does not depend on external types or other specs. Excludes sources, meta, source locations.
//! Schema is explicit and stable: adding Rust fields does not change hashes for unused content.
//!
//! **Format versioning:** `fingerprint_hash` hashes `LMFP` + big-endian `FINGERPRINT_FORMAT_VERSION`
//! (u32) + postcard(`PlanFingerprint`). Bump the version when the encoded semantics change in a
//! way that must not share hashes with prior formats.

use crate::parsing::ast::{CalendarUnit, DateCalendarKind, DateRelativeKind, DateTimeValue};
use crate::planning::execution_plan::{Branch, ExecutableRule, ExecutionPlan};
use crate::planning::semantics::{
    ArithmeticComputation, ComparisonComputation, Expression, ExpressionKind, FactData, FactPath,
    LemmaType, LiteralValue, MathematicalComputation, NegationType, RulePath,
    SemanticConversionTarget, TypeDefiningSpec, TypeExtends, TypeSpecification, ValueKind,
    VetoExpression,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Bumped when the byte layout hashed by [`fingerprint_hash`] changes incompatibly (prefix + postcard).
pub const FINGERPRINT_FORMAT_VERSION: u32 = 1;

const FINGERPRINT_MAGIC: &[u8; 4] = b"LMFP";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeDefiningSpecFingerprint {
    Local,
    Import {
        /// Spec identifier: name or name~hash when pinned (e.g. `dep` or `dep~a1b2c3d4`).
        spec_id: String,
        effective_from: Option<DateTimeValue>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeExtendsFingerprint {
    Primitive,
    Custom {
        parent: String,
        family: String,
        defining_spec: TypeDefiningSpecFingerprint,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct LemmaTypeFingerprint {
    pub name: Option<String>,
    pub specifications: TypeSpecification,
    pub extends: TypeExtendsFingerprint,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiteralValueFingerprint {
    pub value: ValueKind,
    pub lemma_type: LemmaTypeFingerprint,
}

/// Semantic fingerprint of an execution plan. Contains only content that affects evaluation.
#[derive(Debug, Clone, Serialize)]
pub struct PlanFingerprint {
    pub spec_name: String,
    pub valid_from: Option<DateTimeValue>,
    pub facts: Vec<(FactPath, FactFingerprint)>,
    pub rules: Vec<RuleFingerprint>,
    pub named_types: BTreeMap<String, LemmaTypeFingerprint>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FactFingerprint {
    Value {
        value: LiteralValueFingerprint,
        is_default: bool,
    },
    TypeDeclaration {
        resolved_type: LemmaTypeFingerprint,
    },
    SpecRef {
        /// Spec identifier: name or name~hash when pinned (e.g. `dep` or `dep~a1b2c3d4`).
        spec_id: String,
        effective_from: Option<DateTimeValue>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleFingerprint {
    pub path: RulePath,
    pub name: String,
    pub branches: Vec<BranchFingerprint>,
    pub needs_facts: Vec<FactPath>,
    pub rule_type: LemmaTypeFingerprint,
}

#[derive(Debug, Clone, Serialize)]
pub struct BranchFingerprint {
    pub condition: Option<ExpressionFingerprint>,
    pub result: ExpressionFingerprint,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExpressionFingerprint {
    pub kind: ExpressionKindFingerprint,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionKindFingerprint {
    Literal(Box<LiteralValueFingerprint>),
    FactPath(FactPath),
    RulePath(RulePath),
    LogicalAnd(Box<ExpressionFingerprint>, Box<ExpressionFingerprint>),
    Arithmetic(
        Box<ExpressionFingerprint>,
        ArithmeticComputation,
        Box<ExpressionFingerprint>,
    ),
    Comparison(
        Box<ExpressionFingerprint>,
        ComparisonComputation,
        Box<ExpressionFingerprint>,
    ),
    UnitConversion(Box<ExpressionFingerprint>, SemanticConversionTarget),
    LogicalNegation(Box<ExpressionFingerprint>, NegationType),
    MathematicalComputation(MathematicalComputation, Box<ExpressionFingerprint>),
    Veto(VetoExpression),
    Now,
    DateRelative(
        DateRelativeKind,
        Box<ExpressionFingerprint>,
        Option<Box<ExpressionFingerprint>>,
    ),
    DateCalendar(DateCalendarKind, CalendarUnit, Box<ExpressionFingerprint>),
}

fn type_defining_spec_fingerprint(ds: &TypeDefiningSpec) -> TypeDefiningSpecFingerprint {
    match ds {
        TypeDefiningSpec::Local => TypeDefiningSpecFingerprint::Local,
        TypeDefiningSpec::Import {
            spec,
            resolved_plan_hash,
        } => TypeDefiningSpecFingerprint::Import {
            spec_id: format!("{}~{}", spec.name, resolved_plan_hash),
            effective_from: spec.effective_from.clone(),
        },
    }
}

fn type_extends_fingerprint(e: &TypeExtends) -> TypeExtendsFingerprint {
    match e {
        TypeExtends::Primitive => TypeExtendsFingerprint::Primitive,
        TypeExtends::Custom {
            parent,
            family,
            defining_spec,
        } => TypeExtendsFingerprint::Custom {
            parent: parent.clone(),
            family: family.clone(),
            defining_spec: type_defining_spec_fingerprint(defining_spec),
        },
    }
}

fn lemma_type_fingerprint(lt: &LemmaType) -> LemmaTypeFingerprint {
    LemmaTypeFingerprint {
        name: lt.name.clone(),
        specifications: lt.specifications.clone(),
        extends: type_extends_fingerprint(&lt.extends),
    }
}

fn literal_value_fingerprint(lv: &LiteralValue) -> LiteralValueFingerprint {
    LiteralValueFingerprint {
        value: lv.value.clone(),
        lemma_type: lemma_type_fingerprint(&lv.lemma_type),
    }
}

/// Project ExecutionPlan to semantic fingerprint, excluding sources and meta.
pub fn from_plan(plan: &ExecutionPlan) -> PlanFingerprint {
    let facts: Vec<(FactPath, FactFingerprint)> = plan
        .facts
        .iter()
        .map(|(path, data)| (path.clone(), fact_fingerprint(data)))
        .collect();

    let rules: Vec<RuleFingerprint> = plan.rules.iter().map(rule_fingerprint).collect();

    let named_types: BTreeMap<String, LemmaTypeFingerprint> = plan
        .named_types
        .iter()
        .map(|(k, v)| (k.clone(), lemma_type_fingerprint(v)))
        .collect();

    PlanFingerprint {
        spec_name: plan.spec_name.clone(),
        valid_from: plan.valid_from.clone(),
        facts,
        rules,
        named_types,
    }
}

fn fact_fingerprint(data: &FactData) -> FactFingerprint {
    match data {
        FactData::Value {
            value, is_default, ..
        } => FactFingerprint::Value {
            value: literal_value_fingerprint(value),
            is_default: *is_default,
        },
        FactData::TypeDeclaration { resolved_type, .. } => FactFingerprint::TypeDeclaration {
            resolved_type: lemma_type_fingerprint(resolved_type),
        },
        FactData::SpecRef {
            spec,
            resolved_plan_hash,
            ..
        } => FactFingerprint::SpecRef {
            spec_id: resolved_plan_hash
                .as_ref()
                .map(|h| format!("{}~{}", spec.name, h))
                .unwrap_or_else(|| spec.name.clone()),
            effective_from: spec.effective_from.clone(),
        },
    }
}

fn rule_fingerprint(rule: &ExecutableRule) -> RuleFingerprint {
    RuleFingerprint {
        path: rule.path.clone(),
        name: rule.name.clone(),
        branches: rule.branches.iter().map(branch_fingerprint).collect(),
        needs_facts: rule.needs_facts.iter().cloned().collect(),
        rule_type: lemma_type_fingerprint(&rule.rule_type),
    }
}

fn branch_fingerprint(branch: &Branch) -> BranchFingerprint {
    BranchFingerprint {
        condition: branch.condition.as_ref().map(expression_fingerprint),
        result: expression_fingerprint(&branch.result),
    }
}

fn expression_fingerprint(expr: &Expression) -> ExpressionFingerprint {
    ExpressionFingerprint {
        kind: expression_kind_fingerprint(&expr.kind),
    }
}

fn expression_kind_fingerprint(kind: &ExpressionKind) -> ExpressionKindFingerprint {
    match kind {
        ExpressionKind::Literal(lv) => {
            ExpressionKindFingerprint::Literal(Box::new(literal_value_fingerprint(lv)))
        }
        ExpressionKind::FactPath(fp) => ExpressionKindFingerprint::FactPath(fp.clone()),
        ExpressionKind::RulePath(rp) => ExpressionKindFingerprint::RulePath(rp.clone()),
        ExpressionKind::LogicalAnd(l, r) => ExpressionKindFingerprint::LogicalAnd(
            Box::new(expression_fingerprint(l)),
            Box::new(expression_fingerprint(r)),
        ),
        ExpressionKind::Arithmetic(l, op, r) => ExpressionKindFingerprint::Arithmetic(
            Box::new(expression_fingerprint(l)),
            op.clone(),
            Box::new(expression_fingerprint(r)),
        ),
        ExpressionKind::Comparison(l, op, r) => ExpressionKindFingerprint::Comparison(
            Box::new(expression_fingerprint(l)),
            op.clone(),
            Box::new(expression_fingerprint(r)),
        ),
        ExpressionKind::UnitConversion(inner, target) => ExpressionKindFingerprint::UnitConversion(
            Box::new(expression_fingerprint(inner)),
            target.clone(),
        ),
        ExpressionKind::LogicalNegation(inner, nt) => ExpressionKindFingerprint::LogicalNegation(
            Box::new(expression_fingerprint(inner)),
            nt.clone(),
        ),
        ExpressionKind::MathematicalComputation(mc, inner) => {
            ExpressionKindFingerprint::MathematicalComputation(
                mc.clone(),
                Box::new(expression_fingerprint(inner)),
            )
        }
        ExpressionKind::Veto(ve) => ExpressionKindFingerprint::Veto(ve.clone()),
        ExpressionKind::Now => ExpressionKindFingerprint::Now,
        ExpressionKind::DateRelative(kind, date_expr, tol) => {
            ExpressionKindFingerprint::DateRelative(
                *kind,
                Box::new(expression_fingerprint(date_expr)),
                tol.as_ref().map(|t| Box::new(expression_fingerprint(t))),
            )
        }
        ExpressionKind::DateCalendar(kind, unit, date_expr) => {
            ExpressionKindFingerprint::DateCalendar(
                *kind,
                *unit,
                Box::new(expression_fingerprint(date_expr)),
            )
        }
    }
}

/// Compute deterministic 8-char hex hash from fingerprint.
pub fn fingerprint_hash(fp: &PlanFingerprint) -> String {
    let payload = postcard::to_allocvec(fp).expect("PlanFingerprint serialization");
    let mut prefixed = Vec::with_capacity(FINGERPRINT_MAGIC.len() + 4 + payload.len());
    prefixed.extend_from_slice(FINGERPRINT_MAGIC.as_slice());
    prefixed.extend_from_slice(&FINGERPRINT_FORMAT_VERSION.to_be_bytes());
    prefixed.extend_from_slice(&payload);
    let digest = Sha256::digest(&prefixed);
    let n = (u32::from(digest[0]) << 24)
        | (u32::from(digest[1]) << 16)
        | (u32::from(digest[2]) << 8)
        | u32::from(digest[3]);
    format!("{:08x}", n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use std::collections::HashMap;

    fn empty_plan(spec_name: &str) -> ExecutionPlan {
        ExecutionPlan {
            spec_name: spec_name.to_string(),
            facts: IndexMap::new(),
            rules: vec![],
            sources: HashMap::new(),
            meta: HashMap::new(),
            named_types: BTreeMap::new(),
            valid_from: None,
            valid_to: None,
        }
    }

    #[test]
    fn same_plan_same_fingerprint() {
        let plan = empty_plan("test");
        let fp1 = from_plan(&plan);
        let fp2 = from_plan(&plan);
        assert_eq!(fp1.spec_name, fp2.spec_name);
    }

    #[test]
    fn same_plan_same_hash() {
        let plan = empty_plan("test");
        let h1 = fingerprint_hash(&from_plan(&plan));
        let h2 = fingerprint_hash(&from_plan(&plan));
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_spec_name_different_hash() {
        let h1 = fingerprint_hash(&from_plan(&empty_plan("a")));
        let h2 = fingerprint_hash(&from_plan(&empty_plan("b")));
        assert_ne!(h1, h2);
    }

    /// Golden vectors for `FINGERPRINT_FORMAT_VERSION` + postcard layout. Update when bumping format.
    #[test]
    fn golden_plan_hash_empty_spec_names() {
        assert_eq!(
            fingerprint_hash(&from_plan(&empty_plan("golden_empty"))),
            "fc4c852f"
        );
        assert_eq!(fingerprint_hash(&from_plan(&empty_plan("x"))), "e97e410c");
        let mut p = empty_plan("golden_valid_from");
        p.valid_from = Some(DateTimeValue {
            year: 2024,
            month: 6,
            day: 15,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,
        });
        assert_eq!(fingerprint_hash(&from_plan(&p)), "b301d0c3");
    }
}
