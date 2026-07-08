//! Pure Rust evaluation engine for Lemma
//!
//! Executes pre-validated execution plans. Authoritative results come from each
//! rule's compiled instructions. Structural explanations are built separately in plan order.

pub(crate) mod branch_semantics;
pub mod explanations;
pub mod expression;
pub mod operations;
pub(crate) mod partial;
pub mod response;

use crate::computation::{
    arithmetic_operation, comparison_operation, convert_unit, UnitResolutionContext,
};
use crate::evaluation::operations::VetoType;
use crate::evaluation::response::EvaluatedRule;
use crate::planning::execution_plan::{
    validate_value_against_type, DataOverlay, ExecutionPlan, Instruction, Instructions,
    INSTRUCTIONS_VERSION,
};
use crate::planning::semantics::{
    Data, DataDefinition, DataPath, DataValue, LiteralValue, ReferenceTarget, RulePath, ValueKind,
};
use indexmap::IndexMap;
pub use operations::OperationResult;
pub use response::{DataGroup, Response, RuleResult};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) const DECIMAL_VALUE_LIMIT_VETO_MESSAGE: &str =
    "Calculated result exceeds decimal value limit";

fn literal_value_committable_to_decimal(value: &LiteralValue) -> bool {
    use crate::computation::rational::commit_rational_to_decimal;

    match &value.value {
        ValueKind::Number(rational)
        | ValueKind::Ratio(rational, _)
        | ValueKind::Measure(rational, _) => commit_rational_to_decimal(rational).is_ok(),
        ValueKind::Range(left, right) => {
            literal_value_committable_to_decimal(left)
                && literal_value_committable_to_decimal(right)
        }
        ValueKind::Text(_) | ValueKind::Date(_) | ValueKind::Time(_) | ValueKind::Boolean(_) => {
            true
        }
    }
}

fn ensure_rule_result_within_decimal_limit(result: OperationResult) -> OperationResult {
    match result {
        OperationResult::Value(value) => {
            if literal_value_committable_to_decimal(&value) {
                OperationResult::Value(value)
            } else {
                OperationResult::Veto(VetoType::computation(DECIMAL_VALUE_LIMIT_VETO_MESSAGE))
            }
        }
        OperationResult::Veto(veto) => OperationResult::Veto(veto),
    }
}

/// Evaluation context for storing intermediate results during one plan run.
///
/// Borrows the [`ExecutionPlan`] for expression-scope units ([`ExecutionPlan::expression_unit_index`])
/// and signature disambiguation. Rule-result materialization uses each rule's [`ExecutableRule::rule_type`].
pub(crate) struct EvaluationContext<'plan> {
    plan: &'plan ExecutionPlan,
    data_values: HashMap<DataPath, LiteralValue>,
    pub(crate) rule_results: HashMap<RulePath, OperationResult>,
    /// Recorded executions of each rule's source-shaped instruction stream;
    /// populated only when explanations are requested. Explanations read all
    /// runtime facts (register values, branch decisions, winning arm) from
    /// here — never by re-evaluating expressions.
    pub(crate) recordings: HashMap<RulePath, RuleRecording>,
    now: LiteralValue,
    /// Computation vetoes on data paths that cannot be read.
    vetoes: HashMap<DataPath, VetoType>,
    /// Register file for compiled instruction execution; sized from plan
    /// max_register_count. `None` marks a register that has not been written
    /// yet; reading one is a compiler bug and crashes.
    register_values: Vec<Option<OperationResult>>,
}

/// What one `JumpIfFalse` decided during a recorded execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BranchDecision {
    /// Condition was true: the arm's result branch was taken.
    Taken,
    /// Condition was false: execution jumped past the arm.
    NotTaken,
    /// Condition vetoed: the veto propagated as the rule result.
    Veto,
}

