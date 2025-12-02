//! Execution plan for evaluated documents
//!
//! Provides a complete self-contained execution plan ready for the evaluator.
//! The plan contains all facts, rules flattened into executable branches,
//! and execution order - no document structure needed during evaluation.

use crate::planning::graph::Graph;
use crate::semantic::{
    Expression, FactPath, FactValue, LemmaFact, LemmaType, LiteralValue, RulePath, TypeAnnotation,
};
use crate::LemmaError;
use crate::ResourceLimits;
use crate::Source;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A complete execution plan ready for the evaluator
///
/// Contains the topologically sorted list of rules to execute, along with all facts.
/// Self-contained structure - no document lookups required during evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Main document name
    pub doc_name: String,

    /// All facts with their values
    #[serde(serialize_with = "crate::serialization::serialize_fact_path_map")]
    #[serde(deserialize_with = "crate::serialization::deserialize_fact_path_map")]
    pub facts: HashMap<FactPath, LemmaFact>,

    /// Rules to execute in topological order (sorted by dependencies)
    pub rules: Vec<ExecutableRule>,

    /// Source code for error messages
    pub sources: HashMap<String, String>,
}

/// An executable rule with flattened branches
///
/// Contains all information needed to evaluate a rule without document lookups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableRule {
    /// Unique identifier for this rule
    pub path: RulePath,

    /// Rule name
    pub name: String,

    /// Branches evaluated in order (last matching wins)
    /// First branch has condition=None (default expression)
    /// Subsequent branches have condition=Some(...) (unless clauses)
    /// The evaluation is done in reverse order with the earliest matching branch returning (winning) the result.
    pub branches: Vec<Branch>,

    /// All facts this rule needs (direct + inherited from rule dependencies)
    #[serde(serialize_with = "crate::serialization::serialize_fact_path_set")]
    #[serde(deserialize_with = "crate::serialization::deserialize_fact_path_set")]
    pub needs_facts: HashSet<FactPath>,

    /// Source location for error messages
    pub source: Option<Source>,
}

/// A branch in an executable rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    /// Condition expression (None for default branch)
    pub condition: Option<Expression>,

    /// Result expression
    pub result: Expression,

    /// Source location for error messages
    pub source: Option<Source>,
}

/// Builds an execution plan from a Graph.
/// Internal implementation detail - only called by plan()
pub(crate) fn build_execution_plan(graph: &Graph, main_doc_name: &str) -> ExecutionPlan {
    let execution_order = graph.execution_order();
    let facts: HashMap<FactPath, LemmaFact> = graph
        .facts()
        .iter()
        .filter(|(_, fact)| {
            matches!(
                fact.value,
                FactValue::Literal(_) | FactValue::TypeAnnotation(_)
            )
        })
        .map(|(path, fact)| (path.clone(), fact.clone()))
        .collect();

    let mut executable_rules: Vec<ExecutableRule> = Vec::new();

    for rule_path in execution_order {
        let rule_node = graph.rules().get(rule_path).expect(
            "bug: rule from topological sort not in graph - validation should have caught this",
        );

        let mut executable_branches = Vec::new();
        for (condition, result) in &rule_node.branches {
            executable_branches.push(Branch {
                condition: condition.clone(),
                result: result.clone(),
                source: Some(rule_node.source.clone()),
            });
        }

        executable_rules.push(ExecutableRule {
            path: rule_path.clone(),
            name: rule_path.rule.clone(),
            branches: executable_branches,
            source: Some(rule_node.source.clone()),
            needs_facts: HashSet::new(),
        });
    }

    populate_needs_facts(&mut executable_rules, graph);

    ExecutionPlan {
        doc_name: main_doc_name.to_string(),
        facts,
        rules: executable_rules,
        sources: graph.sources().clone(),
    }
}

