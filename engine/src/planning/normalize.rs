//! Expression normalization: algebraic simplification for drift-free evaluation.
//!
//! Internal [`NormalForm`] IR; public API is [`normalize_expression`].

use crate::computation::rational::{
    planning_rational_operation, rational_is_zero, rational_new, rational_one, rational_zero,
    NumericFailure, NumericOperation, RationalInteger,
};
use crate::computation::UnitResolutionContext;
use crate::parsing::ast::{CalendarPeriodUnit, DateCalendarKind, DateRelativeKind, PrimitiveKind};
use crate::planning::execution_plan::{Instruction, Instructions, INSTRUCTIONS_VERSION};
use crate::planning::semantics::{
    negated_comparison, primitive_boolean_arc, primitive_date_arc, primitive_number_arc,
    ArithmeticComputation, ComparisonComputation, DataDefinition, DataPath, Expression,
    ExpressionKind, LemmaType, LiteralValue, MathematicalComputation, ReferenceTarget, RulePath,
    SemanticConversionTarget, Source, TypeSpecification, ValueKind, VetoExpression,
};
use crate::Error;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn normalization_error(source: Option<Source>, failure: NumericFailure, context: &str) -> Error {
    Error::validation(format!("{context}: {failure}"), source, None::<String>)
}

/// Build the literal for an exactly folded plain-number rational.
///
/// Contract: callers must only fold operands admitted by
/// [`as_rational_literal`] (plain numbers). Unit-bearing operands must never
/// reach this function — quantity folds must carry their unit signature (see
/// [`typed_product_zero`]).
fn literal_from_folded_rational(
    rational: RationalInteger,
    _source: Option<Source>,
) -> Result<LiteralValue, Error> {
    Ok(LiteralValue::number_with_type(
        rational,
        primitive_number_arc().clone(),
    ))
}

/// Normalize a semantic expression to canonical form (fixed-point rewrite).
#[cfg(test)]
pub(crate) fn normalize_expression(
    expr: &Expression,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
) -> Result<Expression, Error> {
    let source = expr.source_location.clone();
    let mut nf = to_normal_form(expr);
    loop {
        let prev = nf.clone();
        nf = normalize_once(nf, unit_ctx, source.clone())?;
        if nf == prev {
            break;
        }
    }
    Ok(to_expression(&nf, source))
}

/// Build unless branches as a single expression (`Piecewise` or lone result).
///
/// Piecewise arms are stored in source order: default first, then each `unless` clause.
/// Compilation walks unless arms in reverse and returns on the first true condition
/// (last match in source order wins).
pub(crate) fn unless_branches_to_piecewise(
    branches: &[(Option<Expression>, Expression)],
) -> Expression {
    assert!(
        !branches.is_empty(),
        "BUG: rule must have at least one branch"
    );
    if branches.len() == 1 {
        return branches[0].1.clone();
    }

    let (_, default_result) = &branches[0];
    let source = default_result.source_location.clone();
    let mut arms: Vec<(Arc<Expression>, Arc<Expression>)> = Vec::with_capacity(branches.len());
    arms.push((
        Arc::new(literal_bool_expression(true, source.clone())),
        Arc::new(default_result.clone()),
    ));
    for (condition, result) in branches.iter().skip(1) {
        let unless_condition = condition
            .as_ref()
            .expect("BUG: non-default branch missing condition");
        arms.push((Arc::new(unless_condition.clone()), Arc::new(result.clone())));
    }

    Expression::with_source(ExpressionKind::Piecewise(arms), source)
}

/// One complete transitive equation per rule: unless → Piecewise, inline deps, normalize, compile.
///
/// Produces two instruction streams from the same inlined expression:
/// - `instructions`: the optimized stream (full `simplify` fixed-point), used
///   for normal evaluation;
/// - `source_instructions`: compiled with **zero rewrite passes**, mirroring
///   the source expression shape; executed with recording when explanations
///   are requested.
pub(crate) struct CompiledRule {
    pub(crate) instructions: Instructions,
    pub(crate) source_instructions: Instructions,
    pub(crate) inlined_expression: Arc<Expression>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_normalized_rule_instructions(
    branches: &[(Option<Expression>, Expression)],
    completed_rules: &HashMap<RulePath, Arc<Expression>>,
    plan_rule_paths: &HashSet<RulePath>,
    data: &IndexMap<DataPath, DataDefinition>,
    unit_ctx: &UnitResolutionContext<'_>,
    source: Option<Source>,
    rule_type: &Arc<LemmaType>,
    max_normalized_expression_nodes: usize,
) -> Result<CompiledRule, Error> {
    let piecewise = unless_branches_to_piecewise(branches);
    let inlined_rules = substitute_completed_rule_paths_arc(&piecewise, completed_rules);
    let rule_target_data = build_in_plan_rule_target_data_map(data);
    let inlined =
        substitute_rule_target_data_paths_arc(inlined_rules, &rule_target_data, completed_rules);
    validate_no_in_plan_rule_paths(inlined.as_ref(), plan_rule_paths);
    validate_no_rule_target_data_paths(inlined.as_ref(), data);
    // The inlined expression shares subtrees through `Arc`, but
    // `to_normal_form` materializes it as a tree. Reject oversized trees
    // before materialization: a short chain of self-doubling rules grows
    // exponentially and would otherwise exhaust memory or overflow the
    // compiler's u16 operand indices.
    if expression_tree_exceeds_node_budget(inlined.as_ref(), max_normalized_expression_nodes) {
        return Err(expression_node_limit_error(
            max_normalized_expression_nodes,
            source,
        ));
    }
    let mut nf = to_normal_form(inlined.as_ref());
    // Source stream: zero rewrite passes — the compiled shape mirrors the
    // source expression, so recorded execution maps back to source nodes.
    let source_instructions = compile_normal_form(&nf, data, unit_ctx, rule_type)?;
    loop {
        let prev = nf.clone();
        nf = normalize_once(nf, Some(unit_ctx), source.clone())?;
        if nf == prev {
            break;
        }
    }
    // Normalization passes may grow the tree (subtract/divide expansion,
    // sqrt-to-power). Re-check before compilation so the u16 operand
    // asserts in `CompileContext` remain genuinely unreachable backstops.
    if normal_form_exceeds_node_budget(&nf, max_normalized_expression_nodes) {
        return Err(expression_node_limit_error(
            max_normalized_expression_nodes,
            source,
        ));
    }
    let instructions = compile_normal_form(&nf, data, unit_ctx, rule_type)?;
    Ok(CompiledRule {
        instructions,
        source_instructions,
        inlined_expression: inlined,
    })
}

fn expression_node_limit_error(limit: usize, source: Option<Source>) -> Error {
    Error::resource_limit_exceeded(
        "max_normalized_expression_nodes",
        format!("{limit} expression nodes"),
        format!("more than {limit} expression nodes after inlining"),
        "Restructure the rule or reduce repeated references to other rules",
        source,
        None,
        None,
    )
}

/// Whether the (possibly `Arc`-shared) expression, walked as a tree — the
/// exact shape `to_normal_form` materializes — exceeds the node budget.
/// Aborts as soon as the budget is exhausted, so pathological inputs are
/// rejected in bounded time without materializing anything.
fn expression_tree_exceeds_node_budget(expr: &Expression, budget: usize) -> bool {
    let mut remaining = budget;
    let mut worklist: Vec<&Expression> = vec![expr];
    while let Some(current) = worklist.pop() {
        if remaining == 0 {
            return true;
        }
        remaining -= 1;
        match &current.kind {
            ExpressionKind::Literal(_)
            | ExpressionKind::DataPath(_)
            | ExpressionKind::RulePath(_)
            | ExpressionKind::Veto(_)
            | ExpressionKind::Now => {}
            ExpressionKind::Arithmetic(left, _, right)
            | ExpressionKind::Comparison(left, _, right)
            | ExpressionKind::LogicalAnd(left, right)
            | ExpressionKind::LogicalOr(left, right)
            | ExpressionKind::RangeLiteral(left, right)
            | ExpressionKind::RangeContainment(left, right) => {
                worklist.push(left);
                worklist.push(right);
            }
            ExpressionKind::LogicalNegation(operand, _)
            | ExpressionKind::MathematicalComputation(_, operand)
            | ExpressionKind::UnitConversion(operand, _)
            | ExpressionKind::DateRelative(_, operand)
            | ExpressionKind::DateCalendar(_, _, operand)
            | ExpressionKind::PastFutureRange(_, operand)
            | ExpressionKind::ResultIsVeto(operand) => {
                worklist.push(operand);
            }
            ExpressionKind::Piecewise(arms) => {
                for (condition, result) in arms.iter() {
                    worklist.push(condition);
                    worklist.push(result);
                }
            }
        }
    }
    false
}

/// Whether the normal form, walked as a tree — the exact shape
/// `compile_normal_form` traverses — exceeds the node budget.
fn normal_form_exceeds_node_budget(nf: &NormalForm, budget: usize) -> bool {
    let mut remaining = budget;
    let mut worklist: Vec<&NormalForm> = vec![nf];
    while let Some(current) = worklist.pop() {
        if remaining == 0 {
            return true;
        }
        remaining -= 1;
        match current {
            NormalForm::Leaf(_) | NormalForm::Veto(_) | NormalForm::Now => {}
            NormalForm::Sum(children)
            | NormalForm::Product(children)
            | NormalForm::And(children)
            | NormalForm::Or(children) => {
                worklist.extend(children.iter());
            }
            NormalForm::Subtract(a, b)
            | NormalForm::Divide(a, b)
            | NormalForm::Power(a, b)
            | NormalForm::Modulo(a, b)
            | NormalForm::Comparison(a, _, b)
            | NormalForm::RangeLiteral(a, b)
            | NormalForm::RangeContainment(a, b) => {
                worklist.push(a.as_ref());
                worklist.push(b.as_ref());
            }
            NormalForm::Negate(x)
            | NormalForm::Reciprocal(x)
            | NormalForm::Not(x)
            | NormalForm::MathOp(_, x)
            | NormalForm::UnitConversion(x, _, _)
            | NormalForm::DateRelative(_, x)
            | NormalForm::DateCalendar(_, _, x)
            | NormalForm::PastFutureRange(_, x)
            | NormalForm::ResultIsVeto(x) => {
                worklist.push(x.as_ref());
            }
            NormalForm::Piecewise(arms) => {
                for (condition, result) in arms.iter() {
                    worklist.push(condition.as_ref());
                    worklist.push(result.as_ref());
                }
            }
        }
    }
    false
}

struct CompileContext<'a> {
    register_types: Vec<Arc<LemmaType>>,
    constants: Vec<LiteralValue>,
    data_manifest: Vec<DataPath>,
    veto_messages: Vec<String>,
    code: Vec<Instruction>,
    arm_tags: Vec<crate::planning::execution_plan::ArmTag>,
    conversion_tags: Vec<crate::planning::execution_plan::ConversionTag>,
    data: &'a IndexMap<DataPath, DataDefinition>,
    unit_ctx: &'a UnitResolutionContext<'a>,
    rule_type: Arc<LemmaType>,
}

impl<'a> CompileContext<'a> {
    fn new(
        data: &'a IndexMap<DataPath, DataDefinition>,
        unit_ctx: &'a UnitResolutionContext<'a>,
        rule_type: &Arc<LemmaType>,
    ) -> Self {
        Self {
            register_types: Vec::new(),
            constants: Vec::new(),
            data_manifest: Vec::new(),
            veto_messages: Vec::new(),
            code: Vec::new(),
            arm_tags: Vec::new(),
            conversion_tags: Vec::new(),
            data,
            unit_ctx,
            rule_type: Arc::clone(rule_type),
        }
    }

    fn allocate_register(&mut self, ty: Arc<LemmaType>) -> u16 {
        let id = self.register_types.len();
        assert!(id < u16::MAX as usize, "BUG: register count overflow");
        self.register_types.push(ty);
        id as u16
    }

    fn resolve_dest(&mut self, dest: Option<u16>, ty: Arc<LemmaType>) -> u16 {
        dest.unwrap_or_else(|| self.allocate_register(ty))
    }

    fn constant_index(&mut self, value: LiteralValue) -> u16 {
        // Named compound-unit literals (e.g. `0.5 eur_per_km`) are lowered to
        // their base-unit signature at interning so both instruction streams
        // carry signatures runtime arithmetic understands. This is lowering,
        // not rewriting: deterministic per literal given the unit index.
        let value = expand_named_quantity_literal(&value, Some(self.unit_ctx)).unwrap_or(value);
        if let Some((idx, _)) = self
            .constants
            .iter()
            .enumerate()
            .find(|(_, existing)| **existing == value)
        {
            return idx as u16;
        }
        let idx = self.constants.len();
        assert!(idx < u16::MAX as usize, "BUG: constant table overflow");
        self.constants.push(value);
        idx as u16
    }

    fn data_index(&mut self, path: DataPath) -> u16 {
        if let Some(idx) = self.data_manifest.iter().position(|p| p == &path) {
            return idx as u16;
        }
        let idx = self.data_manifest.len();
        assert!(idx < u16::MAX as usize, "BUG: data manifest overflow");
        self.data_manifest.push(path);
        idx as u16
    }

    fn veto_message_index(&mut self, message: Option<String>) -> u16 {
        let key = message.clone().unwrap_or_default();
        if let Some(idx) = self.veto_messages.iter().position(|m| m == &key) {
            return idx as u16;
        }
        let idx = self.veto_messages.len();
        assert!(idx < u16::MAX as usize, "BUG: veto message table overflow");
        self.veto_messages.push(key);
        idx as u16
    }

    fn emit(&mut self, insn: Instruction) -> u32 {
        let pc = self.code.len() as u32;
        self.code.push(insn);
        pc
    }

    fn patch_jump_target(&mut self, insn_index: usize, target: u32) {
        match &mut self.code[insn_index] {
            Instruction::JumpIfFalse {
                target_instruction, ..
            } => *target_instruction = target,
            Instruction::Jump { target_instruction } => *target_instruction = target,
            _ => panic!("BUG: patch_jump_target on non-jump instruction"),
        }
    }

    fn load_bool_constant(&mut self, value: bool, dest: u16) {
        let idx = self.constant_index(LiteralValue::from_bool(value));
        self.emit(Instruction::LoadConstant {
            destination_register: dest,
            constant_index: idx,
        });
    }

    fn finish(self) -> Instructions {
        let instructions = Instructions {
            version: INSTRUCTIONS_VERSION,
            register_count: self.register_types.len() as u16,
            register_types: self.register_types,
            constants: self.constants,
            data_manifest: self.data_manifest,
            veto_messages: self.veto_messages,
            code: self.code,
            arm_tags: self.arm_tags,
            conversion_tags: self.conversion_tags,
        };
        if let Err(message) = crate::planning::execution_plan::validate_instructions(&instructions)
        {
            panic!("BUG: compiler produced invalid instructions: {message}");
        }
        instructions
    }
}

