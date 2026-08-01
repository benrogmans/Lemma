//! Exact rational arithmetic bridge between `rust_decimal::Decimal` and fallible `BigInt`.
//!
//! Input literals parse through [`crate::literals::NumberLiteral`] (`Decimal::from_str`).
//! API decimal conversion uses [`RationalInteger::try_to_decimal`], enforcing the same
//! [`Decimal::MAX_SCALE`] digit limit.

use rust_decimal::Decimal;
use rust_decimal::MathematicalOps;
use std::fmt;
use std::str::FromStr;

use crate::computation::bigint::AllocError;

pub use crate::computation::bigint::BigInt;

/// Digit limit for ℚ → [`Decimal`] API conversion.
///
/// Matches [`Decimal::MAX_SCALE`] and [`crate::literals::NumberLiteral`] input parsing.
const DECIMAL_OUTPUT_DIGIT_LIMIT: usize = Decimal::MAX_SCALE as usize;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RationalInteger {
    numer: BigInt,
    denom: BigInt,
}

impl RationalInteger {
    pub fn try_new(numer: BigInt, denom: BigInt) -> Result<Self, NumericFailure> {
        if denom.is_zero() {
            return Err(NumericFailure::DivisionByZero);
        }
        let value = Self { numer, denom };
        value.try_reduce()
    }

    pub fn from_i64_pair(numer: i64, denom: i64) -> Self {
        Self {
            numer: BigInt::from_i64(numer),
            denom: BigInt::from_i64(denom),
        }
        .try_reduce()
        .expect("BUG: i64 rational reduce cannot fail")
    }

    pub fn numer(&self) -> &BigInt {
        &self.numer
    }

    pub fn denom(&self) -> &BigInt {
        &self.denom
    }

    pub fn try_reduce_ref(rational: &RationalInteger) -> Result<Self, NumericFailure> {
        if rational.denom().is_zero() {
            return Err(NumericFailure::DivisionByZero);
        }
        let mut numer = rational.numer().clone();
        let mut denom = rational.denom().clone();
        if denom.is_negative() {
            numer = numer.try_neg().map_err(map_alloc)?;
            denom = denom.try_neg().map_err(map_alloc)?;
        }
        if numer.is_zero() {
            return Ok(Self {
                numer: BigInt::zero(),
                denom: BigInt::one(),
            });
        }
        let gcd = numer.try_abs()?.try_gcd(&denom.try_abs()?)?;
        numer = numer.try_div_trunc(&gcd)?;
        denom = denom.try_div_trunc(&gcd)?;
        Ok(Self { numer, denom })
    }

    pub fn try_reduce(mut self) -> Result<Self, NumericFailure> {
        if self.denom.is_zero() {
            return Err(NumericFailure::DivisionByZero);
        }
        if self.denom.is_negative() {
            self.numer = self.numer.try_neg().map_err(map_alloc)?;
            self.denom = self.denom.try_neg().map_err(map_alloc)?;
        }
        if self.numer.is_zero() {
            self.denom = BigInt::one();
            return Ok(self);
        }
        let gcd = self.numer.try_abs()?.try_gcd(&self.denom.try_abs()?)?;
        self.numer = self.numer.try_div_trunc(&gcd)?;
        self.denom = self.denom.try_div_trunc(&gcd)?;
        Ok(self)
    }

    pub fn try_cmp(&self, other: &Self) -> Result<std::cmp::Ordering, NumericFailure> {
        let left = self.numer().try_mul(other.denom())?;
        let right = other.numer().try_mul(self.denom())?;
        Ok(left.cmp(&right))
    }

