//! Plan-shape assertions for transitive normalization, against the NormalForm table.

use crate::engine::Context;
use crate::limits::ResourceLimits;
use crate::parsing::parse;
use crate::planning::execution_plan::ExecutionPlan;
use crate::planning::normalize::{LeafKind, NormalForm, NormalFormId, NormalFormKind};
use crate::planning::plan;
use crate::planning::semantics::{MathematicalComputation, ValueKind};
use rust_decimal::Decimal;
use std::sync::Arc;

fn plan_from_code(code: &str) -> ExecutionPlan {
    let specs: Vec<_> = parse(
        code,
        crate::parsing::source::SourceType::Volatile,
        &ResourceLimits::default(),
    )
    .expect("parse")
    .into_flattened_specs();

    let mut ctx = Context::new();
    let repository = ctx.workspace();
    for spec in &specs {
        ctx.insert_spec(Arc::clone(&repository), spec.clone())
            .expect("insert spec");
    }

    let result = plan(&ctx, &ResourceLimits::default());
    assert!(
        result.errors.is_empty(),
        "planning errors: {:?}",
        result.errors
    );

    let spec_name = specs.last().expect("spec").name.clone();
    let plans = result
        .plans
        .get_plans(repository.name.as_deref(), &spec_name)
        .expect("spec result");
    assert_eq!(plans.len(), 1, "expected one plan");
    plans.values().next().expect("plan").clone()
}

fn rule_root<'a>(plan: &'a ExecutionPlan, rule_name: &str) -> &'a NormalForm {
    let id = plan.get_rule(rule_name).expect("rule").normal_form;
    plan.normal_form(id)
}

#[test]
fn shipped_normal_forms_are_exactly_reachable_closure() {
    let code = r#"
spec t
data input: 5
rule step1: input + 1
rule step2: step1 * 2
rule step3: step2 - 3
"#;
    let plan = plan_from_code(code);
    let mut reachable: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let mut worklist: Vec<NormalFormId> = plan.rules.values().map(|r| r.normal_form).collect();
    while let Some(id) = worklist.pop() {
        if !reachable.insert(id.index() as u32) {
            continue;
        }
        match &plan.normal_form(id).kind {
            NormalFormKind::Leaf(_) | NormalFormKind::Veto(_) | NormalFormKind::Now => {}
            NormalFormKind::Sum(children)
            | NormalFormKind::Product(children)
            | NormalFormKind::And(children) => {
                worklist.extend(children.iter().copied());
            }
            NormalFormKind::Subtract(a, b)
            | NormalFormKind::Divide(a, b)
            | NormalFormKind::Power(a, b)
            | NormalFormKind::Modulo(a, b)
            | NormalFormKind::Comparison(a, _, b)
            | NormalFormKind::RangeLiteral(a, b)
            | NormalFormKind::RangeContainment(a, b) => {
                worklist.push(*a);
                worklist.push(*b);
            }
            NormalFormKind::Negate(x)
            | NormalFormKind::Reciprocal(x)
            | NormalFormKind::Not(x)
            | NormalFormKind::MathOp(_, x)
            | NormalFormKind::UnitConversion(x, _)
            | NormalFormKind::DateRelative(_, x)
            | NormalFormKind::DateCalendar(_, _, x)
            | NormalFormKind::PastFutureRange(_, x)
            | NormalFormKind::ResultIsVeto(x) => worklist.push(*x),
            NormalFormKind::Piecewise(arms) => {
                for (c, r) in arms {
                    worklist.push(*c);
                    worklist.push(*r);
                }
            }
            NormalFormKind::OrderedDispatch {
                scrutinee, regions, ..
            } => {
                worklist.push(*scrutinee);
                worklist.extend(regions.iter().copied());
            }
        }
        if let Some(origin) = plan.normal_form(id).origin {
            worklist.push(origin);
        }
    }
    assert_eq!(
        reachable.len(),
        plan.normal_forms.len(),
        "shipped table must equal reachable closure from rule roots"
    );
    for id in 0..plan.normal_forms.len() {
        assert!(
            reachable.contains(&(id as u32)),
            "shipped cell {id} not reachable from any rule root"
        );
    }
}

#[test]
fn unless_lookup_chain_folds_to_ordered_dispatch() {
    let code = r#"
spec lookup
data code: text
  -> option "NL"
  -> option "BE"
  -> option "DE"
rule name: veto "unknown"
  unless code is "NL" then "Netherlands"
  unless code is "BE" then "Belgium"
  unless code is "DE" then "Germany"
"#;
    let plan = plan_from_code(code);
    let root = rule_root(&plan, "name");
    let NormalFormKind::OrderedDispatch {
        boundaries,
        regions,
        ..
    } = &root.kind
    else {
        panic!(
            "lookup chain must fold to a dispatch table, got {:?}",
            root.kind
        );
    };
    assert_eq!(boundaries.len(), 3, "one breakpoint per distinct code");
    assert_eq!(regions.len(), 2 * 3 + 1);
    let origin = root.origin.expect("fold must keep its pre-image");
    assert!(
        matches!(plan.normal_form(origin).kind, NormalFormKind::Piecewise(_)),
        "pre-image must be the Piecewise, got {:?}",
        plan.normal_form(origin).kind
    );
}

