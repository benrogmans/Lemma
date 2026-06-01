//! Exact rational arithmetic bridge between `rust_decimal::Decimal` and `num_rational::Ratio<i128>`.

use num_integer::Roots;
use num_rational::Ratio;
use rust_decimal::Decimal;
use rust_decimal::MathematicalOps;
use std::fmt;

pub type RationalInteger = Ratio<i128>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumericFailure {
    DivisionByZero,
    Overflow,
    Irrational,
}

impl fmt::Display for NumericFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumericFailure::DivisionByZero => formatter.write_str("division by zero"),
            NumericFailure::Overflow => formatter.write_str("numeric overflow"),
            NumericFailure::Irrational => formatter.write_str("irrational numeric result"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericOperation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
}

pub fn rational_one() -> RationalInteger {
    RationalInteger::new(1, 1)
}

pub fn rational_zero() -> RationalInteger {
    RationalInteger::new(0, 1)
}

pub fn rational_is_zero(rational: &RationalInteger) -> bool {
    *rational.numer() == 0
}

/// Absolute value in ℚ (negates a negative numerator; zero and positive unchanged).
pub fn rational_abs(rational: &RationalInteger) -> RationalInteger {
    if *rational.numer() >= 0 {
        *rational
    } else {
        RationalInteger::new(-rational.numer(), *rational.denom()).reduced()
    }
}

/// Truncates a rational toward zero as a rational with denominator 1.
pub fn rational_trunc(rational: &RationalInteger) -> RationalInteger {
    RationalInteger::new(rational.numer() / rational.denom(), 1)
}

pub fn decimal_to_rational(decimal: Decimal) -> Result<RationalInteger, NumericFailure> {
    let mantissa = decimal.mantissa();
    if mantissa == 0 {
        return Ok(RationalInteger::new(0, 1));
    }
    let scale = decimal.scale();
    let mut denominator = 1i128;
    for _ in 0..scale {
        denominator = denominator
            .checked_mul(10)
            .ok_or(NumericFailure::Overflow)?;
    }
    Ok(RationalInteger::new(mantissa, denominator).reduced())
}

/// Commit a rational to stored `Decimal` with a single division (finite precision).
pub fn commit_rational_to_decimal(rational: &RationalInteger) -> Result<Decimal, NumericFailure> {
    let reduced = rational.reduced();
    let numerator = *reduced.numer();
    let denominator = *reduced.denom();
    if denominator == 0 {
        unreachable!("BUG: rational with zero denominator");
    }
    if numerator == 0 {
        return Ok(Decimal::ZERO);
    }
    let numerator_decimal = decimal_from_i128(numerator)?;
    let denominator_decimal = decimal_from_i128(denominator)?;
    numerator_decimal
        .checked_div(denominator_decimal)
        .ok_or(NumericFailure::Overflow)
}

/// Format for human display: commit to decimal when possible, else reduced `numer/denom`.
pub fn rational_to_display_str(rational: &RationalInteger) -> String {
    match commit_rational_to_decimal(rational) {
        Ok(decimal) => decimal_to_display_str(&decimal),
        Err(_) => rational_fraction_str(rational),
    }
}

/// API format: exact decimal string only. Never emits a fraction.
pub fn rational_to_wire_str(rational: &RationalInteger) -> Result<String, NumericFailure> {
    commit_rational_to_decimal(rational).map(|decimal| decimal_to_display_str(&decimal))
}

fn decimal_to_display_str(decimal: &Decimal) -> String {
    let normalized = decimal.normalize();
    if normalized.fract().is_zero() {
        normalized.trunc().to_string()
    } else {
        normalized.to_string()
    }
}

fn rational_fraction_str(rational: &RationalInteger) -> String {
    let reduced = rational.reduced();
    let numer = *reduced.numer();
    let denom = *reduced.denom();
    if denom == 1 {
        numer.to_string()
    } else {
        format!("{numer}/{denom}")
    }
}

fn decimal_from_i128(value: i128) -> Result<Decimal, NumericFailure> {
    let max_mantissa = Decimal::MAX.mantissa();
    let min_mantissa = Decimal::MIN.mantissa();
    if value > max_mantissa || value < min_mantissa {
        return Err(NumericFailure::Overflow);
    }
    Ok(Decimal::from(value))
}

