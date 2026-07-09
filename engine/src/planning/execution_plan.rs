//! Execution plan for evaluated specs
//!
//! Provides a complete self-contained execution plan ready for the evaluator.
//! The plan contains all data, rules flattened into executable branches,
//! and execution order - no spec structure needed during evaluation.
//!
//! Reliability model:
//! - `SpecSchema` is the IO contract surface for consumers (data and rule outputs).
//!   IO compatibility is the consumer-facing guarantee.

use crate::computation::UnitResolutionContext;
use crate::parsing::ast::{CalendarPeriodUnit, DateCalendarKind, DateRelativeKind};
use crate::parsing::ast::{DateTimeValue, EffectiveDate, LemmaRepository, LemmaSpec, MetaValue};
use crate::parsing::source::Source;
use crate::planning::data_input::{parse_data_value, DataValueInput};
use crate::planning::graph::Graph;
use crate::planning::graph::ResolvedSpecTypes;
use crate::planning::normalize::{build_normalized_rule_instructions, CompiledRule};
use crate::planning::semantics::{
    value_kind_matches_spec, ArithmeticComputation, ComparisonComputation, DataDefinition,
    DataPath, Expression, LemmaType, LiteralValue, MathematicalComputation, ReferenceTarget,
    RulePath, SemanticConversionTarget, TypeSpecification, ValueKind,
};
use crate::Error;
use crate::ResourceLimits;
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// One spec's contribution to an [`ExecutionPlan`], together with its
/// formatted AST source.
///
/// `repository` is `None` for workspace (root) specs. Including the
/// repository name means two specs with the same base name from different
/// repos are always distinct entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    pub name: String,
    pub effective_from: EffectiveDate,
    pub source: String,
}

pub type SpecSources = Vec<SpecSource>;

/// A complete execution plan ready for the evaluator
///
/// Contains the topologically sorted list of rules to execute, along with all data.
/// Self-contained structure - no spec lookups required during evaluation.
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    /// Main spec name
    pub spec_name: String,

    /// Optional commentary from the `"""..."""` block in the spec source.
    pub commentary: Option<String>,

    /// Per-data data in definition order: value, type-only, or spec reference.
    pub data: IndexMap<DataPath, DataDefinition>,

    /// Rules to execute in topological order (sorted by dependencies)
    pub rules: Vec<ExecutableRule>,

    /// Maximum register file size across all rules; sizes [`EvaluationContext::register_values`].
    pub max_register_count: u16,

    /// Order in which [`DataDefinition::Reference`] entries must be resolved
    /// at evaluation time so that chained references (reference → reference →
    /// data) copy values in the correct sequence. Empty when the plan has no
    /// references.
    pub reference_evaluation_order: Vec<DataPath>,

    /// Spec metadata
    pub meta: HashMap<String, MetaValue>,

    /// Main-spec types from planning. [`ResolvedSpecTypes::unit_index`] is expression-scope
    /// units (local types plus direct `uses` imports). Rule-result units live on each
    /// [`ExecutableRule::rule_type`], not in this index.
    pub resolved_types: ResolvedSpecTypes,

    /// Reverse index: canonical-form unit signature `Vec<(unit_name, exponent)>` →
    /// (unit_name, owning type). Built from expression-scope units during planning so
    /// cross-type Multiply/Divide arithmetic can deterministically resolve a combined
    /// signature back to a single named unit. Ambiguous signatures (the same key matched
    /// by units in two distinct types) are rejected at planning time.
    pub signature_index: crate::computation::arithmetic::SignatureIndex,

    pub effective: EffectiveDate,

    /// Canonical source for all specs in this plan (one entry per spec, includes repository).
    /// Reconstructed from AST — not raw file content.
    pub sources: SpecSources,
}

/// All [`ExecutionPlan`]s for a spec name after dependency resolution.
/// Ordered by [`ExecutionPlan::effective`]. Slice end is derived from the next plan's `effective`.
#[derive(Debug, Clone)]
pub struct ExecutionPlanSet {
    pub spec_name: String,
    pub plans: Vec<ExecutionPlan>,
}

impl ExecutionPlanSet {
    /// Plan covering `[effective[i], effective[i+1])` (half-open).
    #[must_use]
    pub fn plan_at(&self, effective: &EffectiveDate) -> Option<&ExecutionPlan> {
        for (i, plan) in self.plans.iter().enumerate() {
            let from_ok = *effective >= plan.effective;
            let to_ok = self
                .plans
                .get(i + 1)
                .map(|next| *effective < next.effective)
                .unwrap_or(true);
            if from_ok && to_ok {
                return Some(plan);
            }
        }
        None
    }
}

pub const INSTRUCTIONS_VERSION: u32 = 2;

/// How [`Instruction::JumpIfFalse`] treats a vetoed condition register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JumpVetoSemantics {
    /// Expression unless condition: propagate [`VetoType::MissingData`]; other vetoes are non-match.
    #[default]
    UnlessExpression,
    /// Rule-reference unless condition: any veto propagates to the rule result.
    UnlessRuleReference,
}

/// Planning-time invariant: every jump is patched and lands on a real
/// instruction. Panics on violation — compiled output failing this check is a
/// compiler bug.
pub fn validate_instruction_jumps(code: &[Instruction]) {
    if let Err(message) = check_instruction_jumps(code) {
        panic!("BUG: {message}");
    }
}

