use crate::{
    BranchOutcome, Expression, ExpressionId, ExpressionKind, LemmaError, LemmaResult, LiteralValue,
    OperationResult, ShapeBranch, Target, TargetOp,
};
use std::collections::HashMap;

fn is_boolean_false(expr: &Expression) -> bool {
    matches!(
        expr.kind,
        ExpressionKind::Literal(LiteralValue::Boolean(false))
    )
}

fn expressions_semantically_equal(a: &Expression, b: &Expression) -> bool {
    use ExpressionKind as EK;
    match (&a.kind, &b.kind) {
        (EK::Literal(lit_a), EK::Literal(lit_b)) => lit_a == lit_b,
        (EK::FactReference(ref_a), EK::FactReference(ref_b)) => ref_a.reference == ref_b.reference,
        (EK::RuleReference(ref_a), EK::RuleReference(ref_b)) => ref_a.reference == ref_b.reference,
        (EK::Arithmetic(l1, op1, r1), EK::Arithmetic(l2, op2, r2)) => {
            op1 == op2
                && expressions_semantically_equal(l1, l2)
                && expressions_semantically_equal(r1, r2)
        }
        (EK::LogicalAnd(l1, r1), EK::LogicalAnd(l2, r2))
        | (EK::LogicalOr(l1, r1), EK::LogicalOr(l2, r2)) => {
            expressions_semantically_equal(l1, l2) && expressions_semantically_equal(r1, r2)
        }
        (EK::Comparison(l1, op1, r1), EK::Comparison(l2, op2, r2)) => {
            op1 == op2
                && expressions_semantically_equal(l1, l2)
                && expressions_semantically_equal(r1, r2)
        }
        (EK::LogicalNegation(e1, _), EK::LogicalNegation(e2, _)) => {
            expressions_semantically_equal(e1, e2)
        }
        (EK::MathematicalOperator(op1, e1), EK::MathematicalOperator(op2, e2)) => {
            op1 == op2 && expressions_semantically_equal(e1, e2)
        }
        (EK::UnitConversion(e1, target1), EK::UnitConversion(e2, target2)) => {
            target1 == target2 && expressions_semantically_equal(e1, e2)
        }
        (EK::Veto(v1), EK::Veto(v2)) => v1.message == v2.message,
        _ => false,
    }
}