/// Facts retained from one execution of a rule's instruction stream.
///
/// Registers are allocated monotonically and never reused, so the final
/// register file holds the value of every executed instruction; untaken
/// branches remain `None`.
#[derive(Debug, Default, Clone)]
pub(crate) struct RuleRecording {
    pub(crate) registers: Vec<Option<OperationResult>>,
    /// `(pc, decision)` per executed `JumpIfFalse`, in execution order.
    pub(crate) branch_decisions: Vec<(u32, BranchDecision)>,
    /// The pc of the `Return` that produced the result, when one executed
    /// (a vetoing condition propagates without reaching a `Return`).
    pub(crate) returned_pc: Option<u32>,
}

impl<'plan> EvaluationContext<'plan> {
    fn new(plan: &'plan ExecutionPlan, overlay: &DataOverlay, now: LiteralValue) -> Self {
        let mut data_values: HashMap<DataPath, LiteralValue> = plan
            .data
            .iter()
            .filter_map(|(path, definition)| {
                if overlay.violated.contains_key(path) {
                    return None;
                }
                definition
                    .value()
                    .map(|value| (path.clone(), value.clone()))
            })
            .collect();
        for (path, value) in &overlay.values {
            data_values.insert(path.clone(), value.clone());
        }

        for (path, definition) in &plan.data {
            if data_values.contains_key(path) || overlay.violated.contains_key(path) {
                continue;
            }
            if let Some(default) = definition.default_suggestion() {
                data_values.insert(path.clone(), default);
            }
        }

        let mut vetoes: HashMap<DataPath, VetoType> = overlay
            .violated
            .iter()
            .map(|(path, reason)| (path.clone(), VetoType::computation(reason.clone())))
            .collect();

        for reference_path in &plan.reference_evaluation_order {
            if data_values.contains_key(reference_path) {
                continue;
            }
            match plan.data.get(reference_path) {
                Some(DataDefinition::Reference {
                    target: ReferenceTarget::Data(target_path),
                    resolved_type,
                    local_default,
                    ..
                }) => {
                    let copied_kind: Option<ValueKind> =
                        data_values.get(target_path).map(|v| v.value.clone());
                    if let Some(value_kind) = copied_kind {
                        let value = LiteralValue {
                            value: value_kind,
                            lemma_type: Arc::clone(resolved_type),
                        };
                        match validate_value_against_type(resolved_type.as_ref(), &value) {
                            Ok(()) => {
                                data_values.insert(reference_path.clone(), value);
                            }
                            Err(msg) => {
                                vetoes.insert(
                                    reference_path.clone(),
                                    VetoType::computation(format!(
                                        "Reference '{}' violates declared constraint: {}",
                                        reference_path, msg
                                    )),
                                );
                            }
                        }
                    } else if let Some(dv) = local_default {
                        let value = LiteralValue {
                            value: dv.clone(),
                            lemma_type: Arc::clone(resolved_type),
                        };
                        data_values.insert(reference_path.clone(), value);
                    }
                }
                Some(DataDefinition::Reference {
                    target: ReferenceTarget::Rule(_),
                    ..
                }) => {}
                Some(_) => {}
                None => unreachable!(
                    "BUG: reference_evaluation_order references missing data path '{}'",
                    reference_path
                ),
            }
        }

        Self {
            plan,
            data_values,
            rule_results: HashMap::new(),
            recordings: HashMap::new(),
            now,
            vetoes,
            register_values: Vec::with_capacity(plan.max_register_count as usize),
        }
    }

    pub(crate) fn get_veto(&self, data_path: &DataPath) -> Option<&VetoType> {
        self.vetoes.get(data_path)
    }

    pub(crate) fn now(&self) -> &LiteralValue {
        &self.now
    }

    pub(crate) fn get_data_value(&self, data_path: &DataPath) -> Option<&LiteralValue> {
        self.data_values.get(data_path)
    }

    pub(crate) fn plan(&self) -> &ExecutionPlan {
        self.plan
    }

    fn read_register(&self, register: u16) -> OperationResult {
        match self.register_values.get(register as usize) {
            Some(Some(value)) => value.clone(),
            Some(None) => panic!("BUG: read of unset register r{register}"),
            None => panic!("BUG: read of out-of-bounds register r{register}"),
        }
    }