/// Check that every jump is patched (non-zero target) and lands on a real
/// instruction (`target < code.len()`).
fn check_instruction_jumps(code: &[Instruction]) -> Result<(), String> {
    let code_len = code.len();
    for (index, instruction) in code.iter().enumerate() {
        match instruction {
            Instruction::JumpIfFalse {
                target_instruction, ..
            } => {
                if *target_instruction == 0 {
                    return Err(format!("unpatched JumpIfFalse at instruction {index}"));
                }
                if (*target_instruction as usize) >= code_len {
                    return Err(format!(
                        "JumpIfFalse at instruction {index} targets {target_instruction} past the last instruction (length {code_len})"
                    ));
                }
            }
            Instruction::Jump { target_instruction } => {
                if *target_instruction == 0 {
                    return Err(format!("unpatched Jump at instruction {index}"));
                }
                if (*target_instruction as usize) >= code_len {
                    return Err(format!(
                        "Jump at instruction {index} targets {target_instruction} past the last instruction (length {code_len})"
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate a compiled instruction stream against its operand pools: version,
/// jump targets, register indices, constant/data/veto-message table indices,
/// and the trailing [`Instruction::Return`].
///
/// Two call sites with different failure semantics:
/// - the deserialization trust boundary ([`TryFrom<ExecutionPlanSerialized>`]),
///   where a tampered or stale serialized plan must surface as an error;
/// - the compiler (`CompileContext::finish`), where a violation is a compiler
///   bug and crashes.
pub fn validate_instructions(instructions: &Instructions) -> Result<(), String> {
    if instructions.version != INSTRUCTIONS_VERSION {
        return Err(format!(
            "instructions version {} does not match supported version {}",
            instructions.version, INSTRUCTIONS_VERSION
        ));
    }

    check_instruction_jumps(&instructions.code)?;

    let register_count = instructions.register_count;
    let constant_count = instructions.constants.len();
    let data_count = instructions.data_manifest.len();
    let veto_message_count = instructions.veto_messages.len();

    let check_register = |index: usize, name: &str, register: u16| -> Result<(), String> {
        if register >= register_count {
            return Err(format!(
                "instruction {index} {name} register r{register} is out of bounds (register count {register_count})"
            ));
        }
        Ok(())
    };

    for (index, instruction) in instructions.code.iter().enumerate() {
        match instruction {
            Instruction::LoadConstant {
                destination_register,
                constant_index,
            } => {
                check_register(index, "destination", *destination_register)?;
                if (*constant_index as usize) >= constant_count {
                    return Err(format!(
                        "instruction {index} constant index {constant_index} is out of bounds (constant count {constant_count})"
                    ));
                }
            }
            Instruction::LoadData {
                destination_register,
                data_index,
            } => {
                check_register(index, "destination", *destination_register)?;
                if (*data_index as usize) >= data_count {
                    return Err(format!(
                        "instruction {index} data index {data_index} is out of bounds (data manifest size {data_count})"
                    ));
                }
            }
            Instruction::LoadNow {
                destination_register,
            } => {
                check_register(index, "destination", *destination_register)?;
            }
            Instruction::Arithmetic {
                destination_register,
                operation: _,
                left_register,
                right_register,
            }
            | Instruction::Comparison {
                destination_register,
                operation: _,
                left_register,
                right_register,
            }
            | Instruction::RangeLiteral {
                destination_register,
                left_register,
                right_register,
            } => {
                check_register(index, "destination", *destination_register)?;
                check_register(index, "left", *left_register)?;
                check_register(index, "right", *right_register)?;
            }
            Instruction::UnitConversion {
                destination_register,
                source_register,
                target: _,
            }
            | Instruction::Mathematical {
                destination_register,
                operation: _,
                source_register,
            }
            | Instruction::DateRelative {
                destination_register,
                kind: _,
                source_register,
            }
            | Instruction::DateCalendar {
                destination_register,
                kind: _,
                unit: _,
                source_register,
            }
            | Instruction::PastFutureRange {
                destination_register,
                kind: _,
                source_register,
            }
            | Instruction::ResultIsVeto {
                destination_register,
                source_register,
            }
            | Instruction::MoveRegister {
                destination_register,
                source_register,
            } => {
                check_register(index, "destination", *destination_register)?;
                check_register(index, "source", *source_register)?;
            }
            Instruction::RangeContainment {
                destination_register,
                value_register,
                range_register,
            } => {
                check_register(index, "destination", *destination_register)?;
                check_register(index, "value", *value_register)?;
                check_register(index, "range", *range_register)?;
            }
            Instruction::UserVeto {
                destination_register,
                message_index,
            } => {
                check_register(index, "destination", *destination_register)?;
                if (*message_index as usize) >= veto_message_count {
                    return Err(format!(
                        "instruction {index} veto message index {message_index} is out of bounds (veto message count {veto_message_count})"
                    ));
                }
            }
            Instruction::JumpIfFalse {
                condition_register,
                target_instruction: _,
                veto_semantics: _,
            } => {
                check_register(index, "condition", *condition_register)?;
            }
            Instruction::Jump {
                target_instruction: _,
            } => {}
            Instruction::Return { source_register } => {
                check_register(index, "source", *source_register)?;
            }
        }
    }

    match instructions.code.last() {
        Some(Instruction::Return { .. }) => {}
        Some(other) => {
            return Err(format!(
                "instruction stream must end with Return, found {other:?}"
            ))
        }
        None => return Err("instruction stream is empty".to_string()),
    }

    let code_len = instructions.code.len();
    for tag in &instructions.arm_tags {
        if (tag.pc as usize) >= code_len {
            return Err(format!(
                "arm tag pc {} is out of bounds (code length {code_len})",
                tag.pc
            ));
        }
        let tagged = &instructions.code[tag.pc as usize];
        let valid = match tag.role {
            ArmRole::Condition => matches!(tagged, Instruction::JumpIfFalse { .. }),
            ArmRole::Result => matches!(tagged, Instruction::Return { .. }),
        };
        if !valid {
            return Err(format!(
                "arm tag at pc {} does not match its instruction {tagged:?}",
                tag.pc
            ));
        }
    }
    for tag in &instructions.conversion_tags {
        if (tag.pc as usize) >= code_len {
            return Err(format!(
                "conversion tag pc {} is out of bounds (code length {code_len})",
                tag.pc
            ));
        }
        if !matches!(
            instructions.code[tag.pc as usize],
            Instruction::UnitConversion { .. }
        ) {
            return Err(format!(
                "conversion tag at pc {} does not reference a UnitConversion instruction",
                tag.pc
            ));
        }
    }

    validate_register_consumption(&instructions.code)?;

    Ok(())
}

struct InstructionRegisterEffect {
    uses: Vec<u16>,
    kills: Vec<u16>,
    defines: Option<u16>,
}

fn instruction_register_effect(instruction: &Instruction) -> InstructionRegisterEffect {
    match instruction {
        Instruction::LoadConstant {
            destination_register,
            ..
        }
        | Instruction::LoadData {
            destination_register,
            ..
        }
        | Instruction::LoadNow {
            destination_register,
        }
        | Instruction::UserVeto {
            destination_register,
            ..
        } => InstructionRegisterEffect {
            uses: Vec::new(),
            kills: Vec::new(),
            defines: Some(*destination_register),
        },
        Instruction::Arithmetic {
            destination_register,
            left_register,
            right_register,
            ..
        }
        | Instruction::Comparison {
            destination_register,
            left_register,
            right_register,
            ..
        }
        | Instruction::RangeLiteral {
            destination_register,
            left_register,
            right_register,
        } => InstructionRegisterEffect {
            uses: vec![*left_register, *right_register],
            kills: vec![*left_register, *right_register],
            defines: Some(*destination_register),
        },
        Instruction::UnitConversion {
            destination_register,
            source_register,
            ..
        }
        | Instruction::Mathematical {
            destination_register,
            source_register,
            ..
        }
        | Instruction::DateRelative {
            destination_register,
            source_register,
            ..
        }
        | Instruction::DateCalendar {
            destination_register,
            source_register,
            ..
        }
        | Instruction::PastFutureRange {
            destination_register,
            source_register,
            ..
        } => InstructionRegisterEffect {
            uses: vec![*source_register],
            kills: vec![*source_register],
            defines: Some(*destination_register),
        },
        Instruction::RangeContainment {
            destination_register,
            value_register,
            range_register,
        } => InstructionRegisterEffect {
            uses: vec![*value_register, *range_register],
            kills: vec![*value_register, *range_register],
            defines: Some(*destination_register),
        },
        Instruction::ResultIsVeto {
            destination_register,
            source_register,
        } => InstructionRegisterEffect {
            uses: vec![*source_register],
            kills: Vec::new(),
            defines: Some(*destination_register),
        },
        Instruction::MoveRegister {
            destination_register,
            source_register,
        } => InstructionRegisterEffect {
            uses: vec![*source_register],
            kills: vec![*source_register],
            defines: Some(*destination_register),
        },
        Instruction::JumpIfFalse {
            condition_register, ..
        } => InstructionRegisterEffect {
            uses: vec![*condition_register],
            kills: Vec::new(),
            defines: None,
        },
        Instruction::Return { source_register } => InstructionRegisterEffect {
            uses: vec![*source_register],
            kills: vec![*source_register],
            defines: None,
        },
        Instruction::Jump { .. } => InstructionRegisterEffect {
            uses: Vec::new(),
            kills: Vec::new(),
            defines: None,
        },
    }
}

/// Prove that registers consumed by move/destroy instructions are not live on any
/// successor path. Enables the VM to `take` operand registers without cloning.
fn validate_register_consumption(code: &[Instruction]) -> Result<(), String> {
    let len = code.len();
    if len == 0 {
        return Ok(());
    }

    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); len];
    for (pc, instruction) in code.iter().enumerate() {
        match instruction {
            Instruction::Jump { target_instruction } => {
                successors[pc].push(*target_instruction as usize);
            }
            Instruction::JumpIfFalse {
                target_instruction, ..
            } => {
                successors[pc].push(*target_instruction as usize);
                if pc + 1 < len {
                    successors[pc].push(pc + 1);
                }
            }
            Instruction::Return { .. } => {}
            _ => {
                if pc + 1 < len {
                    successors[pc].push(pc + 1);
                }
            }
        }
    }

    let mut live_in: Vec<std::collections::HashSet<u16>> =
        vec![std::collections::HashSet::new(); len];
    let mut changed = true;
    while changed {
        changed = false;
        for pc in (0..len).rev() {
            let mut live_out = std::collections::HashSet::new();
            for succ in &successors[pc] {
                if *succ < len {
                    live_out.extend(live_in[*succ].iter().copied());
                }
            }

            let effect = instruction_register_effect(&code[pc]);
            for register in &effect.kills {
                if live_out.contains(register) {
                    return Err(format!(
                        "instruction {pc} consumes register r{register} but r{register} is live on a successor path"
                    ));
                }
            }

            let mut next_live_in =
                std::collections::HashSet::from_iter(effect.uses.iter().copied());
            if let Some(def) = effect.defines {
                for register in live_out {
                    if register != def {
                        next_live_in.insert(register);
                    }
                }
            } else {
                next_live_in.extend(live_out);
            }

            if live_in[pc] != next_live_in {
                live_in[pc] = next_live_in;
                changed = true;
            }
        }
    }

    Ok(())
}

/// Compiled normalized equation for authoritative evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instructions {
    pub version: u32,
    pub register_count: u16,
    #[serde(with = "register_types_serde")]
    pub register_types: Vec<Arc<LemmaType>>,
    #[serde(with = "constants_serde")]
    pub constants: Vec<Arc<LiteralValue>>,
    pub data_manifest: Vec<DataPath>,
    pub veto_messages: Vec<String>,
    pub code: Vec<Instruction>,
    /// Piecewise arm provenance: which `JumpIfFalse`/`Return` instructions
    /// belong to which source branch (`ExecutableRule::branches` index).
    /// Emitted by `compile_piecewise_rule` after all rewrite passes, so arm
    /// tags are present in both the optimized and source instruction streams.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arm_tags: Vec<ArmTag>,
    /// Unit-conversion provenance: maps a `UnitConversion` instruction back
    /// to the source location of the conversion expression it compiles.
    /// Reliable in the source (un-optimized) stream; best-effort in the
    /// optimized stream (rewrites may fold or merge conversion nodes).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversion_tags: Vec<ConversionTag>,
}

/// Role of an arm-tagged instruction within a rule's piecewise compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArmRole {
    /// A `JumpIfFalse` testing this arm's condition.
    Condition,
    /// A `Return` producing this arm's result.
    Result,
}

/// Provenance tag linking one instruction to a piecewise arm.
///
/// `arm` indexes `ExecutableRule::branches`: 0 is the default branch, 1.. are
/// the unless branches in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmTag {
    pub pc: u32,
    pub arm: u16,
    pub role: ArmRole,
}

/// Provenance tag linking one `UnitConversion` instruction to the source
/// location of the conversion expression it compiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionTag {
    pub pc: u32,
    pub source: Source,
}

mod register_types_serde {
    use super::LemmaType;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S>(values: &[Arc<LemmaType>], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let refs: Vec<&LemmaType> = values.iter().map(|v| v.as_ref()).collect();
        refs.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Arc<LemmaType>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values: Vec<LemmaType> = Vec::deserialize(deserializer)?;
        Ok(values.into_iter().map(Arc::new).collect())
    }
}

mod constants_serde {
    use super::LiteralValue;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S>(values: &[Arc<LiteralValue>], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let refs: Vec<&LiteralValue> = values.iter().map(|v| v.as_ref()).collect();
        refs.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Arc<LiteralValue>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values: Vec<LiteralValue> = Vec::deserialize(deserializer)?;
        Ok(values.into_iter().map(Arc::new).collect())
    }
}

/// One compiled operation in a rule's instruction stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Instruction {
    LoadConstant {
        destination_register: u16,
        constant_index: u16,
    },
    LoadData {
        destination_register: u16,
        data_index: u16,
    },
    LoadNow {
        destination_register: u16,
    },
    Arithmetic {
        destination_register: u16,
        operation: ArithmeticComputation,
        left_register: u16,
        right_register: u16,
    },
    Comparison {
        destination_register: u16,
        operation: ComparisonComputation,
        left_register: u16,
        right_register: u16,
    },
    UnitConversion {
        destination_register: u16,
        source_register: u16,
        target: SemanticConversionTarget,
    },
    Mathematical {
        destination_register: u16,
        operation: MathematicalComputation,
        source_register: u16,
    },
    DateRelative {
        destination_register: u16,
        kind: DateRelativeKind,
        source_register: u16,
    },
    DateCalendar {
        destination_register: u16,
        kind: DateCalendarKind,
        unit: CalendarPeriodUnit,
        source_register: u16,
    },
    RangeLiteral {
        destination_register: u16,
        left_register: u16,
        right_register: u16,
    },
    PastFutureRange {
        destination_register: u16,
        kind: DateRelativeKind,
        source_register: u16,
    },
    RangeContainment {
        destination_register: u16,
        value_register: u16,
        range_register: u16,
    },
    ResultIsVeto {
        destination_register: u16,
        source_register: u16,
    },
    MoveRegister {
        destination_register: u16,
        source_register: u16,
    },
    UserVeto {
        destination_register: u16,
        message_index: u16,
    },
    JumpIfFalse {
        condition_register: u16,
        target_instruction: u32,
        #[serde(default)]
        veto_semantics: JumpVetoSemantics,
    },
    Jump {
        target_instruction: u32,
    },
    Return {
        source_register: u16,
    },
}

/// An executable rule with flattened branches
///
/// Contains all information needed to evaluate a rule without spec lookups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableRule {
    /// Unique identifier for this rule
    pub path: RulePath,

    /// Rule name
    pub name: String,

    /// Source branches (unless cause lines only); used for explanations
    pub branches: Vec<Branch>,

    /// Fully inlined, once-normalized compiled equation; authoritative for evaluation
    pub instructions: Instructions,

    /// Fully inlined equation compiled **without** rewrite passes: instruction
    /// shape mirrors the source expressions. Executed (with recording) when
    /// explanations are requested, so every runtime fact in an explanation
    /// comes from an actual execution of source-shaped code.
    pub source_instructions: Instructions,

    /// Source location for error messages (always present for rules from parsed specs)
    pub source: Source,

    /// Computed type of this rule's result
    /// Every rule MUST have a type (Lemma is strictly typed)
    #[serde(with = "arc_lemma_type")]
    pub rule_type: Arc<LemmaType>,
}

/// A branch in an executable rule (original expressions for explanation trace)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    /// Condition expression (None for default branch)
    pub condition: Option<Expression>,

    /// Result expression as written (`RulePath` refs preserved)
    pub result: Expression,

    /// Source location for error messages (always present for branches from parsed specs)
    pub source: Source,
}

