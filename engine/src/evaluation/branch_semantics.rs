//! Single source of truth for branch and veto decision semantics.
//!
//! The tree evaluator consults [`condition_outcome`] for piecewise unless-arm
//! conditions and `and` conjuncts. One rule for both: a vetoed condition
//! propagates its veto; a `true` boolean takes; a `false` boolean does not.

use super::operations::OperationResult;
use crate::planning::semantics::ValueKind;

/// Outcome of inspecting a branch condition or a boolean operand.
#[derive(Debug, Clone)]
pub(crate) enum BranchOutcome {
    /// The condition decided positively: the branch wins or the conjunct passes.
    Taken,
    /// The condition decided negatively: continue with the next branch or
    /// operand (for the deciding operand of a conjunction this means the
    /// conjunction resolves to `false`).
    NotTaken,
    /// This result becomes the value of the enclosing computation (vetoes
    /// that propagate).
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

/// How a piecewise unless-arm condition or an `and` conjunct decides.
///
/// A vetoed condition propagates its veto as the enclosing result. A `true`
/// boolean takes the branch; a `false` boolean does not.
pub(crate) fn condition_outcome(condition: &OperationResult) -> BranchOutcome {
    if condition.vetoed() {
        return BranchOutcome::Propagate(condition.clone());
    }
    if boolean_value(condition, "branch condition") {
        BranchOutcome::Taken
    } else {
        BranchOutcome::NotTaken
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::operations::VetoType;
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
        OperationResult::Veto(VetoType::missing_data(
            DataPath::new(vec![], "x".to_string()),
            None,
        ))
    }

    #[test]
    fn propagates_any_veto() {
        assert!(matches!(
            condition_outcome(&user_veto()),
            BranchOutcome::Propagate(_)
        ));
        assert!(matches!(
            condition_outcome(&missing_data_veto()),
            BranchOutcome::Propagate(_)
        ));
    }

    #[test]
    fn boolean_decides_when_not_vetoed() {
        assert!(matches!(
            condition_outcome(&boolean(true)),
            BranchOutcome::Taken
        ));
        assert!(matches!(
            condition_outcome(&boolean(false)),
            BranchOutcome::NotTaken
        ));
    }
}