fn data_path_type(data: &IndexMap<DataPath, DataDefinition>, path: &DataPath) -> Arc<LemmaType> {
    match data.get(path) {
        Some(DataDefinition::Value { value, .. }) => Arc::clone(&value.lemma_type),
        Some(DataDefinition::Reference { resolved_type, .. })
        | Some(DataDefinition::TypeDeclaration { resolved_type, .. }) => Arc::clone(resolved_type),
        Some(DataDefinition::Import { .. }) => {
            panic!(
                "BUG: import data path '{}' in instruction data manifest",
                path
            )
        }
        None => panic!("BUG: data path '{}' missing from plan data", path),
    }
}

fn compile_normal_form(
    nf: &NormalForm,
    data: &IndexMap<DataPath, DataDefinition>,
    unit_ctx: &UnitResolutionContext<'_>,
    rule_type: &Arc<LemmaType>,
) -> Result<Instructions, Error> {
    let mut ctx = CompileContext::new(data, unit_ctx, rule_type);
    match nf {
        NormalForm::Piecewise(arms) => compile_piecewise_rule(arms, &mut ctx),
        other => {
            let result_reg = compile_nf(other, &mut ctx, None);
            let return_pc = ctx.emit(Instruction::Return {
                source_register: result_reg,
            });
            ctx.arm_tags.push(crate::planning::execution_plan::ArmTag {
                pc: return_pc,
                arm: 0,
                role: crate::planning::execution_plan::ArmRole::Result,
            });
        }
    }
    Ok(ctx.finish())
}

/// Top-level rule piecewise: unless arms in reverse source order, first match wins (last in source).
fn compile_piecewise_rule(
    arms: &[(Arc<NormalForm>, Arc<NormalForm>)],
    ctx: &mut CompileContext<'_>,
) {
    use crate::planning::execution_plan::{ArmRole, ArmTag, JumpVetoSemantics};
    assert!(!arms.is_empty(), "BUG: empty piecewise rule");
    if arms.len() == 1 {
        let result_reg = compile_nf(&arms[0].1, ctx, None);
        let return_pc = ctx.emit(Instruction::Return {
            source_register: result_reg,
        });
        ctx.arm_tags.push(ArmTag {
            pc: return_pc,
            arm: 0,
            role: ArmRole::Result,
        });
        return;
    }

    for i in (1..arms.len()).rev() {
        let (cond, result) = &arms[i];
        let cond_reg = compile_nf(cond, ctx, None);
        let jump_idx = ctx.code.len();
        ctx.emit(Instruction::JumpIfFalse {
            condition_register: cond_reg,
            target_instruction: 0,
            veto_semantics: JumpVetoSemantics::UnlessRuleReference,
        });
        ctx.arm_tags.push(ArmTag {
            pc: jump_idx as u32,
            arm: i as u16,
            role: ArmRole::Condition,
        });
        let result_reg = compile_nf(result, ctx, None);
        let return_pc = ctx.emit(Instruction::Return {
            source_register: result_reg,
        });
        ctx.arm_tags.push(ArmTag {
            pc: return_pc,
            arm: i as u16,
            role: ArmRole::Result,
        });
        let next_target = ctx.code.len() as u32;
        ctx.patch_jump_target(jump_idx, next_target);
    }

    let result_reg = compile_nf(&arms[0].1, ctx, None);
    let return_pc = ctx.emit(Instruction::Return {
        source_register: result_reg,
    });
    ctx.arm_tags.push(ArmTag {
        pc: return_pc,
        arm: 0,
        role: ArmRole::Result,
    });
}

/// Inlined piecewise subexpression: last unless match wins, then joins `merge`.
fn compile_piecewise_value(
    arms: &[(Arc<NormalForm>, Arc<NormalForm>)],
    ctx: &mut CompileContext<'_>,
    dest: u16,
) {
    use crate::planning::execution_plan::JumpVetoSemantics;
    assert!(!arms.is_empty(), "BUG: empty piecewise value");
    if arms.len() == 1 {
        compile_nf(&arms[0].1, ctx, Some(dest));
        return;
    }

    let mut success_jump_indices = Vec::new();
    for i in (1..arms.len()).rev() {
        let (cond, result) = &arms[i];
        let cond_reg = compile_nf(cond, ctx, None);
        let jump_false_idx = ctx.code.len();
        ctx.emit(Instruction::JumpIfFalse {
            condition_register: cond_reg,
            target_instruction: 0,
            veto_semantics: JumpVetoSemantics::UnlessRuleReference,
        });
        compile_nf(result, ctx, Some(dest));
        let jump_success_idx = ctx.code.len();
        ctx.emit(Instruction::Jump {
            target_instruction: 0,
        });
        success_jump_indices.push(jump_success_idx);
        let next_arm = ctx.code.len() as u32;
        ctx.patch_jump_target(jump_false_idx, next_arm);
    }
    compile_nf(&arms[0].1, ctx, Some(dest));
    let merge = ctx.code.len() as u32;
    for idx in success_jump_indices {
        ctx.patch_jump_target(idx, merge);
    }
}

fn compile_nf(nf: &NormalForm, ctx: &mut CompileContext<'_>, dest: Option<u16>) -> u16 {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(literal)) => {
            let d = ctx.resolve_dest(dest, Arc::clone(&literal.lemma_type));
            let idx = ctx.constant_index((**literal).clone());
            ctx.emit(Instruction::LoadConstant {
                destination_register: d,
                constant_index: idx,
            });
            d
        }
        NormalForm::Leaf(LeafKind::DataPath(path)) => {
            let ty = data_path_type(ctx.data, path);
            let d = ctx.resolve_dest(dest, ty);
            let idx = ctx.data_index(path.clone());
            ctx.emit(Instruction::LoadData {
                destination_register: d,
                data_index: idx,
            });
            d
        }
        NormalForm::Leaf(LeafKind::RulePath(path)) => {
            panic!(
                "BUG: RulePath '{}' must be inlined before compile",
                path.rule
            );
        }
        NormalForm::Now => {
            let d = ctx.resolve_dest(dest, primitive_date_arc().clone());
            ctx.emit(Instruction::LoadNow {
                destination_register: d,
            });
            d
        }
        NormalForm::Veto(veto) => {
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            let idx = ctx.veto_message_index(veto.message.clone());
            ctx.emit(Instruction::UserVeto {
                destination_register: d,
                message_index: idx,
            });
            d
        }
        NormalForm::Subtract(left, right) => {
            let left_reg = compile_nf(left, ctx, None);
            let right_reg = compile_nf(right, ctx, None);
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            ctx.emit(Instruction::Arithmetic {
                destination_register: d,
                operation: ArithmeticComputation::Subtract,
                left_register: left_reg,
                right_register: right_reg,
            });
            d
        }
        NormalForm::Divide(left, right) => {
            let left_reg = compile_nf(left, ctx, None);
            let right_reg = compile_nf(right, ctx, None);
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            ctx.emit(Instruction::Arithmetic {
                destination_register: d,
                operation: ArithmeticComputation::Divide,
                left_register: left_reg,
                right_register: right_reg,
            });
            d
        }
        NormalForm::Power(left, right) => {
            let left_reg = compile_nf(left, ctx, None);
            let right_reg = compile_nf(right, ctx, None);
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            ctx.emit(Instruction::Arithmetic {
                destination_register: d,
                operation: ArithmeticComputation::Power,
                left_register: left_reg,
                right_register: right_reg,
            });
            d
        }
        NormalForm::Modulo(left, right) => {
            let left_reg = compile_nf(left, ctx, None);
            let right_reg = compile_nf(right, ctx, None);
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            ctx.emit(Instruction::Arithmetic {
                destination_register: d,
                operation: ArithmeticComputation::Modulo,
                left_register: left_reg,
                right_register: right_reg,
            });
            d
        }
        NormalForm::Sum(children) => compile_folded_arithmetic(
            children,
            ArithmeticComputation::Add,
            rational_zero(),
            ctx,
            dest,
        ),
        NormalForm::Product(children) => compile_folded_arithmetic(
            children,
            ArithmeticComputation::Multiply,
            rational_one(),
            ctx,
            dest,
        ),
        NormalForm::Negate(inner) => {
            let zero = NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                rational_zero(),
            ))));
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            let zero_reg = compile_nf(&zero, ctx, None);
            let inner_reg = compile_nf(inner, ctx, None);
            ctx.emit(Instruction::Arithmetic {
                destination_register: d,
                operation: ArithmeticComputation::Subtract,
                left_register: zero_reg,
                right_register: inner_reg,
            });
            d
        }
        NormalForm::Reciprocal(inner) => {
            let one = NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                rational_one(),
            ))));
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            let one_reg = compile_nf(&one, ctx, None);
            let inner_reg = compile_nf(inner, ctx, None);
            ctx.emit(Instruction::Arithmetic {
                destination_register: d,
                operation: ArithmeticComputation::Divide,
                left_register: one_reg,
                right_register: inner_reg,
            });
            d
        }
        NormalForm::Comparison(left, op, right) => {
            let left_reg = compile_nf(left, ctx, None);
            let right_reg = compile_nf(right, ctx, None);
            let d = ctx.resolve_dest(dest, primitive_boolean_arc().clone());
            ctx.emit(Instruction::Comparison {
                destination_register: d,
                operation: op.clone(),
                left_register: left_reg,
                right_register: right_reg,
            });
            d
        }
        NormalForm::And(children) => compile_short_circuit_and(children, ctx, dest),
        NormalForm::Or(children) => compile_short_circuit_or(children, ctx, dest),
        NormalForm::Not(inner) => {
            let inner_reg = compile_nf(inner, ctx, None);
            let false_reg = compile_nf(
                &NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(false)))),
                ctx,
                None,
            );
            let d = ctx.resolve_dest(dest, primitive_boolean_arc().clone());
            ctx.emit(Instruction::Comparison {
                destination_register: d,
                operation: ComparisonComputation::Is,
                left_register: inner_reg,
                right_register: false_reg,
            });
            d
        }
        NormalForm::MathOp(op, inner) => {
            let inner_reg = compile_nf(inner, ctx, None);
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            ctx.emit(Instruction::Mathematical {
                destination_register: d,
                operation: op.clone(),
                source_register: inner_reg,
            });
            d
        }
        NormalForm::UnitConversion(inner, target, origin) => {
            let inner_reg = compile_nf(inner, ctx, None);
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            let pc = ctx.emit(Instruction::UnitConversion {
                destination_register: d,
                source_register: inner_reg,
                target: target.clone(),
            });
            if let Some(source) = origin {
                ctx.conversion_tags
                    .push(crate::planning::execution_plan::ConversionTag {
                        pc,
                        source: source.clone(),
                    });
            }
            d
        }
        NormalForm::DateRelative(kind, inner) => {
            let inner_reg = compile_nf(inner, ctx, None);
            let d = ctx.resolve_dest(dest, primitive_date_arc().clone());
            ctx.emit(Instruction::DateRelative {
                destination_register: d,
                kind: *kind,
                source_register: inner_reg,
            });
            d
        }
        NormalForm::DateCalendar(kind, unit, inner) => {
            let inner_reg = compile_nf(inner, ctx, None);
            let d = ctx.resolve_dest(dest, primitive_date_arc().clone());
            ctx.emit(Instruction::DateCalendar {
                destination_register: d,
                kind: *kind,
                unit: *unit,
                source_register: inner_reg,
            });
            d
        }
        NormalForm::RangeLiteral(left, right) => {
            let left_reg = compile_nf(left, ctx, None);
            let right_reg = compile_nf(right, ctx, None);
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            ctx.emit(Instruction::RangeLiteral {
                destination_register: d,
                left_register: left_reg,
                right_register: right_reg,
            });
            d
        }
        NormalForm::PastFutureRange(kind, inner) => {
            let inner_reg = compile_nf(inner, ctx, None);
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            ctx.emit(Instruction::PastFutureRange {
                destination_register: d,
                kind: *kind,
                source_register: inner_reg,
            });
            d
        }
        NormalForm::RangeContainment(value, range) => {
            let value_reg = compile_nf(value, ctx, None);
            let range_reg = compile_nf(range, ctx, None);
            let d = ctx.resolve_dest(dest, primitive_boolean_arc().clone());
            ctx.emit(Instruction::RangeContainment {
                destination_register: d,
                value_register: value_reg,
                range_register: range_reg,
            });
            d
        }
        NormalForm::ResultIsVeto(inner) => {
            let inner_reg = compile_nf(inner, ctx, None);
            let d = ctx.resolve_dest(dest, primitive_boolean_arc().clone());
            ctx.emit(Instruction::ResultIsVeto {
                destination_register: d,
                source_register: inner_reg,
            });
            d
        }
        NormalForm::Piecewise(arms) => {
            let d = ctx.resolve_dest(dest, ctx.rule_type.clone());
            compile_piecewise_value(arms, ctx, d);
            d
        }
    }
}

fn compile_folded_arithmetic(
    children: &[NormalForm],
    operation: ArithmeticComputation,
    _identity: RationalInteger,
    ctx: &mut CompileContext<'_>,
    dest: Option<u16>,
) -> u16 {
    assert!(!children.is_empty(), "BUG: empty n-ary arithmetic");
    if children.len() == 1 {
        return compile_nf(&children[0], ctx, dest);
    }
    let mut acc = compile_nf(&children[0], ctx, None);
    for (i, child) in children.iter().enumerate().skip(1) {
        let right = compile_nf(child, ctx, None);
        let is_last = i == children.len() - 1;
        let out = ctx.resolve_dest(if is_last { dest } else { None }, ctx.rule_type.clone());
        ctx.emit(Instruction::Arithmetic {
            destination_register: out,
            operation: operation.clone(),
            left_register: acc,
            right_register: right,
        });
        acc = out;
    }
    acc
}

fn compile_short_circuit_and(
    children: &[NormalForm],
    ctx: &mut CompileContext<'_>,
    dest: Option<u16>,
) -> u16 {
    assert!(!children.is_empty(), "BUG: empty And");
    if children.len() == 1 {
        return compile_nf(&children[0], ctx, dest);
    }
    let d = ctx.resolve_dest(dest, primitive_boolean_arc().clone());
    let mut false_jump_indices = Vec::new();
    let mut veto_merge_jump_indices = Vec::new();
    for (i, child) in children.iter().enumerate() {
        if i < children.len() - 1 {
            let child_reg = compile_nf(child, ctx, None);
            let is_veto_reg = ctx.allocate_register(primitive_boolean_arc().clone());
            ctx.emit(Instruction::ResultIsVeto {
                destination_register: is_veto_reg,
                source_register: child_reg,
            });
            let skip_veto_idx = ctx.code.len();
            ctx.emit(Instruction::JumpIfFalse {
                condition_register: is_veto_reg,
                target_instruction: 0,
                veto_semantics:
                    crate::planning::execution_plan::JumpVetoSemantics::UnlessExpression,
            });
            ctx.emit(Instruction::MoveRegister {
                destination_register: d,
                source_register: child_reg,
            });
            veto_merge_jump_indices.push(ctx.code.len());
            ctx.emit(Instruction::Jump {
                target_instruction: 0,
            });
            let after_veto_check = ctx.code.len() as u32;
            ctx.patch_jump_target(skip_veto_idx, after_veto_check);
            let idx = ctx.code.len();
            ctx.emit(Instruction::JumpIfFalse {
                condition_register: child_reg,
                target_instruction: 0,
                veto_semantics:
                    crate::planning::execution_plan::JumpVetoSemantics::UnlessExpression,
            });
            false_jump_indices.push(idx);
        } else {
            compile_nf(child, ctx, Some(d));
        }
    }
    let success_jump_idx = ctx.code.len();
    ctx.emit(Instruction::Jump {
        target_instruction: 0,
    });
    let false_label = ctx.code.len() as u32;
    for idx in false_jump_indices {
        ctx.patch_jump_target(idx, false_label);
    }
    ctx.load_bool_constant(false, d);
    let merge = ctx.code.len() as u32;
    for idx in veto_merge_jump_indices {
        ctx.patch_jump_target(idx, merge);
    }
    ctx.patch_jump_target(success_jump_idx, merge);
    d
}