mod arc_lemma_type {
    use super::LemmaType;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S>(value: &Arc<LemmaType>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_ref().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<LemmaType>, D::Error>
    where
        D: Deserializer<'de>,
    {
        LemmaType::deserialize(deserializer).map(Arc::new)
    }
}

/// Builds an execution plan from a Graph for one temporal slice.
/// Internal implementation detail - only called by plan()
pub(crate) fn build_execution_plan(
    graph: &Graph,
    resolved_types: &mut Vec<(Arc<LemmaRepository>, Arc<LemmaSpec>, ResolvedSpecTypes)>,
    effective: &EffectiveDate,
    limits: &crate::limits::ResourceLimits,
) -> Result<ExecutionPlan, Vec<Error>> {
    let execution_order = graph.execution_order();

    let main_spec = graph.main_spec();
    let main_idx = resolved_types
        .iter()
        .position(|(_, spec, _)| Arc::ptr_eq(spec, main_spec));

    let mut sources: SpecSources = Vec::new();
    for (repo, spec, _) in resolved_types.iter() {
        if !sources.iter().any(|e| {
            e.repository == repo.name
                && e.name == spec.name
                && e.effective_from == spec.effective_from
        }) {
            sources.push(SpecSource {
                repository: repo.name.clone(),
                name: spec.name.clone(),
                effective_from: spec.effective_from.clone(),
                source: crate::formatting::format_specs(&[spec.as_ref().clone()]),
            });
        }
    }

    let main_resolved_types = main_idx
        .map(|idx| resolved_types.remove(idx).2)
        .unwrap_or_default();
    let data = graph.build_data(&main_resolved_types.resolved)?;

    // Planning gate: every data-target reference and plain data declaration
    // must carry a fully resolved type. Rule-target references are exempt:
    // they deliberately ship `Undetermined` so runtime veto propagation
    // surfaces the target rule's veto reason directly. A residual
    // `Undetermined` anywhere else would violate the invariant evaluation
    // and schema consumers rely on — report it instead of shipping the plan.
    let undetermined_errors: Vec<Error> = data
        .iter()
        .filter_map(|(path, definition)| {
            let (resolved_type, source) = match definition {
                DataDefinition::TypeDeclaration {
                    resolved_type,
                    source,
                    ..
                } => (resolved_type, source),
                DataDefinition::Reference {
                    target: ReferenceTarget::Data(_),
                    resolved_type,
                    source,
                    ..
                } => (resolved_type, source),
                DataDefinition::Reference {
                    target: ReferenceTarget::Rule(_),
                    ..
                }
                | DataDefinition::Value { .. }
                | DataDefinition::Import { .. } => return None,
            };
            if resolved_type.is_undetermined() {
                Some(Error::validation(
                    format!("could not determine the type of '{path}'"),
                    Some(source.clone()),
                    None::<String>,
                ))
            } else {
                None
            }
        })
        .collect();
    if !undetermined_errors.is_empty() {
        return Err(undetermined_errors);
    }

    let signature_index = crate::planning::graph::build_signature_index(
        &main_spec.name,
        &main_resolved_types.unit_index,
    )
    .expect("BUG: signature_index build already validated during resolve_and_validate");

    let mut executable_rules: Vec<ExecutableRule> = Vec::new();
    let mut max_register_count: u16 = 0;
    let plan_rule_paths: HashSet<RulePath> = graph.rules().keys().cloned().collect();
    let mut completed_rules: HashMap<RulePath, Arc<Expression>> = HashMap::new();

    for rule_path in execution_order {
        let rule_node = graph.rules().get(rule_path).expect(
            "bug: rule from topological sort not in graph - validation should have caught this",
        );

        let mut executable_branches = Vec::new();
        for (condition, result) in &rule_node.branches {
            executable_branches.push(Branch {
                condition: condition.clone(),
                result: result.clone(),
                source: rule_node.source.clone(),
            });
        }

        let unit_ctx = UnitResolutionContext::WithIndex(&main_resolved_types.unit_index);
        let compiled = build_normalized_rule_instructions(
            &rule_node.branches,
            &completed_rules,
            &plan_rule_paths,
            &data,
            &unit_ctx,
            Some(rule_node.source.clone()),
            &rule_node.rule_type,
            limits.max_normalized_expression_nodes,
        )
        // Pass the error through unchanged: it is already a user-facing
        // planning error (resource limit or constant-fold failure) carrying
        // its own kind and source location.
        .map_err(|error| vec![error])?;
        let CompiledRule {
            instructions,
            source_instructions,
            inlined_expression,
        } = compiled;
        max_register_count = max_register_count
            .max(instructions.register_count)
            .max(source_instructions.register_count);
        completed_rules.insert(rule_path.clone(), inlined_expression);

        executable_rules.push(ExecutableRule {
            path: rule_path.clone(),
            name: rule_path.rule.clone(),
            branches: executable_branches,
            instructions,
            source_instructions,
            source: rule_node.source.clone(),
            rule_type: Arc::clone(&rule_node.rule_type),
        });
    }

    Ok(ExecutionPlan {
        spec_name: main_spec.name.clone(),
        commentary: main_spec.commentary.clone(),
        data,
        rules: executable_rules,
        max_register_count,
        reference_evaluation_order: graph.reference_evaluation_order().to_vec(),
        meta: main_spec
            .meta_fields
            .iter()
            .map(|f| (f.key.clone(), f.value.clone()))
            .collect(),
        resolved_types: main_resolved_types,
        signature_index,
        effective: effective.clone(),
        sources,
    })
}

/// A spec's public interface: its data (inputs) and rules (outputs) with
/// full structured type information.
///
/// Built from an [`ExecutionPlan`] via [`ExecutionPlan::schema`] (all data and
/// rules) or [`ExecutionPlan::schema_for_rules`] (scoped to specific rules and
/// only the data they need).
///
/// Shared by the HTTP server, the CLI, the MCP server, WASM, and any other
/// consumer. Carries the real [`LemmaType`] and [`LiteralValue`] so consumers
/// can work at whatever fidelity they need — structured types for input forms,
/// or `Display` for plain text.
///
/// This is the IO contract consumers can rely on:
/// - `data`: required/provided inputs with full type constraints
/// - `rules`: produced outputs with full result types
///
/// For cross-spec composition, planning validates that referenced specs satisfy
/// this contract. Plan hashes are complementary: they lock full behavior.
/// One data input in a [`SpecSchema`].
///
/// A named struct instead of a tuple so JSON-native consumers (TypeScript, Python, ...)
/// get stable field names. `prefilled` is a spec literal or literal `with` binding;
/// `supplied` is caller overlay when building schema; `default` is a `-> default ...`
/// suggestion only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataEntry {
    #[serde(rename = "type")]
    pub lemma_type: LemmaType,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prefilled: Option<LiteralValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub supplied: Option<LiteralValue>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub default: Option<LiteralValue>,
}

/// User-provided data values resolved against a plan's type declarations.
///
/// Lightweight and cheap to construct — no plan cloning required. The
/// [`ExecutionPlan`] stays immutable; callers pass `(&ExecutionPlan, &DataOverlay)`
/// to evaluation and schema methods.
#[derive(Debug, Clone, Default)]
pub struct DataOverlay {
    /// Successfully parsed and validated values supplied by the caller.
    pub values: HashMap<DataPath, Arc<LiteralValue>>,
    /// Values that failed parse or constraint validation. Rules that read
    /// these paths produce [`crate::evaluation::VetoType::Computation`] vetoes.
    pub violated: HashMap<DataPath, String>,
}

impl DataOverlay {
    /// Parse and validate caller-supplied values against the plan's data declarations.
    ///
    /// Unknown data keys and resource-limit violations return [`Err`]. Per-field
    /// parse or constraint failures are recorded in [`Self::violated`] and the
    /// overlay is still returned as `Ok`.
    pub fn resolve(
        plan: &ExecutionPlan,
        raw_values: HashMap<String, DataValueInput>,
        limits: &ResourceLimits,
    ) -> Result<Self, Error> {
        let mut overlay = Self::default();

        for (name, raw_value) in raw_values {
            let data_path = plan.get_data_path_by_str(&name).ok_or_else(|| {
                let available: Vec<String> = plan.data.keys().map(|p| p.input_key()).collect();
                Error::request(
                    format!(
                        "Data '{}' not found. Available data: {}",
                        name,
                        available.join(", ")
                    ),
                    None::<String>,
                )
            })?;
            let data_path = data_path.clone();

            let data_definition = plan
                .data
                .get(&data_path)
                .expect("BUG: data_path was just resolved from plan.data, must exist");

            let data_source = data_definition.source().clone();
            let type_arc = match data_definition {
                DataDefinition::TypeDeclaration { resolved_type, .. }
                | DataDefinition::Reference { resolved_type, .. } => Arc::clone(resolved_type),
                DataDefinition::Value { value, .. } => Arc::clone(&value.lemma_type),
                DataDefinition::Import { .. } => {
                    return Err(Error::request(
                        format!(
                            "Data '{}' is a spec reference; cannot provide a value.",
                            name
                        ),
                        None::<String>,
                    ));
                }
            };

            let literal_value = match parse_data_value(&raw_value, &type_arc, &data_source) {
                Ok(value) => value,
                Err(error) => {
                    overlay
                        .violated
                        .insert(data_path, error.message().to_string());
                    continue;
                }
            };

            let size = literal_value.byte_size();
            if size > limits.max_data_value_bytes {
                return Err(Error::resource_limit_exceeded(
                    "max_data_value_bytes",
                    limits.max_data_value_bytes.to_string(),
                    size.to_string(),
                    format!(
                        "Reduce the size of data values to {} bytes or less",
                        limits.max_data_value_bytes
                    ),
                    Some(data_source.clone()),
                    None,
                    None,
                )
                .with_related_data(&name));
            }

            if let Err(message) = validate_value_against_type(type_arc.as_ref(), &literal_value) {
                overlay.violated.insert(data_path, message);
                continue;
            }

            overlay.values.insert(data_path, Arc::new(literal_value));
        }

        Ok(overlay)
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty() && self.violated.is_empty()
    }

    /// Caller-supplied value for branch-skip analysis.
    ///
    /// Returns `None` when the path is missing or listed in [`Self::violated`].
    pub(crate) fn supplied_value(&self, data_path: &DataPath) -> Option<&LiteralValue> {
        if self.violated.contains_key(data_path) {
            None
        } else {
            self.values.get(data_path).map(|value| value.as_ref())
        }
    }
}

fn schema_prefilled(data: &DataDefinition) -> Option<LiteralValue> {
    data.prefilled_value().cloned()
}

fn schema_supplied(path: &DataPath, overlay: &DataOverlay) -> Option<LiteralValue> {
    overlay.supplied_value(path).cloned()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecSchema {
    /// Resolved spec id (logical name including path segments).
    pub spec: String,
    /// Optional commentary from the `"""..."""` block in the spec source.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub commentary: Option<String>,
    /// The effective date of this specific plan version. `None` for origin (unversioned) specs.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub effective: Option<DateTimeValue>,
    /// All known effective-from dates for this spec, populated by the caller when multiple
    /// temporal versions exist. Empty for single-version specs.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub versions: Vec<DateTimeValue>,
    /// Data (inputs) keyed by name.
    pub data: indexmap::IndexMap<String, DataEntry>,
    /// Rules (outputs) keyed by name, with their computed result types
    pub rules: indexmap::IndexMap<String, LemmaType>,
    /// Spec metadata
    pub meta: HashMap<String, MetaValue>,
}

impl std::fmt::Display for SpecSchema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Spec: {}", self.spec)?;

        if let Some(commentary) = &self.commentary {
            write!(f, "\n  {}", commentary)?;
        }

        if !self.meta.is_empty() {
            write!(f, "\n\nMeta:")?;
            // Sort keys for deterministic output
            let mut entries: Vec<(&String, &MetaValue)> = self.meta.iter().collect();
            entries.sort_by_key(|(k, _)| *k);
            for (key, value) in entries {
                write!(f, "\n  {}: {}", key, value)?;
            }
        }

        if !self.data.is_empty() {
            write!(f, "\n\nData:")?;
            for (name, entry) in &self.data {
                write!(f, "\n  {} ({})", name, entry.lemma_type.specifications)?;
                for line in type_detail_lines(&entry.lemma_type.specifications) {
                    write!(f, "\n    {}", line)?;
                }
                let help = entry.lemma_type.specifications.help();
                if !help.is_empty() {
                    write!(f, "\n    help: {}", help)?;
                }
                if let Some(val) = &entry.prefilled {
                    write!(f, "\n    prefilled: {}", val)?;
                }
                if let Some(val) = &entry.supplied {
                    write!(f, "\n    supplied: {}", val)?;
                }
                if let Some(val) = &entry.default {
                    write!(f, "\n    default: {}", val)?;
                }
            }
        }

        if !self.rules.is_empty() {
            write!(f, "\n\nRules:")?;
            for (name, rule_type) in &self.rules {
                write!(f, "\n  {} ({})", name, rule_type.specifications)?;
            }
        }

        if self.data.is_empty() && self.rules.is_empty() {
            write!(f, "\n  (no data or rules)")?;
        }

        Ok(())
    }
}