    /// Round to [`Decimal::MAX_SCALE`] precision.
    ///
    /// Returns [`NumericFailure::Overflow`] only when `|self| > Decimal::MAX`.
    pub fn try_to_decimal(&self) -> Result<Decimal, NumericFailure> {
        let reduced = Self::try_reduce_ref(self)?;
        if reduced.numer().is_zero() {
            return Ok(Decimal::ZERO);
        }

        let max_rational = decimal_to_rational(Decimal::MAX.normalize())?;
        let min_rational = decimal_to_rational(Decimal::MIN.normalize())?;
        if reduced.try_cmp(&max_rational)? == std::cmp::Ordering::Greater {
            return Err(NumericFailure::Overflow);
        }
        if reduced.try_cmp(&min_rational)? == std::cmp::Ordering::Less {
            return Err(NumericFailure::Overflow);
        }

        let negative = reduced.numer().is_negative();
        let abs_numer = reduced.numer().try_abs()?;
        let abs_denom = reduced.denom().try_abs()?;

        let (int_quotient, mut remainder) = abs_numer.try_div_rem(&abs_denom)?;

        let mut digit_string = if int_quotient.is_zero() {
            String::from("0")
        } else {
            int_quotient.to_string()
        };

        if !remainder.is_zero() {
            digit_string.push('.');
            let integer_significant_digits = if int_quotient.is_zero() {
                0
            } else {
                digit_string.len()
            };

            if int_quotient.is_zero() {
                let mut significant_digits = 0usize;
                while !remainder.is_zero() && significant_digits < DECIMAL_OUTPUT_DIGIT_LIMIT + 1 {
                    remainder = remainder.try_mul(&BigInt::from_i64(10))?;
                    let (digit_quotient, new_remainder) = remainder.try_div_rem(&abs_denom)?;
                    let digit = u8::try_from(
                        digit_quotient
                            .to_i32()
                            .expect("BUG: fractional digit quotient must fit i32"),
                    )
                    .expect("BUG: fractional digit must be 0-9");
                    digit_string.push(char::from(b'0' + digit));
                    remainder = new_remainder;
                    if digit != 0 || significant_digits > 0 {
                        significant_digits += 1;
                    }
                }
            } else {
                let max_fractional_digits = DECIMAL_OUTPUT_DIGIT_LIMIT
                    .saturating_sub(integer_significant_digits)
                    .saturating_add(1);
                let mut fractional_count = 0usize;
                while !remainder.is_zero() && fractional_count < max_fractional_digits {
                    remainder = remainder.try_mul(&BigInt::from_i64(10))?;
                    let (digit_quotient, new_remainder) = remainder.try_div_rem(&abs_denom)?;
                    let digit = u8::try_from(
                        digit_quotient
                            .to_i32()
                            .expect("BUG: fractional digit quotient must fit i32"),
                    )
                    .expect("BUG: fractional digit must be 0-9");
                    digit_string.push(char::from(b'0' + digit));
                    remainder = new_remainder;
                    fractional_count += 1;
                }
            }
        }

        if negative {
            digit_string.insert(0, '-');
        }

        Decimal::from_str(&digit_string).map_err(|_| NumericFailure::Overflow)
    }

    pub fn try_to_decimal_string(&self) -> Result<String, NumericFailure> {
        self.try_to_decimal()
            .map(|decimal| decimal_to_display_str(&decimal))
    }

    /// Human-readable decimal when `try_to_decimal` succeeds; fraction string on magnitude overflow.
    pub fn display_str(&self) -> String {
        match self.try_to_decimal() {
            Ok(decimal) => decimal_to_display_str(&decimal),
            Err(NumericFailure::Overflow) => rational_fraction_str(self),
            Err(NumericFailure::OutOfMemory) => rational_fraction_str(self),
            Err(NumericFailure::DivisionByZero) => {
                unreachable!("BUG: display_str on reduced rational with zero denominator")
            }
            Err(NumericFailure::Irrational) => {
                unreachable!("BUG: display_str does not perform irrational operations")
            }
        }
    }
}

impl PartialOrd for RationalInteger {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RationalInteger {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.try_cmp(other).expect("BUG: rational compare OOM")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumericFailure {
    DivisionByZero,
    Overflow,
    OutOfMemory,
    Irrational,
}

impl fmt::Display for NumericFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumericFailure::DivisionByZero => formatter.write_str("division by zero"),
            NumericFailure::Overflow => formatter.write_str("numeric overflow"),
            NumericFailure::OutOfMemory => formatter.write_str("out of memory"),
            NumericFailure::Irrational => formatter.write_str("irrational numeric result"),
        }
    }
}

fn map_alloc(_: AllocError) -> NumericFailure {
    NumericFailure::OutOfMemory
}