    fn write_register(&mut self, register: u16, value: OperationResult) {
        let index = register as usize;
        if index >= self.register_values.len() {
            assert!(
                index < self.plan.max_register_count as usize,
                "BUG: register {register} exceeds plan max_register_count"
            );
            self.register_values.resize(index + 1, None);
        }
        self.register_values[index] = Some(value);
    }
}

fn operand_value(result: OperationResult) -> OperationResult {
    if result.vetoed() {
        return result;
    }
    result
}

fn unwrap_literal(result: OperationResult, operand: &str) -> LiteralValue {
    result.value().cloned().unwrap_or_else(|| {
        panic!("BUG: {operand} passed veto check but has no value");
    })
}

pub(crate) fn execute_instructions(
    instructions: &Instructions,
    context: &mut EvaluationContext<'_>,
    mut recording: Option<&mut RuleRecording>,
) -> OperationResult {
    if instructions.version != INSTRUCTIONS_VERSION {
        panic!("BUG: instructions version mismatch");
    }
    if instructions.register_count > context.plan.max_register_count {
        panic!("BUG: register_count exceeds plan max_register_count");
    }
    context.register_values.clear();
    context
        .register_values
        .resize(instructions.register_count as usize, None);

    let unit_index = context.plan().resolved_types.unit_index.clone();
    let signature_index = context.plan().signature_index.clone();
    let unit_ctx = UnitResolutionContext::WithIndex(&unit_index);

    // Compiled instruction streams are loop-free: every jump target is
    // forward, so a legitimate execution runs each instruction at most once.
    // The budget is a generous multiple to keep the invariant check far away
    // from legitimate executions while still catching cyclic jumps.
    let step_budget = instructions.code.len().saturating_mul(4).saturating_add(16);
    let mut steps: usize = 0;

    let mut pc: u32 = 0;
    while (pc as usize) < instructions.code.len() {
        steps += 1;
        if steps > step_budget {
            panic!(
                "BUG: instruction step budget exceeded at pc={pc}; compiled instructions must be loop-free"
            );
        }
        let insn = &instructions.code[pc as usize];
        let current_pc = pc;
        pc += 1;
        match insn {
            Instruction::LoadConstant {
                destination_register,
                constant_index,
            } => {
                let constant = instructions
                    .constants
                    .get(*constant_index as usize)
                    .expect("BUG: invalid constant_index");
                context.write_register(
                    *destination_register,
                    OperationResult::Value(constant.clone()),
                );
            }
            Instruction::LoadData {
                destination_register,
                data_index,
            } => {
                let data_path = instructions
                    .data_manifest
                    .get(*data_index as usize)
                    .expect("BUG: invalid data_index");
                let result = expression::resolve_data_path_value(data_path, context);
                context.write_register(*destination_register, result);
            }
            Instruction::LoadNow {
                destination_register,
            } => {
                context.write_register(
                    *destination_register,
                    OperationResult::Value(context.now().clone()),
                );
            }
            Instruction::Arithmetic {
                destination_register,
                operation,
                left_register,
                right_register,
            } => {
                let left = operand_value(context.read_register(*left_register));
                if left.vetoed() {
                    context.write_register(*destination_register, left);
                    continue;
                }
                let right = operand_value(context.read_register(*right_register));
                if right.vetoed() {
                    context.write_register(*destination_register, right);
                    continue;
                }
                let left_val = unwrap_literal(left, "left operand");
                let right_val = unwrap_literal(right, "right operand");
                let result = arithmetic_operation(
                    &left_val,
                    operation,
                    &right_val,
                    &unit_index,
                    &signature_index,
                );
                context.write_register(*destination_register, result);
            }
            Instruction::Comparison {
                destination_register,
                operation,
                left_register,
                right_register,
            } => {
                let left = operand_value(context.read_register(*left_register));
                if left.vetoed() {
                    context.write_register(*destination_register, left);
                    continue;
                }
                let right = operand_value(context.read_register(*right_register));
                if right.vetoed() {
                    context.write_register(*destination_register, right);
                    continue;
                }
                let left_val = unwrap_literal(left, "left operand");
                let right_val = unwrap_literal(right, "right operand");
                let result = comparison_operation(&left_val, operation, &right_val, unit_ctx);
                context.write_register(*destination_register, result);
            }
            Instruction::UnitConversion {
                destination_register,
                source_register,
                target,
            } => {
                let source = operand_value(context.read_register(*source_register));
                if source.vetoed() {
                    context.write_register(*destination_register, source);
                    continue;
                }
                let source_val = unwrap_literal(source, "operand");
                let result = convert_unit(&source_val, target);
                context.write_register(*destination_register, result);
            }
            Instruction::Mathematical {
                destination_register,
                operation,
                source_register,
            } => {
                let source = operand_value(context.read_register(*source_register));
                if source.vetoed() {
                    context.write_register(*destination_register, source);
                    continue;
                }
                let source_val = unwrap_literal(source, "operand");
                let result = expression::evaluate_mathematical_operator(operation, &source_val);
                context.write_register(*destination_register, result);
            }
            Instruction::DateRelative {
                destination_register,
                kind,
                source_register,
            } => {
                let source = operand_value(context.read_register(*source_register));
                if source.vetoed() {
                    context.write_register(*destination_register, source);
                    continue;
                }
                let date_val = unwrap_literal(source, "date operand");
                let date_semantic = match &date_val.value {
                    ValueKind::Date(dt) => dt,
                    other => panic!("BUG: date-relative operand expected date, got {other:?}"),
                };
                let now_semantic = match &context.now().value {
                    ValueKind::Date(dt) => dt,
                    other => panic!("BUG: context.now() must be a date, got {other:?}"),
                };
                let result = crate::computation::datetime::compute_date_relative(
                    kind,
                    date_semantic,
                    now_semantic,
                );
                context.write_register(*destination_register, result);
            }
            Instruction::DateCalendar {
                destination_register,
                kind,
                unit,
                source_register,
            } => {
                let source = operand_value(context.read_register(*source_register));
                if source.vetoed() {
                    context.write_register(*destination_register, source);
                    continue;
                }
                let date_val = unwrap_literal(source, "date operand");
                let date_semantic = match &date_val.value {
                    ValueKind::Date(dt) => dt,
                    other => panic!("BUG: date-calendar operand expected date, got {other:?}"),
                };
                let now_semantic = match &context.now().value {
                    ValueKind::Date(dt) => dt,
                    other => panic!("BUG: context.now() must be a date, got {other:?}"),
                };
                let result = crate::computation::datetime::compute_date_calendar(
                    kind,
                    unit,
                    date_semantic,
                    now_semantic,
                );
                context.write_register(*destination_register, result);
            }
            Instruction::RangeLiteral {
                destination_register,
                left_register,
                right_register,
            } => {
                let left = operand_value(context.read_register(*left_register));
                if left.vetoed() {
                    context.write_register(*destination_register, left);
                    continue;
                }
                let right = operand_value(context.read_register(*right_register));
                if right.vetoed() {
                    context.write_register(*destination_register, right);
                    continue;
                }
                let range_value = LiteralValue::range(
                    unwrap_literal(left, "left endpoint"),
                    unwrap_literal(right, "right endpoint"),
                );
                context.write_register(*destination_register, OperationResult::Value(range_value));
            }
            Instruction::PastFutureRange {
                destination_register,
                kind,
                source_register,
            } => {
                let source = operand_value(context.read_register(*source_register));
                if source.vetoed() {
                    context.write_register(*destination_register, source);
                    continue;
                }
                let now_semantic = match &context.now().value {
                    ValueKind::Date(dt) => dt,
                    other => panic!("BUG: context.now() must be a date, got {other:?}"),
                };
                let offset_val = unwrap_literal(source, "offset operand");
                let result = crate::computation::datetime::evaluate_past_future_range(
                    kind,
                    &offset_val,
                    now_semantic,
                );
                context.write_register(*destination_register, result);
            }
            Instruction::RangeContainment {
                destination_register,
                value_register,
                range_register,
            } => {
                let value = operand_value(context.read_register(*value_register));
                if value.vetoed() {
                    context.write_register(*destination_register, value);
                    continue;
                }
                let range = operand_value(context.read_register(*range_register));
                if range.vetoed() {
                    context.write_register(*destination_register, range);
                    continue;
                }
                let value_literal = unwrap_literal(value, "value operand");
                let range_literal = unwrap_literal(range, "range operand");
                let result = match &range_literal.value {
                    ValueKind::Range(range_left, range_right) => {
                        crate::computation::range::check_containment(
                            &value_literal,
                            range_left.as_ref(),
                            range_right.as_ref(),
                        )
                    }
                    other => panic!("BUG: range containment expected range operand, got {other:?}"),
                };
                context.write_register(*destination_register, result);
            }
            Instruction::ResultIsVeto {
                destination_register,
                source_register,
            } => {
                let source = context.read_register(*source_register);
                context.write_register(
                    *destination_register,
                    OperationResult::Value(LiteralValue::from_bool(source.vetoed())),
                );
            }
            Instruction::MoveRegister {
                destination_register,
                source_register,
            } => {
                let value = context.read_register(*source_register);
                context.write_register(*destination_register, value);
            }
            Instruction::UserVeto {
                destination_register,
                message_index,
            } => {
                let message = instructions
                    .veto_messages
                    .get(*message_index as usize)
                    .expect("BUG: invalid message_index")
                    .clone();
                context.write_register(
                    *destination_register,
                    OperationResult::Veto(VetoType::UserDefined {
                        message: Some(message).filter(|m| !m.is_empty()),
                    }),
                );
            }
            Instruction::JumpIfFalse {
                condition_register,
                target_instruction,
                veto_semantics,
            } => {
                let condition = context.read_register(*condition_register);
                match branch_semantics::unless_condition_outcome(&condition, *veto_semantics) {
                    branch_semantics::BranchOutcome::Propagate(result) => {
                        if let Some(rec) = recording.as_deref_mut() {
                            rec.branch_decisions
                                .push((current_pc, BranchDecision::Veto));
                            rec.registers = context.register_values.clone();
                        }
                        return result;
                    }
                    branch_semantics::BranchOutcome::Taken => {
                        if let Some(rec) = recording.as_deref_mut() {
                            rec.branch_decisions
                                .push((current_pc, BranchDecision::Taken));
                        }
                    }
                    branch_semantics::BranchOutcome::NotTaken => {
                        if let Some(rec) = recording.as_deref_mut() {
                            rec.branch_decisions
                                .push((current_pc, BranchDecision::NotTaken));
                        }
                        pc = *target_instruction;
                    }
                }
            }
            Instruction::Jump { target_instruction } => {
                pc = *target_instruction;
            }
            Instruction::Return { source_register } => {
                if let Some(rec) = recording.as_deref_mut() {
                    rec.returned_pc = Some(current_pc);
                    rec.registers = context.register_values.clone();
                }
                return context.read_register(*source_register);
            }
        }
    }
    panic!("BUG: instruction stream ended without Return");
}