pub fn invert(
    document: &str,
    rule: &str,
    target: Target,
    given_facts: HashMap<String, LiteralValue>,
    documents: &HashMap<String, crate::LemmaDoc>,
) -> LemmaResult<crate::Shape> {
    fn try_fold(expr: &Expression) -> Option<Expression> {
        crate::inversion::hydration::try_constant_fold(expr, &|val| {
            Expression::new(ExpressionKind::Literal(val), None, ExpressionId::new(0))
        })
    }

    let doc_name = document;

    let get_rule = |rule_ref: &[String]| -> Option<&crate::LemmaRule> {
        let (target_doc, rule_name) = match rule_ref.len() {
            1 => (doc_name, rule_ref[0].as_str()),
            2 => (rule_ref[0].as_str(), rule_ref[1].as_str()),
            _ => return None,
        };
        let doc = documents.get(target_doc)?;
        doc.rules.iter().find(|r| r.name == rule_name)
    };

    let doc = documents
        .get(doc_name)
        .ok_or_else(|| LemmaError::Engine(format!("Document not found: {}", doc_name)))?;

    let rule = doc
        .rules
        .iter()
        .find(|r| r.name == rule)
        .ok_or_else(|| LemmaError::Engine(format!("Rule not found: {}.{}", doc_name, rule)))?;

    let rule_path = format!("{}.{}", doc_name, rule);

    let literal_expr = |val: LiteralValue| {
        Expression::new(ExpressionKind::Literal(val), None, ExpressionId::new(0))
    };
    let logical_and = |a: Expression, b: Expression| {
        Expression::new(
            ExpressionKind::LogicalAnd(Box::new(a), Box::new(b)),
            None,
            ExpressionId::new(0),
        )
    };
    let logical_or = |a: Expression, b: Expression| {
        Expression::new(
            ExpressionKind::LogicalOr(Box::new(a), Box::new(b)),
            None,
            ExpressionId::new(0),
        )
    };
    let logical_not = |a: Expression| {
        Expression::new(
            ExpressionKind::LogicalNegation(Box::new(a), crate::NegationType::Not),
            None,
            ExpressionId::new(0),
        )
    };

    // Build unified piecewise
    let mut all_branches: Vec<(Expression, Expression)> = Vec::new();
    all_branches.push((
        literal_expr(LiteralValue::Boolean(true)),
        rule.expression.clone(),
    ));
    for br in &rule.unless_clauses {
        all_branches.push((br.condition.clone(), br.result.clone()));
    }

    // Compute last-wins effective conditions
    let mut suffix_or: Vec<Option<Expression>> = vec![None; all_branches.len()];
    let mut acc: Option<Expression> = None;
    for i in (0..all_branches.len()).rev() {
        suffix_or[i] = acc.clone();
        let cond = &all_branches[i].0;
        acc = Some(match acc {
            None => cond.clone(),
            Some(prev) => logical_or(cond.clone(), prev),
        });
    }

    // Filter and hydrate branches
    let mut branches_out = Vec::new();
    let mut available_outcomes = Vec::new();

    for (idx, (raw_cond, raw_res)) in all_branches.iter().enumerate() {
        let mut eff_cond = raw_cond.clone();
        if let Some(later_or) = &suffix_or[idx] {
            eff_cond = logical_and(eff_cond, logical_not(later_or.clone()));
        }

        let cond_h = crate::inversion::hydration::hydrate_and_simplify(
            &eff_cond,
            doc_name,
            &given_facts,
            &get_rule,
            &|e, g| crate::inversion::hydration::is_simple_for_expansion(e, g),
            &literal_expr,
        );
        let outcome = match &raw_res.kind {
            ExpressionKind::Veto(ve) => BranchOutcome::Veto(ve.message.clone()),
            _ => {
                let res_h = crate::inversion::hydration::hydrate_and_simplify(
                    raw_res,
                    doc_name,
                    &given_facts,
                    &get_rule,
                    &|e, g| crate::inversion::hydration::is_simple_for_expansion(e, g),
                    &literal_expr,
                );
                BranchOutcome::Value(res_h)
            }
        };

        if !is_boolean_false(&cond_h) {
            let outcome_desc = match &outcome {
                BranchOutcome::Value(expr) => {
                    if let ExpressionKind::Literal(lit) = &expr.kind {
                        format!("value {}", lit)
                    } else {
                        "computed value".to_owned()
                    }
                }
                BranchOutcome::Veto(Some(msg)) => format!("veto '{}'", msg),
                BranchOutcome::Veto(None) => "veto".to_owned(),
            };
            available_outcomes.push(outcome_desc);
        }

        if let Some(branch) = filter_branch(
            cond_h,
            outcome,
            &target,
            doc_name,
            &given_facts,
            &get_rule,
            &try_fold,
            &literal_expr,
            &logical_and,
            &logical_not,
            &logical_or,
        )? {
            branches_out.push(branch);
        }
    }

    if branches_out.is_empty() {
        let target_desc = match &target.outcome {
            None => "any value".to_owned(),
            Some(OperationResult::Value(v)) => format!("value {}", v),
            Some(OperationResult::Veto(Some(msg))) => format!("veto '{}'", msg),
            Some(OperationResult::Veto(None)) => "any veto".to_owned(),
        };

        let mut error_msg = format!(
            "Cannot invert rule '{}' for target {} {}.\n",
            rule_path,
            match target.op {
                TargetOp::Eq => "=",
                TargetOp::Neq => "≠",
                TargetOp::Lt => "<",
                TargetOp::Lte => "≤",
                TargetOp::Gt => ">",
                TargetOp::Gte => "≥",
            },
            target_desc
        );

        if !available_outcomes.is_empty() {
            error_msg.push_str("This rule can produce:\n");
            for (i, outcome) in available_outcomes.iter().enumerate() {
                error_msg.push_str(&format!("  {}: {}\n", i + 1, outcome));
            }
        } else {
            error_msg.push_str("No branches in this rule can be satisfied with the given facts.");
        }

        return Err(LemmaError::Engine(error_msg));
    }

    // Handle single branch case
    if rule.unless_clauses.is_empty()
        && branches_out.len() == 1
        && matches!(target.op, TargetOp::Eq)
    {
        if let BranchOutcome::Value(ref expr_h) = branches_out[0].outcome {
            if let Some(OperationResult::Value(ref val)) = target.outcome {
                let unknowns = find_unknown_facts(expr_h, doc_name, &given_facts);

                if unknowns.len() == 1 {
                    if let Some(mut rhs) = crate::inversion::algebra::algebraic_solve(
                        expr_h,
                        &unknowns[0],
                        &literal_expr(val.clone()),
                        &|fr, d, n| fact_reference_matches(fr, d, n),
                    ) {
                        rhs = try_fold(&rhs).unwrap_or(rhs);

                        let rhs_refs = crate::analysis::extract_references(&rhs);
                        if rhs_refs.rules.is_empty() {
                            let _lhs_fp = crate::FactReference {
                                reference: vec![unknowns[0].0.clone(), unknowns[0].1.clone()],
                            };

                            let comparison = Expression::new(
                                crate::ExpressionKind::Comparison(
                                    Box::new(Expression::new(
                                        crate::ExpressionKind::FactReference(
                                            crate::FactReference {
                                                reference: vec![unknowns[0].1.clone()],
                                            },
                                        ),
                                        None,
                                        crate::ExpressionId::new(0),
                                    )),
                                    crate::ComparisonOperator::Equal,
                                    Box::new(rhs),
                                ),
                                None,
                                crate::ExpressionId::new(0),
                            );

                            return Ok(crate::Shape::new(
                                vec![crate::ShapeBranch {
                                    condition: comparison,
                                    outcome: branches_out[0].outcome.clone(),
                                }],
                                Vec::new(),
                            ));
                        }
                    }
                }

                let mut free_vars = collect_free_vars_expr(expr_h, doc_name, &get_rule);
                dedup_and_remove_given(&mut free_vars, doc_name, &given_facts);

                let condition = Expression::new(
                    crate::ExpressionKind::Comparison(
                        Box::new(expr_h.clone()),
                        crate::ComparisonOperator::Equal,
                        Box::new(literal_expr(val.clone())),
                    ),
                    None,
                    crate::ExpressionId::new(0),
                );

                return Ok(crate::Shape::new(
                    vec![crate::ShapeBranch {
                        condition,
                        outcome: branches_out[0].outcome.clone(),
                    }],
                    free_vars,
                ));
            }
        }
    }

    let unified_branches = unify_branches(branches_out, &try_fold, &logical_or);

    if unified_branches.len() == 1 {
        if let Some(OperationResult::Value(ref val)) = target.outcome {
            if matches!(target.op, TargetOp::Eq) {
                if let BranchOutcome::Value(ref _expr) = unified_branches[0].outcome {
                    if let Some((lhs_path, rhs_expr)) =
                        extract_fact_equality(&unified_branches[0].condition)
                    {
                        let rhs = try_fold(&rhs_expr).unwrap_or(rhs_expr.clone());
                        let rhs_final = match rhs.kind {
                            ExpressionKind::Literal(_) => rhs,
                            _ => literal_expr(val.clone()),
                        };
                        let cond_sub = crate::inversion::hydration::substitute_fact_with_expr(
                            &unified_branches[0].condition,
                            &lhs_path,
                            &rhs_final,
                        );
                        let cond_sub_simpl = crate::inversion::boolean::simplify_boolean(
                            &cond_sub,
                            &try_fold,
                            &expressions_semantically_equal,
                        )?;
                        if is_boolean_false(&cond_sub_simpl) {
                            return Err(LemmaError::Engine(format!(
                                "Rule '{}' cannot produce the value {}.\nThe rule's conditions make this outcome impossible.",
                                rule_path, val
                            )));
                        }
                        let mut free_vars_eq: Vec<crate::FactReference> = Vec::new();
                        dedup_and_remove_given(&mut free_vars_eq, doc_name, &given_facts);

                        let fact_ref = Expression::new(
                            crate::ExpressionKind::FactReference(crate::FactReference {
                                reference: lhs_path.reference.to_vec(),
                            }),
                            None,
                            crate::ExpressionId::new(0),
                        );
                        let eq_condition = Expression::new(
                            crate::ExpressionKind::Comparison(
                                Box::new(fact_ref),
                                crate::ComparisonOperator::Equal,
                                Box::new(rhs_final),
                            ),
                            None,
                            crate::ExpressionId::new(0),
                        );

                        return Ok(crate::Shape::new(
                            vec![crate::ShapeBranch {
                                condition: eq_condition,
                                outcome: unified_branches[0].outcome.clone(),
                            }],
                            free_vars_eq,
                        ));
                    }
                }
            }
        }
    }

    let mut free_vars = collect_free_vars_piecewise(&unified_branches, doc_name, &get_rule);
    dedup_and_remove_given(&mut free_vars, doc_name, &given_facts);

    Ok(crate::Shape::new(unified_branches, free_vars))
}