fn populate_needs_facts(rules: &mut [ExecutableRule], graph: &Graph) {
    let mut rule_facts: HashMap<RulePath, HashSet<FactPath>> = HashMap::new();

    for rule in rules.iter_mut() {
        let mut facts = HashSet::new();

        for branch in &rule.branches {
            if let Some(cond) = &branch.condition {
                cond.collect_fact_paths(&mut facts);
            }
            branch.result.collect_fact_paths(&mut facts);
        }

        if let Some(rule_node) = graph.rules().get(&rule.path) {
            for dep_rule in &rule_node.depends_on_rules {
                if let Some(dep_facts) = rule_facts.get(dep_rule) {
                    facts.extend(dep_facts.iter().cloned());
                }
            }
        }

        rule.needs_facts = facts.clone();
        rule_facts.insert(rule.path.clone(), facts);
    }
}

impl ExecutionPlan {
    /// Look up a fact by its path string (e.g., "age" or "rules.base_price").
    pub fn get_fact_by_path_str(&self, name: &str) -> Option<(&FactPath, &LemmaFact)> {
        self.facts.iter().find(|(path, _)| path.to_string() == name)
    }

    /// Look up a local rule by its name (rule in the main document).
    pub fn get_rule(&self, name: &str) -> Option<&ExecutableRule> {
        self.rules
            .iter()
            .find(|r| r.name == name && r.path.segments.is_empty())
    }

    /// Look up a rule by its full path.
    pub fn get_rule_by_path(&self, rule_path: &RulePath) -> Option<&ExecutableRule> {
        self.rules.iter().find(|r| &r.path == rule_path)
    }

    /// Get the literal value for a fact path, if it exists and has a literal value.
    pub fn get_fact_value(&self, path: &FactPath) -> Option<&LiteralValue> {
        self.facts.get(path).and_then(|fact| match &fact.value {
            FactValue::Literal(lit) => Some(lit),
            _ => None,
        })
    }

    /// Provide string values for facts by parsing them to their expected types.
    ///
    /// This is the main entry point for providing user-supplied string values.
    /// It parses each string value to the expected type, checks resource limits,
    /// and applies the values to the plan.
    pub fn with_values(
        self,
        values: HashMap<String, String>,
        limits: &ResourceLimits,
    ) -> Result<Self, LemmaError> {
        if values.is_empty() {
            return Ok(self);
        }

        let typed = self.parse_values(values)?;
        self.with_typed_values(typed, limits)
    }

    /// Provide pre-typed values for facts with resource limit checking.
    ///
    /// Use this for programmatic APIs where values are already parsed.
    pub fn with_typed_values(
        mut self,
        values: HashMap<String, LiteralValue>,
        limits: &ResourceLimits,
    ) -> Result<Self, LemmaError> {
        for (name, value) in &values {
            let size = value.byte_size();
            if size > limits.max_fact_value_bytes {
                return Err(LemmaError::ResourceLimitExceeded {
                    limit_name: "max_fact_value_bytes".to_string(),
                    limit_value: limits.max_fact_value_bytes.to_string(),
                    actual_value: size.to_string(),
                    suggestion: format!(
                        "Reduce the size of fact values to {} bytes or less",
                        limits.max_fact_value_bytes
                    ),
                });
            }

            let (fact_path, existing_fact) = self
                .get_fact_by_path_str(name)
                .ok_or_else(|| LemmaError::Engine(format!("Unknown fact: {}", name)))?;
            let fact_path = fact_path.clone();

            let expected_type = self.get_fact_type(existing_fact)?;
            let actual_type = value.to_type();
            if actual_type != expected_type {
                return Err(LemmaError::Engine(format!(
                    "Type mismatch for fact {}: expected {:?}, got {:?}",
                    name, expected_type, actual_type
                )));
            }

            if let Some(existing) = self.facts.get_mut(&fact_path) {
                existing.value = FactValue::Literal(value.clone());
            }
        }

        Ok(self)
    }

    fn get_fact_type(&self, fact: &LemmaFact) -> Result<LemmaType, LemmaError> {
        match &fact.value {
            FactValue::Literal(lit) => Ok(lit.to_type()),
            FactValue::TypeAnnotation(TypeAnnotation::LemmaType(t)) => Ok(t.clone()),
            FactValue::DocumentReference(_) => Err(LemmaError::Engine(
                "Cannot provide a value for a document reference fact".to_string(),
            )),
        }
    }