#[cfg(test)]
mod vm_tests {
    use super::*;
    use crate::parsing::ast::{DateTimeValue, EffectiveDate};
    use crate::planning::execution_plan::{Instructions, INSTRUCTIONS_VERSION};
    use crate::planning::graph::ResolvedSpecTypes;
    use crate::planning::semantics::primitive_boolean_arc;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn empty_plan(max_register_count: u16) -> ExecutionPlan {
        ExecutionPlan {
            spec_name: "vm_test".to_string(),
            commentary: None,
            data: IndexMap::new(),
            rules: Vec::new(),
            max_register_count,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        }
    }

    fn run_instructions(instructions: &Instructions) -> OperationResult {
        let plan = empty_plan(instructions.register_count);
        let overlay = DataOverlay::default();
        let now = LiteralValue {
            value: ValueKind::Date(crate::planning::semantics::date_time_to_semantic(
                &DateTimeValue::now(),
            )),
            lemma_type: crate::planning::semantics::primitive_date_arc().clone(),
        };
        let mut context = EvaluationContext::new(&plan, &overlay, now);
        execute_instructions(instructions, &mut context, None)
    }

    fn bool_instructions(code: Vec<Instruction>, constants: Vec<LiteralValue>) -> Instructions {
        Instructions {
            version: INSTRUCTIONS_VERSION,
            register_count: 2,
            register_types: vec![
                Arc::clone(primitive_boolean_arc()),
                Arc::clone(primitive_boolean_arc()),
            ],
            constants,
            data_manifest: Vec::new(),
            veto_messages: Vec::new(),
            arm_tags: Vec::new(),
            conversion_tags: Vec::new(),
            code,
        }
    }