#[allow(clippy::too_many_arguments)]
fn filter_branch<'a, F>(
    cond_h: Expression,
    outcome: BranchOutcome,
    target: &Target,
    doc_name: &str,
    given_facts: &HashMap<String, LiteralValue>,
    get_rule: &F,
    try_fold: &impl Fn(&Expression) -> Option<Expression>,
    literal_expr: &impl Fn(LiteralValue) -> Expression,
    logical_and: &impl Fn(Expression, Expression) -> Expression,
    logical_not: &impl Fn(Expression) -> Expression,
    logical_or: &impl Fn(Expression, Expression) -> Expression,
) -> LemmaResult<Option<ShapeBranch>>
where
    F: Fn(&[String]) -> Option<&'a crate::LemmaRule>,
{
    match (&outcome, &target.outcome) {
        (BranchOutcome::Value(_value_expr), None) => {
            // Target is any_value() - matches any non-veto value
            let cond_simpl = crate::inversion::boolean::simplify_boolean(
                &cond_h,
                &try_fold,
                &expressions_semantically_equal,
            )?;
            if is_boolean_false(&cond_simpl) {
                Ok(None)
            } else {
                Ok(Some(ShapeBranch {
                    condition: cond_simpl,
                    outcome,
                }))
            }
        }
        (BranchOutcome::Value(value_expr), Some(OperationResult::Value(_))) => {
            let mut guard = build_value_target_guard(value_expr, target, literal_expr);
            if let ExpressionKind::Comparison(lhs, op, rhs) = &guard.kind {
                if matches!(op, crate::ComparisonOperator::Equal) {
                    if let ExpressionKind::RuleReference(rr) = &lhs.kind {
                        let rule_ref_qualified: Vec<String> = if rr.reference.len() > 1 {
                            rr.reference.clone()
                        } else {
                            vec![doc_name.to_owned(), rr.reference[0].clone()]
                        };
                        if let Some(referenced_rule) = get_rule(&rule_ref_qualified) {
                            let inner_expr = crate::inversion::hydration::hydrate_and_simplify(
                                &referenced_rule.expression,
                                doc_name,
                                given_facts,
                                &get_rule,
                                &|e, g| crate::inversion::hydration::is_simple_for_expansion(e, g),
                                literal_expr,
                            );
                            guard = Expression::new(
                                ExpressionKind::Comparison(
                                    Box::new(inner_expr),
                                    op.clone(),
                                    Box::new((**rhs).clone()),
                                ),
                                None,
                                ExpressionId::new(0),
                            );
                            let mut veto_conds: Vec<Expression> = Vec::new();
                            for br in &referenced_rule.unless_clauses {
                                if let ExpressionKind::Veto(_) = br.result.kind {
                                    veto_conds.push(crate::inversion::hydration::hydrate_and_simplify(
                                        &br.condition, doc_name, given_facts, &get_rule,
                                        &|e, g| crate::inversion::hydration::is_simple_for_expansion(e, g),
                                        literal_expr
                                    ));
                                }
                            }
                            if !veto_conds.is_empty() {
                                let veto_or = veto_conds.into_iter().reduce(logical_or).unwrap();
                                let veto_guard =
                                    logical_not(crate::inversion::hydration::hydrate_and_simplify(
                                        &veto_or,
                                        doc_name,
                                        given_facts,
                                        &get_rule,
                                        &|e, g| {
                                            crate::inversion::hydration::is_simple_for_expansion(
                                                e, g,
                                            )
                                        },
                                        literal_expr,
                                    ));
                                let cond_ext = logical_and(cond_h.clone(), veto_guard);
                                let cond_simpl = crate::inversion::boolean::simplify_boolean(
                                    &cond_ext,
                                    &try_fold,
                                    &expressions_semantically_equal,
                                )?;
                                if is_boolean_false(&cond_simpl) {
                                    return Ok(None);
                                }
                                let guard_h = crate::inversion::hydration::hydrate_and_simplify(
                                    &guard,
                                    doc_name,
                                    given_facts,
                                    &get_rule,
                                    &|e, g| {
                                        crate::inversion::hydration::is_simple_for_expansion(e, g)
                                    },
                                    literal_expr,
                                );
                                let conj = logical_and(cond_simpl, guard_h);
                                let conj_simpl = crate::inversion::boolean::simplify_boolean(
                                    &conj,
                                    &try_fold,
                                    &expressions_semantically_equal,
                                )?;
                                if is_boolean_false(&conj_simpl) {
                                    return Ok(None);
                                }
                                return Ok(Some(ShapeBranch {
                                    condition: conj_simpl,
                                    outcome,
                                }));
                            }
                        }
                    }
                }
            }
            let guard_h = crate::inversion::hydration::hydrate_and_simplify(
                &guard,
                doc_name,
                given_facts,
                &get_rule,
                &|e, g| crate::inversion::hydration::is_simple_for_expansion(e, g),
                literal_expr,
            );
            let conj = logical_and(cond_h, guard_h);
            let conj_simpl = crate::inversion::boolean::simplify_boolean(
                &conj,
                &try_fold,
                &expressions_semantically_equal,
            )?;
            if is_boolean_false(&conj_simpl) {
                Ok(None)
            } else {
                Ok(Some(ShapeBranch {
                    condition: conj_simpl,
                    outcome,
                }))
            }
        }
        (BranchOutcome::Veto(msg), Some(OperationResult::Veto(query_msg))) => {
            let matches = match (query_msg, msg) {
                (None, _) => true,
                (Some(q), Some(m)) => q == m,
                _ => false,
            };
            if !matches {
                return Ok(None);
            }
            let cond_simpl = crate::inversion::boolean::simplify_boolean(
                &cond_h,
                &try_fold,
                &expressions_semantically_equal,
            )?;
            if is_boolean_false(&cond_simpl) {
                Ok(None)
            } else {
                Ok(Some(ShapeBranch {
                    condition: cond_simpl,
                    outcome,
                }))
            }
        }
        _ => Ok(None),
    }
}