    fn parse_values(
        &self,
        values: HashMap<String, String>,
    ) -> Result<HashMap<String, LiteralValue>, LemmaError> {
        let mut typed = HashMap::new();

        for (fact_key, raw_value) in values {
            let (_, fact) = self.get_fact_by_path_str(&fact_key).ok_or_else(|| {
                let available: Vec<String> = self.facts.keys().map(|p| p.to_string()).collect();
                LemmaError::Engine(format!(
                    "Fact '{}' not found. Available facts: {}",
                    fact_key,
                    available.join(", ")
                ))
            })?;

            let expected_type = self.get_fact_type(fact)?;

            let literal_value = expected_type.parse_value(&raw_value).map_err(|e| {
                LemmaError::Engine(format!(
                    "Failed to parse fact '{}' as {}: {}",
                    fact_key, expected_type, e
                ))
            })?;

            typed.insert(fact_key, literal_value);
        }

        Ok(typed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::{FactPath, FactReference, FactValue, LemmaType, LiteralValue};
    use serde_json;

    fn default_limits() -> ResourceLimits {
        ResourceLimits::default()
    }

    #[test]
    fn test_with_typed_values() {
        let fact_path = FactPath {
            segments: vec![],
            fact: "age".to_string(),
        };
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: {
                let mut f = HashMap::new();
                f.insert(
                    fact_path.clone(),
                    LemmaFact {
                        reference: FactReference {
                            segments: vec![],
                            fact: "age".to_string(),
                        },
                        value: FactValue::Literal(LiteralValue::Number(25.into())),
                        source_location: None,
                    },
                );
                f
            },
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("age".to_string(), LiteralValue::Number(30.into()));

        let updated_plan = plan.with_typed_values(values, &default_limits()).unwrap();
        let updated_fact = updated_plan.facts.get(&fact_path).unwrap();
        match &updated_fact.value {
            FactValue::Literal(LiteralValue::Number(n)) => assert_eq!(*n, 30.into()),
            _ => panic!("Expected number literal"),
        }
    }

    #[test]
    fn test_with_typed_values_type_mismatch() {
        let fact_path = FactPath {
            segments: vec![],
            fact: "age".to_string(),
        };
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: {
                let mut f = HashMap::new();
                f.insert(
                    fact_path,
                    LemmaFact {
                        reference: FactReference {
                            segments: vec![],
                            fact: "age".to_string(),
                        },
                        value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(
                            LemmaType::Number,
                        )),
                        source_location: None,
                    },
                );
                f
            },
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("age".to_string(), LiteralValue::Text("thirty".to_string()));

        assert!(plan.with_typed_values(values, &default_limits()).is_err());
    }

    #[test]
    fn test_with_typed_values_unknown_fact() {
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert("unknown".to_string(), LiteralValue::Number(30.into()));

        assert!(plan.with_typed_values(values, &default_limits()).is_err());
    }

    #[test]
    fn test_with_nested_typed_values() {
        use crate::semantic::PathSegment;
        let fact_path = FactPath {
            segments: vec![PathSegment {
                fact: "rules".to_string(),
                doc: "private".to_string(),
            }],
            fact: "base_price".to_string(),
        };
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: {
                let mut f = HashMap::new();
                f.insert(
                    fact_path.clone(),
                    LemmaFact {
                        reference: FactReference {
                            segments: vec!["rules".to_string()],
                            fact: "base_price".to_string(),
                        },
                        value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(
                            LemmaType::Number,
                        )),
                        source_location: None,
                    },
                );
                f
            },
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let mut values = HashMap::new();
        values.insert(
            "rules.base_price".to_string(),
            LiteralValue::Number(100.into()),
        );

        let updated_plan = plan.with_typed_values(values, &default_limits()).unwrap();
        let updated_fact = updated_plan.facts.get(&fact_path).unwrap();
        match &updated_fact.value {
            FactValue::Literal(LiteralValue::Number(n)) => assert_eq!(*n, 100.into()),
            _ => panic!("Expected number literal"),
        }
    }