impl From<AllocError> for NumericFailure {
    fn from(err: AllocError) -> Self {
        map_alloc(err)
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
    rational_new(1, 1)
}

pub fn rational_zero() -> RationalInteger {
    rational_new(0, 1)
}

pub fn rational_new(numer: i64, denom: i64) -> RationalInteger {
    RationalInteger::from_i64_pair(numer, denom)
}

pub fn try_rational_new(numer: BigInt, denom: BigInt) -> Result<RationalInteger, NumericFailure> {
    RationalInteger::try_new(numer, denom)
}

pub fn rational_is_zero(rational: &RationalInteger) -> bool {
    rational.numer().is_zero()
}

pub fn rational_abs(rational: &RationalInteger) -> Result<RationalInteger, NumericFailure> {
    if rational.numer().is_negative() {
        try_rational_new(rational.numer().try_neg()?, rational.denom().try_clone()?)
    } else {
        Ok(RationalInteger {
            numer: rational.numer().try_clone()?,
            denom: rational.denom().try_clone()?,
        })
    }
}

pub fn rational_trunc(rational: &RationalInteger) -> Result<RationalInteger, NumericFailure> {
    let truncated = rational.numer().try_div_trunc(rational.denom())?;
    try_rational_new(truncated, BigInt::one())
}

pub fn decimal_to_rational(decimal: Decimal) -> Result<RationalInteger, NumericFailure> {
    let mantissa = decimal.mantissa();
    if mantissa == 0 {
        return Ok(rational_new(0, 1));
    }
    let scale = decimal.scale();
    let mut denominator = BigInt::one();
    for _ in 0..scale {
        denominator = denominator
            .try_mul(&BigInt::from_i64(10))
            .map_err(map_alloc)?;
    }
    try_rational_new(BigInt::from_i128(mantissa), denominator)
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
    let reduced = rational
        .clone()
        .try_reduce()
        .unwrap_or_else(|_| rational.clone());
    let numer = reduced.numer();
    let denom = reduced.denom();
    if *denom == BigInt::one() {
        numer.to_string()
    } else {
        format!("{numer}/{denom}")
    }
}

pub fn rational_operation(
    left: &RationalInteger,
    operation: NumericOperation,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    match operation {
        NumericOperation::Add => try_add(left, right),
        NumericOperation::Subtract => try_sub(left, right),
        NumericOperation::Multiply => try_mul(left, right),
        NumericOperation::Divide => {
            if rational_is_zero(right) {
                return Err(NumericFailure::DivisionByZero);
            }
            try_div(left, right)
        }
        NumericOperation::Modulo => {
            if rational_is_zero(right) {
                return Err(NumericFailure::DivisionByZero);
            }
            let quotient = try_div(left, right)?;
            let truncated = rational_trunc(&quotient)?;
            let product = try_mul(&truncated, right)?;
            try_sub(left, &product)
        }
        NumericOperation::Power => try_rational_power(left, right),
    }
}

pub fn rational_operation_with_fallback(
    left: &RationalInteger,
    operation: NumericOperation,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    match rational_operation(left, operation, right) {
        Ok(result) => Ok(result),
        Err(NumericFailure::DivisionByZero) => Err(NumericFailure::DivisionByZero),
        Err(NumericFailure::Overflow) => Err(NumericFailure::Overflow),
        Err(NumericFailure::OutOfMemory) => Err(NumericFailure::OutOfMemory),
        Err(NumericFailure::Irrational) => approximate_rational_operation(left, operation, right),
    }
}

fn approximate_rational_operation(
    left: &RationalInteger,
    operation: NumericOperation,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    let left_decimal = left.try_to_decimal()?;
    let right_decimal = right.try_to_decimal()?;
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

pub fn try_add(
    left: &RationalInteger,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    let numerator = left
        .numer()
        .try_mul(right.denom())?
        .try_add(&right.numer().try_mul(left.denom())?)?;
    let denominator = left.denom().try_mul(right.denom())?;
    try_rational_new(numerator, denominator)
}

pub fn try_sub(
    left: &RationalInteger,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    let numerator = left
        .numer()
        .try_mul(right.denom())?
        .try_sub(&right.numer().try_mul(left.denom())?)?;
    let denominator = left.denom().try_mul(right.denom())?;
    try_rational_new(numerator, denominator)
}

pub fn try_mul(
    left: &RationalInteger,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    let numerator = left.numer().try_mul(right.numer())?;
    let denominator = left.denom().try_mul(right.denom())?;
    try_rational_new(numerator, denominator)
}

pub fn try_div(
    left: &RationalInteger,
    right: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    if right.numer().is_zero() {
        return Err(NumericFailure::DivisionByZero);
    }
    let numerator = left.numer().try_mul(right.denom())?;
    let denominator = left.denom().try_mul(right.numer())?;
    try_rational_new(numerator, denominator)
}

pub fn try_pow_i32(
    base: &RationalInteger,
    exponent: i32,
) -> Result<RationalInteger, NumericFailure> {
    if exponent == 0 {
        return Ok(rational_one());
    }
    if exponent < 0 {
        if base.numer().is_zero() {
            return Err(NumericFailure::DivisionByZero);
        }
        let positive_base = try_rational_new(base.denom().try_clone()?, base.numer().try_clone()?)?;
        return try_pow_i32(&positive_base, -exponent);
    }
    let mut result = rational_one();
    let mut factor = base.clone().try_reduce()?;
    let mut remaining = exponent as u32;
    while remaining > 0 {
        if remaining % 2 == 1 {
            result = try_mul(&result, &factor)?;
        }
        remaining /= 2;
        if remaining > 0 {
            factor = try_mul(&factor, &factor)?;
        }
    }
    Ok(result)
}

pub fn try_rational_power(
    base: &RationalInteger,
    exponent: &RationalInteger,
) -> Result<RationalInteger, NumericFailure> {
    let exp_numer = exponent.numer();
    let exp_denom = exponent.denom();

    assert!(
        exp_denom > &BigInt::from_i64(0),
        "BUG: rational exponent must have positive denominator"
    );

    if *exp_denom == BigInt::one() {
        let exponent_i32 = exp_numer.to_i32().ok_or(NumericFailure::Overflow)?;
        return try_pow_i32(base, exponent_i32);
    }

    if base.numer().is_zero() {
        if exp_numer <= &BigInt::from_i64(0) {
            return Err(NumericFailure::DivisionByZero);
        }
        return Ok(rational_new(0, 1));
    }

    let abs_exp_numer = exp_numer.try_abs()?;
    let abs_exp_i32 = abs_exp_numer.to_i32().ok_or(NumericFailure::Overflow)?;
    let raised = try_pow_i32(base, abs_exp_i32)?;

    let root_degree = exp_denom.to_u32().ok_or(NumericFailure::Overflow)?;

    let raised_numer = raised.numer();
    let raised_denom = raised.denom();

    let (numer_root, numer_negative) = if raised_numer.is_negative() {
        if root_degree % 2 == 0 {
            return Err(NumericFailure::Irrational);
        }
        (raised_numer.try_abs()?.try_nth_root(root_degree)?, true)
    } else {
        (raised_numer.try_nth_root(root_degree)?, false)
    };

    let denom_root = raised_denom.try_nth_root(root_degree)?;

    let numer_reconstructed = numer_root.try_pow_u32(root_degree)?;
    let denom_reconstructed = denom_root.try_pow_u32(root_degree)?;

    if numer_reconstructed != raised_numer.try_abs()? {
        return Err(NumericFailure::Irrational);
    }
    if denom_reconstructed != *raised_denom {
        return Err(NumericFailure::Irrational);
    }

    let signed_numer = if numer_negative {
        numer_root.try_neg()?
    } else {
        numer_root
    };

    let result = try_rational_new(signed_numer, denom_root)?;

    if exp_numer.is_negative() {
        if result.numer().is_zero() {
            return Err(NumericFailure::DivisionByZero);
        }
        try_rational_new(result.denom().try_clone()?, result.numer().try_clone()?)
    } else {
        Ok(result)
    }
}

pub use {
    try_add as checked_add, try_div as checked_div, try_mul as checked_mul,
    try_pow_i32 as checked_pow_i32, try_sub as checked_sub,
};

impl fmt::Display for RationalInteger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_str())
    }
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
        assert_eq!(rational, rational_new(1, 2));
    }

    #[test]
    fn try_to_decimal_one_third() {
        let rational = rational_new(1, 3);
        let decimal = rational.try_to_decimal().unwrap();
        let expected = Decimal::from_str("0.3333333333333333333333333333").unwrap();
        assert_eq!(decimal, expected);
    }

    #[test]
    fn try_to_decimal_tiny_rounds_to_zero() {
        let rational = try_rational_new(
            BigInt::one(),
            BigInt::try_from_str_radix("1000000000000000000000000000000", 10).unwrap(),
        )
        .unwrap();
        assert_eq!(rational.try_to_decimal().unwrap(), Decimal::ZERO);
    }

    #[test]
    fn try_to_decimal_magnitude_beyond_max_returns_overflow() {
        let max = Decimal::MAX.normalize();
        let max_rational = decimal_to_rational(max).unwrap();
        let twice = try_mul(&max_rational, &rational_new(2, 1)).unwrap();
        assert_eq!(
            twice.try_to_decimal().unwrap_err(),
            NumericFailure::Overflow,
        );
    }

    #[test]
    fn try_to_decimal_string_integer() {
        let rational = rational_new(37, 1);
        assert_eq!(rational.try_to_decimal_string().unwrap(), "37");
    }

    #[test]
    fn display_str_shows_decimal_when_try_to_decimal_succeeds() {
        let rational = rational_new(355, 113);
        let display = rational.display_str();
        assert!(
            !display.contains('/'),
            "rational with successful try_to_decimal must display as decimal, got {display}"
        );
    }

    #[test]
    fn try_to_decimal_huge_cancelling_rationals() {
        let numer = BigInt::try_from_str_radix("1", 10)
            .unwrap()
            .try_pow_u32(100)
            .unwrap();
        let rational = try_rational_new(numer.clone(), numer).unwrap();
        assert_eq!(rational.try_to_decimal().unwrap(), Decimal::ONE);
    }

    #[test]
    fn try_mul_integer() {
        let left = rational_new(50, 1);
        let right = rational_new(86400, 1);
        let product = try_mul(&left, &right).unwrap();
        assert_eq!(product, rational_new(4_320_000, 1));
    }

    #[test]
    fn try_pow_negative_exponent_inverts_base() {
        let hour_factor = rational_new(3600, 1);
        let inverse = try_pow_i32(&hour_factor, -1).unwrap();
        assert_eq!(inverse, rational_new(1, 3600));
    }

    #[test]
    fn rational_operation_divide_by_zero() {
        let left = rational_new(1, 1);
        let right = rational_new(0, 1);
        let failure = rational_operation(&left, NumericOperation::Divide, &right).unwrap_err();
        assert_eq!(failure, NumericFailure::DivisionByZero);
    }

    #[test]
    fn rational_operation_power_irrational() {
        let base = rational_new(2, 1);
        let exponent = rational_new(1, 2);
        let failure = rational_operation(&base, NumericOperation::Power, &exponent).unwrap_err();
        assert_eq!(failure, NumericFailure::Irrational);
    }

    #[test]
    fn rational_operation_power_exact() {
        let base = rational_new(4, 1);
        let exponent = rational_new(1, 2);
        let result = rational_operation(&base, NumericOperation::Power, &exponent).unwrap();
        assert_eq!(result, rational_new(2, 1));
    }

    #[test]
    fn rational_operation_add() {
        let left = rational_new(1, 3);
        let right = rational_new(1, 6);
        let sum = rational_operation(&left, NumericOperation::Add, &right).unwrap();
        assert_eq!(sum, rational_new(1, 2));
    }

    #[test]
    fn rational_operation_with_fallback_add_exact_rational() {
        let left = rational_new(1, 3);
        let right = rational_new(1, 6);
        let sum = rational_operation_with_fallback(&left, NumericOperation::Add, &right).unwrap();
        assert_eq!(sum, rational_new(1, 2));
    }

    #[test]
    fn rational_operation_with_fallback_power_sqrt_via_decimal() {
        let result = rational_operation_with_fallback(
            &rational_new(2, 1),
            NumericOperation::Power,
            &rational_new(1, 2),
        )
        .unwrap();
        let expected = rational_new(2, 1).try_to_decimal().unwrap().sqrt().unwrap();
        assert_eq!(
            result.try_to_decimal().unwrap().round_dp(27),
            expected.round_dp(27),
        );
    }

    #[test]
    fn rational_abs_negates_negative_numerator() {
        let negative = rational_new(-172_800, 1);
        assert_eq!(rational_abs(&negative).unwrap(), rational_new(172_800, 1));
    }

    #[test]
    fn try_to_decimal_string_rejects_magnitude_overflow() {
        let too_large = try_rational_new(
            BigInt::try_from_str_radix("10000000000000000000000000000000", 10).unwrap(),
            BigInt::one(),
        )
        .unwrap();
        assert_eq!(
            too_large.try_to_decimal().unwrap_err(),
            NumericFailure::Overflow
        );
        assert_eq!(
            too_large.try_to_decimal_string().unwrap_err(),
            NumericFailure::Overflow,
        );
    }

    #[test]
    fn decimal_max_times_decimal_max_stays_exact_without_decimal_fallback() {
        let max_decimal = Decimal::MAX.normalize();
        let left = decimal_to_rational(max_decimal).unwrap();
        let right = decimal_to_rational(max_decimal).unwrap();
        let product = rational_operation(&left, NumericOperation::Multiply, &right).unwrap();
        let expected = try_mul(&left, &right).unwrap();
        assert_eq!(product, expected);
    }

    #[test]
    fn alloc_error_maps_to_out_of_memory() {
        assert_eq!(
            NumericFailure::from(crate::computation::bigint::AllocError),
            NumericFailure::OutOfMemory
        );
    }
}
