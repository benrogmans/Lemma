//! Boolean expression simplification using BDDs

use crate::{Expression, ExpressionId, ExpressionKind, LiteralValue};

/// Simplify a boolean expression using BDD-based simplification
pub fn simplify_boolean<F>(
    expr: &Expression,
    try_fold: &F,
    expr_eq: &impl Fn(&Expression, &Expression) -> bool,
) -> crate::LemmaResult<Expression>
where
    F: Fn(&Expression) -> Option<Expression>,
{
    let folded = try_fold(expr).unwrap_or_else(|| expr.clone());

    let mut atoms: Vec<Expression> = Vec::new();
    if let Some(bexpr) = to_bool_expr(&folded, &mut atoms, expr_eq) {
        const MAX_ATOMS: usize = 64;
        if atoms.len() <= MAX_ATOMS {
            let simplified = bexpr.simplify_via_bdd();
            let rebuilt = from_bool_expr(&simplified, &atoms);
            return Ok(try_fold(&rebuilt).unwrap_or(rebuilt));
        }
    }

    Ok(folded)
}

/// Simplify OR expressions using BDD
pub fn simplify_or_expression<F>(
    expr: &Expression,
    try_fold: &F,
    expr_eq: &impl Fn(&Expression, &Expression) -> bool,
) -> Expression
where
    F: Fn(&Expression) -> Option<Expression>,
{
    let folded = try_fold(expr).unwrap_or_else(|| expr.clone());

    let mut atoms: Vec<Expression> = Vec::new();
    if let Some(bexpr) = to_bool_expr(&folded, &mut atoms, expr_eq) {
        const MAX_ATOMS: usize = 64;
        if atoms.len() <= MAX_ATOMS {
            let simplified = bexpr.simplify_via_bdd();
            let rebuilt = from_bool_expr(&simplified, &atoms);
            return try_fold(&rebuilt).unwrap_or(rebuilt);
        }
    }

    folded
}

fn to_bool_expr(
    expr: &Expression,
    atoms: &mut Vec<Expression>,
    expr_eq: &impl Fn(&Expression, &Expression) -> bool,
) -> Option<boolean_expression::Expr<usize>> {
    use boolean_expression::Expr as BExpr;
    use ExpressionKind as EK;

    match &expr.kind {
        EK::Literal(LiteralValue::Boolean(b)) => Some(BExpr::Const(*b)),
        EK::LogicalAnd(l, r) => {
            let lbe = to_bool_expr(l, atoms, expr_eq)?;
            let rbe = to_bool_expr(r, atoms, expr_eq)?;
            Some(BExpr::and(lbe, rbe))
        }
        EK::LogicalOr(l, r) => {
            let lbe = to_bool_expr(l, atoms, expr_eq)?;
            let rbe = to_bool_expr(r, atoms, expr_eq)?;
            Some(BExpr::or(lbe, rbe))
        }
        EK::LogicalNegation(inner, _) => {
            let ibe = to_bool_expr(inner, atoms, expr_eq)?;
            Some(BExpr::not(ibe))
        }
        EK::Comparison(_, _, _) | EK::FactHasAnyValue(_) => {
            let mut idx_opt = None;
            for (i, a) in atoms.iter().enumerate() {
                if expr_eq(a, expr) {
                    idx_opt = Some(i);
                    break;
                }
            }
            let idx = match idx_opt {
                Some(i) => i,
                None => {
                    atoms.push(expr.clone());
                    atoms.len() - 1
                }
            };
            Some(BExpr::Terminal(idx))
        }
        EK::Literal(_)
        | EK::Arithmetic(_, _, _)
        | EK::UnitConversion(_, _)
        | EK::MathematicalComputation(_, _)
        | EK::FactReference(_)
        | EK::RuleReference(_)
        | EK::Veto(_) => None,
    }
}

fn from_bool_expr(be: &boolean_expression::Expr<usize>, atoms: &[Expression]) -> Expression {
    use boolean_expression::Expr as BExpr;
    use ExpressionKind as EK;

    match be {
        BExpr::Const(b) => Expression::new(
            EK::Literal(LiteralValue::Boolean(*b)),
            None,
            ExpressionId::new(0),
        ),
        BExpr::Terminal(i) => atoms.get(*i).cloned().unwrap_or_else(|| {
            Expression::new(
                EK::Literal(LiteralValue::Boolean(false)),
                None,
                ExpressionId::new(0),
            )
        }),
        BExpr::Not(inner) => {
            let inner_expr = from_bool_expr(inner, atoms);
            Expression::new(
                EK::LogicalNegation(Box::new(inner_expr), crate::NegationType::Not),
                None,
                ExpressionId::new(0),
            )
        }
        BExpr::And(l, r) => {
            let l_expr = from_bool_expr(l, atoms);
            let r_expr = from_bool_expr(r, atoms);
            Expression::new(
                EK::LogicalAnd(Box::new(l_expr), Box::new(r_expr)),
                None,
                ExpressionId::new(0),
            )
        }
        BExpr::Or(l, r) => {
            let l_expr = from_bool_expr(l, atoms);
            let r_expr = from_bool_expr(r, atoms);
            Expression::new(
                EK::LogicalOr(Box::new(l_expr), Box::new(r_expr)),
                None,
                ExpressionId::new(0),
            )
        }
    }
}