fn build_value_target_guard(
    expr: &Expression,
    target: &Target,
    literal_expr: &impl Fn(LiteralValue) -> Expression,
) -> Expression {
    let rhs = match &target.outcome {
        Some(OperationResult::Value(v)) => literal_expr(v.clone()),
        _ => unreachable!("build_value_target_guard called with non-value target"),
    };
    let op = match target.op {
        TargetOp::Eq => crate::ComparisonOperator::Equal,
        TargetOp::Neq => crate::ComparisonOperator::NotEqual,
        TargetOp::Lt => crate::ComparisonOperator::LessThan,
        TargetOp::Lte => crate::ComparisonOperator::LessThanOrEqual,
        TargetOp::Gt => crate::ComparisonOperator::GreaterThan,
        TargetOp::Gte => crate::ComparisonOperator::GreaterThanOrEqual,
    };
    Expression::new(
        ExpressionKind::Comparison(Box::new(expr.clone()), op, Box::new(rhs)),
        None,
        ExpressionId::new(0),
    )
}

fn extract_fact_equality(cond: &Expression) -> Option<(crate::FactReference, Expression)> {
    use ExpressionKind as EK;
    match &cond.kind {
        EK::Comparison(l, crate::ComparisonOperator::Equal, r) => {
            if let EK::FactReference(fr) = &l.kind {
                let fp = if fr.reference.len() == 1 {
                    crate::FactReference {
                        reference: vec![fr.reference[0].clone()],
                    }
                } else if fr.reference.len() == 2 {
                    crate::FactReference {
                        reference: vec![fr.reference[0].clone(), fr.reference[1].clone()],
                    }
                } else {
                    return None;
                };
                return Some((fp, (**r).clone()));
            }
            if let EK::FactReference(fr) = &r.kind {
                let fp = if fr.reference.len() == 1 {
                    crate::FactReference {
                        reference: vec![fr.reference[0].clone()],
                    }
                } else if fr.reference.len() == 2 {
                    crate::FactReference {
                        reference: vec![fr.reference[0].clone(), fr.reference[1].clone()],
                    }
                } else {
                    return None;
                };
                return Some((fp, (**l).clone()));
            }
            None
        }
        EK::LogicalAnd(a, b) => extract_fact_equality(a).or_else(|| extract_fact_equality(b)),
        EK::LogicalNegation(inner, _) => extract_fact_equality(inner),
        _ => None,
    }
}