    #[test]
    fn short_circuit_and_first_false_skips_second_conjunct() {
        let false_lit = LiteralValue::from_bool(false);
        let true_lit = LiteralValue::from_bool(true);
        let instructions = bool_instructions(
            vec![
                Instruction::LoadConstant {
                    destination_register: 0,
                    constant_index: 0,
                },
                Instruction::JumpIfFalse {
                    condition_register: 0,
                    target_instruction: 4,
                    veto_semantics: Default::default(),
                },
                Instruction::LoadConstant {
                    destination_register: 1,
                    constant_index: 1,
                },
                Instruction::Jump {
                    target_instruction: 5,
                },
                Instruction::LoadConstant {
                    destination_register: 1,
                    constant_index: 0,
                },
                Instruction::Return { source_register: 1 },
            ],
            vec![false_lit, true_lit],
        );
        let result = run_instructions(&instructions);
        assert_eq!(result.value().unwrap().value, ValueKind::Boolean(false));
    }

    #[test]
    fn short_circuit_and_both_true_returns_true() {
        let true_lit = LiteralValue::from_bool(true);
        let false_lit = LiteralValue::from_bool(false);
        let instructions = bool_instructions(
            vec![
                Instruction::LoadConstant {
                    destination_register: 0,
                    constant_index: 0,
                },
                Instruction::JumpIfFalse {
                    condition_register: 0,
                    target_instruction: 4,
                    veto_semantics: Default::default(),
                },
                Instruction::LoadConstant {
                    destination_register: 1,
                    constant_index: 0,
                },
                Instruction::Jump {
                    target_instruction: 5,
                },
                Instruction::LoadConstant {
                    destination_register: 1,
                    constant_index: 1,
                },
                Instruction::Return { source_register: 1 },
            ],
            vec![true_lit, false_lit],
        );
        let result = run_instructions(&instructions);
        assert_eq!(result.value().unwrap().value, ValueKind::Boolean(true));
    }