    fn create_literal_expr(value: LiteralValue) -> Expression {
        use crate::parsing::ast::ExpressionId;
        use crate::semantic::ExpressionKind;
        Expression::new(ExpressionKind::Literal(value), None, ExpressionId::new(0))
    }

    #[test]
    fn test_serialize_deserialize_execution_plan() {
        let fact_path = FactPath {
            segments: vec![],
            fact: "age".to_string(),
        };
        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: {
                let mut f = HashMap::new();
                f.insert(
                    fact_path.clone(),
                    LemmaFact {
                        reference: FactReference {
                            segments: vec![],
                            fact: "age".to_string(),
                        },
                        value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(
                            LemmaType::Number,
                        )),
                        source_location: None,
                    },
                );
                f
            },
            rules: Vec::new(),
            sources: {
                let mut s = HashMap::new();
                s.insert("test.lemma".to_string(), "fact age: number".to_string());
                s
            },
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.doc_name, plan.doc_name);
        assert_eq!(deserialized.facts.len(), plan.facts.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
        assert_eq!(deserialized.sources.len(), plan.sources.len());
    }

    #[test]
    fn test_serialize_deserialize_plan_with_rules() {
        use crate::parsing::ast::ExpressionId;
        use crate::semantic::ExpressionKind;

        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let age_path = FactPath::local("age".to_string());
        plan.facts.insert(
            age_path.clone(),
            LemmaFact {
                reference: FactReference {
                    segments: vec![],
                    fact: "age".to_string(),
                },
                value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(LemmaType::Number)),
                source_location: None,
            },
        );

        let rule = ExecutableRule {
            path: RulePath::local("can_drive".to_string()),
            name: "can_drive".to_string(),
            branches: vec![Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Box::new(Expression::new(
                            ExpressionKind::FactPath(age_path.clone()),
                            None,
                            ExpressionId::new(1),
                        )),
                        crate::ComparisonComputation::GreaterThanOrEqual,
                        Box::new(create_literal_expr(LiteralValue::Number(18.into()))),
                    ),
                    None,
                    ExpressionId::new(2),
                )),
                result: create_literal_expr(LiteralValue::Boolean(crate::BooleanValue::True)),
                source: None,
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(age_path);
                set
            },
            source: None,
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.doc_name, plan.doc_name);
        assert_eq!(deserialized.facts.len(), plan.facts.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
        assert_eq!(deserialized.rules[0].name, "can_drive");
        assert_eq!(deserialized.rules[0].branches.len(), 1);
        assert_eq!(deserialized.rules[0].needs_facts.len(), 1);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_nested_fact_paths() {
        use crate::semantic::PathSegment;
        let fact_path = FactPath {
            segments: vec![PathSegment {
                fact: "employee".to_string(),
                doc: "private".to_string(),
            }],
            fact: "salary".to_string(),
        };

        let plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: {
                let mut f = HashMap::new();
                f.insert(
                    fact_path.clone(),
                    LemmaFact {
                        reference: FactReference {
                            segments: vec!["employee".to_string()],
                            fact: "salary".to_string(),
                        },
                        value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(
                            LemmaType::Number,
                        )),
                        source_location: None,
                    },
                );
                f
            },
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.facts.len(), 1);
        let (deserialized_path, _) = deserialized.facts.iter().next().unwrap();
        assert_eq!(deserialized_path.segments.len(), 1);
        assert_eq!(deserialized_path.segments[0].fact, "employee");
        assert_eq!(deserialized_path.fact, "salary");
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_fact_types() {
        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        plan.facts.insert(
            FactPath::local("name".to_string()),
            LemmaFact {
                reference: FactReference {
                    segments: vec![],
                    fact: "name".to_string(),
                },
                value: FactValue::Literal(LiteralValue::Text("Alice".to_string())),
                source_location: None,
            },
        );

        plan.facts.insert(
            FactPath::local("age".to_string()),
            LemmaFact {
                reference: FactReference {
                    segments: vec![],
                    fact: "age".to_string(),
                },
                value: FactValue::Literal(LiteralValue::Number(30.into())),
                source_location: None,
            },
        );

        plan.facts.insert(
            FactPath::local("active".to_string()),
            LemmaFact {
                reference: FactReference {
                    segments: vec![],
                    fact: "active".to_string(),
                },
                value: FactValue::Literal(LiteralValue::Boolean(crate::BooleanValue::True)),
                source_location: None,
            },
        );

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.facts.len(), 3);

        let name_fact = deserialized
            .facts
            .get(&FactPath::local("name".to_string()))
            .unwrap();
        match &name_fact.value {
            FactValue::Literal(LiteralValue::Text(s)) => assert_eq!(s, "Alice"),
            _ => panic!("Expected text literal"),
        }

        let age_fact = deserialized
            .facts
            .get(&FactPath::local("age".to_string()))
            .unwrap();
        match &age_fact.value {
            FactValue::Literal(LiteralValue::Number(n)) => assert_eq!(*n, 30.into()),
            _ => panic!("Expected number literal"),
        }

        let active_fact = deserialized
            .facts
            .get(&FactPath::local("active".to_string()))
            .unwrap();
        match &active_fact.value {
            FactValue::Literal(LiteralValue::Boolean(b)) => {
                assert_eq!(*b, crate::BooleanValue::True)
            }
            _ => panic!("Expected boolean literal"),
        }
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_branches() {
        use crate::parsing::ast::ExpressionId;
        use crate::semantic::ExpressionKind;

        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let points_path = FactPath::local("points".to_string());
        plan.facts.insert(
            points_path.clone(),
            LemmaFact {
                reference: FactReference {
                    segments: vec![],
                    fact: "points".to_string(),
                },
                value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(LemmaType::Number)),
                source_location: None,
            },
        );

        let rule = ExecutableRule {
            path: RulePath::local("tier".to_string()),
            name: "tier".to_string(),
            branches: vec![
                Branch {
                    condition: None,
                    result: create_literal_expr(LiteralValue::Text("bronze".to_string())),
                    source: None,
                },
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Box::new(Expression::new(
                                ExpressionKind::FactPath(points_path.clone()),
                                None,
                                ExpressionId::new(1),
                            )),
                            crate::ComparisonComputation::GreaterThanOrEqual,
                            Box::new(create_literal_expr(LiteralValue::Number(100.into()))),
                        ),
                        None,
                        ExpressionId::new(2),
                    )),
                    result: create_literal_expr(LiteralValue::Text("silver".to_string())),
                    source: None,
                },
                Branch {
                    condition: Some(Expression::new(
                        ExpressionKind::Comparison(
                            Box::new(Expression::new(
                                ExpressionKind::FactPath(points_path.clone()),
                                None,
                                ExpressionId::new(3),
                            )),
                            crate::ComparisonComputation::GreaterThanOrEqual,
                            Box::new(create_literal_expr(LiteralValue::Number(500.into()))),
                        ),
                        None,
                        ExpressionId::new(4),
                    )),
                    result: create_literal_expr(LiteralValue::Text("gold".to_string())),
                    source: None,
                },
            ],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(points_path);
                set
            },
            source: None,
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.rules.len(), 1);
        assert_eq!(deserialized.rules[0].branches.len(), 3);
        assert!(deserialized.rules[0].branches[0].condition.is_none());
        assert!(deserialized.rules[0].branches[1].condition.is_some());
        assert!(deserialized.rules[0].branches[2].condition.is_some());
    }

    #[test]
    fn test_serialize_deserialize_empty_plan() {
        let plan = ExecutionPlan {
            doc_name: "empty".to_string(),
            facts: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.doc_name, "empty");
        assert_eq!(deserialized.facts.len(), 0);
        assert_eq!(deserialized.rules.len(), 0);
        assert_eq!(deserialized.sources.len(), 0);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_arithmetic_expressions() {
        use crate::parsing::ast::ExpressionId;
        use crate::semantic::ExpressionKind;

        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: HashMap::new(),
            rules: Vec::new(),
            sources: HashMap::new(),
        };

        let x_path = FactPath::local("x".to_string());
        plan.facts.insert(
            x_path.clone(),
            LemmaFact {
                reference: FactReference {
                    segments: vec![],
                    fact: "x".to_string(),
                },
                value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(LemmaType::Number)),
                source_location: None,
            },
        );

        let rule = ExecutableRule {
            path: RulePath::local("doubled".to_string()),
            name: "doubled".to_string(),
            branches: vec![Branch {
                condition: None,
                result: Expression::new(
                    ExpressionKind::Arithmetic(
                        Box::new(Expression::new(
                            ExpressionKind::FactPath(x_path.clone()),
                            None,
                            ExpressionId::new(1),
                        )),
                        crate::ArithmeticComputation::Multiply,
                        Box::new(create_literal_expr(LiteralValue::Number(2.into()))),
                    ),
                    None,
                    ExpressionId::new(2),
                ),
                source: None,
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(x_path);
                set
            },
            source: None,
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        assert_eq!(deserialized.rules.len(), 1);
        match &deserialized.rules[0].branches[0].result.kind {
            ExpressionKind::Arithmetic(left, op, right) => {
                assert_eq!(*op, crate::ArithmeticComputation::Multiply);
                match &left.kind {
                    ExpressionKind::FactPath(_) => {}
                    _ => panic!("Expected FactPath in left operand"),
                }
                match &right.kind {
                    ExpressionKind::Literal(_) => {}
                    _ => panic!("Expected Literal in right operand"),
                }
            }
            _ => panic!("Expected Arithmetic expression"),
        }
    }

    #[test]
    fn test_serialize_deserialize_round_trip_equality() {
        use crate::parsing::ast::ExpressionId;
        use crate::semantic::ExpressionKind;

        let mut plan = ExecutionPlan {
            doc_name: "test".to_string(),
            facts: HashMap::new(),
            rules: Vec::new(),
            sources: {
                let mut s = HashMap::new();
                s.insert("test.lemma".to_string(), "fact age: number".to_string());
                s
            },
        };

        let age_path = FactPath::local("age".to_string());
        plan.facts.insert(
            age_path.clone(),
            LemmaFact {
                reference: FactReference {
                    segments: vec![],
                    fact: "age".to_string(),
                },
                value: FactValue::TypeAnnotation(TypeAnnotation::LemmaType(LemmaType::Number)),
                source_location: None,
            },
        );

        let rule = ExecutableRule {
            path: RulePath::local("is_adult".to_string()),
            name: "is_adult".to_string(),
            branches: vec![Branch {
                condition: Some(Expression::new(
                    ExpressionKind::Comparison(
                        Box::new(Expression::new(
                            ExpressionKind::FactPath(age_path.clone()),
                            None,
                            ExpressionId::new(1),
                        )),
                        crate::ComparisonComputation::GreaterThanOrEqual,
                        Box::new(create_literal_expr(LiteralValue::Number(18.into()))),
                    ),
                    None,
                    ExpressionId::new(2),
                )),
                result: create_literal_expr(LiteralValue::Boolean(crate::BooleanValue::True)),
                source: None,
            }],
            needs_facts: {
                let mut set = HashSet::new();
                set.insert(age_path);
                set
            },
            source: None,
        };

        plan.rules.push(rule);

        let json = serde_json::to_string(&plan).expect("Should serialize");
        let deserialized: ExecutionPlan = serde_json::from_str(&json).expect("Should deserialize");

        let json2 = serde_json::to_string(&deserialized).expect("Should serialize again");
        let deserialized2: ExecutionPlan =
            serde_json::from_str(&json2).expect("Should deserialize again");

        assert_eq!(deserialized2.doc_name, plan.doc_name);
        assert_eq!(deserialized2.facts.len(), plan.facts.len());
        assert_eq!(deserialized2.rules.len(), plan.rules.len());
        assert_eq!(deserialized2.sources.len(), plan.sources.len());
        assert_eq!(deserialized2.rules[0].name, plan.rules[0].name);
        assert_eq!(
            deserialized2.rules[0].branches.len(),
            plan.rules[0].branches.len()
        );
    }
}