fn unify_branches(
    branches: Vec<ShapeBranch>,
    try_fold: &impl Fn(&Expression) -> Option<Expression>,
    logical_or: &impl Fn(Expression, Expression) -> Expression,
) -> Vec<ShapeBranch> {
    if branches.is_empty() {
        return branches;
    }

    let mut result = Vec::new();
    let mut processed = vec![false; branches.len()];

    for i in 0..branches.len() {
        if processed[i] {
            continue;
        }

        let mut matching_indices = vec![i];
        for j in (i + 1)..branches.len() {
            if !processed[j] && outcomes_equal(&branches[i].outcome, &branches[j].outcome) {
                matching_indices.push(j);
                processed[j] = true;
            }
        }
        processed[i] = true;

        let unified_condition = if matching_indices.len() == 1 {
            branches[i].condition.clone()
        } else {
            let or_expr = matching_indices.iter().skip(1).fold(
                branches[matching_indices[0]].condition.clone(),
                |acc, &idx| logical_or(acc, branches[idx].condition.clone()),
            );
            crate::inversion::boolean::simplify_or_expression(
                &or_expr,
                &try_fold,
                &expressions_semantically_equal,
            )
        };

        result.push(ShapeBranch {
            condition: unified_condition,
            outcome: branches[i].outcome.clone(),
        });
    }

    result
}