/// Produce a human-readable summary of type constraints, or `None` when there
/// are no constraints worth showing (e.g. bare `boolean`).
/// Returns one formatted string per constraint or property of the type specification.
/// Uses `rational_to_display_str` for all rational bounds so they render as decimals,
/// not as raw fractions.
pub fn type_detail_lines(spec: &TypeSpecification) -> Vec<String> {
    use crate::computation::rational::rational_to_display_str;
    let mut lines = Vec::new();
    match spec {
        TypeSpecification::Measure {
            minimum,
            maximum,
            decimals,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                lines.push(format!("units: {}", unit_names.join(", ")));
            }
            if let Some(d) = decimals {
                lines.push(format!("decimals: {}", d));
            }
            if let Some((magnitude, unit_name)) = minimum {
                lines.push(format!(
                    "minimum: {} {}",
                    rational_to_display_str(magnitude),
                    unit_name
                ));
            }
            if let Some((magnitude, unit_name)) = maximum {
                lines.push(format!(
                    "maximum: {} {}",
                    rational_to_display_str(magnitude),
                    unit_name
                ));
            }
        }
        TypeSpecification::Number {
            minimum,
            maximum,
            decimals,
            ..
        } => {
            if let Some(d) = decimals {
                lines.push(format!("decimals: {}", d));
            }
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", rational_to_display_str(v)));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", rational_to_display_str(v)));
            }
        }
        TypeSpecification::Ratio {
            minimum,
            maximum,
            decimals,
            units,
            ..
        } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                lines.push(format!("units: {}", unit_names.join(", ")));
            }
            if let Some(d) = decimals {
                lines.push(format!("decimals: {}", d));
            }
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", rational_to_display_str(v)));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", rational_to_display_str(v)));
            }
        }
        TypeSpecification::Text {
            options, length, ..
        } => {
            if let Some(l) = length {
                lines.push(format!("length: {}", l));
            }
            if !options.is_empty() {
                let quoted: Vec<String> = options.iter().map(|o| format!("\"{}\"", o)).collect();
                lines.push(format!("options: {}", quoted.join(", ")));
            }
        }
        TypeSpecification::Date {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::Time {
            minimum, maximum, ..
        } => {
            if let Some(v) = minimum {
                lines.push(format!("minimum: {}", v));
            }
            if let Some(v) = maximum {
                lines.push(format!("maximum: {}", v));
            }
        }
        TypeSpecification::MeasureRange { units, .. } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                lines.push(format!("units: {}", unit_names.join(", ")));
            }
        }
        TypeSpecification::RatioRange { units, .. } => {
            let unit_names: Vec<&str> = units.0.iter().map(|u| u.name.as_str()).collect();
            if !unit_names.is_empty() {
                lines.push(format!("units: {}", unit_names.join(", ")));
            }
        }
        TypeSpecification::Boolean { .. }
        | TypeSpecification::NumberRange { .. }
        | TypeSpecification::DateRange { .. }
        | TypeSpecification::TimeRange { .. }
        | TypeSpecification::Veto { .. }
        | TypeSpecification::Undetermined => {}
    }
    lines
}

impl ExecutionPlan {
    /// Expression-scope unit index (local types plus direct `uses` imports).
    /// Rule-result units outside this scope are resolved from [`ExecutableRule::rule_type`]
    /// at materialization time.
    pub(crate) fn expression_unit_index(&self) -> &HashMap<String, Arc<LemmaType>> {
        &self.resolved_types.unit_index
    }

    /// Build a [`SpecSchema`] describing this plan's public IO contract.
    ///
    /// Only data transitively reachable from live branches of at least one local
    /// rule is included. Spec-reference data (which have no schema type) are also
    /// excluded. Only local rules (no cross-spec segments) are included. Data are
    /// sorted local-first, then by source position within each depth.
    ///
    /// If the caller supplies a [`DataOverlay`], branches whose conditions are
    /// definitively false given those values are pruned, reducing the returned
    /// data set to only what is still needed.
    /// Names of local (main-spec) rules in plan topological order.
    pub fn local_rule_names(&self) -> Vec<String> {
        self.rules
            .iter()
            .filter(|r| r.path.segments.is_empty())
            .map(|r| r.name.clone())
            .collect()
    }

    pub fn schema(&self, overlay: &DataOverlay) -> SpecSchema {
        let all_local_rules = self.local_rule_names();
        self.schema_for_rules(&all_local_rules, overlay)
            .expect("BUG: all_local_rules sourced from self.rules")
    }

    /// Every typed data input and local rule — the full spec surface.
    ///
    /// Excludes [`DataDefinition::Reference`] (`with` bindings) and imports; those
    /// are not caller inputs.
    pub fn interface_schema(&self, overlay: &DataOverlay) -> SpecSchema {
        let mut data_entries: Vec<(usize, usize, String, DataEntry)> = self
            .data
            .iter()
            .filter(|(_, data)| {
                data.schema_type().is_some() && !matches!(data, DataDefinition::Reference { .. })
            })
            .map(|(path, data)| {
                let lemma_type = data
                    .schema_type()
                    .expect("BUG: filter above ensured schema_type is Some")
                    .clone();
                let prefilled = schema_prefilled(data);
                let supplied = schema_supplied(path, overlay);
                let default = data.default_suggestion();
                (
                    path.segments.len(),
                    data.source().span.start,
                    path.input_key(),
                    DataEntry {
                        lemma_type,
                        prefilled,
                        supplied,
                        default,
                    },
                )
            })
            .collect();
        data_entries.sort_by_key(|(depth, pos, _, _)| (*depth, *pos));

        let rule_entries: Vec<(String, LemmaType)> = self
            .rules
            .iter()
            .filter(|r| r.path.segments.is_empty())
            .map(|r| (r.name.clone(), (*r.rule_type).clone()))
            .collect();

        SpecSchema {
            spec: self.spec_name.clone(),
            commentary: self.commentary.clone(),
            effective: self.effective.as_ref().cloned(),
            versions: Vec::new(),
            data: data_entries
                .into_iter()
                .map(|(_, _, name, data)| (name, data))
                .collect(),
            rules: rule_entries.into_iter().collect(),
            meta: self.meta.clone(),
        }
    }

    /// Build a [`SpecSchema`] scoped to specific rules.
    ///
    /// The returned schema contains only the data **needed** by the given rules
    /// (transitively, through live arms of normalized expressions)
    /// and only those rules. This is the "what do I need to evaluate these rules?"
    /// view. [`DataDefinition::Reference`] entries (`with` bindings) are omitted —
    /// their values are fixed in the spec.
    ///
    /// When the caller supplies a [`DataOverlay`], branches whose conditions are
    /// definitively false given those values are pruned, reducing the returned
    /// data set to only what is still needed.
    ///
    /// Data are sorted local-first, then by source position within each depth.
    ///
    /// Returns `Err` if any rule name is not found in the plan.
    pub fn schema_for_rules(
        &self,
        rule_names: &[String],
        overlay: &DataOverlay,
    ) -> Result<SpecSchema, Error> {
        let mut rule_entries: Vec<(String, LemmaType)> = Vec::new();
        for rule_name in rule_names {
            let rule = self.get_rule(rule_name).ok_or_else(|| {
                Error::request(
                    format!(
                        "Rule '{}' not found in spec '{}'",
                        rule_name, self.spec_name
                    ),
                    None::<String>,
                )
            })?;
            rule_entries.push((rule.name.clone(), (*rule.rule_type).clone()));
        }

        let needed_data = self.collect_needed_data_paths(rule_names, overlay)?;

        let mut data_entries: Vec<(usize, usize, String, DataEntry)> = self
            .data
            .iter()
            .filter(|(path, _)| needed_data.contains(path))
            .filter(|(_, data)| !matches!(data, DataDefinition::Reference { .. }))
            .filter_map(|(path, data)| {
                let lemma_type = data.schema_type()?.clone();
                let prefilled = schema_prefilled(data);
                let supplied = schema_supplied(path, overlay);
                let default = data.default_suggestion();
                Some((
                    path.segments.len(),
                    data.source().span.start,
                    path.input_key(),
                    DataEntry {
                        lemma_type,
                        prefilled,
                        supplied,
                        default,
                    },
                ))
            })
            .collect();
        data_entries.sort_by_key(|(depth, pos, _, _)| (*depth, *pos));
        let data_entries: Vec<(String, DataEntry)> = data_entries
            .into_iter()
            .map(|(_, _, name, data)| (name, data))
            .collect();

        Ok(SpecSchema {
            spec: self.spec_name.clone(),
            commentary: self.commentary.clone(),
            effective: self.effective.as_ref().cloned(),
            versions: Vec::new(),
            data: data_entries.into_iter().collect(),
            rules: rule_entries.into_iter().collect(),
            meta: self.meta.clone(),
        })
    }

    /// Look up a data by its input key (e.g., "age" or "rules.base_price").
    pub fn get_data_path_by_str(&self, name: &str) -> Option<&DataPath> {
        let canonical_name = crate::parsing::ast::ascii_lowercase_logical_name(name.to_string());
        self.data
            .keys()
            .find(|path| path.input_key() == canonical_name)
    }

    /// Validate caller-requested rule names and return canonical local rule names.
    ///
    /// `None` means all local rules. `Some(&[])` is an error. Unknown names in `Some` slice error.
    pub fn validated_response_rule_names(
        &self,
        rules: Option<&[String]>,
    ) -> Result<std::collections::HashSet<String>, Error> {
        let Some(rules) = rules else {
            return Ok(self.local_rule_names().into_iter().collect());
        };
        if rules.is_empty() {
            return Err(Error::request(
                "at least one rule required".to_string(),
                None::<String>,
            ));
        }
        let mut names = std::collections::HashSet::new();
        for rule_name in rules {
            let rule = self.get_rule(rule_name).ok_or_else(|| {
                Error::request(
                    format!("Rule '{rule_name}' not found in spec '{}'", self.spec_name),
                    None::<String>,
                )
            })?;
            names.insert(rule.name.clone());
        }
        Ok(names)
    }

    /// Look up a local rule by its name (rule in the main spec).
    pub fn get_rule(&self, name: &str) -> Option<&ExecutableRule> {
        let canonical_name = crate::parsing::ast::ascii_lowercase_logical_name(name.to_string());
        self.rules
            .iter()
            .find(|r| r.name == canonical_name && r.path.segments.is_empty())
    }

