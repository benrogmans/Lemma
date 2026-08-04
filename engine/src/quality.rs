//! Structural quality analysis over loaded specs.
//!
//! Advisory only: never affects planning or evaluation. Distinct from [`Error`] /
//! Veto / panic.

use crate::literals::Value;
use crate::parsing::ast::{
    DataValue, Expression, ExpressionKind, LemmaData, LemmaRule, LemmaSpec, ParentType,
    PrimitiveKind, TypeConstraintCommand,
};
use crate::parsing::source::{Source, SourceType};
use crate::Engine;
use std::fmt;

/// Kind of structural quality Recommendation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RecommendationKind {
    SpecMissingCommentary,
    SpecMissingEffectiveDate,
    DataMissingHelp { data: String },
    TextDataWithoutOptions { data: String },
    OpenInputWithoutSuggestion { data: String },
    VetoAsRejectionCascade { rule: String },
}

/// One structural quality Recommendation from [`Engine::quality`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Recommendation {
    pub kind: RecommendationKind,
    pub repository: Option<String>,
    pub spec: String,
    pub source_location: Source,
}

impl fmt::Display for Recommendation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            RecommendationKind::SpecMissingCommentary => write!(
                f,
                "Spec '{}' has no commentary block. Add `\"\"\"...\"\"\"` immediately after the spec line so callers know what this policy covers.",
                self.spec
            ),
            RecommendationKind::SpecMissingEffectiveDate => write!(
                f,
                "Spec '{}' has no effective date. If this encodes a dated policy, declare `spec {} YYYY-MM-DD` so temporal history stays answerable; if the policy is timeless, confirm with the author.",
                self.spec, self.spec
            ),
            RecommendationKind::DataMissingHelp { data } => write!(
                f,
                "`{data}` has no `-> help`. Where does a user find this value? Confirm with the author, then document it."
            ),
            RecommendationKind::TextDataWithoutOptions { data } => write!(
                f,
                "`{data}` accepts any text. If the policy defines a closed set, declare them with `-> option`; if free text is intended, confirm with the author."
            ),
            RecommendationKind::OpenInputWithoutSuggestion { data } => write!(
                f,
                "`{data}` is an open input with no `-> suggest`. If a common default exists in the policy, declare it as a suggestion (UI hint only); otherwise confirm with the author."
            ),
            RecommendationKind::VetoAsRejectionCascade { rule } => write!(
                f,
                "`{rule}` defaults to a boolean and overrides only with veto. Denial is a valid answer (`false`), not an unanswerable question (veto). Prefer boolean sub-rules composed with `and`; confirm the intended outcome with the author."
            ),
        }
    }
}

impl Engine {
    /// Structural quality Recommendations across all loaded specs and their relationships.
    ///
    /// Advisory only: never affects planning or evaluation. Skips dependency
    /// repositories (embedded stdlib and `@owner/repo` imports).
    #[must_use]
    pub fn quality(&self) -> Vec<Recommendation> {
        let mut out = Vec::new();
        for (repository, inner) in self.context.repositories().iter() {
            if repository.dependency.is_some() {
                continue;
            }
            let repo_name = repository.name.clone();
            for (_, spec_set) in inner.iter() {
                for spec in spec_set.iter_specs() {
                    analyze_spec(repo_name.clone(), spec, &mut out);
                }
            }
        }
        out.sort_by(|a, b| {
            (
                a.repository.as_deref().unwrap_or(""),
                a.spec.as_str(),
                a.source_location.span.line,
                a.source_location.span.col,
                a.source_location.span.start,
            )
                .cmp(&(
                    b.repository.as_deref().unwrap_or(""),
                    b.spec.as_str(),
                    b.source_location.span.line,
                    b.source_location.span.col,
                    b.source_location.span.start,
                ))
        });
        out
    }
}

fn analyze_spec(repository: Option<String>, spec: &LemmaSpec, out: &mut Vec<Recommendation>) {
    let loc = spec_location(spec);
    if spec.commentary.is_none() {
        out.push(Recommendation {
            kind: RecommendationKind::SpecMissingCommentary,
            repository: repository.clone(),
            spec: spec.name.clone(),
            source_location: loc.clone(),
        });
    }
    if spec.effective_from.is_origin() {
        out.push(Recommendation {
            kind: RecommendationKind::SpecMissingEffectiveDate,
            repository: repository.clone(),
            spec: spec.name.clone(),
            source_location: loc,
        });
    }

    for data in &spec.data {
        analyze_data(repository.clone(), &spec.name, data, out);
    }
    for rule in &spec.rules {
        analyze_rule(repository.clone(), &spec.name, rule, out);
    }
}

fn spec_location(spec: &LemmaSpec) -> Source {
    Source::new(
        spec.source_type.clone().unwrap_or(SourceType::Volatile),
        crate::parsing::ast::Span {
            start: 0,
            end: 0,
            line: spec.start_line,
            col: 0,
        },
    )
}

