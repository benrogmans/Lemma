use comfy_table::{presets::UTF8_FULL, Cell, CellAlignment, Row, Table};
use lemma::{ComputationKind, Fact, LiteralValue, OperationRecord, Response, RuleResult};

enum LineType {
    RuleName,
    Computation,
    FinalResult,
    UnlessMatched,
    UnlessRejected,
    UnlessFinalResult,
    DefaultValue,
}

impl LineType {
    fn format_line(&self, base_prefix: &str, content: &str) -> String {
        let symbol = match self {
            LineType::RuleName => "├─",
            LineType::Computation => "├─ =",
            LineType::FinalResult => "└─ =",
            LineType::UnlessMatched => "├─>",
            LineType::UnlessRejected => "×",
            LineType::UnlessFinalResult => "└─>",
            LineType::DefaultValue => "└─ =",
        };
        format!("{}{} {}\n", base_prefix, symbol, content)
    }
}

pub struct Formatter {}

impl Default for Formatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter {
    pub fn new() -> Self {
        Self {}
    }

    pub fn format_response(&self, response: &Response, _raw: bool) -> String {
        let mut output = String::new();

        if !response.facts.is_empty() {
            output.push_str(&self.format_facts_table(&response.facts));
            output.push('\n');
        }

        // Sort results by source order
        let mut sorted_results = response.results.clone();
        sorted_results.sort_by_key(|result| {
            result
                .rule
                .span
                .as_ref()
                .map(|s| s.start)
                .unwrap_or(usize::MAX)
        });

        for result in &sorted_results {
            output.push_str(&self.format_rule_result(result));
            output.push('\n');
        }

        output
    }

    fn format_facts_table(&self, facts: &[Fact]) -> String {
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(Row::from(vec![
            Cell::new("Fact").set_alignment(CellAlignment::Left),
            Cell::new("Value").set_alignment(CellAlignment::Left),
        ]));

        for fact in facts {
            let value_str = fact
                .value
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".to_string());
            table.add_row(Row::from(vec![&fact.name, &value_str]));
        }

        table.to_string()
    }

    fn format_rule_result(&self, result: &RuleResult) -> String {
        let title = format!(
            "{} = {}",
            result.rule.name,
            result
                .result
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "?".to_string())
        );

        let mut content = String::new();
        self.format_operations(&result.operations, &mut content);

        let mut table = Table::new();
        table.load_preset(UTF8_FULL);

        table.add_row(Row::from(vec![Cell::new(&title)]));

        let content = content.trim_end();
        if !content.is_empty() {
            table.add_row(Row::from(vec![Cell::new(content)]));
        }