pub fn rational_operation(
    left: &RationalInteger,
    operation: NumericOperation,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    match operation {
        NumericOperation::Add => checked_add(left, right),
        NumericOperation::Subtract => checked_sub(left, right),
        NumericOperation::Multiply => checked_mul(left, right),
        NumericOperation::Divide => {
            if rational_is_zero(right) {
                return Err(NumericFailure::DivisionByZero);
            }
            checked_div(left, right)
        }
        NumericOperation::Modulo => {
            if rational_is_zero(right) {
                return Err(NumericFailure::DivisionByZero);
            }
            let quotient = checked_div(left, right)?;
            let truncated = rational_trunc(&quotient);
            let product = checked_mul(&truncated, right)?;
            checked_sub(left, &product)
        }
        NumericOperation::Power => checked_rational_power(left, right),
    }
}

/// Exact rational operation; on overflow or irrational power, fall back to 28-digit
/// decimal arithmetic and lift the result back to rational when possible.
pub fn rational_operation_with_fallback(
    left: &RationalInteger,
    operation: NumericOperation,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    match rational_operation(left, operation, right) {
        Ok(result) => Ok(result),
        Err(NumericFailure::DivisionByZero) => Err(NumericFailure::DivisionByZero),
        Err(_) => approximate_rational_operation(left, operation, right),
    }
}

fn approximate_rational_operation(
    left: &RationalInteger,
    operation: NumericOperation,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    let left_decimal = commit_rational_to_decimal(left)?;
    let right_decimal = commit_rational_to_decimal(right)?;
    let result_decimal = decimal_arithmetic(left_decimal, operation, right_decimal)?;
    decimal_to_rational(result_decimal)
}

fn decimal_arithmetic(
    left: Decimal,
    operation: NumericOperation,
    right: Decimal,
) -> Result<Decimal, NumericFailure> {
    match operation {
        NumericOperation::Add => left.checked_add(right).ok_or(NumericFailure::Overflow),
        NumericOperation::Subtract => left.checked_sub(right).ok_or(NumericFailure::Overflow),
        NumericOperation::Multiply => left.checked_mul(right).ok_or(NumericFailure::Overflow),
        NumericOperation::Divide => {
            if right.is_zero() {
                return Err(NumericFailure::DivisionByZero);
            }
            left.checked_div(right).ok_or(NumericFailure::Overflow)
        }
        NumericOperation::Modulo => {
            if right.is_zero() {
                return Err(NumericFailure::DivisionByZero);
            }
            let quotient = left.checked_div(right).ok_or(NumericFailure::Overflow)?;
            let truncated = quotient.trunc();
            let product = truncated
                .checked_mul(right)
                .ok_or(NumericFailure::Overflow)?;
            left.checked_sub(product).ok_or(NumericFailure::Overflow)
        }
        NumericOperation::Power => decimal_power(left, right),
    }
}

fn decimal_is_half(exponent: Decimal) -> bool {
    exponent
        .checked_mul(Decimal::TWO)
        .is_some_and(|doubled| doubled == Decimal::ONE)
}

fn decimal_power(base: Decimal, exponent: Decimal) -> Result<Decimal, NumericFailure> {
    if exponent.fract().is_zero() {
        let exponent_i64 =
            i64::try_from(exponent.trunc().mantissa()).map_err(|_| NumericFailure::Overflow)?;
        return base
            .checked_powi(exponent_i64)
            .ok_or(NumericFailure::Overflow);
    }
    if decimal_is_half(exponent) {
        return base.sqrt().ok_or(NumericFailure::Irrational);
    }
    Err(NumericFailure::Irrational)
}

pub fn checked_add(
    left: &RationalInteger,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    left.numer()
        .checked_mul(*right.denom())
        .and_then(|left_cross| {
            right
                .numer()
                .checked_mul(*left.denom())
                .map(|right_cross| (left_cross, right_cross))
        })
        .and_then(|(left_cross, right_cross)| {
            left_cross
                .checked_add(right_cross)
                .map(|numerator| (numerator, left.denom().checked_mul(*right.denom())))
        })
        .and_then(|(numerator, denominator)| {
            if denominator == Some(0) {
                None
            } else {
                denominator
                    .map(|denominator| RationalInteger::new(numerator, denominator).reduced())
            }
        })
        .ok_or(NumericFailure::Overflow)
}

pub fn checked_sub(
    left: &RationalInteger,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    left.numer()
        .checked_mul(*right.denom())
        .and_then(|left_cross| {
            right
                .numer()
                .checked_mul(*left.denom())
                .map(|right_cross| (left_cross, right_cross))
        })
        .and_then(|(left_cross, right_cross)| {
            left_cross
                .checked_sub(right_cross)
                .map(|numerator| (numerator, left.denom().checked_mul(*right.denom())))
        })
        .and_then(|(numerator, denominator)| {
            if denominator == Some(0) {
                None
            } else {
                denominator
                    .map(|denominator| RationalInteger::new(numerator, denominator).reduced())
            }
        })
        .ok_or(NumericFailure::Overflow)
}