    #[test]
    fn short_circuit_or_first_true_skips_second_disjunct() {
        let false_lit = LiteralValue::from_bool(false);
        let true_lit = LiteralValue::from_bool(true);
        let instructions = bool_instructions(
            vec![
                Instruction::LoadConstant {
                    destination_register: 0,
                    constant_index: 0,
                },
                Instruction::JumpIfFalse {
                    condition_register: 0,
                    target_instruction: 3,
                    veto_semantics: Default::default(),
                },
                Instruction::LoadConstant {
                    destination_register: 1,
                    constant_index: 0,
                },
                Instruction::Jump {
                    target_instruction: 5,
                },
                Instruction::LoadConstant {
                    destination_register: 1,
                    constant_index: 1,
                },
                Instruction::Return { source_register: 1 },
            ],
            vec![true_lit, false_lit],
        );
        let result = run_instructions(&instructions);
        assert_eq!(result.value().unwrap().value, ValueKind::Boolean(true));
    }

    #[test]
    fn short_circuit_or_both_false_returns_false() {
        let false_lit = LiteralValue::from_bool(false);
        let true_lit = LiteralValue::from_bool(true);
        let instructions = bool_instructions(
            vec![
                Instruction::LoadConstant {
                    destination_register: 0,
                    constant_index: 0,
                },
                Instruction::JumpIfFalse {
                    condition_register: 0,
                    target_instruction: 4,
                    veto_semantics: Default::default(),
                },
                Instruction::LoadConstant {
                    destination_register: 1,
                    constant_index: 1,
                },
                Instruction::Jump {
                    target_instruction: 5,
                },
                Instruction::LoadConstant {
                    destination_register: 1,
                    constant_index: 0,
                },
                Instruction::Return { source_register: 1 },
            ],
            vec![false_lit, true_lit],
        );
        let result = run_instructions(&instructions);
        assert_eq!(result.value().unwrap().value, ValueKind::Boolean(false));
    }