fn compile_short_circuit_or(
    children: &[NormalForm],
    ctx: &mut CompileContext<'_>,
    dest: Option<u16>,
) -> u16 {
    assert!(!children.is_empty(), "BUG: empty Or");
    if children.len() == 1 {
        return compile_nf(&children[0], ctx, dest);
    }
    let d = ctx.resolve_dest(dest, primitive_boolean_arc().clone());
    let mut success_jump_indices = Vec::new();
    for (i, child) in children.iter().enumerate() {
        if i < children.len() - 1 {
            let child_reg = compile_nf(child, ctx, None);
            let jump_false_idx = ctx.code.len();
            ctx.emit(Instruction::JumpIfFalse {
                condition_register: child_reg,
                target_instruction: 0,
                veto_semantics:
                    crate::planning::execution_plan::JumpVetoSemantics::UnlessExpression,
            });
            ctx.load_bool_constant(true, d);
            success_jump_indices.push(ctx.code.len());
            ctx.emit(Instruction::Jump {
                target_instruction: 0,
            });
            let next_child = ctx.code.len() as u32;
            ctx.patch_jump_target(jump_false_idx, next_child);
        } else {
            compile_nf(child, ctx, Some(d));
        }
    }
    let merge = ctx.code.len() as u32;
    for idx in success_jump_indices {
        ctx.patch_jump_target(idx, merge);
    }
    d
}

