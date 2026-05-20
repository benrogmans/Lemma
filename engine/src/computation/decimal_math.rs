//! Decimal-precision implementations of transcendental functions not provided by
//! `rust_decimal::MathematicalOps`: `atan`, `asin`, `acos`.
//!
//! All other transcendental functions (`sqrt`, `sin`, `cos`, `tan`, `ln`, `exp`) are
//! delegated to `rust_decimal::MathematicalOps` which operates at ~28-digit precision.

use rust_decimal::Decimal;
use rust_decimal::MathematicalOps;
use std::str::FromStr;

fn half_pi() -> Decimal {
    Decimal::from_str("1.5707963267948966192313216916").expect("BUG: π/2 constant failed to parse")
}

/// atan(x) using Taylor series for |x| <= 1, identity `sign(x)*π/2 - atan(1/x)` for |x| > 1.
pub fn decimal_atan(x: Decimal) -> Option<Decimal> {
    if x.is_zero() {
        return Some(Decimal::ZERO);
    }
    let abs_x = x.abs();
    if abs_x > Decimal::ONE {
        // atan(x) = sign(x) * π/2 - atan(1/x)
        let recip = Decimal::ONE.checked_div(x)?;
        let inner = decimal_atan(recip)?;
        let sign = if x > Decimal::ZERO {
            Decimal::ONE
        } else {
            Decimal::NEGATIVE_ONE
        };
        let hp = half_pi();
        return (sign * hp).checked_sub(inner);
    }
    // Taylor series: atan(x) = x - x^3/3 + x^5/5 - ...
    // Converges for |x| <= 1; iterate until contribution falls below Decimal precision.
    let x2 = x.checked_mul(x)?;
    let mut term = x;
    let mut result = x;
    let mut n: i64 = 1;
    loop {
        n += 2;
        term = term.checked_mul(x2)?;
        term = term.checked_mul(Decimal::NEGATIVE_ONE)?;
        let contribution = term.checked_div(Decimal::from(n))?;
        let prev = result;
        result = result.checked_add(contribution)?;
        if result == prev {
            break;
        }
    }
    Some(result)
}

/// asin(x) = atan(x / sqrt(1 - x²)) for |x| < 1, ±π/2 for |x| = 1.
/// Returns None for |x| > 1 (domain error — legitimate Veto condition).
pub fn decimal_asin(x: Decimal) -> Option<Decimal> {
    let hp = half_pi();
    if x == Decimal::ONE {
        return Some(hp);
    }
    if x == Decimal::NEGATIVE_ONE {
        return Some(-hp);
    }
    if x.abs() > Decimal::ONE {
        return None;
    }
    let one_minus_x2 = Decimal::ONE.checked_sub(x.checked_mul(x)?)?;
    let denom = one_minus_x2.sqrt()?;
    if denom.is_zero() {
        return None;
    }
    decimal_atan(x.checked_div(denom)?)
}

/// acos(x) = π/2 - asin(x).
/// Returns None for |x| > 1 (domain error — legitimate Veto condition).
pub fn decimal_acos(x: Decimal) -> Option<Decimal> {
    let asin_x = decimal_asin(x)?;
    half_pi().checked_sub(asin_x)
}