    /// Collect the data paths statically referenced by the named local rules.
    ///
    /// Walks the live branches of each named rule transitively: rule-target
    /// data references extend the walk to the referenced rules. Branches whose
    /// conditions are definitively decided by caller-supplied overlay values
    /// are skipped, mirroring [`ExecutionPlan::schema_for_rules`].
    ///
    /// Returns `Err` if any rule name is not found in the plan.
    pub fn collect_needed_data_paths(
        &self,
        rule_names: &[String],
        overlay: &DataOverlay,
    ) -> Result<HashSet<DataPath>, Error> {
        let mut needed_data: HashSet<DataPath> = HashSet::new();
        let mut visited_rules: HashSet<RulePath> = HashSet::new();
        let mut rule_worklist: Vec<RulePath> = Vec::new();

        // Validate and seed the worklist with the selected rules.
        for rule_name in rule_names {
            let rule = self.get_rule(rule_name).ok_or_else(|| {
                Error::request(
                    format!(
                        "Rule '{}' not found in spec '{}'",
                        rule_name, self.spec_name
                    ),
                    None::<String>,
                )
            })?;
            rule_worklist.push(rule.path.clone());
        }

        // Walk the live branches of each reachable rule, collecting data paths.
        // Rule-target references discovered via data paths extend the worklist.
        while let Some(rule_path) = rule_worklist.pop() {
            if !visited_rules.insert(rule_path.clone()) {
                continue;
            }

            let rule = self.get_rule_by_path(&rule_path).unwrap_or_else(|| {
                panic!(
                    "BUG: rule path '{}' placed on worklist but not found in plan '{}'",
                    rule_path.rule, self.spec_name
                )
            });

            for (branch_index, branch) in rule.branches.iter().enumerate() {
                if branch_index == 0 {
                    let any_unless_applies = rule.branches[1..].iter().any(|unless_branch| {
                        let unless_condition = unless_branch
                            .condition
                            .as_ref()
                            .expect("BUG: unless branch missing condition");
                        crate::evaluation::partial::unless_condition_truth(
                            unless_condition,
                            self,
                            overlay,
                        ) == Some(true)
                    });
                    if any_unless_applies {
                        continue;
                    }
                } else if let Some(condition) = &branch.condition {
                    if crate::evaluation::partial::unless_condition_truth(condition, self, overlay)
                        == Some(false)
                    {
                        continue;
                    }
                }

                let mut branch_data: HashSet<DataPath> = HashSet::new();
                if let Some(condition) = &branch.condition {
                    condition.collect_data_paths(&mut branch_data);
                }
                branch.result.collect_data_paths(&mut branch_data);

                let mut branch_rules: HashSet<RulePath> = HashSet::new();
                if let Some(condition) = &branch.condition {
                    condition.collect_rule_paths(&mut branch_rules);
                }
                branch.result.collect_rule_paths(&mut branch_rules);

                for data_path in &branch_data {
                    if let Some(DataDefinition::Reference {
                        target: ReferenceTarget::Rule(target_rule),
                        ..
                    }) = self.data.get(data_path)
                    {
                        branch_rules.insert(target_rule.clone());
                    }
                }

                needed_data.extend(branch_data);
                rule_worklist.extend(branch_rules);
            }
        }

        Ok(needed_data)
    }

    /// Look up a rule by its full path.
    pub fn get_rule_by_path(&self, rule_path: &RulePath) -> Option<&ExecutableRule> {
        self.rules.iter().find(|r| &r.path == rule_path)
    }

    /// Get the literal value for a data path, if it exists and has a literal value.
    pub fn get_data_value(&self, path: &DataPath) -> Option<&LiteralValue> {
        self.data.get(path).and_then(|d| d.value())
    }
}