fn outcomes_equal(a: &BranchOutcome, b: &BranchOutcome) -> bool {
    match (a, b) {
        (BranchOutcome::Veto(msg_a), BranchOutcome::Veto(msg_b)) => msg_a == msg_b,
        (BranchOutcome::Value(expr_a), BranchOutcome::Value(expr_b)) => {
            expressions_semantically_equal(expr_a, expr_b)
        }
        _ => false,
    }
}

fn find_unknown_facts(
    expr: &Expression,
    doc_name: &str,
    given: &HashMap<String, LiteralValue>,
) -> Vec<(String, String)> {
    let refs = crate::analysis::extract_references(expr);
    let mut facts: Vec<(String, String)> = refs
        .facts
        .into_iter()
        .map(|p| {
            let segments = p.reference;
            if segments.len() > 1 {
                (
                    segments[0].clone(),
                    segments.last().cloned().unwrap_or_default(),
                )
            } else {
                (doc_name.to_owned(), segments[0].clone())
            }
        })
        .collect();
    facts.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    facts.dedup();
    let mut given_keys: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    for key in given.keys() {
        if let Some((d, f)) = key.split_once('.') {
            given_keys.insert((d.to_string(), f.to_string()));
        } else {
            given_keys.insert((doc_name.to_string(), key.clone()));
        }
    }
    facts
        .into_iter()
        .filter(|k| !given_keys.contains(k))
        .collect()
}

