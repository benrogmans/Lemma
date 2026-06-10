//! Transitive rule inlining before normalization (plan-shape tests).

#[cfg(test)]
mod tests {
    use crate::engine::Context;
    use crate::limits::ResourceLimits;
    use crate::parse;
    use crate::planning::execution_plan::{ExecutionPlan, Instruction};
    use crate::planning::normalize::follow_data_reference_to_rule_target;
    use crate::planning::plan;
    use crate::planning::semantics::ValueKind;
    use crate::Error;
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
            ctx.insert_spec(Arc::clone(&repository), Arc::new(spec.clone()))
                .expect("insert spec");
        }

        let result = plan(&ctx, &ResourceLimits::default());
        let errors: Vec<Error> = result
            .results
            .iter()
            .flat_map(|r| r.errors().cloned())
            .collect();
        assert!(errors.is_empty(), "planning errors: {errors:?}");

        let spec_name = specs.last().expect("spec").name.clone();
        let set = result
            .results
            .into_iter()
            .find(|r| r.name == spec_name)
            .expect("spec result");
        let mut plans = set.execution_plan_set().plans;
        assert_eq!(plans.len(), 1, "expected one plan");
        plans.remove(0)
    }

    fn rule_instructions<'a>(
        plan: &'a ExecutionPlan,
        rule_name: &str,
    ) -> &'a crate::planning::execution_plan::Instructions {
        &plan.get_rule(rule_name).expect("rule").instructions
    }

    fn assert_no_rule_target_data_in_manifest(plan: &ExecutionPlan) {
        for rule in &plan.rules {
            for path in &rule.instructions.data_manifest {
                assert!(
                    follow_data_reference_to_rule_target(&plan.data, path).is_none(),
                    "rule '{}' instructions still load rule-target DataPath '{}'",
                    rule.name,
                    path
                );
            }
        }
    }

    #[test]
    fn cross_rule_chain_has_no_in_plan_rule_paths() {
        let code = r#"
spec t
data input: 5
rule step1: input + 1
rule step2: step1 * 2
rule step3: step2 - 3
"#;
        let plan = plan_from_code(code);
        assert!(plan.rules.iter().all(|r| !r.instructions.code.is_empty()));
    }

    #[test]
    fn unless_chain_inlines_dependency_bodies() {
        let code = r#"
spec t
data flag: boolean
data base: 10
rule doubled: base * 2
rule scaled: doubled
  unless flag then doubled * 3
"#;
        let plan = plan_from_code(code);
        let scaled = rule_instructions(&plan, "scaled");
        assert!(
            scaled
                .code
                .iter()
                .any(|insn| matches!(insn, Instruction::JumpIfFalse { .. })),
            "unless rule must compile to piecewise jump chain, got {:?}",
            scaled.code
        );
    }

    /// Pinning test: rewrite passes preserve the piecewise arm list. Both
    /// instruction streams must carry a Condition tag per unless arm and a
    /// Result tag per arm — even when a condition is constant-foldable —
    /// because explanations attribute recorded branch decisions by arm tag.
    #[test]
    fn rewrite_passes_preserve_piecewise_arm_tags() {
        use crate::planning::execution_plan::ArmRole;
        let code = r#"
spec t
data flag: boolean
data other: boolean
rule out: 1
  unless flag then 2
  unless other and true then 3
"#;
        let plan = plan_from_code(code);
        let rule = plan.get_rule("out").expect("rule");
        for (label, instructions) in [
            ("optimized", &rule.instructions),
            ("source", &rule.source_instructions),
        ] {
            let mut condition_arms: Vec<u16> = instructions
                .arm_tags
                .iter()
                .filter(|t| t.role == ArmRole::Condition)
                .map(|t| t.arm)
                .collect();
            condition_arms.sort_unstable();
            assert_eq!(
                condition_arms,
                vec![1, 2],
                "{label} stream must tag every unless condition"
            );
            let mut result_arms: Vec<u16> = instructions
                .arm_tags
                .iter()
                .filter(|t| t.role == ArmRole::Result)
                .map(|t| t.arm)
                .collect();
            result_arms.sort_unstable();
            assert_eq!(
                result_arms,
                vec![0, 1, 2],
                "{label} stream must tag every arm result"
            );
        }
    }

    #[test]
    fn sqrt_product_folds_to_literal_two_in_plan() {
        let code = r#"
spec test
rule sqrt_two: sqrt 2
rule sqrt_product: sqrt_two * sqrt_two
"#;
        let plan = plan_from_code(code);
        let instructions = rule_instructions(&plan, "sqrt_product");
        assert_eq!(
            instructions.code.len(),
            2,
            "sqrt_product must fold to LoadConstant + Return, got {:?}",
            instructions.code
        );
        assert!(matches!(
            instructions.code[0],
            Instruction::LoadConstant { .. }
        ));
        assert!(matches!(instructions.code[1], Instruction::Return { .. }));
        let constant = &instructions.constants[0];
        match &constant.value {
            ValueKind::Number(rational) => {
                let decimal = ValueKind::Number(rational.clone())
                    .as_decimal_magnitude()
                    .expect("numeric literal");
                assert_eq!(decimal, Decimal::from(2));
            }
            other => panic!("expected numeric literal, got {other:?}"),
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
        let instructions = rule_instructions(&plan, "log_one");
        assert_eq!(
            instructions.code.len(),
            2,
            "log_one must fold to LoadConstant + Return, got {:?}",
            instructions.code
        );
        let constant = &instructions.constants[0];
        match &constant.value {
            ValueKind::Number(rational) => {
                let decimal = ValueKind::Number(rational.clone())
                    .as_decimal_magnitude()
                    .expect("numeric literal");
                assert_eq!(decimal, Decimal::ONE);
            }
            other => panic!("expected numeric literal, got {other:?}"),
        }
    }

    #[test]
    fn in_plan_rule_target_data_ref_is_fully_inlined() {
        let code = r#"
spec inner
data slot: number

spec t
uses i: inner
with i.slot: doubled
rule doubled: 20
rule consumer: i.slot
"#;
        let plan = plan_from_code(code);
        assert_no_rule_target_data_in_manifest(&plan);
        let consumer = rule_instructions(&plan, "consumer");
        assert!(
            consumer.data_manifest.is_empty(),
            "consumer should inline rule-target i.slot, leaving empty data_manifest"
        );
    }
}
