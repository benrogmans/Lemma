use crate::evaluation::operations::{OperationRecord, OperationResult};
use indexmap::IndexMap;
use serde::Serialize;

/// Facts from a specific document
#[derive(Debug, Clone, Serialize)]
pub struct Facts {
    pub fact_path: String,
    pub referencing_fact_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_reference: Option<String>,
    pub facts: Vec<crate::LemmaFact>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub referenced_docs: Vec<Facts>,
}

/// Response from evaluating a Lemma document
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub doc_name: String,
    pub facts: Vec<Facts>,
    pub results: IndexMap<String, RuleResult>,
}

/// Result of evaluating a single rule
#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    #[serde(skip_serializing)]
    pub rule: crate::LemmaRule,
    pub result: OperationResult,
    pub facts: Vec<crate::LemmaFact>,
    #[serde(skip_serializing)]
    pub operations: Vec<OperationRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<crate::evaluation::proof::Proof>,
    /// Computed type of this rule's result
    /// Every rule MUST have a type (Lemma is strictly typed)
    pub rule_type: crate::LemmaType,
}

impl Response {
    pub fn add_result(&mut self, result: RuleResult) {
        self.results.insert(result.rule.name.clone(), result);
    }

    pub fn filter_rules(&mut self, rule_names: &[String]) {
        self.results.retain(|name, _| rule_names.contains(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Expression, ExpressionKind, LemmaRule, LiteralValue, OperationResult};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn dummy_rule(name: &str) -> LemmaRule {
        LemmaRule {
            name: name.to_string(),
            expression: Expression {
                kind: ExpressionKind::Literal(LiteralValue::boolean(crate::BooleanValue::True)),
                source_location: None,
            },
            unless_clauses: vec![],
            source_location: None,
        }
    }

    #[test]
    fn test_response_serialization() {
        let mut results = IndexMap::new();
        results.insert(
            "test_rule".to_string(),
            RuleResult {
                rule: dummy_rule("test_rule"),
                result: OperationResult::Value(LiteralValue::number(
                    Decimal::from_str("42").unwrap(),
                )),
                facts: vec![],
                operations: vec![],
                proof: None,
                rule_type: crate::semantic::standard_number().clone(),
            },
        );
        let response = Response {
            doc_name: "test_doc".to_string(),
            facts: vec![],
            results,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test_doc"));
        assert!(json.contains("test_rule"));
        assert!(json.contains("results"));
    }

    #[test]
    fn test_response_filter_rules() {
        let mut results = IndexMap::new();
        results.insert(
            "rule1".to_string(),
            RuleResult {
                rule: dummy_rule("rule1"),
                result: OperationResult::Value(LiteralValue::boolean(crate::BooleanValue::True)),
                facts: vec![],
                operations: vec![],
                proof: None,
                rule_type: crate::semantic::standard_boolean().clone(),
            },
        );
        results.insert(
            "rule2".to_string(),
            RuleResult {
                rule: dummy_rule("rule2"),
                result: OperationResult::Value(LiteralValue::boolean(crate::BooleanValue::False)),
                facts: vec![],
                operations: vec![],
                proof: None,
                rule_type: crate::semantic::standard_boolean().clone(),
            },
        );
        let mut response = Response {
            doc_name: "test_doc".to_string(),
            facts: vec![],
            results,
        };

        response.filter_rules(&["rule1".to_string()]);

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results.values().next().unwrap().rule.name, "rule1");
    }

    #[test]
    fn test_rule_result_types() {
        let success = RuleResult {
            rule: dummy_rule("rule1"),
            result: OperationResult::Value(LiteralValue::boolean(crate::BooleanValue::True)),
            facts: vec![],
            operations: vec![],
            proof: None,
            rule_type: crate::semantic::standard_boolean().clone(),
        };
        assert!(matches!(success.result, OperationResult::Value(_)));

        let missing = RuleResult {
            rule: dummy_rule("rule3"),
            result: OperationResult::Veto(Some("Missing fact: fact1".to_string())),
            facts: vec![crate::LemmaFact {
                reference: crate::FactReference::from_path(vec!["fact1".to_string()]),
                value: crate::FactValue::TypeDeclaration {
                    base: "number".to_string(),
                    overrides: None,
                    from: None,
                },
                source_location: None,
            }],
            operations: vec![],
            proof: None,
            rule_type: crate::LemmaType::veto_type(),
        };
        assert_eq!(missing.facts.len(), 1);
        assert_eq!(missing.facts[0].reference.to_string(), "fact1");
        assert!(matches!(
            missing.facts[0].value,
            crate::FactValue::TypeDeclaration { .. }
        ));
        assert!(matches!(missing.result, OperationResult::Veto(_)));

        let veto = RuleResult {
            rule: dummy_rule("rule4"),
            result: OperationResult::Veto(Some("Vetoed".to_string())),
            facts: vec![],
            operations: vec![],
            proof: None,
            rule_type: crate::LemmaType::veto_type(),
        };
        assert_eq!(
            veto.result,
            OperationResult::Veto(Some("Vetoed".to_string()))
        );
    }
}