    #[test]
    #[should_panic(expected = "BUG: instruction step budget exceeded")]
    fn unpatched_jump_to_zero_hits_step_budget() {
        let false_lit = LiteralValue::from_bool(false);
        let instructions = bool_instructions(
            vec![
                Instruction::LoadConstant {
                    destination_register: 0,
                    constant_index: 0,
                },
                Instruction::JumpIfFalse {
                    condition_register: 0,
                    target_instruction: 0,
                    veto_semantics: Default::default(),
                },
            ],
            vec![false_lit],
        );
        run_instructions(&instructions);
    }
}

#[cfg(test)]
mod runtime_invariant_tests {
    use super::*;
    use crate::parsing::ast::DateTimeValue;
    use crate::Engine;

    #[test]
    fn reference_runtime_value_carries_resolved_type_not_target_type() {
        let code = r#"
spec inner
data slot: number -> minimum 0 -> maximum 100

spec source_spec
data v: number -> default 5

spec outer
uses i: inner
uses src: source_spec
with i.slot: src.v
rule r: i.slot
"#;
        let mut engine = Engine::new();
        engine
            .load(
                code,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "ref_invariant.lemma",
                ))),
            )
            .expect("must load");

        let now = DateTimeValue::now();
        let plan_basis = engine
            .get_plan(None, "outer", Some(&now))
            .expect("must plan");

        let reference_path = plan_basis
            .data
            .iter()
            .find_map(|(path, def)| match def {
                DataDefinition::Reference { .. } => Some(path.clone()),
                _ => None,
            })
            .expect("plan must contain the reference for `i.slot`");

        let resolved_type = match plan_basis.data.get(&reference_path).expect("entry exists") {
            DataDefinition::Reference { resolved_type, .. } => Arc::clone(resolved_type),
            _ => unreachable!("filter above kept only Reference entries"),
        };

        let overlay = DataOverlay::default();

        let now_lit = LiteralValue {
            value: crate::planning::semantics::ValueKind::Date(
                crate::planning::semantics::date_time_to_semantic(&now),
            ),
            lemma_type: crate::planning::semantics::primitive_date_arc().clone(),
        };
        let context = EvaluationContext::new(plan_basis, &overlay, now_lit);

        let stored = context
            .data_values
            .get(&reference_path)
            .expect("EvaluationContext must populate reference path with the copied value");

        assert_eq!(
            stored.lemma_type, resolved_type,
            "stored LiteralValue must carry the reference's resolved_type \
             (LHS-merged), not the target's loose type. \
             stored = {:?}, resolved = {:?}",
            stored.lemma_type, resolved_type
        );
    }
}

/// Evaluates Lemma rules within their spec context
#[derive(Default)]
pub(crate) struct Evaluator;

