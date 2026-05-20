use lemma::evaluation::OperationResult;
use lemma::parsing::ast::DateTimeValue;
use lemma::Engine;
use rust_decimal::Decimal;
use std::collections::HashMap;

#[test]
fn rule_vetoes_when_result_exceeds_decimal_wire_limit() {
    let max_decimal = Decimal::MAX.normalize().to_string();
    let code = format!(
        r#"
spec decimal_limit
data max_val: {max_decimal}
data two: 2
rule over_limit: max_val * two
"#
    );

    let mut engine = Engine::new();
    engine.load(&code, lemma::SourceType::Volatile).unwrap();

    let now = DateTimeValue::now();
    let response = engine
        .run(
            None,
            "decimal_limit",
            Some(&now),
            HashMap::new(),
            false,
            lemma::EvaluationRequest::default(),
        )
        .expect("evaluation must complete");

    let rule = response
        .results
        .get("over_limit")
        .expect("over_limit rule must be present");

    match &rule.result {
        OperationResult::Veto(veto) => {
            assert_eq!(
                veto.to_string(),
                "Calculated result exceeds decimal value limit"
            );
        }
        OperationResult::Value(value) => {
            panic!("expected Veto for uncommittable result, got {value:?}");
        }
    }
}

#[test]
fn response_number_json_never_uses_fraction_notation() {
    use indexmap::IndexMap;
    use lemma::evaluation::response::{EvaluatedRule, Response, RuleResult};
    use lemma::planning::semantics::{
        Expression, ExpressionKind, LiteralValue, RulePath, Source, Span,
    };

    fn dummy_source() -> Source {
        Source::new(
            lemma::SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 1,
            },
        )
    }

    let mut results = IndexMap::new();
    results.insert(
        "third".to_string(),
        RuleResult {
            rule: EvaluatedRule {
                name: "third".to_string(),
                path: RulePath::new(vec![], "third".to_string()),
                default_expression: Expression::new(
                    ExpressionKind::Literal(Box::new(LiteralValue::number_from_decimal(
                        Decimal::new(1, 1) / Decimal::new(3, 1),
                    ))),
                    dummy_source(),
                ),
                unless_branches: vec![],
                source_location: dummy_source(),
                rule_type: lemma::planning::semantics::primitive_number().clone(),
            },
            result: OperationResult::Value(Box::new(LiteralValue::number_from_decimal(
                Decimal::new(1, 1) / Decimal::new(3, 1),
            ))),
            data: vec![],
            operations: vec![],
            explanation: None,
            rule_type: lemma::planning::semantics::primitive_number().clone(),
        },
    );

    let response = Response {
        spec_name: "test".to_string(),
        spec_hash: None,
        spec_effective_from: None,
        spec_effective_to: None,
        data: vec![],
        results,
    };

    let json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
    let number = json["results"]["third"]["result"]["value"]["value"]["number"]
        .as_str()
        .expect("number must be a JSON string");
    assert!(
        !number.contains('/'),
        "wire number must not use fraction notation, got {number}"
    );
}
