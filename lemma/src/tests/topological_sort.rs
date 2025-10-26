use crate::evaluator::topological_sort;
use crate::RulePath;
use std::collections::{HashMap, HashSet};

#[test]
fn test_topological_sort_simple() {
    let mut graph = HashMap::new();

    let rule_a = RulePath {
        rule: "A".to_string(),
        segments: vec![],
    };
    let rule_b = RulePath {
        rule: "B".to_string(),
        segments: vec![],
    };

    // A depends on B, B depends on nothing
    graph.insert(rule_a.clone(), {
        let mut deps = HashSet::new();
        deps.insert(rule_b.clone());
        deps
    });
    graph.insert(rule_b.clone(), HashSet::new());

    let order = topological_sort(&graph).unwrap();

    // B should come before A
    let b_pos = order.iter().position(|n| n == &rule_b).unwrap();
    let a_pos = order.iter().position(|n| n == &rule_a).unwrap();
    assert!(b_pos < a_pos, "B should come before A");
}

#[test]
fn test_topological_sort_chain() {
    let mut graph = HashMap::new();

    let rule_a = RulePath {
        rule: "A".to_string(),
        segments: vec![],
    };
    let rule_b = RulePath {
        rule: "B".to_string(),
        segments: vec![],
    };
    let rule_c = RulePath {
        rule: "C".to_string(),
        segments: vec![],
    };

    // C depends on B, B depends on A, A depends on nothing
    graph.insert(rule_c.clone(), {
        let mut deps = HashSet::new();
        deps.insert(rule_b.clone());
        deps
    });
    graph.insert(rule_b.clone(), {
        let mut deps = HashSet::new();
        deps.insert(rule_a.clone());
        deps
    });
    graph.insert(rule_a.clone(), HashSet::new());

    let order = topological_sort(&graph).unwrap();

    // Should be A, B, C
    let a_pos = order.iter().position(|n| n == &rule_a).unwrap();
    let b_pos = order.iter().position(|n| n == &rule_b).unwrap();
    let c_pos = order.iter().position(|n| n == &rule_c).unwrap();
    assert!(
        a_pos < b_pos && b_pos < c_pos,
        "Should be ordered A < B < C"
    );
}