#[test]
fn ordering_chain_over_a_number_folds_to_ordered_dispatch() {
    let code = r#"
spec tiers
data quantity: number
rule discount: 0
  unless quantity >= 10 then 5
  unless quantity >= 100 then 10
"#;
    let plan = plan_from_code(code);
    let root = rule_root(&plan, "discount");
    let NormalFormKind::OrderedDispatch { boundaries, .. } = &root.kind else {
        panic!(
            "ordering chain must fold to a dispatch table, got {:?}",
            root.kind
        );
    };
    assert_eq!(boundaries.len(), 2);
}

#[test]
fn unless_chain_embeds_dependency_as_piecewise() {
    let code = r#"
spec t
data flag: boolean
data base: 10
rule doubled: base * 2
rule scaled: doubled
  unless flag then doubled * 3
"#;
    let plan = plan_from_code(code);
    let scaled = rule_root(&plan, "scaled");
    assert!(
        matches!(scaled.kind, NormalFormKind::Piecewise(_)),
        "unless rule must normalize to a Piecewise, got {:?}",
        scaled.kind
    );
}

#[test]
fn doubling_chain_graph_stays_linear() {
    let mut code = String::from("spec doubling\ndata input: number\nrule r0: input > 0\n");
    let n = 40;
    for index in 1..=n {
        code.push_str(&format!(
            "rule r{index}: r{previous} and not r{previous}\n",
            previous = index - 1
        ));
    }
    let plan = plan_from_code(&code);
    let cells = plan.normal_forms.len();
    // Shared DAG: a few cells per level (~4/n historically), not 2^n.
    assert!(
        cells <= 5 * n,
        "expected linear shared graph (<=5n), got {cells} for n={n}"
    );
}

#[test]
fn sqrt_product_folds_to_literal_two_in_plan() {
    let code = r#"
spec test
rule sqrt_two: sqrt 2
rule sqrt_product: sqrt_two * sqrt_two
"#;
    let plan = plan_from_code(code);
    let body = rule_root(&plan, "sqrt_product");
    match &body.kind {
        NormalFormKind::Leaf(LeafKind::Literal(literal)) => match &literal.value {
            ValueKind::Number(rational) => {
                let decimal = ValueKind::Number(rational.clone())
                    .as_decimal_magnitude()
                    .expect("numeric literal");
                assert_eq!(decimal, Decimal::from(2));
            }
            other => panic!("expected numeric literal, got {other:?}"),
        },
        other => panic!("sqrt_product must fold to a literal leaf, got {other:?}"),
    }
}

#[test]
fn log_exp_chain_folds_to_literal_one_in_plan() {
    let code = r#"
spec test
rule exp_one: exp 1
rule log_one: log exp_one
"#;
    let plan = plan_from_code(code);
    let body = rule_root(&plan, "log_one");
    match &body.kind {
        NormalFormKind::Leaf(LeafKind::Literal(literal)) => match &literal.value {
            ValueKind::Number(rational) => {
                let decimal = ValueKind::Number(rational.clone())
                    .as_decimal_magnitude()
                    .expect("numeric literal");
                assert_eq!(decimal, Decimal::ONE);
            }
            other => panic!("expected numeric literal, got {other:?}"),
        },
        other => panic!("log_one must fold to a literal leaf, got {other:?}"),
    }
}

#[test]
fn bare_use_site_exposes_exp_kind_with_rule_embed() {
    let code = r#"
spec test
rule exp_one: exp 1
rule just: exp_one
"#;
    let plan = plan_from_code(code);
    let just = rule_root(&plan, "just");
    assert!(
        matches!(
            just.kind,
            NormalFormKind::MathOp(MathematicalComputation::Exp, _)
        ),
        "bare use-site must share body Exp Kind, got {:?}",
        just.kind
    );
    let embed = just
        .rule_embed
        .as_ref()
        .expect("bare use-site must keep rule_embed");
    assert_eq!(embed.rule, "exp_one");
}

#[test]
fn identity_elim_keeps_rule_embed_on_survivor() {
    let code = r#"
spec test
rule exp_one: exp 1
rule r: exp_one + 0
"#;
    let plan = plan_from_code(code);
    let r = rule_root(&plan, "r");
    assert!(
        matches!(
            r.kind,
            NormalFormKind::MathOp(MathematicalComputation::Exp, _)
        ),
        "identity-elim survivor must keep Exp Kind, got {:?}",
        r.kind
    );
    let embed = r
        .rule_embed
        .as_ref()
        .expect("identity-elim must copy rule_embed onto survivor");
    assert_eq!(embed.rule, "exp_one");
}