fn analyze_data(
    repository: Option<String>,
    spec_name: &str,
    data: &LemmaData,
    out: &mut Vec<Recommendation>,
) {
    let DataValue::Definition {
        base,
        constraints,
        value,
    } = &data.value
    else {
        return;
    };

    let name = data.reference.name.clone();
    let constraints = constraints.as_deref().unwrap_or(&[]);
    let has_help = constraints
        .iter()
        .any(|(c, _)| matches!(c, TypeConstraintCommand::Help));
    let has_option = constraints.iter().any(|(c, _)| {
        matches!(
            c,
            TypeConstraintCommand::Option | TypeConstraintCommand::Options
        )
    });
    let has_suggest = constraints
        .iter()
        .any(|(c, _)| matches!(c, TypeConstraintCommand::Suggest));

    if !has_help {
        out.push(Recommendation {
            kind: RecommendationKind::DataMissingHelp { data: name.clone() },
            repository: repository.clone(),
            spec: spec_name.to_string(),
            source_location: data.source_location.clone(),
        });
    }

    if is_primitive_text(base.as_ref()) && !has_option {
        out.push(Recommendation {
            kind: RecommendationKind::TextDataWithoutOptions { data: name.clone() },
            repository: repository.clone(),
            spec: spec_name.to_string(),
            source_location: data.source_location.clone(),
        });
    }

    if value.is_none() && !has_suggest {
        out.push(Recommendation {
            kind: RecommendationKind::OpenInputWithoutSuggestion { data: name },
            repository,
            spec: spec_name.to_string(),
            source_location: data.source_location.clone(),
        });
    }
}

fn is_primitive_text(base: Option<&ParentType>) -> bool {
    matches!(
        base,
        Some(ParentType::Primitive {
            primitive: PrimitiveKind::Text
        })
    )
}

fn analyze_rule(
    repository: Option<String>,
    spec_name: &str,
    rule: &LemmaRule,
    out: &mut Vec<Recommendation>,
) {
    if !is_boolean_literal(&rule.expression) {
        return;
    }
    if rule.unless_clauses.is_empty() {
        return;
    }
    let all_veto = rule
        .unless_clauses
        .iter()
        .all(|u| matches!(u.result.kind, ExpressionKind::Veto(_)));
    if !all_veto {
        return;
    }
    out.push(Recommendation {
        kind: RecommendationKind::VetoAsRejectionCascade {
            rule: rule.name.clone(),
        },
        repository,
        spec: spec_name.to_string(),
        source_location: rule.source_location.clone(),
    });
}

fn is_boolean_literal(expr: &Expression) -> bool {
    matches!(&expr.kind, ExpressionKind::Literal(Value::Boolean(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::source::SourceType;

    fn load(code: &str) -> Engine {
        let mut engine = Engine::new();
        engine
            .load([(
                SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("test.lemma"))),
                code.to_string(),
            )])
            .expect("BUG: test source must load");
        engine
    }

    fn kinds(engine: &Engine) -> Vec<RecommendationKind> {
        engine.quality().into_iter().map(|r| r.kind).collect()
    }

    #[test]
    fn missing_commentary_and_effective_date() {
        let engine = load("spec pricing\ndata x: number\nrule y: x\n");
        let ks = kinds(&engine);
        assert!(ks.contains(&RecommendationKind::SpecMissingCommentary));
        assert!(ks.contains(&RecommendationKind::SpecMissingEffectiveDate));
    }

    #[test]
    fn clean_spec_has_no_recommendations() {
        let engine = load(
            r#"spec pricing 2026-01-01
"""
Bulk pricing.
"""

data qty: number
  -> minimum 0
  -> help "Order quantity."
  -> suggest 10

rule total: qty
"#,
        );
        assert!(engine.quality().is_empty(), "got: {:?}", engine.quality());
    }

    #[test]
    fn data_missing_help() {
        let engine = load(
            r#"spec pricing 2026-01-01
"""
x
"""

data qty: number -> suggest 1
rule total: qty
"#,
        );
        assert!(kinds(&engine).iter().any(|k| matches!(
            k,
            RecommendationKind::DataMissingHelp { data } if data == "qty"
        )));
    }

    #[test]
    fn text_without_options() {
        let engine = load(
            r#"spec pricing 2026-01-01
"""
x
"""

data status: text
  -> help "Status."
  -> suggest "active"
rule ok: status is "active"
"#,
        );
        assert!(kinds(&engine).iter().any(|k| matches!(
            k,
            RecommendationKind::TextDataWithoutOptions { data } if data == "status"
        )));
    }

    #[test]
    fn open_input_without_suggestion() {
        let engine = load(
            r#"spec pricing 2026-01-01
"""
x
"""

data qty: number
  -> help "Quantity."
rule total: qty
"#,
        );
        assert!(kinds(&engine).iter().any(|k| matches!(
            k,
            RecommendationKind::OpenInputWithoutSuggestion { data } if data == "qty"
        )));
    }

    #[test]
    fn veto_as_rejection_cascade() {
        let engine = load(
            r#"spec eligibility 2026-01-01
"""
Age gate.
"""

data age: number
  -> help "Customer age."
  -> suggest 30

rule is_eligible: true
  unless age < 18 then veto "Must be 18+"
"#,
        );
        assert!(kinds(&engine).iter().any(|k| matches!(
            k,
            RecommendationKind::VetoAsRejectionCascade { rule } if rule == "is_eligible"
        )));
    }

    #[test]
    fn boolean_denial_is_not_cascade() {
        let engine = load(
            r#"spec eligibility 2026-01-01
"""
Age gate.
"""

data age: number
  -> help "Customer age."
  -> suggest 30

rule is_eligible: true
  unless age < 18 then false
"#,
        );
        assert!(!kinds(&engine)
            .iter()
            .any(|k| matches!(k, RecommendationKind::VetoAsRejectionCascade { .. })));
    }

    #[test]
    fn stdlib_dependency_not_reported() {
        let engine = load(
            r#"spec ship 2026-01-01
"""
Weight check.
"""

uses lemma units

data package_weight: units.mass
  -> help "Package weight."
  -> suggest 1 kilogram

rule heavy: package_weight > 20 kilogram
"#,
        );
        let recs = engine.quality();
        assert!(
            recs.iter().all(|r| r.spec != "units"),
            "stdlib units must not appear: {recs:?}"
        );
    }
}