        table.to_string()
    }

    fn format_operations(&self, operations: &[OperationRecord], output: &mut String) {
        // Walk the flat operations list once, using depth for indentation
        // The engine has already inlined nested operations right after RuleUsed records

        let mut i = 0;
        while i < operations.len() {
            let op = &operations[i];
            let depth = self.depth(op);
            let indent = "│  ".repeat(depth);

            match &op.kind {
                lemma::OperationKind::RuleUsed { rule_ref, .. } => {
                    // Show rule name - nested operations follow
                    output.push_str(
                        &LineType::RuleName.format_line(&indent, &format!("{}?", rule_ref)),
                    );
                }

                lemma::OperationKind::Computation {
                    kind,
                    inputs,
                    result,
                    ..
                } => {
                    // For comparisons: check if this is part of an unless clause
                    if matches!(kind, ComputationKind::Comparison(_)) {
                        // Check if the next operation is a matched unless clause
                        if let Some(next_op) = operations.get(i + 1) {
                            if let lemma::OperationKind::UnlessClauseEvaluated {
                                matched: true,
                                ..
                            } = &next_op.kind
                            {
                                if next_op.depth == depth {
                                    // This is the condition for the unless - show it
                                    let calc = self.format_computation(kind, inputs);
                                    output.push_str(
                                        &LineType::Computation.format_line(&indent, &calc),
                                    );
                                    i += 1;
                                    continue;
                                }
                            }
                        }
                    }

                    // For arithmetic: only show if it's the last one at this depth
                    if !matches!(kind, ComputationKind::Comparison(_))
                        && self.is_last_arithmetic_at_depth(operations, i, depth)
                    {
                        let calc = self.format_computation(kind, inputs);
                        output.push_str(&LineType::Computation.format_line(&indent, &calc));
                        output.push_str(
                            &LineType::FinalResult.format_line(&indent, &result.to_string()),
                        );
                    }
                }

                lemma::OperationKind::UnlessClauseEvaluated {
                    matched,
                    condition_expr,
                    result_expr,
                    index,
                    ..
                } => {
                    if *matched {
                        // Show matched condition if not already shown by a rule expansion
                        if !self.has_expanded_rule_before(operations, i, depth) {
                            if let Some(cond) = condition_expr {
                                if depth == 0 {
                                    output.push_str(&format!("{}\n", cond));
                                } else {
                                    output.push_str(
                                        &LineType::UnlessMatched.format_line(&indent, cond),
                                    );
                                }
                            }
                        }

                        // Show rejected unless clauses that come after this one
                        for rej_op in operations.iter().skip(i + 1) {
                            if let lemma::OperationKind::UnlessClauseEvaluated {
                                matched: false,
                                condition_expr: Some(cond),
                                index: rej_index,
                                ..
                            } = &rej_op.kind
                            {
                                if rej_op.depth == depth && *rej_index > *index {
                                    output.push_str(
                                        &LineType::UnlessRejected.format_line(&indent, cond),
                                    );
                                }
                            }
                        }

                        // Show result expression
                        if let Some(expr) = result_expr {
                            // Check if there are arithmetic operations after
                            let has_arithmetic_after = operations[i+1..].iter().any(|op| {
                                op.depth == depth && matches!(&op.kind, lemma::OperationKind::Computation { kind, .. } if !matches!(kind, ComputationKind::Comparison(_)))
                            });

                            if !has_arithmetic_after {
                                output.push_str(
                                    &LineType::UnlessFinalResult.format_line(&indent, expr),
                                );
                            } else {
                                output
                                    .push_str(&LineType::UnlessMatched.format_line(&indent, expr));
                            }
                        }
                    }
                }

                lemma::OperationKind::DefaultValue { expr, .. } => {
                    // Only show if not preceded by an expanded rule
                    if !self.has_expanded_rule_at_depth(operations, depth) {
                        if let Some(e) = expr {
                            if depth == 0 {
                                output.push_str(&format!("{}\n", e));
                            } else {
                                output.push_str(&LineType::DefaultValue.format_line(&indent, e));
                            }
                        }
                    }
                }

                _ => {}
            }

            i += 1;
        }
    }

    fn depth(&self, op: &OperationRecord) -> usize {
        op.depth
    }

    fn has_expanded_rule_before(
        &self,
        operations: &[OperationRecord],
        idx: usize,
        target_depth: usize,
    ) -> bool {
        // Check if there's a RuleUsed with nested logic before this index at the same depth
        for i in (0..idx).rev() {
            let depth = self.depth(&operations[i]);
            if depth < target_depth {
                break;
            }
            if depth == target_depth
                && matches!(&operations[i].kind, lemma::OperationKind::RuleUsed { .. })
            {
                // Check if it has nested operations
                if i + 1 < operations.len() && self.depth(&operations[i + 1]) > target_depth {
                    return true;
                }
            }
        }
        false
    }

    fn has_expanded_rule_at_depth(
        &self,
        operations: &[OperationRecord],
        target_depth: usize,
    ) -> bool {
        operations.iter().enumerate().any(|(i, op)| {
            self.depth(op) == target_depth
                && matches!(&op.kind, lemma::OperationKind::RuleUsed { .. })
                && i + 1 < operations.len()
                && self.depth(&operations[i + 1]) > target_depth
        })
    }

    fn is_last_arithmetic_at_depth(
        &self,
        operations: &[OperationRecord],
        idx: usize,
        depth: usize,
    ) -> bool {
        !operations.iter().skip(idx + 1).any(|op| {
            self.depth(op) == depth
                &&                 matches!(
                    &op.kind,
                    lemma::OperationKind::Computation { kind, .. } if !matches!(kind, ComputationKind::Comparison(_))
                )
        })
    }

    fn format_computation(&self, kind: &ComputationKind, inputs: &[LiteralValue]) -> String {
        match kind {
            ComputationKind::Arithmetic(op) => {
                if inputs.len() == 2 {
                    let symbol = match op {
                        lemma::ArithmeticComputation::Add => "+",
                        lemma::ArithmeticComputation::Subtract => "-",
                        lemma::ArithmeticComputation::Multiply => "*",
                        lemma::ArithmeticComputation::Divide => "/",
                        lemma::ArithmeticComputation::Modulo => "%",
                        lemma::ArithmeticComputation::Power => "^",
                    };
                    format!("{} {} {}", inputs[0], symbol, inputs[1])
                } else {
                    format!("{:?} {:?}", op, inputs)
                }
            }
            ComputationKind::Comparison(op) => {
                if inputs.len() == 2 {
                    let symbol = match op {
                        lemma::ComparisonComputation::GreaterThan => ">",
                        lemma::ComparisonComputation::LessThan => "<",
                        lemma::ComparisonComputation::GreaterThanOrEqual => ">=",
                        lemma::ComparisonComputation::LessThanOrEqual => "<=",
                        lemma::ComparisonComputation::Equal => "==",
                        lemma::ComparisonComputation::NotEqual => "!=",
                        lemma::ComparisonComputation::Is => "is",
                        lemma::ComparisonComputation::IsNot => "is not",
                    };
                    format!("{} {} {}", inputs[0], symbol, inputs[1])
                } else {
                    format!("{:?} {:?}", op, inputs)
                }
            }
            ComputationKind::Mathematical(op) => {
                if !inputs.is_empty() {
                    let func = match op {
                        lemma::MathematicalComputation::Sqrt => "sqrt",
                        lemma::MathematicalComputation::Sin => "sin",
                        lemma::MathematicalComputation::Cos => "cos",
                        lemma::MathematicalComputation::Tan => "tan",
                        lemma::MathematicalComputation::Asin => "asin",
                        lemma::MathematicalComputation::Acos => "acos",
                        lemma::MathematicalComputation::Atan => "atan",
                        lemma::MathematicalComputation::Abs => "abs",
                        lemma::MathematicalComputation::Floor => "floor",
                        lemma::MathematicalComputation::Ceil => "ceil",
                        lemma::MathematicalComputation::Round => "round",
                        lemma::MathematicalComputation::Log => "log",
                        lemma::MathematicalComputation::Exp => "exp",
                    };
                    format!("{}({})", func, inputs[0])
                } else {
                    format!("{:?} {:?}", op, inputs)
                }
            }
        }
    }

    pub fn format_document_inspection(
        &self,
        doc: &lemma::LemmaDoc,
        facts: &[&lemma::LemmaFact],
        rules: &[&lemma::LemmaRule],
    ) -> String {
        let mut output = String::new();
        output.push_str(&format!("Document: {}\n\n", doc.name));
        output.push_str(&format!("facts ({}):\n", facts.len()));
        for fact in facts {
            output.push_str(&format!("  - {}\n", fact.fact_type));
        }
        output.push_str(&format!("\nrules ({}):\n", rules.len()));
        for rule in rules {
            output.push_str(&format!("  - {}\n", rule.name));
        }
        output
    }

    pub fn format_workspace_summary(
        &self,
        file_count: usize,
        doc_count: usize,
        doc_stats: &[(String, usize, usize)],
    ) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "Workspace contains {} files, {} documents\n\n",
            file_count, doc_count
        ));
        for (name, facts, rules) in doc_stats {
            output.push_str(&format!("{}: {} facts, {} rules\n", name, facts, rules));
        }
        output
    }

    pub fn format_inversion_result(
        &self,
        solutions: &[std::collections::HashMap<lemma::FactReference, lemma::Domain>],
    ) -> String {
        let mut output = String::new();
        output.push_str(&format!("Found {} solution(s)\n\n", solutions.len()));
        for (idx, solution) in solutions.iter().enumerate() {
            output.push_str(&format!("Solution {}:\n", idx + 1));
            for (fact_ref, domain) in solution {
                output.push_str(&format!(
                    "  {}: {:?}\n",
                    fact_ref.reference.join("."),
                    domain
                ));
            }
            output.push('\n');
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lemma::*;

    fn dummy_rule(name: &str) -> LemmaRule {
        LemmaRule {
            name: name.to_string(),
            expression: Expression {
                kind: ExpressionKind::Literal(LiteralValue::Number(rust_decimal::Decimal::new(
                    0, 0,
                ))),
                span: None,
                id: ExpressionId::new(0),
            },
            unless_clauses: vec![],
            span: None,
        }
    }

    fn rule_ref(name: &str) -> RuleReference {
        RuleReference {
            reference: vec![name.to_string()],
        }
    }

    static mut NEXT_OP_ID: usize = 0;

    fn next_op_id() -> lemma::OperationId {
        unsafe {
            let id = lemma::OperationId(NEXT_OP_ID);
            NEXT_OP_ID += 1;
            id
        }
    }

    fn op(kind: lemma::OperationKind, depth: usize) -> OperationRecord {
        OperationRecord {
            id: next_op_id(),
            parent_id: None,
            depth,
            kind,
        }
    }

    #[test]
    fn test_simple_fact_display() {
        let response = Response {
            doc_name: "test".to_string(),
            facts: vec![Fact {
                name: "amount".to_string(),
                value: Some(LiteralValue::Number(rust_decimal::Decimal::new(100000, 2))),
            }],
            results: vec![],
        };

        let formatter = Formatter::new();
        let output = formatter.format_response(&response, false);

        assert!(output.contains("amount"));
        assert!(output.contains("1_000"));
    }

    #[test]
    fn test_simple_computation() {
        let response = Response {
            doc_name: "test".to_string(),
            facts: vec![],
            results: vec![RuleResult {
                rule: dummy_rule("total"),
                result: Some(LiteralValue::Number(rust_decimal::Decimal::new(150, 0))),
                veto_message: None,
                operations: vec![op(
                    lemma::OperationKind::Computation {
                        kind: ComputationKind::Arithmetic(ArithmeticComputation::Add),
                        inputs: vec![
                            LiteralValue::Number(rust_decimal::Decimal::new(100, 0)),
                            LiteralValue::Number(rust_decimal::Decimal::new(50, 0)),
                        ],
                        result: LiteralValue::Number(rust_decimal::Decimal::new(150, 0)),
                        expr: Some("100 + 50".to_string()),
                    },
                    0,
                )],
                facts: vec![],
            }],
        };

        let formatter = Formatter::new();
        let output = formatter.format_response(&response, false);

        assert!(output.contains("total = 150"));
        assert!(output.contains("100 + 50"));
    }

    #[test]
    fn test_nested_rule_no_duplicate_results() {
        let response = Response {
            doc_name: "test".to_string(),
            facts: vec![],
            results: vec![RuleResult {
                rule: dummy_rule("outer"),
                result: Some(LiteralValue::Number(rust_decimal::Decimal::new(200, 0))),
                veto_message: None,
                operations: vec![
                    op(
                        lemma::OperationKind::RuleUsed {
                            rule_ref: rule_ref("inner"),
                            value: LiteralValue::Number(rust_decimal::Decimal::new(100, 0)),
                        },
                        0,
                    ),
                    op(
                        lemma::OperationKind::Computation {
                            kind: ComputationKind::Arithmetic(ArithmeticComputation::Add),
                            inputs: vec![
                                LiteralValue::Number(rust_decimal::Decimal::new(50, 0)),
                                LiteralValue::Number(rust_decimal::Decimal::new(50, 0)),
                            ],
                            result: LiteralValue::Number(rust_decimal::Decimal::new(100, 0)),
                            expr: Some("50 + 50".to_string()),
                        },
                        1,
                    ),
                    op(
                        lemma::OperationKind::Computation {
                            kind: ComputationKind::Arithmetic(ArithmeticComputation::Multiply),
                            inputs: vec![
                                LiteralValue::Number(rust_decimal::Decimal::new(100, 0)),
                                LiteralValue::Number(rust_decimal::Decimal::new(2, 0)),
                            ],
                            result: LiteralValue::Number(rust_decimal::Decimal::new(200, 0)),
                            expr: Some("inner? * 2".to_string()),
                        },
                        0,
                    ),
                ],
                facts: vec![],
            }],
        };

        let formatter = Formatter::new();
        let output = formatter.format_response(&response, false);

        // Should show nested rule expansion
        assert!(output.contains("inner?"));
        assert!(output.contains("50 + 50"));
        assert!(output.contains("100 * 2"));
        assert!(output.contains("200"));

        // Should NOT show duplicate results after rule expansion
        let inner_result_count = output.matches("└─ = 100").count();
        assert_eq!(
            inner_result_count, 1,
            "inner? result should appear exactly once"
        );
    }

    #[test]
    fn test_unless_clause_formatting() {
        let response = Response {
            doc_name: "test".to_string(),
            facts: vec![],
            results: vec![RuleResult {
                rule: dummy_rule("bonus"),
                result: Some(LiteralValue::Percentage(rust_decimal::Decimal::new(10, 0))),
                veto_message: None,
                operations: vec![
                    op(
                        lemma::OperationKind::Computation {
                            kind: ComputationKind::Comparison(
                                ComparisonComputation::GreaterThanOrEqual,
                            ),
                            inputs: vec![
                                LiteralValue::Number(rust_decimal::Decimal::new(4, 0)),
                                LiteralValue::Number(rust_decimal::Decimal::new(35, 1)),
                            ],
                            result: LiteralValue::Boolean(true),
                            expr: Some("rating >= 3.5".to_string()),
                        },
                        0,
                    ),
                    op(
                        lemma::OperationKind::UnlessClauseEvaluated {
                            index: 0,
                            matched: true,
                            result_if_matched: Some(LiteralValue::Percentage(
                                rust_decimal::Decimal::new(10, 0),
                            )),
                            condition_expr: Some("rating >= 3.5".to_string()),
                            result_expr: Some("10%".to_string()),
                        },
                        0,
                    ),
                    op(
                        lemma::OperationKind::UnlessClauseEvaluated {
                            index: 1,
                            matched: false,
                            result_if_matched: Some(LiteralValue::Percentage(
                                rust_decimal::Decimal::new(15, 0),
                            )),
                            condition_expr: Some("rating >= 4.5".to_string()),
                            result_expr: Some("15%".to_string()),
                        },
                        0,
                    ),
                ],
                facts: vec![],
            }],
        };

        let formatter = Formatter::new();
        let output = formatter.format_response(&response, false);

        // Should show matched condition and result
        assert!(output.contains("rating >= 3.5"));
        assert!(output.contains("10%"));

        // Should show rejected clause with cross
        assert!(output.contains("×"));
        assert!(output.contains("rating >= 4.5"));
    }

    #[test]
    fn test_default_value_formatting() {
        let response = Response {
            doc_name: "test".to_string(),
            facts: vec![],
            results: vec![RuleResult {
                rule: dummy_rule("fallback"),
                result: Some(LiteralValue::Number(rust_decimal::Decimal::new(0, 0))),
                veto_message: None,
                operations: vec![op(
                    lemma::OperationKind::DefaultValue {
                        expr: Some("0".to_string()),
                        value: LiteralValue::Number(rust_decimal::Decimal::new(0, 0)),
                    },
                    0,
                )],
                facts: vec![],
            }],
        };

        let formatter = Formatter::new();
        let output = formatter.format_response(&response, false);

        // Should show default value (either as "└─ = 0" or just "0")
        assert!(output.contains("0"));
        assert!(output.contains("fallback"));
    }

    #[test]
    fn test_percentage_rounding() {
        let response = Response {
            doc_name: "test".to_string(),
            facts: vec![],
            results: vec![RuleResult {
                rule: dummy_rule("rate"),
                result: Some(LiteralValue::Percentage(rust_decimal::Decimal::new(
                    21361, 3,
                ))),
                veto_message: None,
                operations: vec![],
                facts: vec![],
            }],
        };

        let formatter = Formatter::new();
        let output = formatter.format_response(&response, false);

        // Should round percentage to 2 decimal places
        assert!(output.contains("21.36%"));
    }

    #[test]
    fn test_number_thousand_separators() {
        let response = Response {
            doc_name: "test".to_string(),
            facts: vec![Fact {
                name: "large".to_string(),
                value: Some(LiteralValue::Number(rust_decimal::Decimal::new(1000000, 0))),
            }],
            results: vec![],
        };

        let formatter = Formatter::new();
        let output = formatter.format_response(&response, false);

        // Should format with thousand separators
        assert!(output.contains("1_000_000"));
    }

    #[test]
    fn test_rules_sorted_by_source_order() {
        let mut rule_a = dummy_rule("rule_a");
        rule_a.span = Some(Span {
            start: 100,
            end: 150,
            line: 10,
            col: 0,
        });

        let mut rule_b = dummy_rule("rule_b");
        rule_b.span = Some(Span {
            start: 50,
            end: 80,
            line: 5,
            col: 0,
        });

        let response = Response {
            doc_name: "test".to_string(),
            facts: vec![],
            results: vec![
                RuleResult {
                    rule: rule_a,
                    result: Some(LiteralValue::Number(rust_decimal::Decimal::new(1, 0))),
                    veto_message: None,
                    operations: vec![],
                    facts: vec![],
                },
                RuleResult {
                    rule: rule_b,
                    result: Some(LiteralValue::Number(rust_decimal::Decimal::new(2, 0))),
                    veto_message: None,
                    operations: vec![],
                    facts: vec![],
                },
            ],
        };

        let formatter = Formatter::new();
        let output = formatter.format_response(&response, false);

        // Should display in source order (rule_b before rule_a)
        let rule_b_pos = output.find("rule_b").unwrap();
        let rule_a_pos = output.find("rule_a").unwrap();
        assert!(rule_b_pos < rule_a_pos, "Rules should be in source order");
    }
}