impl Evaluator {
    /// Evaluate an execution plan: one compiled-instruction pass per rule.
    ///
    /// Returns the response and evaluation context. Call [`Self::explain`] to
    /// attach structural explanation trees.
    pub(crate) fn evaluate<'a>(
        &self,
        plan: &'a ExecutionPlan,
        overlay: &DataOverlay,
        now: LiteralValue,
        explain: bool,
        response_rules: &std::collections::HashSet<String>,
    ) -> (Response, EvaluationContext<'a>) {
        let effective = match &now.value {
            ValueKind::Date(date) => date.to_string(),
            other => panic!("BUG: evaluation now must be a date, got {other:?}"),
        };
        let mut context = EvaluationContext::new(plan, overlay, now);

        let mut response = Response {
            spec_name: plan.spec_name.clone(),
            effective,
            spec_hash: None,
            spec_effective_from: None,
            spec_effective_to: None,
            data: Vec::new(),
            results: IndexMap::new(),
        };

        let vm_rules: std::collections::HashSet<String> = if explain {
            plan.local_rule_names().into_iter().collect()
        } else {
            response_rules.clone()
        };

        for exec_rule in &plan.rules {
            let should_vm = if explain {
                !exec_rule.path.segments.is_empty() || vm_rules.contains(&exec_rule.name)
            } else {
                exec_rule.path.segments.is_empty() && vm_rules.contains(&exec_rule.name)
            };
            if !should_vm {
                continue;
            }

            // Explanations execute the source-shaped stream with recording:
            // that run's result is the authoritative result, so the response
            // and the explanation come from the same execution.
            let result = if explain {
                let mut recording = RuleRecording::default();
                let result = execute_instructions(
                    &exec_rule.source_instructions,
                    &mut context,
                    Some(&mut recording),
                );
                context.recordings.insert(exec_rule.path.clone(), recording);
                result
            } else {
                execute_instructions(&exec_rule.instructions, &mut context, None)
            };
            // The decimal limit applies before the result is stored: every
            // consumer — downstream rule-target references, `is veto`
            // checks, explanations, and the response — sees the same value.
            let result = ensure_rule_result_within_decimal_limit(result);
            context
                .rule_results
                .insert(exec_rule.path.clone(), result.clone());

            if !exec_rule.path.segments.is_empty() || !response_rules.contains(&exec_rule.name) {
                continue;
            }

            response.add_result(RuleResult::from_operation_result(
                EvaluatedRule {
                    name: exec_rule.name.clone(),
                    path: exec_rule.path.clone(),
                    source_location: exec_rule.source.clone(),
                    rule_type: (*exec_rule.rule_type).clone(),
                },
                result,
                exec_rule.rule_type.as_ref(),
                plan.expression_unit_index(),
                None,
            ));
        }

        let response_rule_names: Vec<String> = response_rules.iter().cloned().collect();
        let needed_data_paths = plan
            .collect_needed_data_paths(&response_rule_names, overlay)
            .expect("BUG: response rule names must be pre-validated by the caller");

        let data_list: Vec<Data> = plan
            .data
            .keys()
            .filter(|path| needed_data_paths.contains(*path))
            .filter_map(|path| {
                context.get_data_value(path).map(|value| Data {
                    path: path.clone(),
                    value: DataValue::from_literal(value.clone()),
                    source: None,
                })
            })
            .collect();

        if !data_list.is_empty() {
            response.data = vec![DataGroup {
                data_path: String::new(),
                referencing_data_name: String::new(),
                data: data_list,
            }];
        }

        (response, context)
    }

    /// Build structural explanations from source branches and attach them to rule results.
    pub(crate) fn explain(
        &self,
        response: &mut Response,
        plan: &ExecutionPlan,
        context: &EvaluationContext<'_>,
    ) {
        use crate::evaluation::explanations::{build_explanation, Explanation};
        use std::collections::HashMap;

        let mut built: HashMap<RulePath, Explanation> = HashMap::new();

        for exec_rule in &plan.rules {
            let explanation = build_explanation(exec_rule, context, plan, &built);
            built.insert(exec_rule.path.clone(), explanation.clone());

            if let Some(result) = response.results.get_mut(&exec_rule.name) {
                result.explanation = Some(explanation);
            }
        }
    }
}
