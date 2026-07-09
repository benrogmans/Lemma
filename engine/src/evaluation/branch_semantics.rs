//! Single source of truth for branch and veto decision semantics.
//!
//! Two consumers must agree on how a condition or boolean operand decides:
//!
//! - the virtual machine's [`Instruction::JumpIfFalse`] handler
//!   (`execute_instructions`), which consults [`unless_condition_outcome`];
//! - the instruction compiler (`compile_short_circuit_and`,
//!   `compile_short_circuit_or`, `compile_piecewise_rule` in
//!   `planning::normalize`), which emits jump sequences implementing
//!   [`and_conjunct_outcome`] and [`or_disjunct_outcome`] in instruction form.
//!
//! Explanations no longer evaluate anything: they read recorded VM execution
//! (see `evaluation::explanations`), so they agree with these semantics by
//! construction. [`and_conjunct_outcome`] and [`or_disjunct_outcome`] remain
//! as the executable specification (verified by this module's tests) that
//! the compiled jump sequences implement.
//!
//! [`Instruction::JumpIfFalse`]: crate::planning::execution_plan::Instruction::JumpIfFalse

use super::operations::{OperationResult, VetoType};
use crate::planning::execution_plan::JumpVetoSemantics;
use crate::planning::semantics::ValueKind;

/// Outcome of inspecting a branch condition or a boolean operand.
#[derive(Debug, Clone)]
pub(crate) enum BranchOutcome {
    /// The condition decided positively: the branch wins, the conjunct
    /// passes, or the disjunction resolves to `true`.
    Taken,
    /// The condition decided negatively: continue with the next branch or
    /// operand (for the deciding operand of a conjunction this means the
    /// conjunction resolves to `false`).
    NotTaken,
    /// This result becomes the value of the enclosing computation (vetoes
    /// that propagate, and the last disjunct's value).
    Propagate(OperationResult),
}

/// Read the boolean out of a non-vetoed result. Planning guarantees boolean
/// operands in these positions; anything else is a compiler bug and crashes.
fn boolean_value(result: &OperationResult, context: &str) -> bool {
    match result {
        OperationResult::Value(literal) => match &literal.value {
            ValueKind::Boolean(boolean) => *boolean,
            other => panic!("BUG: {context} expected a boolean, got {other:?}"),
        },
        OperationResult::Veto(veto) => {
            panic!("BUG: {context} inspected for a boolean while vetoed: {veto}")
        }
    }
}

/// How an `unless` condition decides under the given compiled veto semantics.
///
/// - [`JumpVetoSemantics::UnlessRuleReference`] (piecewise branch
///   conditions): any veto propagates as the rule result.
/// - [`JumpVetoSemantics::UnlessExpression`] (conjunct/disjunct checks
///   inside `and`/`or` sequences): a missing-data veto propagates; other
///   vetoes are treated as non-match.
pub(crate) fn unless_condition_outcome(
    condition: &OperationResult,
    veto_semantics: JumpVetoSemantics,
) -> BranchOutcome {
    match veto_semantics {
        JumpVetoSemantics::UnlessRuleReference => {
            if condition.vetoed() {
                return BranchOutcome::Propagate(condition.clone());
            }
        }
        JumpVetoSemantics::UnlessExpression => {
            if matches!(
                condition,
                OperationResult::Veto(VetoType::MissingData { .. })
            ) {
                return BranchOutcome::Propagate(condition.clone());
            }
            if condition.vetoed() {
                return BranchOutcome::NotTaken;
            }
        }
    }
    if boolean_value(condition, "unless condition") {
        BranchOutcome::Taken
    } else {
        BranchOutcome::NotTaken
    }
}

/// How a non-last conjunct decides within `and`: any veto propagates as the
/// conjunction's value; `false` decides the conjunction ([`BranchOutcome::NotTaken`]);
/// `true` passes to the next conjunct ([`BranchOutcome::Taken`]).
///
/// The last conjunct is not inspected through this function: its result
/// (value or veto) is the conjunction's result directly, mirroring the
/// compiled form which computes it into the destination register.
#[cfg(test)]
pub(crate) fn and_conjunct_outcome(conjunct: &OperationResult) -> BranchOutcome {
    if conjunct.vetoed() {
        return BranchOutcome::Propagate(conjunct.clone());
    }
    if boolean_value(conjunct, "and conjunct") {
        BranchOutcome::Taken
    } else {
        BranchOutcome::NotTaken
    }
}