pub fn checked_mul(
    left: &RationalInteger,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    left.numer()
        .checked_mul(*right.numer())
        .and_then(|numerator| {
            left.denom()
                .checked_mul(*right.denom())
                .map(|denominator| (numerator, denominator))
        })
        .map(|(numerator, denominator)| RationalInteger::new(numerator, denominator).reduced())
        .ok_or(NumericFailure::Overflow)
}

pub fn checked_div(
    left: &RationalInteger,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    if *right.numer() == 0 {
        return Err(NumericFailure::DivisionByZero);
    }
    left.numer()
        .checked_mul(*right.denom())
        .and_then(|numerator| {
            left.denom()
                .checked_mul(*right.numer())
                .map(|denominator| (numerator, denominator))
        })
        .map(|(numerator, denominator)| RationalInteger::new(numerator, denominator).reduced())
        .ok_or(NumericFailure::Overflow)
}

pub fn checked_pow_i32(
    base: &RationalInteger,
    exponent: i32,
) -> Result<RationalInteger, NumericFailure> {
    if exponent == 0 {
        return Ok(rational_one());
    }
    if exponent < 0 {
        if *base.numer() == 0 {
            return Err(NumericFailure::DivisionByZero);
        }
        let positive_base = RationalInteger::new(*base.denom(), *base.numer()).reduced();
        return checked_pow_i32(&positive_base, -exponent);
    }
    let mut result = rational_one();
    let mut factor = base.reduced();
    let mut remaining = exponent as u32;
    while remaining > 0 {
        if remaining % 2 == 1 {
            result = checked_mul(&result, &factor)?;
        }
        remaining /= 2;
        if remaining > 0 {
            factor = checked_mul(&factor, &factor)?;
        }
    }
    Ok(result)
}

pub fn checked_rational_power(
    base: &RationalInteger,
    exponent: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    let exp_numer = *exponent.numer();
    let exp_denom = *exponent.denom();

    assert!(
        exp_denom > 0,
        "BUG: rational exponent must have positive denominator (canonical Ratio invariant)"
    );

    if exp_denom == 1 {
        let exponent = i32::try_from(exp_numer).map_err(|_| NumericFailure::Overflow)?;
        return checked_pow_i32(base, exponent);
    }

    if *base.numer() == 0 {
        if exp_numer <= 0 {
            return Err(NumericFailure::DivisionByZero);
        }
        return Ok(RationalInteger::new(0, 1));
    }

    let abs_exp_numer = exp_numer.unsigned_abs();
    let abs_exp_i32 = i32::try_from(abs_exp_numer).map_err(|_| NumericFailure::Overflow)?;
    let raised = checked_pow_i32(base, abs_exp_i32)?;

    let root_degree = u32::try_from(exp_denom).map_err(|_| NumericFailure::Overflow)?;

    let raised_numer = *raised.numer();
    let raised_denom = *raised.denom();

    let (numer_root, numer_negative) = if raised_numer < 0 {
        if root_degree % 2 == 0 {
            return Err(NumericFailure::Irrational);
        }
        (raised_numer.unsigned_abs().nth_root(root_degree), true)
    } else {
        ((raised_numer as u128).nth_root(root_degree), false)
    };

    let denom_root = (raised_denom as u128).nth_root(root_degree);

    let numer_root_i128 = i128::try_from(numer_root).map_err(|_| NumericFailure::Overflow)?;
    let denom_root_i128 = i128::try_from(denom_root).map_err(|_| NumericFailure::Overflow)?;

    let numer_reconstructed = numer_root_i128
        .checked_pow(root_degree)
        .ok_or(NumericFailure::Overflow)?;
    let denom_reconstructed = denom_root_i128
        .checked_pow(root_degree)
        .ok_or(NumericFailure::Overflow)?;

    if numer_reconstructed != raised_numer.unsigned_abs() as i128 {
        return Err(NumericFailure::Irrational);
    }
    if denom_reconstructed != raised_denom {
        return Err(NumericFailure::Irrational);
    }

    let signed_numer = if numer_negative {
        -numer_root_i128
    } else {
        numer_root_i128
    };

    let result = RationalInteger::new(signed_numer, denom_root_i128).reduced();

    if exp_numer < 0 {
        if *result.numer() == 0 {
            return Err(NumericFailure::DivisionByZero);
        }
        Ok(RationalInteger::new(*result.denom(), *result.numer()).reduced())
    } else {
        Ok(result)
    }
}

