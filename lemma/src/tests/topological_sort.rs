use crate::evaluator::topological_sort;
use std::collections::{HashMap, HashSet};

#[test]
fn test_topological_sort_simple() {
    let mut graph = HashMap::new();

    // A depends on B, B depends on nothing
    graph.insert("A".to_string(), {
        let mut deps = HashSet::new();
        deps.insert("B".to_string());
        deps
    });
    graph.insert("B".to_string(), HashSet::new());

    let order = topological_sort(&graph).unwrap();

    // B should come before A
    let b_pos = order.iter().position(|n| n == "B").unwrap();
    let a_pos = order.iter().position(|n| n == "A").unwrap();
    assert!(b_pos < a_pos, "B should come before A");
}

#[test]
fn test_topological_sort_chain() {
    let mut graph = HashMap::new();

    // C depends on B, B depends on A, A depends on nothing
    graph.insert("C".to_string(), {
        let mut deps = HashSet::new();
        deps.insert("B".to_string());
        deps
    });
    graph.insert("B".to_string(), {
        let mut deps = HashSet::new();
        deps.insert("A".to_string());
        deps
    });
    graph.insert("A".to_string(), HashSet::new());

    let order = topological_sort(&graph).unwrap();

    // Should be A, B, C
    let a_pos = order.iter().position(|n| n == "A").unwrap();
    let b_pos = order.iter().position(|n| n == "B").unwrap();
    let c_pos = order.iter().position(|n| n == "C").unwrap();
    assert!(
        a_pos < b_pos && b_pos < c_pos,
        "Should be ordered A < B < C"
    );
}
