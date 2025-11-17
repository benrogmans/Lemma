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
    use boolean_expression::Expr;
    use ExpressionKind;

    match &expr.kind {
        ExpressionKind::Literal(LiteralValue::Boolean(b)) => Some(Expr::Const(b.into())),
        ExpressionKind::LogicalAnd(l, r) => {
            let lbe = to_bool_expr(l, atoms, expr_eq)?;
            let rbe = to_bool_expr(r, atoms, expr_eq)?;
            Some(Expr::and(lbe, rbe))
        }
        ExpressionKind::LogicalOr(l, r) => {
            let lbe = to_bool_expr(l, atoms, expr_eq)?;
            let rbe = to_bool_expr(r, atoms, expr_eq)?;
            Some(Expr::or(lbe, rbe))
        }
        ExpressionKind::LogicalNegation(inner, _) => {
            let ibe = to_bool_expr(inner, atoms, expr_eq)?;
            Some(Expr::not(ibe))
        }
        ExpressionKind::Comparison(_, _, _) | ExpressionKind::FactHasAnyValue(_) => {
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
            Some(Expr::Terminal(idx))
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Arithmetic(_, _, _)
        | ExpressionKind::UnitConversion(_, _)
        | ExpressionKind::MathematicalComputation(_, _)
        | ExpressionKind::FactReference(_)
        | ExpressionKind::RuleReference(_)
        | ExpressionKind::Veto(_) => None,
    }
}

fn from_bool_expr(be: &boolean_expression::Expr<usize>, atoms: &[Expression]) -> Expression {
    use boolean_expression::Expr;
    use ExpressionKind;

    match be {
        Expr::Const(b) => Expression::new(
            ExpressionKind::Literal(LiteralValue::Boolean((*b).into())),
            None,
            ExpressionId::new(0),
        ),
        Expr::Terminal(i) => atoms.get(*i).cloned().unwrap_or_else(|| {
            Expression::new(
                ExpressionKind::Literal(LiteralValue::Boolean(crate::BooleanValue::False)),
                None,
                ExpressionId::new(0),
            )
        }),
        Expr::Not(inner) => {
            let inner_expr = from_bool_expr(inner, atoms);
            Expression::new(
                ExpressionKind::LogicalNegation(Box::new(inner_expr), crate::NegationType::Not),
                None,
                ExpressionId::new(0),
            )
        }
        Expr::And(l, r) => {
            let l_expr = from_bool_expr(l, atoms);
            let r_expr = from_bool_expr(r, atoms);
            Expression::new(
                ExpressionKind::LogicalAnd(Box::new(l_expr), Box::new(r_expr)),
                None,
                ExpressionId::new(0),
            )
        }
        Expr::Or(l, r) => {
            let l_expr = from_bool_expr(l, atoms);
            let r_expr = from_bool_expr(r, atoms);
            Expression::new(
                ExpressionKind::LogicalOr(Box::new(l_expr), Box::new(r_expr)),
                None,
                ExpressionId::new(0),
            )
        }
    }
}