/// Multiply a quantity magnitude by a unit conversion ratio in ℚ, then commit once.
pub fn convert_quantity_magnitude_rational(
    magnitude: RationalInteger,
    from_factor: &RationalInteger,
    to_factor: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    let ratio = checked_div(from_factor, to_factor)?;
    checked_mul(&magnitude, &ratio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn rational_zero_is_zero() {
        assert!(rational_is_zero(&rational_zero()));
    }

    #[test]
    fn decimal_one_half_lifts_to_rational() {
        let decimal = Decimal::from_str("0.5").unwrap();
        let rational = decimal_to_rational(decimal).unwrap();
        assert_eq!(rational, RationalInteger::new(1, 2));
    }

    #[test]
    fn commit_one_third_to_decimal() {
        let rational = RationalInteger::new(1, 3);
        let decimal = commit_rational_to_decimal(&rational).unwrap();
        let expected = Decimal::from_str("0.3333333333333333333333333333").unwrap();
        assert_eq!(decimal, expected);
    }

    #[test]
    fn checked_mul_integer() {
        let left = RationalInteger::new(50, 1);
        let right = RationalInteger::new(86400, 1);
        let product = checked_mul(&left, &right).unwrap();
        assert_eq!(product, RationalInteger::new(4_320_000, 1));
    }

    #[test]
    fn checked_pow_negative_exponent_inverts_base() {
        let hour_factor = RationalInteger::new(3600, 1);
        let inverse = checked_pow_i32(&hour_factor, -1).unwrap();
        assert_eq!(inverse, RationalInteger::new(1, 3600));
    }

    #[test]
    fn rational_operation_divide_by_zero() {
        let left = RationalInteger::new(1, 1);
        let right = RationalInteger::new(0, 1);
        let failure = rational_operation(&left, NumericOperation::Divide, &right).unwrap_err();
        assert_eq!(failure, NumericFailure::DivisionByZero);
    }

    #[test]
    fn rational_operation_power_irrational() {
        let base = RationalInteger::new(2, 1);
        let exponent = RationalInteger::new(1, 2);
        let failure = rational_operation(&base, NumericOperation::Power, &exponent).unwrap_err();
        assert_eq!(failure, NumericFailure::Irrational);
    }

    #[test]
    fn rational_operation_power_exact() {
        let base = RationalInteger::new(4, 1);
        let exponent = RationalInteger::new(1, 2);
        let result = rational_operation(&base, NumericOperation::Power, &exponent).unwrap();
        assert_eq!(result, RationalInteger::new(2, 1));
    }

    #[test]
    fn rational_operation_add() {
        let left = RationalInteger::new(1, 3);
        let right = RationalInteger::new(1, 6);
        let sum = rational_operation(&left, NumericOperation::Add, &right).unwrap();
        assert_eq!(sum, RationalInteger::new(1, 2));
    }

    #[test]
    fn rational_operation_with_fallback_add_exact_rational() {
        let left = RationalInteger::new(1, 3);
        let right = RationalInteger::new(1, 6);
        let sum = rational_operation_with_fallback(&left, NumericOperation::Add, &right).unwrap();
        assert_eq!(sum, RationalInteger::new(1, 2));
    }

    #[test]
    fn rational_operation_with_fallback_power_sqrt_via_decimal() {
        let result = rational_operation_with_fallback(
            &RationalInteger::new(2, 1),
            NumericOperation::Power,
            &RationalInteger::new(1, 2),
        )
        .unwrap();
        assert_eq!(
            commit_rational_to_decimal(&result).unwrap(),
            commit_rational_to_decimal(&RationalInteger::new(2, 1))
                .unwrap()
                .sqrt()
                .unwrap(),
        );
    }

    #[test]
    fn rational_abs_negates_negative_numerator() {
        let negative = RationalInteger::new(-172_800, 1);
        assert_eq!(rational_abs(&negative), RationalInteger::new(172_800, 1));
    }

    #[test]
    fn rational_to_wire_str_rejects_uncommittable() {
        let too_large = RationalInteger::new(i128::MAX, 1);
        assert!(commit_rational_to_decimal(&too_large).is_err());
        assert!(rational_to_wire_str(&too_large).is_err());
    }

    #[test]
    fn rational_to_wire_str_matches_commit_for_committable() {
        let rational = RationalInteger::new(37, 1);
        assert_eq!(rational_to_wire_str(&rational).unwrap(), "37");
    }

    #[test]
    fn rational_to_display_str_falls_back_to_fraction_when_commit_fails() {
        // 1/3 commits to a repeating decimal in rust_decimal; if commit ever fails,
        // display falls back to reduced fraction notation.
        let rational = RationalInteger::new(355, 113);
        let display = rational_to_display_str(&rational);
        assert!(
            display.contains('/') || commit_rational_to_decimal(&rational).is_ok(),
            "display must be either committable decimal or fraction, got {display}"
        );
    }
}