/// How a disjunct decides within `or`.
///
/// The last disjunct is compiled directly into the destination register, so
/// its result (value or veto) is the disjunction's result
/// ([`BranchOutcome::Propagate`]). For non-last disjuncts a missing-data
/// veto propagates, other vetoes are non-match ([`BranchOutcome::NotTaken`]),
/// `true` resolves the disjunction ([`BranchOutcome::Taken`]), and `false`
/// continues with the next disjunct.
///
/// The source tree is binary while the compiled form is flattened n-ary;
/// composing binary nodes with `is_last_disjunct = true` for every right
/// child reproduces the flat semantics exactly (source order is never
/// reordered for `and`/`or`).
#[cfg(test)]
pub(crate) fn or_disjunct_outcome(
    disjunct: &OperationResult,
    is_last_disjunct: bool,
) -> BranchOutcome {
    if is_last_disjunct {
        return BranchOutcome::Propagate(disjunct.clone());
    }
    if matches!(
        disjunct,
        OperationResult::Veto(VetoType::MissingData { .. })
    ) {
        return BranchOutcome::Propagate(disjunct.clone());
    }
    if disjunct.vetoed() {
        return BranchOutcome::NotTaken;
    }
    if boolean_value(disjunct, "or disjunct") {
        BranchOutcome::Taken
    } else {
        BranchOutcome::NotTaken
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::semantics::{DataPath, LiteralValue};

    fn boolean(value: bool) -> OperationResult {
        OperationResult::from_literal(LiteralValue::from_bool(value))
    }

    fn user_veto() -> OperationResult {
        OperationResult::Veto(VetoType::UserDefined {
            message: Some("blocked".to_string()),
        })
    }

    fn missing_data_veto() -> OperationResult {
        OperationResult::Veto(VetoType::MissingData {
            data: DataPath::new(vec![], "x".to_string()),
        })
    }

    #[test]
    fn unless_rule_reference_propagates_any_veto() {
        assert!(matches!(
            unless_condition_outcome(&user_veto(), JumpVetoSemantics::UnlessRuleReference),
            BranchOutcome::Propagate(_)
        ));
        assert!(matches!(
            unless_condition_outcome(&missing_data_veto(), JumpVetoSemantics::UnlessRuleReference),
            BranchOutcome::Propagate(_)
        ));
        assert!(matches!(
            unless_condition_outcome(&boolean(true), JumpVetoSemantics::UnlessRuleReference),
            BranchOutcome::Taken
        ));
        assert!(matches!(
            unless_condition_outcome(&boolean(false), JumpVetoSemantics::UnlessRuleReference),
            BranchOutcome::NotTaken
        ));
    }

    #[test]
    fn unless_expression_propagates_only_missing_data() {
        assert!(matches!(
            unless_condition_outcome(&missing_data_veto(), JumpVetoSemantics::UnlessExpression),
            BranchOutcome::Propagate(_)
        ));
        assert!(matches!(
            unless_condition_outcome(&user_veto(), JumpVetoSemantics::UnlessExpression),
            BranchOutcome::NotTaken
        ));
    }

    #[test]
    fn and_conjunct_propagates_every_veto() {
        assert!(matches!(
            and_conjunct_outcome(&user_veto()),
            BranchOutcome::Propagate(_)
        ));
        assert!(matches!(
            and_conjunct_outcome(&missing_data_veto()),
            BranchOutcome::Propagate(_)
        ));
        assert!(matches!(
            and_conjunct_outcome(&boolean(true)),
            BranchOutcome::Taken
        ));
        assert!(matches!(
            and_conjunct_outcome(&boolean(false)),
            BranchOutcome::NotTaken
        ));
    }

    #[test]
    fn or_disjunct_treats_non_missing_vetoes_as_non_match_when_not_last() {
        assert!(matches!(
            or_disjunct_outcome(&user_veto(), false),
            BranchOutcome::NotTaken
        ));
        assert!(matches!(
            or_disjunct_outcome(&missing_data_veto(), false),
            BranchOutcome::Propagate(_)
        ));
        assert!(matches!(
            or_disjunct_outcome(&boolean(true), false),
            BranchOutcome::Taken
        ));
        assert!(matches!(
            or_disjunct_outcome(&boolean(false), false),
            BranchOutcome::NotTaken
        ));
    }

    #[test]
    fn or_last_disjunct_always_propagates_its_result() {
        assert!(matches!(
            or_disjunct_outcome(&user_veto(), true),
            BranchOutcome::Propagate(_)
        ));
        assert!(matches!(
            or_disjunct_outcome(&boolean(false), true),
            BranchOutcome::Propagate(_)
        ));
    }
}