fn fact_reference_matches(fr: &crate::FactReference, doc_name: &str, fact_name: &str) -> bool {
    match fr.reference.as_slice() {
        [only] => only == fact_name,
        [d, f] => d == doc_name && f == fact_name,
        _ => false,
    }
}

fn collect_free_vars_piecewise<'a, F>(
    branches: &[ShapeBranch],
    doc_name: &str,
    get_rule: &F,
) -> Vec<crate::FactReference>
where
    F: Fn(&[String]) -> Option<&'a crate::LemmaRule>,
{
    let mut vars = Vec::new();
    for br in branches {
        vars.extend(collect_free_vars_expr(&br.condition, doc_name, get_rule));
        if let BranchOutcome::Value(expr) = &br.outcome {
            vars.extend(collect_free_vars_expr(expr, doc_name, get_rule));
        }
    }
    vars
}

fn collect_free_vars_expr<'a, F>(
    expr: &Expression,
    doc_name: &str,
    get_rule: &F,
) -> Vec<crate::FactReference>
where
    F: Fn(&[String]) -> Option<&'a crate::LemmaRule>,
{
    let mut result = Vec::new();
    let refs = crate::analysis::extract_references(expr);

    // Collect fact references as-is
    for path in refs.facts {
        result.push(path);
    }

    // Collect transitive dependencies through rule references
    for rule_ref in refs.rules {
        let mut qualified_rule_ref = rule_ref.clone();
        if qualified_rule_ref.len() == 1 {
            qualified_rule_ref.insert(0, doc_name.to_string());
        }

        // Recursively get dependencies of the referenced rule
        if let Some(referenced_rule) = get_rule(&qualified_rule_ref) {
            // Get facts from the rule's expression
            result.extend(collect_free_vars_expr(
                &referenced_rule.expression,
                doc_name,
                get_rule,
            ));

            // Also check branches
            for branch in &referenced_rule.unless_clauses {
                result.extend(collect_free_vars_expr(
                    &branch.condition,
                    doc_name,
                    get_rule,
                ));
                result.extend(collect_free_vars_expr(&branch.result, doc_name, get_rule));
            }
        }
    }

    result
}

fn dedup_and_remove_given(
    vars: &mut Vec<crate::FactReference>,
    doc_name: &str,
    given: &HashMap<String, LiteralValue>,
) {
    vars.sort_by(|a, b| a.reference.cmp(&b.reference));
    vars.dedup();
    vars.retain(|path| {
        let segments = &path.reference;

        // Check if the full path is given (e.g., "doc.field" or just "field")
        let full_key = segments.join(".");
        if given.contains_key(&full_key) {
            return false;
        }

        // For single-segment local references, also check with doc prefix
        if segments.len() == 1 {
            let qualified_key = format!("{}.{}", doc_name, segments[0]);
            if given.contains_key(&qualified_key) {
                return false;
            }
        }

        // For already-qualified references (2+ segments), check without the first segment
        if segments.len() >= 2 {
            let short_key = segments[1..].join(".");
            if given.contains_key(&short_key) {
                return false;
            }
        }

        true
    });
}