fn substitute_completed_rule_paths_arc(
    expr: &Expression,
    completed_rules: &HashMap<RulePath, Arc<Expression>>,
) -> Arc<Expression> {
    if let ExpressionKind::RulePath(path) = &expr.kind {
        if let Some(replacement) = completed_rules.get(path) {
            return Arc::clone(replacement);
        }
    }

    let source = expr.source_location.clone();
    let kind = match &expr.kind {
        ExpressionKind::Literal(_)
        | ExpressionKind::DataPath(_)
        | ExpressionKind::RulePath(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Now => expr.kind.clone(),
        ExpressionKind::LogicalAnd(left, right) => ExpressionKind::LogicalAnd(
            substitute_completed_rule_paths_arc(left, completed_rules),
            substitute_completed_rule_paths_arc(right, completed_rules),
        ),
        ExpressionKind::LogicalOr(left, right) => ExpressionKind::LogicalOr(
            substitute_completed_rule_paths_arc(left, completed_rules),
            substitute_completed_rule_paths_arc(right, completed_rules),
        ),
        ExpressionKind::Arithmetic(left, op, right) => ExpressionKind::Arithmetic(
            substitute_completed_rule_paths_arc(left, completed_rules),
            op.clone(),
            substitute_completed_rule_paths_arc(right, completed_rules),
        ),
        ExpressionKind::Comparison(left, op, right) => ExpressionKind::Comparison(
            substitute_completed_rule_paths_arc(left, completed_rules),
            op.clone(),
            substitute_completed_rule_paths_arc(right, completed_rules),
        ),
        ExpressionKind::UnitConversion(inner, target) => ExpressionKind::UnitConversion(
            substitute_completed_rule_paths_arc(inner, completed_rules),
            target.clone(),
        ),
        ExpressionKind::LogicalNegation(inner, negation) => ExpressionKind::LogicalNegation(
            substitute_completed_rule_paths_arc(inner, completed_rules),
            negation.clone(),
        ),
        ExpressionKind::MathematicalComputation(op, inner) => {
            ExpressionKind::MathematicalComputation(
                op.clone(),
                substitute_completed_rule_paths_arc(inner, completed_rules),
            )
        }
        ExpressionKind::DateRelative(kind, inner) => ExpressionKind::DateRelative(
            *kind,
            substitute_completed_rule_paths_arc(inner, completed_rules),
        ),
        ExpressionKind::DateCalendar(kind, unit, inner) => ExpressionKind::DateCalendar(
            *kind,
            *unit,
            substitute_completed_rule_paths_arc(inner, completed_rules),
        ),
        ExpressionKind::RangeLiteral(left, right) => ExpressionKind::RangeLiteral(
            substitute_completed_rule_paths_arc(left, completed_rules),
            substitute_completed_rule_paths_arc(right, completed_rules),
        ),
        ExpressionKind::PastFutureRange(kind, inner) => ExpressionKind::PastFutureRange(
            *kind,
            substitute_completed_rule_paths_arc(inner, completed_rules),
        ),
        ExpressionKind::RangeContainment(left, right) => ExpressionKind::RangeContainment(
            substitute_completed_rule_paths_arc(left, completed_rules),
            substitute_completed_rule_paths_arc(right, completed_rules),
        ),
        ExpressionKind::ResultIsVeto(inner) => ExpressionKind::ResultIsVeto(
            substitute_completed_rule_paths_arc(inner, completed_rules),
        ),
        ExpressionKind::Piecewise(arms) => ExpressionKind::Piecewise(
            arms.iter()
                .map(|(condition, result)| {
                    (
                        substitute_completed_rule_paths_arc(condition, completed_rules),
                        substitute_completed_rule_paths_arc(result, completed_rules),
                    )
                })
                .collect(),
        ),
    };
    Arc::new(Expression::with_source(kind, source))
}

fn substitute_rule_target_data_paths_arc(
    expr: Arc<Expression>,
    rule_target_data: &HashMap<DataPath, RulePath>,
    completed_rules: &HashMap<RulePath, Arc<Expression>>,
) -> Arc<Expression> {
    if let ExpressionKind::DataPath(data_path) = &expr.kind {
        if let Some(rule_path) = rule_target_data.get(data_path) {
            if let Some(replacement) = completed_rules.get(rule_path) {
                return Arc::clone(replacement);
            }
        }
    }

    let source = expr.source_location.clone();
    let kind = match &expr.kind {
        ExpressionKind::Literal(_)
        | ExpressionKind::DataPath(_)
        | ExpressionKind::RulePath(_)
        | ExpressionKind::Veto(_)
        | ExpressionKind::Now => expr.kind.clone(),
        ExpressionKind::LogicalAnd(left, right) => ExpressionKind::LogicalAnd(
            substitute_rule_target_data_paths_arc(
                Arc::clone(left),
                rule_target_data,
                completed_rules,
            ),
            substitute_rule_target_data_paths_arc(
                Arc::clone(right),
                rule_target_data,
                completed_rules,
            ),
        ),
        ExpressionKind::LogicalOr(left, right) => ExpressionKind::LogicalOr(
            substitute_rule_target_data_paths_arc(
                Arc::clone(left),
                rule_target_data,
                completed_rules,
            ),
            substitute_rule_target_data_paths_arc(
                Arc::clone(right),
                rule_target_data,
                completed_rules,
            ),
        ),
        ExpressionKind::Arithmetic(left, operation, right) => ExpressionKind::Arithmetic(
            substitute_rule_target_data_paths_arc(
                Arc::clone(left),
                rule_target_data,
                completed_rules,
            ),
            operation.clone(),
            substitute_rule_target_data_paths_arc(
                Arc::clone(right),
                rule_target_data,
                completed_rules,
            ),
        ),
        ExpressionKind::Comparison(left, operation, right) => ExpressionKind::Comparison(
            substitute_rule_target_data_paths_arc(
                Arc::clone(left),
                rule_target_data,
                completed_rules,
            ),
            operation.clone(),
            substitute_rule_target_data_paths_arc(
                Arc::clone(right),
                rule_target_data,
                completed_rules,
            ),
        ),
        ExpressionKind::UnitConversion(inner, target) => ExpressionKind::UnitConversion(
            substitute_rule_target_data_paths_arc(
                Arc::clone(inner),
                rule_target_data,
                completed_rules,
            ),
            target.clone(),
        ),
        ExpressionKind::LogicalNegation(inner, negation) => ExpressionKind::LogicalNegation(
            substitute_rule_target_data_paths_arc(
                Arc::clone(inner),
                rule_target_data,
                completed_rules,
            ),
            negation.clone(),
        ),
        ExpressionKind::MathematicalComputation(operation, inner) => {
            ExpressionKind::MathematicalComputation(
                operation.clone(),
                substitute_rule_target_data_paths_arc(
                    Arc::clone(inner),
                    rule_target_data,
                    completed_rules,
                ),
            )
        }
        ExpressionKind::DateRelative(kind, inner) => ExpressionKind::DateRelative(
            *kind,
            substitute_rule_target_data_paths_arc(
                Arc::clone(inner),
                rule_target_data,
                completed_rules,
            ),
        ),
        ExpressionKind::DateCalendar(kind, unit, inner) => ExpressionKind::DateCalendar(
            *kind,
            *unit,
            substitute_rule_target_data_paths_arc(
                Arc::clone(inner),
                rule_target_data,
                completed_rules,
            ),
        ),
        ExpressionKind::RangeLiteral(left, right) => ExpressionKind::RangeLiteral(
            substitute_rule_target_data_paths_arc(
                Arc::clone(left),
                rule_target_data,
                completed_rules,
            ),
            substitute_rule_target_data_paths_arc(
                Arc::clone(right),
                rule_target_data,
                completed_rules,
            ),
        ),
        ExpressionKind::PastFutureRange(kind, inner) => ExpressionKind::PastFutureRange(
            *kind,
            substitute_rule_target_data_paths_arc(
                Arc::clone(inner),
                rule_target_data,
                completed_rules,
            ),
        ),
        ExpressionKind::RangeContainment(left, right) => ExpressionKind::RangeContainment(
            substitute_rule_target_data_paths_arc(
                Arc::clone(left),
                rule_target_data,
                completed_rules,
            ),
            substitute_rule_target_data_paths_arc(
                Arc::clone(right),
                rule_target_data,
                completed_rules,
            ),
        ),
        ExpressionKind::ResultIsVeto(inner) => {
            ExpressionKind::ResultIsVeto(substitute_rule_target_data_paths_arc(
                Arc::clone(inner),
                rule_target_data,
                completed_rules,
            ))
        }
        ExpressionKind::Piecewise(arms) => ExpressionKind::Piecewise(
            arms.iter()
                .map(|(condition, result)| {
                    (
                        substitute_rule_target_data_paths_arc(
                            Arc::clone(condition),
                            rule_target_data,
                            completed_rules,
                        ),
                        substitute_rule_target_data_paths_arc(
                            Arc::clone(result),
                            rule_target_data,
                            completed_rules,
                        ),
                    )
                })
                .collect(),
        ),
    };
    Arc::new(Expression::with_source(kind, source))
}

pub(crate) fn follow_data_reference_to_rule_target(
    data: &IndexMap<DataPath, DataDefinition>,
    start: &DataPath,
) -> Option<RulePath> {
    let mut visited: HashSet<DataPath> = HashSet::new();
    let mut cursor = start.clone();
    loop {
        if !visited.insert(cursor.clone()) {
            return None;
        }
        let Some(DataDefinition::Reference { target, .. }) = data.get(&cursor) else {
            return None;
        };
        match target {
            ReferenceTarget::Data(next) => cursor = next.clone(),
            ReferenceTarget::Rule(rule_path) => return Some(rule_path.clone()),
        }
    }
}

fn build_in_plan_rule_target_data_map(
    data: &IndexMap<DataPath, DataDefinition>,
) -> HashMap<DataPath, RulePath> {
    let mut out: HashMap<DataPath, RulePath> = HashMap::new();
    for (path, definition) in data {
        if !matches!(definition, DataDefinition::Reference { .. }) {
            continue;
        }
        if let Some(rule_path) = follow_data_reference_to_rule_target(data, path) {
            out.insert(path.clone(), rule_path);
        }
    }
    out
}

/// Compiler invariant: after transitive substitution, the inlined expression
/// must not contain a data path that resolves to an in-plan rule target.
/// A leftover means the substitution pass missed it — an engine bug.
fn validate_no_rule_target_data_paths(
    expr: &Expression,
    data: &IndexMap<DataPath, DataDefinition>,
) {
    let mut remaining: HashSet<DataPath> = HashSet::new();
    expr.collect_data_paths(&mut remaining);
    for data_path in remaining {
        if resolves_to_in_plan_rule_target(data, &data_path) {
            panic!(
                "BUG: data reference '{}' was not fully inlined into the normalized expression",
                data_path
            );
        }
    }
}

fn resolves_to_in_plan_rule_target(
    data: &IndexMap<DataPath, DataDefinition>,
    data_path: &DataPath,
) -> bool {
    follow_data_reference_to_rule_target(data, data_path).is_some()
}

/// Compiler invariant: after transitive substitution, the inlined expression
/// must not reference a rule that belongs to this plan. Rules compile in
/// topological order, so every in-plan rule reference must have been
/// substituted with the referenced rule's completed expression — a leftover
/// is an engine bug.
fn validate_no_in_plan_rule_paths(expr: &Expression, plan_rule_paths: &HashSet<RulePath>) {
    let mut remaining = HashSet::new();
    expr.collect_rule_paths(&mut remaining);
    for path in remaining {
        if plan_rule_paths.contains(&path) {
            panic!(
                "BUG: rule reference '{}' was not fully inlined into the normalized expression",
                path.rule
            );
        }
    }
}

fn literal_bool_expression(value: bool, source: Option<Source>) -> Expression {
    Expression::with_source(
        ExpressionKind::Literal(Box::new(LiteralValue::from_bool(value))),
        source,
    )
}

#[derive(Clone, Debug, PartialEq)]
enum NormalForm {
    Leaf(LeafKind),
    Sum(Vec<NormalForm>),
    Product(Vec<NormalForm>),
    Subtract(Arc<NormalForm>, Arc<NormalForm>),
    Divide(Arc<NormalForm>, Arc<NormalForm>),
    Power(Arc<NormalForm>, Arc<NormalForm>),
    Modulo(Arc<NormalForm>, Arc<NormalForm>),
    Negate(Arc<NormalForm>),
    Reciprocal(Arc<NormalForm>),
    Comparison(Arc<NormalForm>, ComparisonComputation, Arc<NormalForm>),
    And(Vec<NormalForm>),
    Or(Vec<NormalForm>),
    Not(Arc<NormalForm>),
    MathOp(MathematicalComputation, Arc<NormalForm>),
    UnitConversion(Arc<NormalForm>, SemanticConversionTarget, Option<Source>),
    Veto(VetoExpression),
    DateRelative(DateRelativeKind, Arc<NormalForm>),
    DateCalendar(DateCalendarKind, CalendarPeriodUnit, Arc<NormalForm>),
    RangeLiteral(Arc<NormalForm>, Arc<NormalForm>),
    PastFutureRange(DateRelativeKind, Arc<NormalForm>),
    RangeContainment(Arc<NormalForm>, Arc<NormalForm>),
    ResultIsVeto(Arc<NormalForm>),
    Now,
    Piecewise(Vec<(Arc<NormalForm>, Arc<NormalForm>)>),
}

#[derive(Clone, Debug, PartialEq)]
enum LeafKind {
    Literal(Arc<LiteralValue>),
    DataPath(crate::planning::semantics::DataPath),
    RulePath(crate::planning::semantics::RulePath),
}

fn normalize_once(
    nf: NormalForm,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalForm, Error> {
    let nf = normalize_children(nf, unit_ctx, source.clone())?;
    simplify(nf, unit_ctx, source)
}

fn normalize_children(
    nf: NormalForm,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalForm, Error> {
    let normalize_vec = |children: Vec<NormalForm>| -> Result<Vec<NormalForm>, Error> {
        children
            .into_iter()
            .map(|child| normalize_once(child, unit_ctx, source.clone()))
            .collect()
    };
    match nf {
        NormalForm::Sum(children) => Ok(NormalForm::Sum(normalize_vec(children)?)),
        NormalForm::Product(children) => Ok(NormalForm::Product(normalize_vec(children)?)),
        NormalForm::Subtract(a, b) => Ok(NormalForm::Subtract(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Divide(a, b) => Ok(NormalForm::Divide(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Power(a, b) => Ok(NormalForm::Power(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Modulo(a, b) => Ok(NormalForm::Modulo(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Negate(x) => Ok(NormalForm::Negate(Arc::new(normalize_once(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::Reciprocal(x) => Ok(NormalForm::Reciprocal(Arc::new(normalize_once(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::Comparison(a, op, b) => Ok(NormalForm::Comparison(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            op,
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::And(children) => Ok(NormalForm::And(normalize_vec(children)?)),
        NormalForm::Or(children) => Ok(NormalForm::Or(normalize_vec(children)?)),
        NormalForm::Not(x) => Ok(NormalForm::Not(Arc::new(normalize_once(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::MathOp(op, x) => Ok(NormalForm::MathOp(
            op,
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::UnitConversion(x, target, origin) => Ok(NormalForm::UnitConversion(
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
            target,
            origin,
        )),
        NormalForm::DateRelative(kind, x) => Ok(NormalForm::DateRelative(
            kind,
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::DateCalendar(kind, unit, x) => Ok(NormalForm::DateCalendar(
            kind,
            unit,
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::RangeLiteral(a, b) => Ok(NormalForm::RangeLiteral(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::PastFutureRange(kind, x) => Ok(NormalForm::PastFutureRange(
            kind,
            Arc::new(normalize_once((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::RangeContainment(a, b) => Ok(NormalForm::RangeContainment(
            Arc::new(normalize_once((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(normalize_once((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::ResultIsVeto(operand) => Ok(NormalForm::ResultIsVeto(Arc::new(
            normalize_once((*operand).clone(), unit_ctx, source)?,
        ))),
        NormalForm::Piecewise(arms) => {
            let mut normalized_arms = Vec::with_capacity(arms.len());
            for (condition, result) in arms {
                normalized_arms.push((
                    Arc::new(normalize_once(
                        (*condition).clone(),
                        unit_ctx,
                        source.clone(),
                    )?),
                    Arc::new(normalize_once((*result).clone(), unit_ctx, source.clone())?),
                ));
            }
            Ok(NormalForm::Piecewise(normalized_arms))
        }
        leaf @ (NormalForm::Leaf(_) | NormalForm::Veto(_) | NormalForm::Now) => Ok(leaf),
    }
}

fn simplify(
    nf: NormalForm,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalForm, Error> {
    let nf = expand_numeric_subtract_divide(nf);
    let nf = flatten_associative(nf);
    let nf = eliminate_identities(nf);
    let nf = double_negate_reciprocal(nf);
    let nf = power_laws(nf);
    let nf = constant_fold(nf, source.clone())?;
    let nf = fold_unit_literals(nf, unit_ctx, source.clone())?;
    let nf = demorgan(nf);
    let nf = logical_flatten(nf);
    let nf = logical_short_circuit(nf);
    let nf = logical_idempotency(nf);
    let nf = negated_comparisons(nf);
    let nf = math_identities(nf);
    Ok(canonical_order(nf))
}

fn fold_unit_literals(
    nf: NormalForm,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
    source: Option<Source>,
) -> Result<NormalForm, Error> {
    let fold_vec = |children: Vec<NormalForm>| -> Result<Vec<NormalForm>, Error> {
        children
            .into_iter()
            .map(|child| fold_unit_literals(child, unit_ctx, source.clone()))
            .collect()
    };
    match nf {
        NormalForm::Sum(children) => Ok(NormalForm::Sum(fold_vec(children)?)),
        NormalForm::Product(children) => Ok(NormalForm::Product(fold_vec(children)?)),
        NormalForm::Subtract(a, b) => Ok(NormalForm::Subtract(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Divide(a, b) => Ok(NormalForm::Divide(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Power(a, b) => Ok(NormalForm::Power(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Modulo(a, b) => Ok(NormalForm::Modulo(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::Negate(x) => Ok(NormalForm::Negate(Arc::new(fold_unit_literals(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::Reciprocal(x) => Ok(NormalForm::Reciprocal(Arc::new(fold_unit_literals(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::Comparison(a, op, b) => Ok(NormalForm::Comparison(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            op,
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source)?),
        )),
        NormalForm::And(children) => Ok(NormalForm::And(fold_vec(children)?)),
        NormalForm::Or(children) => Ok(NormalForm::Or(fold_vec(children)?)),
        NormalForm::Not(x) => Ok(NormalForm::Not(Arc::new(fold_unit_literals(
            (*x).clone(),
            unit_ctx,
            source.clone(),
        )?))),
        NormalForm::MathOp(op, x) => Ok(NormalForm::MathOp(
            op,
            Arc::new(fold_unit_literals((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::UnitConversion(inner, target, origin) => {
            let inner_done = fold_unit_literals((*inner).clone(), unit_ctx, source.clone())?;
            if let (Some(_unit_context), NormalForm::Leaf(LeafKind::Literal(literal))) =
                (unit_ctx, &inner_done)
            {
                if let (
                    ValueKind::Number(number),
                    SemanticConversionTarget::Type(PrimitiveKind::Number),
                ) = (&literal.value, &target)
                {
                    return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                        LiteralValue::number_with_type(
                            number.clone(),
                            primitive_number_arc().clone(),
                        ),
                    ))));
                }
            }
            Ok(NormalForm::UnitConversion(
                Arc::new(inner_done),
                target,
                origin,
            ))
        }
        NormalForm::DateRelative(kind, x) => Ok(NormalForm::DateRelative(
            kind,
            Arc::new(fold_unit_literals((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::DateCalendar(kind, unit, x) => Ok(NormalForm::DateCalendar(
            kind,
            unit,
            Arc::new(fold_unit_literals((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::RangeLiteral(a, b) => Ok(NormalForm::RangeLiteral(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source)?),
        )),
        NormalForm::PastFutureRange(kind, x) => Ok(NormalForm::PastFutureRange(
            kind,
            Arc::new(fold_unit_literals((*x).clone(), unit_ctx, source.clone())?),
        )),
        NormalForm::RangeContainment(a, b) => Ok(NormalForm::RangeContainment(
            Arc::new(fold_unit_literals((*a).clone(), unit_ctx, source.clone())?),
            Arc::new(fold_unit_literals((*b).clone(), unit_ctx, source)?),
        )),
        NormalForm::Piecewise(arms) => {
            let mut folded_arms = Vec::with_capacity(arms.len());
            for (condition, result) in arms {
                folded_arms.push((
                    Arc::new(fold_unit_literals(
                        (*condition).clone(),
                        unit_ctx,
                        source.clone(),
                    )?),
                    Arc::new(fold_unit_literals(
                        (*result).clone(),
                        unit_ctx,
                        source.clone(),
                    )?),
                ));
            }
            Ok(NormalForm::Piecewise(folded_arms))
        }
        NormalForm::Leaf(LeafKind::Literal(literal)) => {
            if let Some(expanded) = expand_named_quantity_literal(literal.as_ref(), unit_ctx) {
                Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(expanded))))
            } else {
                Ok(NormalForm::Leaf(LeafKind::Literal(literal)))
            }
        }
        leaf @ (NormalForm::Leaf(_)
        | NormalForm::Veto(_)
        | NormalForm::ResultIsVeto(_)
        | NormalForm::Now) => Ok(leaf),
    }
}

pub(crate) fn expand_named_quantity_literal(
    literal: &LiteralValue,
    unit_ctx: Option<&UnitResolutionContext<'_>>,
) -> Option<LiteralValue> {
    let UnitResolutionContext::WithIndex(unit_index) = unit_ctx? else {
        return None;
    };
    let ValueKind::Quantity(magnitude, signature) = &literal.value else {
        return None;
    };
    if signature.len() != 1 || signature[0].1 != 1 {
        return None;
    }
    let unit_name = &signature[0].0;
    let owning_type = unit_index.get(unit_name)?;
    let TypeSpecification::Quantity { units, .. } = &owning_type.specifications else {
        return None;
    };
    let unit = units.get(unit_name).ok()?;
    if unit.derived_quantity_factors.is_empty() {
        return None;
    }
    let expanded =
        crate::computation::arithmetic::expand_signature_to_base_units(signature, unit_index);
    Some(LiteralValue::quantity_with_signature(
        magnitude.clone(),
        expanded,
        Arc::clone(&literal.lemma_type),
    ))
}

fn to_normal_form(expr: &Expression) -> NormalForm {
    match &expr.kind {
        ExpressionKind::Literal(lit) => {
            NormalForm::Leaf(LeafKind::Literal(Arc::new((**lit).clone())))
        }
        ExpressionKind::DataPath(p) => NormalForm::Leaf(LeafKind::DataPath(p.clone())),
        ExpressionKind::RulePath(p) => NormalForm::Leaf(LeafKind::RulePath(p.clone())),
        ExpressionKind::LogicalAnd(left, right) => {
            NormalForm::And(vec![to_normal_form(left), to_normal_form(right)])
        }
        ExpressionKind::LogicalOr(left, right) => {
            NormalForm::Or(vec![to_normal_form(left), to_normal_form(right)])
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Subtract, right) => {
            NormalForm::Subtract(
                Arc::new(to_normal_form(left)),
                Arc::new(to_normal_form(right)),
            )
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Divide, right) => {
            NormalForm::Divide(
                Arc::new(to_normal_form(left)),
                Arc::new(to_normal_form(right)),
            )
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Add, right) => {
            NormalForm::Sum(vec![to_normal_form(left), to_normal_form(right)])
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Multiply, right) => {
            NormalForm::Product(vec![to_normal_form(left), to_normal_form(right)])
        }
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Power, right) => NormalForm::Power(
            Arc::new(to_normal_form(left)),
            Arc::new(to_normal_form(right)),
        ),
        ExpressionKind::Arithmetic(left, ArithmeticComputation::Modulo, right) => {
            NormalForm::Modulo(
                Arc::new(to_normal_form(left)),
                Arc::new(to_normal_form(right)),
            )
        }
        ExpressionKind::Comparison(left, op, right) => NormalForm::Comparison(
            Arc::new(to_normal_form(left)),
            op.clone(),
            Arc::new(to_normal_form(right)),
        ),
        ExpressionKind::UnitConversion(inner, target) => NormalForm::UnitConversion(
            Arc::new(to_normal_form(inner)),
            target.clone(),
            expr.source_location.clone(),
        ),
        ExpressionKind::LogicalNegation(inner, _) => {
            NormalForm::Not(Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::MathematicalComputation(op, inner) => {
            NormalForm::MathOp(op.clone(), Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::Veto(v) => NormalForm::Veto(v.clone()),
        ExpressionKind::Now => NormalForm::Now,
        ExpressionKind::DateRelative(kind, inner) => {
            NormalForm::DateRelative(*kind, Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::DateCalendar(kind, unit, inner) => {
            NormalForm::DateCalendar(*kind, *unit, Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::RangeLiteral(left, right) => NormalForm::RangeLiteral(
            Arc::new(to_normal_form(left)),
            Arc::new(to_normal_form(right)),
        ),
        ExpressionKind::PastFutureRange(kind, inner) => {
            NormalForm::PastFutureRange(*kind, Arc::new(to_normal_form(inner)))
        }
        ExpressionKind::RangeContainment(left, right) => NormalForm::RangeContainment(
            Arc::new(to_normal_form(left)),
            Arc::new(to_normal_form(right)),
        ),
        ExpressionKind::ResultIsVeto(operand) => {
            NormalForm::ResultIsVeto(Arc::new(to_normal_form(operand)))
        }
        ExpressionKind::Piecewise(arms) => NormalForm::Piecewise(
            arms.iter()
                .map(|(condition, result)| {
                    (
                        Arc::new(to_normal_form(condition)),
                        Arc::new(to_normal_form(result)),
                    )
                })
                .collect(),
        ),
    }
}

#[cfg(test)]
use crate::planning::semantics::NegationType;

#[cfg(test)]
fn to_expression(nf: &NormalForm, source: Option<Source>) -> Expression {
    let kind = nf_to_kind(nf, source.clone());
    Expression::with_source(kind, source)
}

#[cfg(test)]
fn nf_to_kind(nf: &NormalForm, source: Option<Source>) -> ExpressionKind {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(lit)) => {
            ExpressionKind::Literal(Box::new((**lit).clone()))
        }
        NormalForm::Leaf(LeafKind::DataPath(p)) => ExpressionKind::DataPath(p.clone()),
        NormalForm::Leaf(LeafKind::RulePath(p)) => ExpressionKind::RulePath(p.clone()),
        NormalForm::Sum(children) => sum_to_kind(children, source),
        NormalForm::Product(children) => product_to_kind(children, source),
        NormalForm::Subtract(a, b) => ExpressionKind::Arithmetic(
            Arc::new(to_expression(a, source.clone())),
            ArithmeticComputation::Subtract,
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::Divide(a, b) => ExpressionKind::Arithmetic(
            Arc::new(to_expression(a, source.clone())),
            ArithmeticComputation::Divide,
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::Power(base, exp) => ExpressionKind::Arithmetic(
            Arc::new(to_expression(base, source.clone())),
            ArithmeticComputation::Power,
            Arc::new(to_expression(exp, source.clone())),
        ),
        NormalForm::Modulo(a, b) => ExpressionKind::Arithmetic(
            Arc::new(to_expression(a, source.clone())),
            ArithmeticComputation::Modulo,
            Arc::new(to_expression(b, source.clone())),
        ),
        NormalForm::Negate(x) => {
            let zero = Expression::with_source(
                ExpressionKind::Literal(Box::new(LiteralValue::number(rational_zero()))),
                source.clone(),
            );
            ExpressionKind::Arithmetic(
                Arc::new(zero),
                ArithmeticComputation::Subtract,
                Arc::new(to_expression(x, source)),
            )
        }
        NormalForm::Reciprocal(x) => {
            let one = Expression::with_source(
                ExpressionKind::Literal(Box::new(LiteralValue::number(rational_one()))),
                source.clone(),
            );
            ExpressionKind::Arithmetic(
                Arc::new(one),
                ArithmeticComputation::Divide,
                Arc::new(to_expression(x, source)),
            )
        }
        NormalForm::Comparison(a, op, b) => ExpressionKind::Comparison(
            Arc::new(to_expression(a, source.clone())),
            op.clone(),
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::And(children) => and_to_kind(children, source),
        NormalForm::Or(children) => or_to_kind(children, source),
        NormalForm::Not(x) => {
            ExpressionKind::LogicalNegation(Arc::new(to_expression(x, source)), NegationType::Not)
        }
        NormalForm::MathOp(op, x) => {
            ExpressionKind::MathematicalComputation(op.clone(), Arc::new(to_expression(x, source)))
        }
        NormalForm::UnitConversion(x, target, _) => {
            ExpressionKind::UnitConversion(Arc::new(to_expression(x, source)), target.clone())
        }
        NormalForm::Veto(v) => ExpressionKind::Veto(v.clone()),
        NormalForm::Now => ExpressionKind::Now,
        NormalForm::DateRelative(kind, x) => {
            ExpressionKind::DateRelative(*kind, Arc::new(to_expression(x, source)))
        }
        NormalForm::DateCalendar(kind, unit, x) => {
            ExpressionKind::DateCalendar(*kind, *unit, Arc::new(to_expression(x, source)))
        }
        NormalForm::RangeLiteral(a, b) => ExpressionKind::RangeLiteral(
            Arc::new(to_expression(a, source.clone())),
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::PastFutureRange(kind, x) => {
            ExpressionKind::PastFutureRange(*kind, Arc::new(to_expression(x, source)))
        }
        NormalForm::RangeContainment(a, b) => ExpressionKind::RangeContainment(
            Arc::new(to_expression(a, source.clone())),
            Arc::new(to_expression(b, source)),
        ),
        NormalForm::ResultIsVeto(operand) => {
            ExpressionKind::ResultIsVeto(Arc::new(to_expression(operand, source)))
        }
        NormalForm::Piecewise(arms) => ExpressionKind::Piecewise(
            arms.iter()
                .map(|(condition, result)| {
                    (
                        Arc::new(to_expression(condition, source.clone())),
                        Arc::new(to_expression(result, source.clone())),
                    )
                })
                .collect(),
        ),
    }
}

#[cfg(test)]
fn sum_to_kind(children: &[NormalForm], source: Option<Source>) -> ExpressionKind {
    match children {
        [] => ExpressionKind::Literal(Box::new(LiteralValue::number(rational_zero()))),
        [one] => nf_to_kind(one, source),
        [first, rest @ ..] => {
            let mut acc =
                Expression::with_source(nf_to_kind(first, source.clone()), source.clone());
            for c in rest {
                acc = Expression::with_source(
                    ExpressionKind::Arithmetic(
                        Arc::new(acc),
                        ArithmeticComputation::Add,
                        Arc::new(to_expression(c, source.clone())),
                    ),
                    source.clone(),
                );
            }
            acc.kind
        }
    }
}

#[cfg(test)]
fn product_to_kind(children: &[NormalForm], source: Option<Source>) -> ExpressionKind {
    match children {
        [] => ExpressionKind::Literal(Box::new(LiteralValue::number(rational_one()))),
        [one] => nf_to_kind(one, source),
        [first, rest @ ..] => {
            let mut acc =
                Expression::with_source(nf_to_kind(first, source.clone()), source.clone());
            for c in rest {
                acc = Expression::with_source(
                    ExpressionKind::Arithmetic(
                        Arc::new(acc),
                        ArithmeticComputation::Multiply,
                        Arc::new(to_expression(c, source.clone())),
                    ),
                    source.clone(),
                );
            }
            acc.kind
        }
    }
}

#[cfg(test)]
fn and_to_kind(children: &[NormalForm], source: Option<Source>) -> ExpressionKind {
    match children {
        [] => ExpressionKind::Literal(Box::new(LiteralValue::from_bool(true))),
        [one] => nf_to_kind(one, source),
        [first, rest @ ..] => {
            let mut acc =
                Expression::with_source(nf_to_kind(first, source.clone()), source.clone());
            for c in rest {
                acc = Expression::with_source(
                    ExpressionKind::LogicalAnd(
                        Arc::new(acc),
                        Arc::new(to_expression(c, source.clone())),
                    ),
                    source.clone(),
                );
            }
            acc.kind
        }
    }
}

#[cfg(test)]
fn or_to_kind(children: &[NormalForm], source: Option<Source>) -> ExpressionKind {
    match children {
        [] => ExpressionKind::Literal(Box::new(LiteralValue::from_bool(false))),
        [one] => nf_to_kind(one, source),
        [first, rest @ ..] => {
            let mut acc =
                Expression::with_source(nf_to_kind(first, source.clone()), source.clone());
            for c in rest {
                acc = Expression::with_source(
                    ExpressionKind::LogicalOr(
                        Arc::new(acc),
                        Arc::new(to_expression(c, source.clone())),
                    ),
                    source.clone(),
                );
            }
            acc.kind
        }
    }
}

fn expand_numeric_subtract_divide(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Subtract(a, b) if is_numeric_only(&a) && is_numeric_only(&b) => {
            NormalForm::Sum(vec![(*a).clone(), NormalForm::Negate(b)])
        }
        NormalForm::Divide(a, b) if is_numeric_only(&a) && is_numeric_only(&b) => {
            NormalForm::Product(vec![(*a).clone(), NormalForm::Reciprocal(b)])
        }
        NormalForm::Sum(children) => NormalForm::Sum(
            children
                .into_iter()
                .map(expand_numeric_subtract_divide)
                .collect(),
        ),
        NormalForm::Product(children) => NormalForm::Product(
            children
                .into_iter()
                .map(expand_numeric_subtract_divide)
                .collect(),
        ),
        NormalForm::Subtract(a, b) => NormalForm::Subtract(
            Arc::new(expand_numeric_subtract_divide((*a).clone())),
            Arc::new(expand_numeric_subtract_divide((*b).clone())),
        ),
        NormalForm::Divide(a, b) => NormalForm::Divide(
            Arc::new(expand_numeric_subtract_divide((*a).clone())),
            Arc::new(expand_numeric_subtract_divide((*b).clone())),
        ),
        NormalForm::Power(a, b) => NormalForm::Power(
            Arc::new(expand_numeric_subtract_divide((*a).clone())),
            Arc::new(expand_numeric_subtract_divide((*b).clone())),
        ),
        NormalForm::Negate(x) => {
            NormalForm::Negate(Arc::new(expand_numeric_subtract_divide((*x).clone())))
        }
        NormalForm::Reciprocal(x) => {
            NormalForm::Reciprocal(Arc::new(expand_numeric_subtract_divide((*x).clone())))
        }
        NormalForm::And(children) => NormalForm::And(
            children
                .into_iter()
                .map(expand_numeric_subtract_divide)
                .collect(),
        ),
        NormalForm::Or(children) => NormalForm::Or(
            children
                .into_iter()
                .map(expand_numeric_subtract_divide)
                .collect(),
        ),
        NormalForm::Not(x) => {
            NormalForm::Not(Arc::new(expand_numeric_subtract_divide((*x).clone())))
        }
        NormalForm::Comparison(a, op, b) => NormalForm::Comparison(
            Arc::new(expand_numeric_subtract_divide((*a).clone())),
            op,
            Arc::new(expand_numeric_subtract_divide((*b).clone())),
        ),
        NormalForm::MathOp(op, x) => {
            NormalForm::MathOp(op, Arc::new(expand_numeric_subtract_divide((*x).clone())))
        }
        NormalForm::UnitConversion(x, target, origin) => NormalForm::UnitConversion(
            Arc::new(expand_numeric_subtract_divide((*x).clone())),
            target,
            origin,
        ),
        other => other,
    }
}

fn is_numeric_only(nf: &NormalForm) -> bool {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(lit)) => {
            matches!(lit.value, crate::planning::semantics::ValueKind::Number(_))
        }
        NormalForm::Sum(children) | NormalForm::Product(children) => {
            children.iter().all(is_numeric_only)
        }
        NormalForm::Subtract(a, b) | NormalForm::Divide(a, b) => {
            is_numeric_only(a) && is_numeric_only(b)
        }
        NormalForm::Power(a, b) => is_numeric_only(a) && is_numeric_only(b),
        NormalForm::Negate(x) | NormalForm::Reciprocal(x) => is_numeric_only(x),
        _ => false,
    }
}

fn flatten_associative(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Sum(children) => {
            let mut flat = Vec::new();
            for c in children {
                match flatten_associative(c) {
                    NormalForm::Sum(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }
            NormalForm::Sum(flat)
        }
        NormalForm::Product(children) => {
            let mut flat = Vec::new();
            for c in children {
                match flatten_associative(c) {
                    NormalForm::Product(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }
            NormalForm::Product(flat)
        }
        // Do not flatten nested And: guard chains are `And(cond, val)`; merging would yield
        // `And(And(cond, val), …)` rebuilds as left-assoc and leaves a non-boolean on the
        // left of an outer `LogicalAnd` (invalid for evaluator / unless semantics).
        NormalForm::And(children) => {
            NormalForm::And(children.into_iter().map(flatten_associative).collect())
        }
        NormalForm::Or(children) => {
            let mut flat = Vec::new();
            for c in children {
                match flatten_associative(c) {
                    NormalForm::Or(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }
            NormalForm::Or(flat)
        }
        other => other,
    }
}

fn eliminate_identities(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Sum(children) => {
            let children: Vec<_> = children
                .into_iter()
                .filter_map(|c| {
                    if is_numeric_zero(&c) {
                        None
                    } else {
                        Some(eliminate_identities(c))
                    }
                })
                .collect();
            match children.len() {
                0 => NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_zero(),
                )))),
                1 => children.into_iter().next().expect("BUG: len 1"),
                _ => NormalForm::Sum(children),
            }
        }
        NormalForm::Product(children) => {
            // The absorbing fold deletes every sibling of the zero, so it is
            // only semantics-preserving when every sibling is total (cannot
            // veto) and the replacement zero carries the product's unit
            // signature. Otherwise the product stays and the runtime multiply
            // propagates operand vetoes and units.
            if children.iter().any(is_numeric_zero) {
                if let Some(zero) = typed_product_zero(&children) {
                    return zero;
                }
            }
            let children: Vec<_> = children
                .into_iter()
                .filter_map(|c| {
                    if is_numeric_one(&c) {
                        None
                    } else {
                        Some(eliminate_identities(c))
                    }
                })
                .collect();
            match children.len() {
                0 => NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_one(),
                )))),
                1 => children.into_iter().next().expect("BUG: len 1"),
                _ => NormalForm::Product(children),
            }
        }
        NormalForm::Power(base, exp) => {
            let base = Arc::new(eliminate_identities((*base).clone()));
            let exp = Arc::new(eliminate_identities((*exp).clone()));
            if is_numeric_one(&exp) {
                return (*base).clone();
            }
            // `x ^ 0 → 1` deletes the base: gated on the base being a total,
            // nonzero plain number. Non-total bases stay for the runtime
            // (veto propagation); literal `0 ^ 0` is left for `constant_fold`
            // so the engine has a single source of truth for its outcome.
            if is_numeric_zero(&exp)
                && as_rational_literal(&base).is_some_and(|rational| !rational_is_zero(&rational))
            {
                return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_one(),
                ))));
            }
            NormalForm::Power(base, exp)
        }
        NormalForm::Negate(x) => {
            let inner = eliminate_identities((*x).clone());
            if is_numeric_zero(&inner) {
                return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
                    rational_zero(),
                ))));
            }
            NormalForm::Negate(Arc::new(inner))
        }
        NormalForm::Subtract(a, b) => {
            let a = eliminate_identities((*a).clone());
            let b = eliminate_identities((*b).clone());
            if is_numeric_zero(&a) {
                return NormalForm::Negate(Arc::new(b));
            }
            if is_numeric_zero(&b) {
                return a;
            }
            NormalForm::Subtract(Arc::new(a), Arc::new(b))
        }
        NormalForm::Divide(a, b) => {
            let a = eliminate_identities((*a).clone());
            let b = eliminate_identities((*b).clone());
            if is_numeric_one(&b) {
                return a;
            }
            if is_numeric_one(&a) {
                return NormalForm::Reciprocal(Arc::new(b));
            }
            NormalForm::Divide(Arc::new(a), Arc::new(b))
        }
        NormalForm::And(children) => {
            // `… and false → false` deletes every sibling: gated on all
            // children being total. Otherwise a sibling's veto (for example
            // missing data) would be erased.
            if children.iter().any(|child| is_literal_bool(child, false))
                && children.iter().all(is_total)
            {
                return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                    false,
                ))));
            }
            // Dropping literal `true` conjuncts is safe in any position: AND
            // propagates vetoes from every conjunct, so positions carry no
            // special semantics.
            let children: Vec<_> = children
                .into_iter()
                .filter_map(|child| {
                    if is_literal_bool(&child, true) {
                        None
                    } else {
                        Some(eliminate_identities(child))
                    }
                })
                .collect();
            match children.len() {
                0 => NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(true)))),
                1 => children.into_iter().next().expect("BUG: len 1"),
                _ => NormalForm::And(children),
            }
        }
        NormalForm::Or(children) => {
            // `… or true → true` deletes every sibling: gated on all children
            // being total, mirroring the AND case.
            if children.iter().any(|child| is_literal_bool(child, true))
                && children.iter().all(is_total)
            {
                return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                    true,
                ))));
            }
            // Dropping a literal `false` disjunct is only safe in non-last
            // positions: the last disjunct is the one whose veto propagates
            // (earlier non-missing-data vetoes are treated as non-match), so
            // removing the last child would change its predecessor's veto
            // behavior.
            let last_index = children.len().saturating_sub(1);
            let children: Vec<_> = children
                .into_iter()
                .enumerate()
                .filter_map(|(index, child)| {
                    if index != last_index && is_literal_bool(&child, false) {
                        None
                    } else {
                        Some(eliminate_identities(child))
                    }
                })
                .collect();
            match children.len() {
                0 => unreachable!("BUG: Or identity elimination always keeps the last disjunct"),
                1 => children.into_iter().next().expect("BUG: len 1"),
                _ => NormalForm::Or(children),
            }
        }
        NormalForm::Not(inner) => {
            let inner = eliminate_identities((*inner).clone());
            if is_literal_bool(&inner, true) {
                return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                    false,
                ))));
            }
            if is_literal_bool(&inner, false) {
                return NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::from_bool(
                    true,
                ))));
            }
            NormalForm::Not(Arc::new(inner))
        }
        other => other,
    }
}

/// For a product containing a numeric zero child where every child is a
/// literal: the correctly typed zero the runtime multiply would produce.
///
/// `None` when folding is not semantics-preserving: any non-literal child
/// (its veto would be erased), more than one quantity literal (combining
/// signatures must reproduce the runtime's decomposition and signature-index
/// logic, so it is left to the runtime multiply), or a non-numeric literal
/// child (a type error planning reports elsewhere).
fn typed_product_zero(children: &[NormalForm]) -> Option<NormalForm> {
    let mut quantity_literal: Option<&LiteralValue> = None;
    for child in children {
        let NormalForm::Leaf(LeafKind::Literal(literal)) = child else {
            return None;
        };
        match &literal.value {
            ValueKind::Number(_) => {}
            ValueKind::Quantity(_, _) => {
                if quantity_literal.is_some() {
                    return None;
                }
                quantity_literal = Some(literal.as_ref());
            }
            _ => return None,
        }
    }
    match quantity_literal {
        None => Some(NormalForm::Leaf(LeafKind::Literal(Arc::new(
            LiteralValue::number(rational_zero()),
        )))),
        Some(literal) => {
            let ValueKind::Quantity(_, signature) = &literal.value else {
                unreachable!("BUG: quantity literal collected above must carry a quantity value");
            };
            // Mirrors the runtime number-times-quantity multiply: the
            // quantity's signature and type are preserved on the result.
            Some(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                LiteralValue::quantity_with_signature(
                    rational_zero(),
                    signature.clone(),
                    literal.lemma_type.clone(),
                ),
            ))))
        }
    }
}

fn is_literal_bool(nf: &NormalForm, expected: bool) -> bool {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(literal)) => {
            matches!(literal.value, ValueKind::Boolean(value) if value == expected)
        }
        _ => false,
    }
}

fn double_negate_reciprocal(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Negate(x) => match (*x).clone() {
            NormalForm::Negate(y) => (*y).clone(),
            other => NormalForm::Negate(Arc::new(double_negate_reciprocal(other))),
        },
        NormalForm::Reciprocal(x) => match (*x).clone() {
            // `1 / (1 / y) → y` erases the inner division-by-zero veto when
            // `y` is zero at runtime, so the collapse is gated on a total
            // (literal) operand. Literal zero is left alone too: planning
            // reports literal division by zero elsewhere.
            NormalForm::Reciprocal(y) if is_total(&y) => (*y).clone(),
            other => NormalForm::Reciprocal(Arc::new(double_negate_reciprocal(other))),
        },
        NormalForm::Not(x) => match (*x).clone() {
            NormalForm::Not(y) => (*y).clone(),
            other => NormalForm::Not(Arc::new(double_negate_reciprocal(other))),
        },
        other => other,
    }
}

fn power_laws(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Power(base, exp) => {
            let base_simplified = power_laws((*base).clone());
            let exp_simplified = power_laws((*exp).clone());
            if let NormalForm::Power(inner_base, inner_exp) = &base_simplified {
                let merge_preserves_domain = is_total(inner_base)
                    || nested_power_merge_preserves_domain(inner_exp, &exp_simplified);
                if merge_preserves_domain {
                    if let Some(new_exp) = try_multiply_nf_rational(inner_exp, &exp_simplified) {
                        return NormalForm::Power(Arc::clone(inner_base), Arc::new(new_exp));
                    }
                }
            }
            NormalForm::Power(Arc::new(base_simplified), Arc::new(exp_simplified))
        }
        NormalForm::Product(children) => {
            let children: Vec<_> = children.into_iter().map(power_laws).collect();
            collect_like_base_powers(children)
        }
        other => other,
    }
}

/// Merging `(b ^ e1) ^ e2 → b ^ (e1 * e2)` preserves the runtime domain only
/// when both exponents are positive integers: fractional exponents restrict
/// the base domain (`b ^ (1/2)` requires a non-negative base) and negative
/// exponents veto on a zero base — `(b ^ -2) ^ -3 → b ^ 6` would erase the
/// zero-base veto because the merged exponent flips sign.
fn nested_power_merge_preserves_domain(
    inner_exponent: &NormalForm,
    outer_exponent: &NormalForm,
) -> bool {
    let Some(inner) = as_integer_literal(inner_exponent) else {
        return false;
    };
    let Some(outer) = as_integer_literal(outer_exponent) else {
        return false;
    };
    inner.numer().is_positive() && outer.numer().is_positive()
}

/// Merging `b ^ e1 * b ^ e2 → b ^ (e1 + e2)` preserves the runtime domain
/// only when both exponents are integers of the same sign: the sum of
/// same-sign integers keeps that sign, so the merged power has the same
/// zero-base and fractional-domain behavior as the separate factors.
fn like_base_merge_preserves_domain(
    left_exponent: &NormalForm,
    right_exponent: &NormalForm,
) -> bool {
    let Some(left) = as_integer_literal(left_exponent) else {
        return false;
    };
    let Some(right) = as_integer_literal(right_exponent) else {
        return false;
    };
    (left.numer().is_positive() && right.numer().is_positive())
        || (left.numer().is_negative() && right.numer().is_negative())
}

fn collect_like_base_powers(children: Vec<NormalForm>) -> NormalForm {
    let mut powers: Vec<(NormalForm, NormalForm)> = Vec::new();
    let mut other = Vec::new();
    for c in children {
        if let NormalForm::Power(base, exp) = c {
            let base_val = (*base).clone();
            if let Some((_, stored_exp)) = powers.iter_mut().find(|(b, _)| *b == base_val) {
                let merge_preserves_domain =
                    is_total(&base_val) || like_base_merge_preserves_domain(stored_exp, &exp);
                if merge_preserves_domain {
                    if let Some(sum_exp) = try_add_nf_rational(stored_exp, &exp) {
                        *stored_exp = sum_exp;
                        continue;
                    }
                }
            }
            powers.push((base_val, (*exp).clone()));
        } else {
            other.push(c);
        }
    }
    for (base, exp) in powers {
        other.push(NormalForm::Power(Arc::new(base), Arc::new(exp)));
    }
    match other.len() {
        0 => NormalForm::Leaf(LeafKind::Literal(Arc::new(LiteralValue::number(
            rational_one(),
        )))),
        1 => other.into_iter().next().expect("BUG: single product term"),
        _ => NormalForm::Product(other),
    }
}

fn try_multiply_nf_rational(a: &NormalForm, b: &NormalForm) -> Option<NormalForm> {
    let ra = as_rational_literal(a)?;
    let rb = as_rational_literal(b)?;
    let rational = planning_rational_operation(&ra, NumericOperation::Multiply, &rb).ok()?;
    let literal = literal_from_folded_rational(rational, None).ok()?;
    Some(NormalForm::Leaf(LeafKind::Literal(Arc::new(literal))))
}

fn try_add_nf_rational(a: &NormalForm, b: &NormalForm) -> Option<NormalForm> {
    let ra = as_rational_literal(a)?;
    let rb = as_rational_literal(b)?;
    let rational = planning_rational_operation(&ra, NumericOperation::Add, &rb).ok()?;
    let literal = literal_from_folded_rational(rational, None).ok()?;
    Some(NormalForm::Leaf(LeafKind::Literal(Arc::new(literal))))
}

fn constant_fold(nf: NormalForm, source: Option<Source>) -> Result<NormalForm, Error> {
    match nf {
        NormalForm::Sum(children) => {
            if children.iter().all(|c| as_rational_literal(c).is_some()) {
                let mut acc = rational_new(0, 1);
                for child in &children {
                    let rational = as_rational_literal(child).expect("BUG: all numeric");
                    acc = planning_rational_operation(&acc, NumericOperation::Add, &rational)
                        .map_err(|failure| {
                            normalization_error(source.clone(), failure, "constant fold sum")
                        })?;
                }
                return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                    literal_from_folded_rational(acc, source.clone())?,
                ))));
            }
            Ok(NormalForm::Sum(
                children
                    .into_iter()
                    .map(|child| constant_fold(child, source.clone()))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        NormalForm::Product(children) => {
            if children.iter().all(|c| as_rational_literal(c).is_some()) {
                let mut acc = rational_new(1, 1);
                for child in &children {
                    let rational = as_rational_literal(child).expect("BUG: all numeric");
                    acc = planning_rational_operation(&acc, NumericOperation::Multiply, &rational)
                        .map_err(|failure| {
                            normalization_error(source.clone(), failure, "constant fold product")
                        })?;
                }
                return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                    literal_from_folded_rational(acc, source.clone())?,
                ))));
            }
            Ok(NormalForm::Product(
                children
                    .into_iter()
                    .map(|child| constant_fold(child, source.clone()))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        NormalForm::Power(base, exp) => {
            let base = constant_fold((*base).clone(), source.clone())?;
            let exp = constant_fold((*exp).clone(), source.clone())?;
            if let (Some(base_rational), Some(exp_rational)) =
                (as_rational_literal(&base), as_rational_literal(&exp))
            {
                if let Ok(rational) = planning_rational_operation(
                    &base_rational,
                    NumericOperation::Power,
                    &exp_rational,
                ) {
                    return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(rational, source.clone())?,
                    ))));
                }
            }
            Ok(NormalForm::Power(Arc::new(base), Arc::new(exp)))
        }
        NormalForm::Negate(x) => {
            let inner = constant_fold((*x).clone(), source.clone())?;
            if let Some(rational) = as_rational_literal(&inner) {
                let zero = rational_new(0, 1);
                let negated =
                    planning_rational_operation(&zero, NumericOperation::Subtract, &rational)
                        .map_err(|failure| {
                            normalization_error(source.clone(), failure, "constant fold negate")
                        })?;
                return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                    literal_from_folded_rational(negated, source.clone())?,
                ))));
            }
            Ok(NormalForm::Negate(Arc::new(inner)))
        }
        NormalForm::Not(x) => {
            let inner = constant_fold((*x).clone(), source.clone())?;
            if let NormalForm::Leaf(LeafKind::Literal(literal)) = &inner {
                if let ValueKind::Boolean(boolean) = &literal.value {
                    return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                        LiteralValue::from_bool(!boolean),
                    ))));
                }
            }
            Ok(NormalForm::Not(Arc::new(inner)))
        }
        NormalForm::MathOp(op, x) => {
            let inner = constant_fold((*x).clone(), source.clone())?;
            if let Some(rational) = as_rational_literal(&inner) {
                if let Some(folded) = fold_math_op(&op, &rational) {
                    return Ok(NormalForm::Leaf(LeafKind::Literal(Arc::new(
                        literal_from_folded_rational(folded, source.clone())?,
                    ))));
                }
            }
            Ok(NormalForm::MathOp(op, Arc::new(inner)))
        }
        other => Ok(other),
    }
}

fn fold_math_op(
    op: &MathematicalComputation,
    operand: &RationalInteger,
) -> Option<RationalInteger> {
    let zero = rational_new(0, 1);
    let one = rational_new(1, 1);
    match op {
        MathematicalComputation::Sin if *operand == zero => Some(zero),
        MathematicalComputation::Cos if *operand == zero => Some(one),
        MathematicalComputation::Tan if *operand == zero => Some(zero),
        MathematicalComputation::Log if *operand == one => Some(zero),
        MathematicalComputation::Exp if *operand == zero => Some(one),
        _ => None,
    }
}

fn demorgan(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Not(x) => match (*x).clone() {
            // De Morgan is only veto-preserving for total children: compiled
            // AND propagates every conjunct's veto, while compiled OR treats
            // non-missing-data vetoes in non-last disjuncts as non-match.
            // Rewriting between them changes which vetoes are observable.
            NormalForm::And(children) if children.iter().all(is_total) => NormalForm::Or(
                children
                    .into_iter()
                    .map(|c| NormalForm::Not(Arc::new(c)))
                    .map(demorgan)
                    .collect(),
            ),
            NormalForm::Or(children) if children.iter().all(is_total) => NormalForm::And(
                children
                    .into_iter()
                    .map(|c| NormalForm::Not(Arc::new(c)))
                    .map(demorgan)
                    .collect(),
            ),
            NormalForm::Comparison(a, op, b) => {
                NormalForm::Comparison(a, negated_comparison(op), b)
            }
            other => NormalForm::Not(Arc::new(demorgan(other))),
        },
        other => other,
    }
}

fn logical_flatten(nf: NormalForm) -> NormalForm {
    flatten_associative(nf)
}

fn logical_short_circuit(nf: NormalForm) -> NormalForm {
    // Both folds delete every sibling of the deciding literal, so they are
    // gated on all children being total — otherwise a sibling's veto (for
    // example missing data) would be erased.
    match nf {
        NormalForm::And(children) => {
            let all_total = children.iter().all(is_total);
            if all_total {
                for c in &children {
                    if matches!(
                        c,
                        NormalForm::Leaf(LeafKind::Literal(l))
                            if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(false))
                    ) {
                        return NormalForm::Leaf(LeafKind::Literal(Arc::new(
                            LiteralValue::from_bool(false),
                        )));
                    }
                }
            }
            NormalForm::And(children)
        }
        NormalForm::Or(children) => {
            let all_total = children.iter().all(is_total);
            if all_total {
                for c in &children {
                    if matches!(
                        c,
                        NormalForm::Leaf(LeafKind::Literal(l))
                            if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(true))
                    ) {
                        return NormalForm::Leaf(LeafKind::Literal(Arc::new(
                            LiteralValue::from_bool(true),
                        )));
                    }
                }
            }
            NormalForm::Or(children)
        }
        other => other,
    }
}

fn logical_idempotency(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::And(children) => {
            // AND propagates vetoes from every conjunct, so removing a later
            // duplicate never changes the outcome.
            let mut unique = Vec::new();
            for c in children {
                if !unique.contains(&c) {
                    unique.push(c);
                }
            }
            NormalForm::And(unique)
        }
        NormalForm::Or(children) => {
            // OR gives the last disjunct special veto semantics (its veto
            // propagates; earlier non-missing-data vetoes are non-match), so
            // the last child is never removed and only total (literal)
            // duplicates are deduplicated.
            let last_index = children.len().saturating_sub(1);
            let mut unique: Vec<NormalForm> = Vec::new();
            for (index, child) in children.into_iter().enumerate() {
                let removable = index != last_index && is_total(&child) && unique.contains(&child);
                if !removable {
                    unique.push(child);
                }
            }
            NormalForm::Or(unique)
        }
        other => other,
    }
}

fn negated_comparisons(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Not(x) => match (*x).clone() {
            NormalForm::Comparison(a, op, b) => {
                NormalForm::Comparison(a, negated_comparison(op), b)
            }
            other => NormalForm::Not(Arc::new(negated_comparisons(other))),
        },
        other => other,
    }
}

fn math_identities(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::MathOp(MathematicalComputation::Abs, x) => match (*x).clone() {
            NormalForm::MathOp(MathematicalComputation::Abs, inner) => {
                NormalForm::MathOp(MathematicalComputation::Abs, inner)
            }
            other => NormalForm::MathOp(
                MathematicalComputation::Abs,
                Arc::new(math_identities(other)),
            ),
        },
        NormalForm::MathOp(MathematicalComputation::Exp, x) => match (*x).clone() {
            // `exp (log y) → y` erases the `log` domain veto for `y <= 0`,
            // so the collapse is gated on a total (literal) operand.
            NormalForm::MathOp(MathematicalComputation::Log, inner) if is_total(&inner) => {
                (*inner).clone()
            }
            other => NormalForm::MathOp(
                MathematicalComputation::Exp,
                Arc::new(math_identities(other)),
            ),
        },
        NormalForm::MathOp(MathematicalComputation::Log, x) => match (*x).clone() {
            // `log (exp y) → y` erases the `exp` overflow veto for very large
            // `y`, so the collapse is gated on a total (literal) operand.
            NormalForm::MathOp(MathematicalComputation::Exp, inner) if is_total(&inner) => {
                (*inner).clone()
            }
            other => NormalForm::MathOp(
                MathematicalComputation::Log,
                Arc::new(math_identities(other)),
            ),
        },
        NormalForm::MathOp(MathematicalComputation::Sqrt, x) => {
            let base = math_identities((*x).clone());
            let half = NormalForm::Leaf(LeafKind::Literal(Arc::new(
                literal_from_folded_rational(rational_new(1, 2), None)
                    .expect("BUG: literal 1/2 must commit at normalize"),
            )));
            NormalForm::Power(Arc::new(base), Arc::new(half))
        }
        other => other,
    }
}

fn canonical_order(nf: NormalForm) -> NormalForm {
    match nf {
        NormalForm::Sum(mut children) => {
            if children.iter().all(is_numeric_only) {
                children.sort_by_cached_key(sort_key);
            }
            NormalForm::Sum(children)
        }
        NormalForm::Product(mut children) => {
            if children.iter().all(is_numeric_only) {
                children.sort_by_cached_key(sort_key);
            }
            NormalForm::Product(children)
        }
        // And/Or: operand order is semantically significant (guards, unless-chain / short-circuit).
        NormalForm::And(children) => {
            NormalForm::And(children.into_iter().map(canonical_order).collect())
        }
        NormalForm::Or(children) => {
            NormalForm::Or(children.into_iter().map(canonical_order).collect())
        }
        other => other,
    }
}

fn sort_key(nf: &NormalForm) -> u8 {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(_)) => 0,
        _ => 1,
    }
}

fn is_numeric_zero(nf: &NormalForm) -> bool {
    as_rational_literal(nf).is_some_and(|r| rational_is_zero(&r))
}

fn is_numeric_one(nf: &NormalForm) -> bool {
    as_rational_literal(nf).is_some_and(|r| r == rational_new(1, 1))
}

fn as_rational_literal(nf: &NormalForm) -> Option<RationalInteger> {
    match nf {
        NormalForm::Leaf(LeafKind::Literal(literal)) => match &literal.value {
            ValueKind::Number(number) => Some(number.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Whether evaluating this normal form can never veto and reads no runtime
/// state. Only literal leaves qualify: every composite either reads runtime
/// state (`now`, data paths, rule paths), can veto at runtime (division,
/// fractional powers, mathematical functions, unit conversion, date
/// arithmetic, `veto`), or — when all of its operands are literals — is
/// collapsed to a literal leaf by `constant_fold` before the fixpoint loop
/// reconsiders the gated rewrites.
///
/// Deleting a total subexpression from a rewrite preserves veto semantics;
/// deleting anything else can erase a veto the runtime would produce.
fn is_total(nf: &NormalForm) -> bool {
    matches!(nf, NormalForm::Leaf(LeafKind::Literal(_)))
}

/// The integer value of a plain-number literal (denominator one), if any.
fn as_integer_literal(nf: &NormalForm) -> Option<RationalInteger> {
    let rational = as_rational_literal(nf)?;
    if rational.denom() == &crate::computation::bigint::BigInt::one() {
        Some(rational)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::rational_new;
    use crate::computation::UnitResolutionContext;
    use crate::planning::semantics::{
        ComparisonComputation, DataPath, ExpressionKind, NegationType, SemanticConversionTarget,
        ValueKind,
    };

    fn num_expr(n: i64) -> Expression {
        Expression::with_source(
            ExpressionKind::Literal(Box::new(LiteralValue::number(rational_new(n, 1)))),
            None,
        )
    }

    fn bool_expr(b: bool) -> Expression {
        Expression::with_source(
            ExpressionKind::Literal(Box::new(LiteralValue::from_bool(b))),
            None,
        )
    }

    fn dx() -> Expression {
        Expression::with_source(
            ExpressionKind::DataPath(DataPath::new(vec![], "x".into())),
            None,
        )
    }

    fn add_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Add, Arc::new(b)),
            None,
        )
    }

    fn mul_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Multiply, Arc::new(b)),
            None,
        )
    }

    fn pow_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Arithmetic(Arc::new(a), ArithmeticComputation::Power, Arc::new(b)),
            None,
        )
    }

    fn and_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(ExpressionKind::LogicalAnd(Arc::new(a), Arc::new(b)), None)
    }

    fn or_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(ExpressionKind::LogicalOr(Arc::new(a), Arc::new(b)), None)
    }

    fn not_expr(inner: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::LogicalNegation(Arc::new(inner), NegationType::Not),
            None,
        )
    }

    fn lt_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Comparison(Arc::new(a), ComparisonComputation::LessThan, Arc::new(b)),
            None,
        )
    }

    fn ge_expr(a: Expression, b: Expression) -> Expression {
        Expression::with_source(
            ExpressionKind::Comparison(
                Arc::new(a),
                ComparisonComputation::GreaterThanOrEqual,
                Arc::new(b),
            ),
            None,
        )
    }

    #[test]
    fn unless_discount_chain_compiles_patched_jumps() {
        use std::collections::{HashMap, HashSet};

        let ge10 = ge_expr(dx(), num_expr(10));
        let ge50 = ge_expr(dx(), num_expr(50));
        let branches = vec![
            (None, num_expr(0)),
            (Some(ge10), num_expr(5)),
            (Some(ge50), num_expr(15)),
        ];
        let plan_paths = HashSet::from([RulePath::new(vec![], "discount".into())]);
        let mut data: IndexMap<DataPath, DataDefinition> = IndexMap::new();
        data.insert(
            DataPath::new(vec![], "x".into()),
            DataDefinition::TypeDeclaration {
                resolved_type: Arc::clone(crate::planning::semantics::primitive_number_arc()),
                declared_default: None,
                source: crate::parsing::source::Source::new(
                    crate::parsing::source::SourceType::Volatile,
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                ),
            },
        );
        let rule_type = Arc::clone(crate::planning::semantics::primitive_number_arc());
        let unit_ctx = UnitResolutionContext::NamedQuantityOnly;
        build_normalized_rule_instructions(
            &branches,
            &HashMap::new(),
            &plan_paths,
            &data,
            &unit_ctx,
            None,
            &rule_type,
            crate::limits::ResourceLimits::default().max_normalized_expression_nodes,
        )
        .expect("hex-style unless chain must compile without unpatched jumps");
    }

    #[test]
    fn compiled_or_short_circuit_compiles_strict_jumps() {
        use std::collections::{HashMap, HashSet};

        let cond = or_expr(ge_expr(dx(), num_expr(10)), ge_expr(dx(), num_expr(50)));
        let branches = vec![(None, num_expr(0)), (Some(cond), num_expr(99))];
        let plan_paths = HashSet::from([RulePath::new(vec![], "pick".into())]);
        let mut data: IndexMap<DataPath, DataDefinition> = IndexMap::new();
        data.insert(
            DataPath::new(vec![], "x".into()),
            DataDefinition::TypeDeclaration {
                resolved_type: Arc::clone(crate::planning::semantics::primitive_number_arc()),
                declared_default: None,
                source: crate::parsing::source::Source::new(
                    crate::parsing::source::SourceType::Volatile,
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                ),
            },
        );
        let rule_type = Arc::clone(crate::planning::semantics::primitive_number_arc());
        let unit_ctx = UnitResolutionContext::NamedQuantityOnly;
        build_normalized_rule_instructions(
            &branches,
            &HashMap::new(),
            &plan_paths,
            &data,
            &unit_ctx,
            None,
            &rule_type,
            crate::limits::ResourceLimits::default().max_normalized_expression_nodes,
        )
        .expect("runtime Or must compile with strict jump targets");
    }

    #[test]
    fn nested_logical_and_compiles_patched_jumps() {
        use std::collections::{HashMap, HashSet};

        let cond = and_expr(ge_expr(dx(), num_expr(10)), ge_expr(dx(), num_expr(50)));
        let branches = vec![(None, num_expr(0)), (Some(cond), num_expr(99))];
        let plan_paths = HashSet::from([RulePath::new(vec![], "tier".into())]);
        let mut data: IndexMap<DataPath, DataDefinition> = IndexMap::new();
        data.insert(
            DataPath::new(vec![], "x".into()),
            DataDefinition::TypeDeclaration {
                resolved_type: Arc::clone(crate::planning::semantics::primitive_number_arc()),
                declared_default: None,
                source: crate::parsing::source::Source::new(
                    crate::parsing::source::SourceType::Volatile,
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 0,
                    },
                ),
            },
        );
        let rule_type = Arc::clone(crate::planning::semantics::primitive_number_arc());
        let unit_ctx = UnitResolutionContext::NamedQuantityOnly;
        build_normalized_rule_instructions(
            &branches,
            &HashMap::new(),
            &plan_paths,
            &data,
            &unit_ctx,
            None,
            &rule_type,
            crate::limits::ResourceLimits::default().max_normalized_expression_nodes,
        )
        .expect("runtime And must compile without unpatched jumps");
    }

    #[test]
    fn flatten_associative_sum_nested() {
        let inner = add_expr(num_expr(1), num_expr(2));
        let expr = add_expr(inner, num_expr(3));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(6, 1))
        ));
    }

    #[test]
    fn flatten_associative_product_nested() {
        let inner = mul_expr(num_expr(2), num_expr(3));
        let expr = mul_expr(inner, num_expr(4));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(24, 1))
        ));
    }

    #[test]
    fn flatten_associative_or_nested() {
        let inner = or_expr(bool_expr(false), bool_expr(false));
        let expr = or_expr(inner, bool_expr(true));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(true))
        ));
    }

    #[test]
    fn nested_and_is_not_associative_flattened() {
        // Guard-shaped inner And must stay grouped; flattening inner clauses into one n-ary And
        // would break LogicalAnd evaluation (non-boolean left).
        let inner = and_expr(bool_expr(true), num_expr(7));
        let outer = and_expr(inner, bool_expr(false));
        let nf = to_normal_form(&outer);
        let NormalForm::And(top) = nf else {
            panic!("expected And(..), got {nf:?}");
        };
        assert_eq!(
            top.len(),
            2,
            "nested And must remain 2 children at NF level, got {top:?}"
        );
        assert!(matches!(&top[0], NormalForm::And(inner) if inner.len() == 2));
    }

    #[test]
    fn identity_add_zero() {
        let expr = add_expr(dx(), num_expr(0));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn identity_mul_one() {
        let expr = mul_expr(dx(), num_expr(1));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn identity_mul_zero_with_data_path_is_not_folded() {
        // `x * 0` must stay a runtime multiply: folding it to `0` would erase
        // a missing-data veto for `x` and drop `x` from the data manifest.
        let expr = mul_expr(dx(), num_expr(0));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::Arithmetic(left, ArithmeticComputation::Multiply, right) = norm.kind
        else {
            panic!("expected preserved multiply, got {:?}", norm.kind);
        };
        assert!(matches!(left.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        assert!(matches!(
            right.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(0, 1))
        ));
    }

    #[test]
    fn identity_mul_zero_with_literal_folds() {
        // All-literal products still fold: `7 * 0 → 0`.
        let expr = mul_expr(num_expr(7), num_expr(0));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(0, 1))
        ));
    }

    #[test]
    fn identity_pow_one_and_zero() {
        let p1 = normalize_expression(&pow_expr(dx(), num_expr(1)), None).expect("normalize");
        assert!(matches!(p1.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        // `x ^ 0` must stay a runtime power: folding it to `1` would erase a
        // missing-data veto for `x`.
        let p0 = normalize_expression(&pow_expr(dx(), num_expr(0)), None).expect("normalize");
        let ExpressionKind::Arithmetic(base, ArithmeticComputation::Power, exp) = p0.kind else {
            panic!("expected preserved power, got {:?}", p0.kind);
        };
        assert!(matches!(base.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        assert!(matches!(
            exp.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(0, 1))
        ));
        // A total, nonzero literal base still folds: `7 ^ 0 → 1`.
        let literal_pow_zero =
            normalize_expression(&pow_expr(num_expr(7), num_expr(0)), None).expect("normalize");
        assert!(matches!(
            literal_pow_zero.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(1, 1))
        ));
    }

    #[test]
    fn double_logical_negation() {
        let norm =
            normalize_expression(&not_expr(not_expr(bool_expr(true))), None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(true))
        ));
    }

    #[test]
    fn double_numeric_negation() {
        let zero = num_expr(0);
        let inner = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(zero.clone()),
                ArithmeticComputation::Subtract,
                Arc::new(dx()),
            ),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(zero),
                ArithmeticComputation::Subtract,
                Arc::new(inner),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn double_reciprocal_with_data_path_is_not_collapsed() {
        // `1 / (1 / x)` must stay nested: collapsing it to `x` would erase
        // the inner division-by-zero veto for `x = 0`.
        let one = num_expr(1);
        let inner = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(one.clone()),
                ArithmeticComputation::Divide,
                Arc::new(dx()),
            ),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(one),
                ArithmeticComputation::Divide,
                Arc::new(inner),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::Arithmetic(_, ArithmeticComputation::Divide, outer_denominator) =
            norm.kind
        else {
            panic!("expected preserved outer division, got {:?}", norm.kind);
        };
        let ExpressionKind::Arithmetic(_, ArithmeticComputation::Divide, inner_denominator) =
            &outer_denominator.kind
        else {
            panic!(
                "expected preserved inner division, got {:?}",
                outer_denominator.kind
            );
        };
        assert!(matches!(inner_denominator.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn sqrt_squared_numeric_folds_to_exact_rational() {
        let sqrt2 = Expression::with_source(
            ExpressionKind::MathematicalComputation(
                MathematicalComputation::Sqrt,
                Arc::new(num_expr(2)),
            ),
            None,
        );
        let expr = pow_expr(sqrt2, num_expr(2));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(2, 1))
        ));
    }

    /// `2^(1/2)` has no exact rational result: constant_fold must not substitute a decimal.
    #[test]
    fn literal_power_with_irrational_result_stays_power() {
        let half = Expression::with_source(
            ExpressionKind::Literal(Box::new(
                literal_from_folded_rational(rational_new(1, 2), None)
                    .expect("BUG: literal 1/2 must commit at normalize"),
            )),
            None,
        );
        let expr = pow_expr(num_expr(2), half);
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(
            matches!(
                norm.kind,
                ExpressionKind::Arithmetic(_, ArithmeticComputation::Power, _)
            ),
            "expected symbolic Power, got {:?}",
            norm.kind
        );
    }

    #[test]
    fn de_morgan_not_applied_to_data_paths() {
        // De Morgan must not rewrite `not (x and y)` for non-total children:
        // AND propagates every conjunct's veto while OR swallows non-last
        // non-missing-data vetoes, so the rewrite changes veto observability.
        fn dy() -> Expression {
            Expression::with_source(
                ExpressionKind::DataPath(DataPath::new(vec![], "y".into())),
                None,
            )
        }
        let expr = not_expr(and_expr(dx(), dy()));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::LogicalNegation(inner, _) = norm.kind else {
            panic!("expected preserved negation, got {:?}", norm.kind);
        };
        assert!(
            matches!(inner.kind, ExpressionKind::LogicalAnd(_, _)),
            "expected preserved conjunction under the negation, got {:?}",
            inner.kind
        );
    }

    #[test]
    fn power_law_nested_power() {
        let expr = pow_expr(pow_expr(dx(), num_expr(2)), num_expr(3));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Arithmetic(_, ArithmeticComputation::Power, _)
        ));
        let ExpressionKind::Arithmetic(base, ArithmeticComputation::Power, exp) = norm.kind else {
            unreachable!();
        };
        assert!(matches!(
            base.kind,
            ExpressionKind::DataPath(ref p) if p.data == "x"
        ));
        assert!(matches!(
            exp.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(6, 1))
        ));
    }

    #[test]
    fn power_law_like_base_product() {
        let expr = mul_expr(pow_expr(dx(), num_expr(2)), pow_expr(dx(), num_expr(3)));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::Arithmetic(base, ArithmeticComputation::Power, exp) = norm.kind else {
            panic!("expected single power, got {:?}", norm.kind);
        };
        assert!(matches!(base.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        assert!(matches!(
            exp.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(5, 1))
        ));
    }

    #[test]
    fn negated_comparison_not_less_than() {
        let cmp = lt_expr(dx(), num_expr(0));
        let norm = normalize_expression(&not_expr(cmp), None).expect("normalize");
        let ExpressionKind::Comparison(_, op, _) = norm.kind else {
            panic!("expected Comparison, got {:?}", norm.kind);
        };
        assert_eq!(op, ComparisonComputation::GreaterThanOrEqual);
    }

    #[test]
    fn logical_short_circuit_and_false_with_data_path_is_not_folded() {
        // `false and x` must stay a conjunction: folding it to `false` would
        // erase a missing-data veto for `x`.
        let expr = and_expr(bool_expr(false), dx());
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::LogicalAnd(left, right) = norm.kind else {
            panic!("expected preserved conjunction, got {:?}", norm.kind);
        };
        assert!(matches!(
            left.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(false))
        ));
        assert!(matches!(right.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn logical_short_circuit_and_false_all_literal_folds() {
        let expr = and_expr(bool_expr(false), bool_expr(true));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(false))
        ));
    }

    #[test]
    fn logical_short_circuit_or_true_with_data_path_is_not_folded() {
        // `x or true` must stay a disjunction: folding it to `true` would
        // erase a missing-data veto for `x`.
        let expr = or_expr(dx(), bool_expr(true));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::LogicalOr(left, right) = norm.kind else {
            panic!("expected preserved disjunction, got {:?}", norm.kind);
        };
        assert!(matches!(left.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        assert!(matches!(
            right.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(true))
        ));
    }

    #[test]
    fn logical_short_circuit_or_true_all_literal_folds() {
        let expr = or_expr(bool_expr(false), bool_expr(true));
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(true))
        ));
    }

    #[test]
    fn or_keeps_trailing_false_disjunct() {
        // The last disjunct carries the propagating veto position: `x or
        // false` must not collapse to `x`, which would change which vetoes
        // of `x` are observable.
        let expr = or_expr(dx(), bool_expr(false));
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::LogicalOr(left, right) = norm.kind else {
            panic!("expected preserved disjunction, got {:?}", norm.kind);
        };
        assert!(matches!(left.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
        assert!(matches!(
            right.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Boolean(false))
        ));
    }

    #[test]
    fn or_drops_non_last_false_disjunct() {
        // A literal `false` in a non-last position is dead: dropping it does
        // not change which disjunct is last.
        let expr = or_expr(bool_expr(false), dx());
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn or_dedup_keeps_non_total_duplicates() {
        // OR deduplication must not remove non-literal duplicates: removing
        // one could change which disjunct occupies the last (veto-carrying)
        // position relative to other rewrites. Only total (literal)
        // duplicates in non-last positions are removed.
        let expr = or_expr(or_expr(dx(), dx()), dx());
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::LogicalOr(left, _) = &norm.kind else {
            panic!("expected preserved disjunction, got {:?}", norm.kind);
        };
        assert!(
            matches!(&left.kind, ExpressionKind::LogicalOr(_, _))
                || matches!(&left.kind, ExpressionKind::DataPath(_)),
            "non-total duplicates must not be removed wholesale, got {:?}",
            norm.kind
        );
    }

    #[test]
    fn logical_idempotency_and_duplicate_paths() {
        let a = dx();
        let expr = and_expr(a.clone(), a);
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(norm.kind, ExpressionKind::DataPath(ref p) if p.data == "x"));
    }

    #[test]
    fn unit_conversion_fold_number_to_number() {
        let inner = num_expr(42);
        let expr = Expression::with_source(
            ExpressionKind::UnitConversion(
                Arc::new(inner),
                SemanticConversionTarget::Type(PrimitiveKind::Number),
            ),
            None,
        );
        let ctx = UnitResolutionContext::NamedQuantityOnly;
        let norm = normalize_expression(&expr, Some(&ctx)).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l)
                if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(42, 1))
        ));
    }

    #[test]
    fn piecewise_single_branch() {
        let branches = vec![(None as Option<Expression>, num_expr(9))];
        let expr = unless_branches_to_piecewise(&branches);
        assert!(matches!(expr.kind, ExpressionKind::Literal(_)));
    }

    #[test]
    fn sqrt_product_folds_to_literal_two() {
        use crate::computation::UnitResolutionContext;
        use std::collections::{HashMap, HashSet};

        let sqrt_two = Expression::with_source(
            ExpressionKind::MathematicalComputation(
                MathematicalComputation::Sqrt,
                Arc::new(num_expr(2)),
            ),
            None,
        );
        let sqrt_two_path = RulePath::new(vec![], "sqrt_two".into());
        let mut completed = HashMap::new();
        completed.insert(sqrt_two_path.clone(), Arc::new(sqrt_two));

        let product = mul_expr(
            Expression::with_source(ExpressionKind::RulePath(sqrt_two_path.clone()), None),
            Expression::with_source(ExpressionKind::RulePath(sqrt_two_path), None),
        );
        let branches = vec![(None as Option<Expression>, product)];
        let plan_paths = HashSet::from([RulePath::new(vec![], "sqrt_product".into())]);
        let unit_ctx = UnitResolutionContext::NamedQuantityOnly;
        let data: IndexMap<DataPath, DataDefinition> = IndexMap::new();
        let rule_type = Arc::clone(crate::planning::semantics::primitive_number_arc());
        let compiled = build_normalized_rule_instructions(
            &branches,
            &completed,
            &plan_paths,
            &data,
            &unit_ctx,
            None,
            &rule_type,
            crate::limits::ResourceLimits::default().max_normalized_expression_nodes,
        )
        .expect("build normalized rule instructions");
        let instructions = compiled.instructions;

        assert_eq!(
            instructions.code.len(),
            2,
            "sqrt_two * sqrt_two must fold to LoadConstant + Return, got {:?}",
            instructions.code
        );
        assert!(matches!(
            instructions.code[0],
            crate::planning::execution_plan::Instruction::LoadConstant { .. }
        ));
        match &instructions.constants[0].value {
            ValueKind::Number(n) => assert_eq!(n, &rational_new(2, 1)),
            other => panic!("expected literal 2, got {other:?}"),
        }
    }

    #[test]
    fn named_compound_unit_literal_must_expand_signature_in_normalized_instructions() {
        use crate::parsing::source::SourceType;
        use crate::Engine;

        let code = r#"
spec t
uses lemma units

data money: quantity
  -> unit eur 1.00

data rate: quantity
  -> unit eur_per_hour eur/hour

rule total: 100 eur_per_hour
"#;
        let mut engine = Engine::new();
        engine
            .load(
                code,
                SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "compound_rate.lemma",
                ))),
            )
            .expect("load with embedded lemma units");

        let execution_plan = engine.get_plan(None, "t", None).expect("plan");
        let rule = execution_plan.get_rule("total").expect("total");
        let uses_named_only = rule.instructions.constants.iter().any(|constant| {
            matches!(
                &constant.value,
                ValueKind::Quantity(_, signature)
                    if signature.len() == 1 && signature[0].0 == "eur_per_hour"
            )
        });
        let expanded_in_index = execution_plan
            .expression_unit_index()
            .keys()
            .any(|key| key.contains("eur") && key.contains("hour"));

        assert!(
            expanded_in_index,
            "plan unit index must contain expanded compound signature; keys={:?}",
            execution_plan
                .expression_unit_index()
                .keys()
                .collect::<Vec<_>>()
        );
        assert!(
            !uses_named_only,
            "normalized constants must not keep unresolved named-only signature {:?}",
            rule.instructions
                .constants
                .iter()
                .map(|c| &c.value)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn piecewise_unless_arms_in_source_order() {
        let c_big = lt_expr(num_expr(10), dx());
        let c_small = lt_expr(num_expr(5), dx());
        let branches = vec![
            (None, num_expr(0)),
            (Some(c_big.clone()), num_expr(100)),
            (Some(c_small.clone()), num_expr(50)),
        ];
        let ExpressionKind::Piecewise(arms) = unless_branches_to_piecewise(&branches).kind else {
            panic!("expected Piecewise for unless chain");
        };
        assert_eq!(arms.len(), 3);
        assert!(matches!(
            arms[0].0.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Boolean(true))
        ));
        assert!(
            matches!(arms[0].1.kind, ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Number(ref n) if n == &rational_new(0, 1)))
        );
        assert!(matches!(
            arms[1].0.kind,
            ExpressionKind::Comparison(_, _, _)
        ));
        assert!(
            matches!(arms[1].1.kind, ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Number(ref n) if n == &rational_new(100, 1)))
        );
        assert!(matches!(
            arms[2].0.kind,
            ExpressionKind::Comparison(_, _, _)
        ));
        assert!(
            matches!(arms[2].1.kind, ExpressionKind::Literal(ref l) if matches!(l.value, ValueKind::Number(ref n) if n == &rational_new(50, 1)))
        );
    }

    #[test]
    fn power_laws_sqrt_squared_is_not_merged_for_data_paths() {
        let x = Expression::with_source(
            ExpressionKind::DataPath(DataPath::new(vec![], "x".into())),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(Expression::with_source(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Sqrt,
                        Arc::new(x.clone()),
                    ),
                    None,
                )),
                ArithmeticComputation::Multiply,
                Arc::new(Expression::with_source(
                    ExpressionKind::MathematicalComputation(
                        MathematicalComputation::Sqrt,
                        Arc::new(x),
                    ),
                    None,
                )),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        let nf = to_normal_form(&norm);
        // `sqrt(x) * sqrt(x)` must not merge to `x`: the merge erases the
        // `x < 0` domain veto (fractional exponents restrict the domain).
        assert!(
            matches!(nf, NormalForm::Product(ref children) if children.len() == 2),
            "sqrt(x)*sqrt(x) must stay a product of two powers, got {:?}",
            format!("{:?}", nf)
        );
    }

    #[test]
    fn exp_log_not_folded_for_data_path() {
        // `exp (log x)` must stay nested: folding it to `x` would erase the
        // `log` domain veto for `x <= 0`.
        let inner = Expression::with_source(
            ExpressionKind::MathematicalComputation(MathematicalComputation::Log, Arc::new(dx())),
            None,
        );
        let expr = Expression::with_source(
            ExpressionKind::MathematicalComputation(MathematicalComputation::Exp, Arc::new(inner)),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        let ExpressionKind::MathematicalComputation(MathematicalComputation::Exp, inner) =
            norm.kind
        else {
            panic!("expected preserved exp, got {:?}", norm.kind);
        };
        assert!(
            matches!(
                inner.kind,
                ExpressionKind::MathematicalComputation(MathematicalComputation::Log, _)
            ),
            "expected preserved log under exp, got {:?}",
            inner.kind
        );
    }

    #[test]
    fn constant_fold_add() {
        let expr = Expression::with_source(
            ExpressionKind::Arithmetic(
                Arc::new(num_expr(2)),
                ArithmeticComputation::Add,
                Arc::new(num_expr(3)),
            ),
            None,
        );
        let norm = normalize_expression(&expr, None).expect("normalize");
        assert!(matches!(
            norm.kind,
            ExpressionKind::Literal(ref l) if matches!(l.value, crate::planning::semantics::ValueKind::Number(ref n) if n == &rational_new(5, 1))
        ));
    }

    #[test]
    #[should_panic(expected = "BUG: data reference 'proxy' was not fully inlined")]
    fn validate_no_rule_target_data_paths_crashes_on_uninlined_reference() {
        use crate::parsing::source::{Source, SourceType};
        use crate::planning::semantics::{primitive_number_arc, ReferenceTarget, RulePath};
        use indexmap::IndexMap;

        let proxy = DataPath::new(vec![], "proxy".into());
        let mut data = IndexMap::new();
        data.insert(
            proxy.clone(),
            DataDefinition::Reference {
                target: ReferenceTarget::Rule(RulePath::new(vec![], "target".into())),
                resolved_type: primitive_number_arc().clone(),
                local_constraints: None,
                local_default: None,
                source: Source::new(
                    SourceType::Volatile,
                    crate::parsing::ast::Span {
                        start: 0,
                        end: 0,
                        line: 1,
                        col: 1,
                    },
                ),
            },
        );
        let expr = Expression::with_source(ExpressionKind::DataPath(proxy), None);
        validate_no_rule_target_data_paths(&expr, &data);
    }
}