pub(crate) fn validate_value_against_type(
    expected_type: &LemmaType,
    value: &LiteralValue,
) -> Result<(), String> {
    use crate::computation::rational::{commit_rational_to_decimal, RationalInteger};
    use crate::planning::semantics::TypeSpecification;

    fn exceeds_decimal_places(magnitude: &RationalInteger, max_decimals: u8) -> bool {
        match commit_rational_to_decimal(magnitude) {
            Ok(decimal) => decimal.scale() > u32::from(max_decimals),
            Err(_) => true,
        }
    }

    fn format_rational_for_validation_message(
        expected_type: &crate::planning::semantics::LemmaType,
        magnitude: &RationalInteger,
    ) -> String {
        use crate::computation::rational::rational_to_display_str;
        expected_type
            .try_materialize_rational_as_decimal_string(magnitude)
            .unwrap_or_else(|_| rational_to_display_str(magnitude))
    }

    match (&expected_type.specifications, &value.value) {
        (
            TypeSpecification::Number {
                minimum,
                maximum,
                decimals,
                ..
            },
            ValueKind::Number(n),
        ) => {
            if let Some(d) = decimals {
                if exceeds_decimal_places(n, *d) {
                    return Err(format!(
                        "{} exceeds decimals constraint {d}",
                        format_rational_for_validation_message(expected_type, n)
                    ));
                }
            }
            if let Some(min) = minimum {
                if n < min {
                    return Err(format!(
                        "{} is below minimum {}",
                        format_rational_for_validation_message(expected_type, n),
                        format_rational_for_validation_message(expected_type, min)
                    ));
                }
            }
            if let Some(max) = maximum {
                if n > max {
                    return Err(format!(
                        "{} is above maximum {}",
                        format_rational_for_validation_message(expected_type, n),
                        format_rational_for_validation_message(expected_type, max)
                    ));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Measure {
                minimum,
                maximum,
                decimals,
                units,
                ..
            },
            ValueKind::Measure(magnitude, signature),
        ) => {
            use crate::computation::rational::checked_div;
            use crate::planning::semantics::measure_declared_bound_canonical;
            let unit = signature
                .first()
                .map(|(n, _)| n.as_str())
                .expect("BUG: Measure value has empty signature in execution plan validation");
            let measure_unit = units.get(unit)?;
            let factor = &measure_unit.factor;
            let in_unit = checked_div(magnitude, factor).map_err(|failure| {
                format!("cannot de-canonicalize measure for validation: {failure}")
            })?;
            if let Some(d) = decimals {
                if exceeds_decimal_places(&in_unit, *d) {
                    return Err(format!(
                        "{} {unit} exceeds decimals constraint {d}",
                        format_rational_for_validation_message(expected_type, &in_unit)
                    ));
                }
            }
            if let Some(bound) = minimum {
                let canonical_min = measure_declared_bound_canonical(
                    bound,
                    units,
                    expected_type.name().as_str(),
                    "minimum",
                )?;
                if magnitude < &canonical_min {
                    let min_in_unit = checked_div(&canonical_min, factor).map_err(|failure| {
                        format!("cannot de-canonicalize minimum for validation: {failure}")
                    })?;
                    let value_display = format!(
                        "{} {}",
                        format_rational_for_validation_message(expected_type, &in_unit),
                        unit
                    );
                    let bound_display = format!(
                        "{} {}",
                        format_rational_for_validation_message(expected_type, &min_in_unit),
                        measure_unit.name
                    );
                    return Err(format!("{value_display} is below minimum {bound_display}"));
                }
            }
            if let Some(bound) = maximum {
                let canonical_max = measure_declared_bound_canonical(
                    bound,
                    units,
                    expected_type.name().as_str(),
                    "maximum",
                )?;
                if magnitude > &canonical_max {
                    let max_in_unit = checked_div(&canonical_max, factor).map_err(|failure| {
                        format!("cannot de-canonicalize maximum for validation: {failure}")
                    })?;
                    let value_display = format!(
                        "{} {}",
                        format_rational_for_validation_message(expected_type, &in_unit),
                        unit
                    );
                    let bound_display = format!(
                        "{} {}",
                        format_rational_for_validation_message(expected_type, &max_in_unit),
                        measure_unit.name
                    );
                    return Err(format!("{value_display} is above maximum {bound_display}"));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Text {
                length, options, ..
            },
            ValueKind::Text(s),
        ) => {
            let len = s.chars().count();
            if let Some(exact) = length {
                if len != *exact {
                    return Err(format!(
                        "'{}' has length {} but required length is {}",
                        s, len, exact
                    ));
                }
            }
            if !options.is_empty() && !options.iter().any(|opt| opt == s) {
                return Err(format!(
                    "'{}' is not in allowed options: {}",
                    s,
                    options.join(", ")
                ));
            }
            Ok(())
        }
        (
            TypeSpecification::Ratio {
                minimum,
                maximum,
                decimals,
                units,
                ..
            },
            ValueKind::Ratio(r, unit_name),
        ) => {
            use crate::computation::rational::checked_mul;

            if let Some(d) = decimals {
                if exceeds_decimal_places(r, *d) {
                    return Err(format!(
                        "{} exceeds decimals constraint {d}",
                        format_rational_for_validation_message(expected_type, r)
                    ));
                }
            }
            if let Some(type_minimum) = minimum {
                if r < type_minimum {
                    let message = match unit_name.as_deref() {
                        Some(unit) => {
                            let ratio_unit = units.get(unit)?;
                            let value_per_unit = checked_mul(r, &ratio_unit.value)
                                .map_err(|failure| failure.to_string())?;
                            let bound_per_unit = ratio_unit.minimum.clone().expect(
                                "BUG: RatioUnit.minimum missing after type minimum set by sync_ratio_units_from_canonical",
                            );
                            format!(
                                "{} {unit} is below minimum {} {unit}",
                                format_rational_for_validation_message(
                                    expected_type,
                                    &value_per_unit
                                ),
                                format_rational_for_validation_message(
                                    expected_type,
                                    &bound_per_unit.clone()
                                ),
                            )
                        }
                        None => format!(
                            "{} is below minimum {}",
                            format_rational_for_validation_message(expected_type, r),
                            format_rational_for_validation_message(expected_type, type_minimum),
                        ),
                    };
                    return Err(message);
                }
            }
            if let Some(type_maximum) = maximum {
                if r > type_maximum {
                    let message = match unit_name.as_deref() {
                        Some(unit) => {
                            let ratio_unit = units.get(unit)?;
                            let value_per_unit = checked_mul(r, &ratio_unit.value)
                                .map_err(|failure| failure.to_string())?;
                            let bound_per_unit = ratio_unit.maximum.clone().expect(
                                "BUG: RatioUnit.maximum missing after type maximum set by sync_ratio_units_from_canonical",
                            );
                            format!(
                                "{} {unit} is above maximum {} {unit}",
                                format_rational_for_validation_message(
                                    expected_type,
                                    &value_per_unit
                                ),
                                format_rational_for_validation_message(
                                    expected_type,
                                    &bound_per_unit.clone()
                                ),
                            )
                        }
                        None => format!(
                            "{} is above maximum {}",
                            format_rational_for_validation_message(expected_type, r),
                            format_rational_for_validation_message(expected_type, type_maximum),
                        ),
                    };
                    return Err(message);
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Date {
                minimum, maximum, ..
            },
            ValueKind::Date(dt),
        ) => {
            use crate::planning::semantics::{compare_semantic_dates, date_time_to_semantic};
            use std::cmp::Ordering;
            if let Some(min) = minimum {
                let min_sem = date_time_to_semantic(min);
                if compare_semantic_dates(dt, &min_sem) == Ordering::Less {
                    return Err(format!("{} is below minimum {}", dt, min));
                }
            }
            if let Some(max) = maximum {
                let max_sem = date_time_to_semantic(max);
                if compare_semantic_dates(dt, &max_sem) == Ordering::Greater {
                    return Err(format!("{} is above maximum {}", dt, max));
                }
            }
            Ok(())
        }
        (
            TypeSpecification::Time {
                minimum, maximum, ..
            },
            ValueKind::Time(t),
        ) => {
            use crate::planning::semantics::{compare_semantic_times, time_to_semantic};
            use std::cmp::Ordering;
            if let Some(min) = minimum {
                let min_sem = time_to_semantic(min);
                if compare_semantic_times(t, &min_sem) == Ordering::Less {
                    return Err(format!("{} is below minimum {}", t, min));
                }
            }
            if let Some(max) = maximum {
                let max_sem = time_to_semantic(max);
                if compare_semantic_times(t, &max_sem) == Ordering::Greater {
                    return Err(format!("{} is above maximum {}", t, max));
                }
            }
            Ok(())
        }
        (TypeSpecification::Boolean { .. }, ValueKind::Boolean(_))
        | (TypeSpecification::NumberRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::DateRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::TimeRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::MeasureRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::RatioRange { .. }, ValueKind::Range(_, _))
        | (TypeSpecification::Veto { .. }, _)
        | (TypeSpecification::Undetermined, _) => Ok(()),
        (spec, value_kind) if !value_kind_matches_spec(value_kind, spec) => unreachable!(
            "BUG: validate_value_against_type called with mismatched type/value: \
             spec={:?}, value={:?} — typing must be enforced before validation",
            spec, value_kind
        ),
        (_, _) => Ok(()),
    }
}

pub(crate) fn validate_literal_data_against_types(plan: &ExecutionPlan) -> Vec<Error> {
    let mut errors = Vec::new();

    for (data_path, data_definition) in &plan.data {
        let (expected_type, lit) = match data_definition {
            DataDefinition::Value { value, .. } => (&value.lemma_type, value),
            DataDefinition::TypeDeclaration { .. }
            | DataDefinition::Import { .. }
            | DataDefinition::Reference { .. } => continue,
        };

        if let Err(msg) = validate_value_against_type(expected_type, lit) {
            let source = data_definition.source().clone();
            errors.push(Error::validation(
                format!(
                    "Invalid value for data {} (expected {}): {}",
                    data_path,
                    expected_type.name().as_str(),
                    msg
                ),
                Some(source),
                None::<String>,
            ));
        }
    }

    errors
}

pub(crate) fn validate_unit_conversion_targets(plan: &ExecutionPlan) -> Result<(), Error> {
    for rule in &plan.rules {
        for instructions in [&rule.instructions, &rule.source_instructions] {
            for insn in &instructions.code {
                let Instruction::UnitConversion { target, .. } = insn else {
                    continue;
                };
                let Some((unit_name, owning_type)) =
                    crate::computation::units::conversion_target_declares_unit(target)
                else {
                    continue;
                };
                if crate::computation::units::owning_type_declares_unit_name(
                    owning_type.as_ref(),
                    unit_name,
                ) {
                    continue;
                }
                return Err(Error::validation(
                    format!(
                        "Unit conversion target '{unit_name}' is not declared on owning type '{}'",
                        owning_type.name()
                    ),
                    None::<Source>,
                    Some(plan.spec_name.clone()),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_unit_index_references(plan: &ExecutionPlan) -> Result<(), Error> {
    validate_unit_conversion_targets(plan)
}

/// The serializable form of an [`ExecutionPlan`].
///
/// `ExecutionPlan` itself is not `Serialize`/`Deserialize`: it contains derived
/// runtime state (`signature_index`, `resolved_types.resolved`,
/// `resolved_types.declared_defaults`) that is either recomputed on reconstruction
/// or belongs to the planning phase only. This struct is the sole canonical
/// representation for persistence and transport.
///
/// Convert via [`From<&ExecutionPlan>`] to serialize and [`TryFrom<ExecutionPlanSerialized>`]
/// to reconstruct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlanSerialized {
    pub spec_name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub commentary: Option<String>,
    #[serde(
        serialize_with = "serialize_resolved_data_value_map",
        deserialize_with = "deserialize_resolved_data_value_map"
    )]
    pub data: IndexMap<DataPath, DataDefinition>,
    #[serde(default)]
    pub rules: Vec<ExecutableRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_evaluation_order: Vec<DataPath>,
    #[serde(default)]
    pub meta: HashMap<String, MetaValue>,
    /// Only the unit index is persisted from `resolved_types`; the rest is
    /// ephemeral planning state that is not needed after planning.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub unit_index: HashMap<String, Arc<LemmaType>>,
    pub effective: EffectiveDate,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: SpecSources,
}

impl From<&ExecutionPlan> for ExecutionPlanSerialized {
    fn from(plan: &ExecutionPlan) -> Self {
        Self {
            spec_name: plan.spec_name.clone(),
            commentary: plan.commentary.clone(),
            data: plan.data.clone(),
            rules: plan.rules.clone(),
            reference_evaluation_order: plan.reference_evaluation_order.clone(),
            meta: plan.meta.clone(),
            unit_index: plan.resolved_types.unit_index.clone(),
            effective: plan.effective.clone(),
            sources: plan.sources.clone(),
        }
    }
}

impl TryFrom<ExecutionPlanSerialized> for ExecutionPlan {
    type Error = crate::Error;

    fn try_from(serialized: ExecutionPlanSerialized) -> Result<Self, Self::Error> {
        let signature_index = crate::planning::graph::build_signature_index(
            &serialized.spec_name,
            &serialized.unit_index,
        )?;
        // Serialized plans cross a trust boundary: a tampered or stale plan
        // must surface as an error here, never as a hang or crash in the
        // virtual machine.
        for rule in &serialized.rules {
            validate_instructions(&rule.instructions).map_err(|message| {
                crate::Error::request(
                    format!(
                        "Serialized execution plan for spec '{}' contains invalid instructions for rule '{}': {message}",
                        serialized.spec_name, rule.name
                    ),
                    None::<String>,
                )
            })?;
            validate_instructions(&rule.source_instructions).map_err(|message| {
                crate::Error::request(
                    format!(
                        "Serialized execution plan for spec '{}' contains invalid source instructions for rule '{}': {message}",
                        serialized.spec_name, rule.name
                    ),
                    None::<String>,
                )
            })?;
        }
        let max_register_count = serialized
            .rules
            .iter()
            .map(|rule| rule.instructions.register_count)
            .max()
            .unwrap_or(0);
        let plan = Self {
            spec_name: serialized.spec_name,
            commentary: serialized.commentary,
            data: serialized.data,
            rules: serialized.rules,
            max_register_count,
            reference_evaluation_order: serialized.reference_evaluation_order,
            meta: serialized.meta,
            resolved_types: ResolvedSpecTypes {
                unit_index: serialized.unit_index,
                ..ResolvedSpecTypes::default()
            },
            signature_index,
            effective: serialized.effective,
            sources: serialized.sources,
        };
        validate_unit_index_references(&plan).map_err(|error| {
            crate::Error::request(
                format!(
                    "Serialized execution plan for spec '{}' is invalid: {}",
                    plan.spec_name,
                    error.message()
                ),
                None::<String>,
            )
        })?;
        Ok(plan)
    }
}

fn serialize_resolved_data_value_map<S>(
    map: &IndexMap<DataPath, DataDefinition>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let entries: Vec<(&DataPath, &DataDefinition)> = map.iter().collect();
    entries.serialize(serializer)
}

fn deserialize_resolved_data_value_map<'de, D>(
    deserializer: D,
) -> Result<IndexMap<DataPath, DataDefinition>, D::Error>
where
    D: Deserializer<'de>,
{
    let entries: Vec<(DataPath, DataDefinition)> = Vec::deserialize(deserializer)?;
    Ok(entries.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computation::rational::{rational_new, rational_zero};
    use crate::literals::DateGranularity;
    use crate::parsing::ast::DateTimeValue;
    use crate::planning::semantics::{
        primitive_boolean, primitive_text, DataPath, LiteralValue, PathSegment, RulePath,
    };
    use crate::Engine;
    use serde_json;
    use std::str::FromStr;
    use std::sync::Arc;

    fn default_limits() -> ResourceLimits {
        ResourceLimits::default()
    }

    fn roundtrip_execution_plan(plan: &ExecutionPlan) -> ExecutionPlan {
        let serialized = ExecutionPlanSerialized::from(plan);
        let json = serde_json::to_string(&serialized).expect("Should serialize");
        let back: ExecutionPlanSerialized =
            serde_json::from_str(&json).expect("Should deserialize");
        ExecutionPlan::try_from(back).expect("Should reconstruct")
    }

    fn input_data(pairs: &[(&str, &str)]) -> HashMap<String, DataValueInput> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), DataValueInput::convenience(*v)))
            .collect()
    }

    #[test]
    fn test_with_raw_values() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec test
                data age: number -> default 25
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan(None, "test", Some(&now)).unwrap();
        let data_path = DataPath::new(vec![], "age".to_string());

        let values = input_data(&[("age", "30")]);

        let overlay = DataOverlay::resolve(plan, values, &default_limits()).unwrap();
        let updated_value = overlay.values.get(&data_path).unwrap().as_ref();
        match &updated_value.value {
            crate::planning::semantics::ValueKind::Number(n) => {
                assert_eq!(n, &rational_new(30, 1));
            }
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    #[test]
    fn test_with_raw_values_type_mismatch() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec test
                data age: number
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan(None, "test", Some(&now)).unwrap();

        let values = input_data(&[("age", "thirty")]);

        let overlay = DataOverlay::resolve(plan, values, &default_limits()).unwrap();
        let data_path = DataPath::new(vec![], "age".to_string());
        match overlay.violated.get(&data_path) {
            Some(reason) => {
                assert!(
                    reason.contains("number"),
                    "type mismatch must record violation reason, got: {reason}"
                );
            }
            None => panic!("expected violated data for age=thirty"),
        }
    }

    #[test]
    fn test_with_raw_values_unknown_data() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec test
                data known: number
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan(None, "test", Some(&now)).unwrap();

        let values = input_data(&[("unknown", "30")]);

        assert!(DataOverlay::resolve(plan, values, &default_limits()).is_err());
    }

    #[test]
    fn test_with_raw_values_nested() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec private
                data base_price: number

                spec test
                uses rules: private
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();

        let now = DateTimeValue::now();
        let plan = engine.get_plan(None, "test", Some(&now)).unwrap();

        let values = input_data(&[("rules.base_price", "100")]);

        let overlay = DataOverlay::resolve(plan, values, &default_limits()).unwrap();
        let data_path = DataPath {
            segments: vec![PathSegment {
                data: "rules".to_string(),
                spec: "private".to_string(),
            }],
            data: "base_price".to_string(),
        };
        let updated_value = overlay.values.get(&data_path).unwrap().as_ref();
        match &updated_value.value {
            crate::planning::semantics::ValueKind::Number(n) => {
                assert_eq!(n, &rational_new(100, 1));
            }
            other => panic!("Expected number literal, got {:?}", other),
        }
    }

    fn test_source() -> Source {
        use crate::parsing::ast::Span;
        Source::new(
            crate::parsing::source::SourceType::Volatile,
            Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        )
    }

    fn create_literal_expr(value: LiteralValue) -> Expression {
        Expression::new(
            crate::planning::semantics::ExpressionKind::Literal(Box::new(value)),
            test_source(),
        )
    }

    fn create_data_path_expr(path: DataPath) -> Expression {
        Expression::new(
            crate::planning::semantics::ExpressionKind::DataPath(path),
            test_source(),
        )
    }

    fn constant_return_instructions(literal: LiteralValue) -> Instructions {
        Instructions {
            version: INSTRUCTIONS_VERSION,
            register_count: 1,
            register_types: vec![Arc::clone(&literal.lemma_type)],
            constants: vec![Arc::new(literal)],
            data_manifest: Vec::new(),
            veto_messages: Vec::new(),
            arm_tags: Vec::new(),
            conversion_tags: Vec::new(),
            code: vec![
                Instruction::LoadConstant {
                    destination_register: 0,
                    constant_index: 0,
                },
                Instruction::Return { source_register: 0 },
            ],
        }
    }

    fn create_number_literal(n: rust_decimal::Decimal) -> LiteralValue {
        LiteralValue::number_from_decimal(n)
    }

    fn create_boolean_literal(b: bool) -> LiteralValue {
        LiteralValue::from_bool(b)
    }

    fn create_text_literal(s: String) -> LiteralValue {
        LiteralValue::text(s)
    }

    #[test]
    fn with_values_should_enforce_number_maximum_constraint() {
        // Higher-standard requirement: user input must be validated against type constraints.
        // If this test fails, Lemma accepts invalid values and gives false reassurance.
        let data_path = DataPath::new(vec![], "x".to_string());

        let max10 = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Number {
                minimum: None,
                maximum: Some(rational_new(10, 1)),
                decimals: None,
                help: String::new(),
            },
        );
        let source = Source::new(
            crate::parsing::source::SourceType::Volatile,
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: crate::planning::semantics::LiteralValue::number_with_type(
                    rational_new(0, 1),
                    Arc::new(max10.clone()),
                ),
                source: source.clone(),
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let values = input_data(&[("x", "11")]);

        let overlay = DataOverlay::resolve(&plan, values, &default_limits()).unwrap();
        match overlay.violated.get(&data_path) {
            Some(reason) => {
                assert!(
                    reason.contains("maximum") || reason.contains("10"),
                    "x=11 must violate maximum 10, got: {reason}"
                );
            }
            None => panic!("expected violated data for x=11"),
        }
    }

    #[test]
    fn with_values_should_enforce_text_enum_options() {
        // Higher-standard requirement: enum options must be enforced for text types.
        let data_path = DataPath::new(vec![], "tier".to_string());

        let tier = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Text {
                length: None,
                options: vec!["silver".to_string(), "gold".to_string()],
                help: String::new(),
            },
        );
        let source = Source::new(
            crate::parsing::source::SourceType::Volatile,
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: crate::planning::semantics::LiteralValue::text_with_type(
                    "silver".to_string(),
                    Arc::new(tier.clone()),
                ),
                source,
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let values = input_data(&[("tier", "platinum")]);

        let overlay = DataOverlay::resolve(&plan, values, &default_limits()).unwrap();
        match overlay.violated.get(&data_path) {
            Some(reason) => {
                assert!(
                    reason.contains("allowed options") || reason.contains("platinum"),
                    "invalid enum must record violation, got: {reason}"
                );
            }
            None => panic!("expected violated data for tier=platinum"),
        }
    }

    #[test]
    fn with_values_should_enforce_measure_decimals() {
        // Higher-standard requirement: decimals should be enforced on measure inputs,
        // unless the language explicitly defines rounding semantics.
        let data_path = DataPath::new(vec![], "price".to_string());

        let money = crate::planning::semantics::LemmaType::primitive(
            crate::planning::semantics::TypeSpecification::Measure {
                minimum: None,
                maximum: None,
                decimals: Some(2),
                units: crate::planning::semantics::MeasureUnits::from(vec![
                    crate::planning::semantics::MeasureUnit::from_decimal_factor(
                        "eur".to_string(),
                        rust_decimal::Decimal::from_str("1.0").unwrap(),
                        Vec::new(),
                    )
                    .expect("eur unit factor must be exact decimal"),
                ]),
                traits: Vec::new(),
                decomposition: None,
                help: String::new(),
            },
        );
        let source = Source::new(
            crate::parsing::source::SourceType::Volatile,
            crate::parsing::ast::Span {
                start: 0,
                end: 0,
                line: 1,
                col: 0,
            },
        );
        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: crate::planning::semantics::LiteralValue::measure_with_type(
                    rational_zero(),
                    "eur".to_string(),
                    Arc::new(money.clone()),
                ),
                source,
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let values = input_data(&[("price", "1.234 eur")]);

        let overlay = DataOverlay::resolve(&plan, values, &default_limits()).unwrap();
        match overlay.violated.get(&data_path) {
            Some(reason) => {
                assert!(
                    reason.contains("decimals") || reason.contains("decimal"),
                    "1.234 eur must violate decimals=2, got: {reason}"
                );
            }
            None => panic!("expected violated data for price=1.234 eur"),
        }
    }

    #[test]
    fn test_serialize_deserialize_execution_plan() {
        let data_path = DataPath {
            segments: vec![],
            data: "age".to_string(),
        };
        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let deserialized = roundtrip_execution_plan(&plan);

        assert_eq!(deserialized.spec_name, plan.spec_name);
        assert_eq!(deserialized.data.len(), plan.data.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
    }

    #[test]
    fn test_serialize_deserialize_plan_with_imported_named_type_defining_spec() {
        let dep_spec = Arc::new(crate::parsing::ast::LemmaSpec::new("examples".to_string()));
        let imported_type = crate::planning::semantics::LemmaType::new(
            "salary".to_string(),
            TypeSpecification::measure(),
            crate::planning::semantics::TypeExtends::Custom {
                parent: "money".to_string(),
                family: "money".to_string(),
                defining_spec: crate::planning::semantics::TypeDefiningSpec::Import {
                    spec: Arc::clone(&dep_spec),
                },
            },
        );

        let salary_path = DataPath::new(vec![], "salary".to_string());
        let mut data = IndexMap::new();
        data.insert(
            salary_path,
            crate::planning::semantics::DataDefinition::TypeDeclaration {
                resolved_type: Arc::new(imported_type),
                declared_default: None,
                source: test_source(),
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let deserialized = roundtrip_execution_plan(&plan);

        let recovered = deserialized
            .data
            .get(&DataPath::new(vec![], "salary".to_string()))
            .and_then(|d| d.schema_type())
            .expect("salary type should be present in plan.data");
        match &recovered.extends {
            crate::planning::semantics::TypeExtends::Custom {
                defining_spec: crate::planning::semantics::TypeDefiningSpec::Import { spec },
                ..
            } => {
                assert_eq!(spec.name, "examples");
            }
            other => panic!(
                "Expected imported defining_spec after round-trip, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_serialize_deserialize_plan_with_rules() {
        use crate::planning::semantics::ExpressionKind;

        let age_path = DataPath::new(vec![], "age".to_string());
        let mut data = IndexMap::new();
        data.insert(
            age_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "can_drive".to_string()),
            name: "can_drive".to_string(),
            branches: vec![{
                let result = create_literal_expr(create_boolean_literal(true));
                let condition = Expression::new(
                    ExpressionKind::Comparison(
                        Arc::new(create_data_path_expr(age_path.clone())),
                        crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                        Arc::new(create_literal_expr(create_number_literal(18.into()))),
                    ),
                    test_source(),
                );
                Branch {
                    condition: Some(condition.clone()),
                    result: result.clone(),
                    source: test_source(),
                }
            }],
            instructions: constant_return_instructions(create_boolean_literal(true)),
            source_instructions: constant_return_instructions(create_boolean_literal(true)),
            source: test_source(),
            rule_type: Arc::new(primitive_boolean().clone()),
        };

        plan.rules.push(rule);
        plan.max_register_count = plan.rules[0].instructions.register_count;

        let deserialized = roundtrip_execution_plan(&plan);

        assert_eq!(deserialized.spec_name, plan.spec_name);
        assert_eq!(deserialized.data.len(), plan.data.len());
        assert_eq!(deserialized.rules.len(), plan.rules.len());
        assert_eq!(deserialized.rules[0].name, "can_drive");
        assert_eq!(deserialized.rules[0].branches.len(), 1);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_nested_data_paths() {
        use crate::planning::semantics::PathSegment;
        let data_path = DataPath {
            segments: vec![PathSegment {
                data: "employee".to_string(),
                spec: "private".to_string(),
            }],
            data: "salary".to_string(),
        };

        let mut data = IndexMap::new();
        data.insert(
            data_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let deserialized = roundtrip_execution_plan(&plan);

        assert_eq!(deserialized.data.len(), 1);
        let (deserialized_path, _) = deserialized.data.iter().next().unwrap();
        assert_eq!(deserialized_path.segments.len(), 1);
        assert_eq!(deserialized_path.segments[0].data, "employee");
        assert_eq!(deserialized_path.data, "salary");
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_data_types() {
        let name_path = DataPath::new(vec![], "name".to_string());
        let age_path = DataPath::new(vec![], "age".to_string());
        let active_path = DataPath::new(vec![], "active".to_string());

        let mut data = IndexMap::new();
        data.insert(
            name_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_text_literal("Alice".to_string()),
                source: test_source(),
            },
        );
        data.insert(
            age_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(30.into()),
                source: test_source(),
            },
        );
        data.insert(
            active_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_boolean_literal(true),
                source: test_source(),
            },
        );

        let plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let deserialized = roundtrip_execution_plan(&plan);

        assert_eq!(deserialized.data.len(), 3);

        assert_eq!(
            deserialized.get_data_value(&name_path).unwrap().value,
            crate::planning::semantics::ValueKind::Text("Alice".to_string())
        );
        assert_eq!(
            deserialized.get_data_value(&age_path).unwrap().value,
            crate::planning::semantics::ValueKind::Number(rational_new(30, 1))
        );
        assert_eq!(
            deserialized.get_data_value(&active_path).unwrap().value,
            crate::planning::semantics::ValueKind::Boolean(true)
        );
    }

    #[test]
    fn test_serialize_deserialize_plan_with_multiple_branches() {
        use crate::planning::semantics::ExpressionKind;

        let points_path = DataPath::new(vec![], "points".to_string());
        let mut data = IndexMap::new();
        data.insert(
            points_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "tier".to_string()),
            name: "tier".to_string(),
            branches: vec![
                {
                    let result = create_literal_expr(create_text_literal("bronze".to_string()));
                    Branch {
                        condition: None,
                        result: result.clone(),
                        source: test_source(),
                    }
                },
                {
                    let result = create_literal_expr(create_text_literal("silver".to_string()));
                    Branch {
                        condition: Some(Expression::new(
                            ExpressionKind::Comparison(
                                Arc::new(create_data_path_expr(points_path.clone())),
                                crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                                Arc::new(create_literal_expr(create_number_literal(100.into()))),
                            ),
                            test_source(),
                        )),
                        result: result.clone(),
                        source: test_source(),
                    }
                },
                {
                    let result = create_literal_expr(create_text_literal("gold".to_string()));
                    Branch {
                        condition: Some(Expression::new(
                            ExpressionKind::Comparison(
                                Arc::new(create_data_path_expr(points_path.clone())),
                                crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                                Arc::new(create_literal_expr(create_number_literal(500.into()))),
                            ),
                            test_source(),
                        )),
                        result: result.clone(),
                        source: test_source(),
                    }
                },
            ],
            instructions: constant_return_instructions(create_text_literal("bronze".to_string())),
            source_instructions: constant_return_instructions(create_text_literal(
                "bronze".to_string(),
            )),
            source: test_source(),
            rule_type: Arc::new(primitive_text().clone()),
        };

        plan.rules.push(rule);
        plan.max_register_count = plan.rules[0].instructions.register_count;

        let deserialized = roundtrip_execution_plan(&plan);

        assert_eq!(deserialized.rules.len(), 1);
        assert_eq!(deserialized.rules[0].branches.len(), 3);
        assert!(deserialized.rules[0].branches[0].condition.is_none());
        assert!(deserialized.rules[0].branches[1].condition.is_some());
        assert!(deserialized.rules[0].branches[2].condition.is_some());
    }

    #[test]
    fn test_serialize_deserialize_empty_plan() {
        let plan = ExecutionPlan {
            spec_name: "empty".to_string(),
            commentary: None,
            data: IndexMap::new(),
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let deserialized = roundtrip_execution_plan(&plan);

        assert_eq!(deserialized.spec_name, "empty");
        assert_eq!(deserialized.data.len(), 0);
        assert_eq!(deserialized.rules.len(), 0);
    }

    #[test]
    fn test_serialize_deserialize_plan_with_arithmetic_expressions() {
        use crate::planning::semantics::ExpressionKind;

        let x_path = DataPath::new(vec![], "x".to_string());
        let mut data = IndexMap::new();
        data.insert(
            x_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "doubled".to_string()),
            name: "doubled".to_string(),
            branches: vec![{
                let result = Expression::new(
                    ExpressionKind::Arithmetic(
                        Arc::new(create_data_path_expr(x_path.clone())),
                        crate::parsing::ast::ArithmeticComputation::Multiply,
                        Arc::new(create_literal_expr(create_number_literal(2.into()))),
                    ),
                    test_source(),
                );
                Branch {
                    condition: None,
                    result: result.clone(),
                    source: test_source(),
                }
            }],
            instructions: constant_return_instructions(create_number_literal(0.into())),
            source_instructions: constant_return_instructions(create_number_literal(0.into())),
            source: test_source(),
            rule_type: Arc::new(crate::planning::semantics::primitive_number().clone()),
        };

        plan.rules.push(rule);
        plan.max_register_count = plan.rules[0].instructions.register_count;

        let deserialized = roundtrip_execution_plan(&plan);

        assert_eq!(deserialized.rules.len(), 1);
        match &deserialized.rules[0].branches[0].result.kind {
            ExpressionKind::Arithmetic(left, op, right) => {
                assert_eq!(*op, crate::parsing::ast::ArithmeticComputation::Multiply);
                match &left.kind {
                    ExpressionKind::DataPath(_) => {}
                    _ => panic!("Expected DataPath in left operand"),
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
        use crate::planning::semantics::ExpressionKind;

        let age_path = DataPath::new(vec![], "age".to_string());
        let mut data = IndexMap::new();
        data.insert(
            age_path.clone(),
            crate::planning::semantics::DataDefinition::Value {
                value: create_number_literal(0.into()),
                source: test_source(),
            },
        );
        let mut plan = ExecutionPlan {
            spec_name: "test".to_string(),
            commentary: None,
            data,
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective: EffectiveDate::Origin,
            sources: Vec::new(),
        };

        let rule = ExecutableRule {
            path: RulePath::new(vec![], "is_adult".to_string()),
            name: "is_adult".to_string(),
            branches: vec![{
                let result = create_literal_expr(create_boolean_literal(true));
                let condition = Expression::new(
                    ExpressionKind::Comparison(
                        Arc::new(create_data_path_expr(age_path.clone())),
                        crate::parsing::ast::ComparisonComputation::GreaterThanOrEqual,
                        Arc::new(create_literal_expr(create_number_literal(18.into()))),
                    ),
                    test_source(),
                );
                Branch {
                    condition: Some(condition.clone()),
                    result: result.clone(),
                    source: test_source(),
                }
            }],
            instructions: constant_return_instructions(create_boolean_literal(true)),
            source_instructions: constant_return_instructions(create_boolean_literal(true)),
            source: test_source(),
            rule_type: Arc::new(primitive_boolean().clone()),
        };

        plan.rules.push(rule);
        plan.max_register_count = plan.rules[0].instructions.register_count;

        let deserialized = roundtrip_execution_plan(&plan);
        let deserialized2 = roundtrip_execution_plan(&deserialized);

        assert_eq!(deserialized2.spec_name, plan.spec_name);
        assert_eq!(deserialized2.data.len(), plan.data.len());
        assert_eq!(deserialized2.rules.len(), plan.rules.len());
        assert_eq!(deserialized2.rules[0].name, plan.rules[0].name);
        assert_eq!(
            deserialized2.rules[0].branches.len(),
            plan.rules[0].branches.len()
        );
    }

    fn empty_plan(effective: crate::parsing::ast::EffectiveDate) -> ExecutionPlan {
        ExecutionPlan {
            spec_name: "s".into(),
            commentary: None,
            data: IndexMap::new(),
            rules: Vec::new(),
            max_register_count: 0,
            reference_evaluation_order: Vec::new(),
            meta: HashMap::new(),
            resolved_types: ResolvedSpecTypes::default(),
            signature_index: HashMap::new(),
            effective,
            sources: Vec::new(),
        }
    }

    #[test]
    fn plan_at_exact_boundary_selects_later_slice() {
        use crate::parsing::ast::{DateTimeValue, EffectiveDate};

        let june = DateTimeValue {
            year: 2025,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };
        let dec = DateTimeValue {
            year: 2025,
            month: 12,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };

        let set = ExecutionPlanSet {
            spec_name: "s".into(),
            plans: vec![
                empty_plan(EffectiveDate::Origin),
                empty_plan(EffectiveDate::DateTimeValue(june.clone())),
                empty_plan(EffectiveDate::DateTimeValue(dec.clone())),
            ],
        };

        assert!(std::ptr::eq(
            set.plan_at(&EffectiveDate::DateTimeValue(june.clone()))
                .expect("boundary instant"),
            &set.plans[1]
        ));
        assert!(std::ptr::eq(
            set.plan_at(&EffectiveDate::DateTimeValue(dec.clone()))
                .expect("dec boundary"),
            &set.plans[2]
        ));
    }

    #[test]
    fn plan_at_day_before_boundary_stays_in_earlier_slice() {
        use crate::parsing::ast::{DateTimeValue, EffectiveDate};

        let june = DateTimeValue {
            year: 2025,
            month: 6,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };
        let may_end = DateTimeValue {
            year: 2025,
            month: 5,
            day: 31,
            hour: 23,
            minute: 59,
            second: 59,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::DateTime,
        };

        let set = ExecutionPlanSet {
            spec_name: "s".into(),
            plans: vec![
                empty_plan(EffectiveDate::Origin),
                empty_plan(EffectiveDate::DateTimeValue(june)),
            ],
        };

        assert!(std::ptr::eq(
            set.plan_at(&EffectiveDate::DateTimeValue(may_end))
                .expect("may 31"),
            &set.plans[0]
        ));
    }

    #[test]
    fn plan_at_single_plan_matches_any_instant_after_start() {
        use crate::parsing::ast::{DateTimeValue, EffectiveDate};

        let t = DateTimeValue {
            year: 2025,
            month: 3,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            timezone: None,

            granularity: DateGranularity::Full,
        };
        let set = ExecutionPlanSet {
            spec_name: "s".into(),
            plans: vec![empty_plan(EffectiveDate::DateTimeValue(DateTimeValue {
                year: 2025,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                microsecond: 0,
                timezone: None,

                granularity: DateGranularity::Full,
            }))],
        };
        assert!(std::ptr::eq(
            set.plan_at(&EffectiveDate::DateTimeValue(t))
                .expect("inside single slice"),
            &set.plans[0]
        ));
    }

    /// The schema JSON shape is the IO contract for every non-Rust consumer
    /// (WASM playground, Hex, HTTP, TypeScript). Nail the exact envelope.
    #[test]
    fn schema_json_shape_contract() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec pricing
                data bridge_height: measure
                  -> unit meter 1
                  -> default 100 meter
                data quantity: number -> minimum 0
                rule cost: bridge_height * quantity
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "test.lemma",
                ))),
            )
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine
            .get_plan(None, "pricing", Some(&now))
            .unwrap()
            .schema(&DataOverlay::default());

        let value: serde_json::Value = serde_json::to_value(&schema).unwrap();

        let bh = &value["data"]["bridge_height"];
        assert!(
            bh.is_object(),
            "data entry must be a named object, not tuple"
        );
        assert!(
            bh.get("type").is_some(),
            "data entry must expose `type` field"
        );
        assert!(
            bh.get("default").is_some(),
            "bridge_height exposes `-> default` as schema default suggestion"
        );
        assert!(
            bh.get("prefilled").is_none(),
            "bridge_height is not prefilled from spec"
        );
        assert!(
            bh.get("supplied").is_none(),
            "bridge_height has no caller overlay"
        );

        let ty = &bh["type"];
        assert_eq!(
            ty["kind"], "measure",
            "kind tag sits on the type object itself"
        );
        assert!(
            ty["units"].is_array(),
            "measure-only fields flatten up to top level"
        );
        assert!(
            ty.get("options").is_none(),
            "text-only fields must not leak"
        );

        let quantity = &value["data"]["quantity"];
        assert_eq!(quantity["type"]["kind"], "number");
        assert!(
            quantity.get("default").is_none(),
            "quantity has no default suggestion"
        );
        assert!(
            quantity.get("prefilled").is_none(),
            "quantity has no prefilled literal"
        );
        assert!(
            quantity.get("supplied").is_none(),
            "quantity has no caller overlay"
        );

        let cost = &value["rules"]["cost"];
        assert_eq!(
            cost["kind"], "measure",
            "rule types use the same flat shape"
        );
        assert!(
            cost["units"].is_array() && !cost["units"].as_array().unwrap().is_empty(),
            "measure rule result types expose declared units"
        );
        assert!(
            cost["units"][0].get("factor").is_some(),
            "measure rule units use factor field"
        );
    }

    #[test]
    fn schema_rule_result_units_contract() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec units_contract
                data money: measure
                  -> unit eur 1
                  -> unit usd 0.91
                data rate: ratio
                  -> unit basis_points 10000
                  -> unit percent 100
                  -> default 500 basis_points
                rule total: money
                rule rate_out: rate
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from(
                    "units_contract.lemma",
                ))),
            )
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine
            .get_plan(None, "units_contract", Some(&now))
            .unwrap()
            .schema(&DataOverlay::default());
        let value: serde_json::Value = serde_json::to_value(&schema).unwrap();

        let money_units = &value["data"]["money"]["type"]["units"];
        assert!(money_units.is_array() && !money_units.as_array().unwrap().is_empty());
        assert!(money_units[0].get("name").is_some());
        assert!(money_units[0].get("factor").is_some());
        assert!(money_units[0]["factor"].get("numer").is_some());
        assert!(money_units[0]["factor"].get("denom").is_some());

        let rate_units = &value["data"]["rate"]["type"]["units"];
        assert!(rate_units.is_array() && !rate_units.as_array().unwrap().is_empty());
        assert!(rate_units[0].get("name").is_some());
        assert!(rate_units[0].get("value").is_some());
        assert!(rate_units[0]["value"].get("numer").is_some());
        assert!(rate_units[0]["value"].get("denom").is_some());

        let total_rule_units = &value["rules"]["total"]["units"];
        let money_unit_names: Vec<_> = money_units
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["name"].as_str().unwrap())
            .collect();
        let total_rule_unit_names: Vec<_> = total_rule_units
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["name"].as_str().unwrap())
            .collect();
        assert_eq!(total_rule_unit_names, money_unit_names);

        let rate_out_rule_units = &value["rules"]["rate_out"]["units"];
        let rate_unit_names: Vec<_> = rate_units
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["name"].as_str().unwrap())
            .collect();
        let rate_out_rule_unit_names: Vec<_> = rate_out_rule_units
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["name"].as_str().unwrap())
            .collect();
        assert_eq!(rate_out_rule_unit_names, rate_unit_names);
    }

    #[test]
    fn schema_json_round_trip_preserves_shape() {
        let mut engine = Engine::new();
        engine
            .load(
                r#"
                spec s
                data age: number -> minimum 0 -> default 18
                data grade: text -> options "A" "B" "C"
                rule adult: age >= 18
                "#,
                crate::SourceType::Path(std::sync::Arc::new(std::path::PathBuf::from("s.lemma"))),
            )
            .unwrap();
        let now = DateTimeValue::now();
        let schema = engine
            .get_plan(None, "s", Some(&now))
            .unwrap()
            .schema(&DataOverlay::default());

        let json = serde_json::to_string(&schema).unwrap();
        let round_tripped: SpecSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(schema, round_tripped);
    }
}

// ---------------------------------------------------------------------------
// ExecutionPlanSet (formerly plan_set.rs)
// ---------------------------------------------------------------------------
