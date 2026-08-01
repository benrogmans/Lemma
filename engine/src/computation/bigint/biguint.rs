//! Fallible arbitrary-precision unsigned integers (algorithms from num-bigint 0.4.6).

use std::cmp::Ordering;
use std::fmt;

use super::alloc::{try_reserve_exact, try_with_capacity, AllocError};
use super::digit::{BigDigit, DoubleBigDigit, BITS};

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BigUint {
    digits: Vec<BigDigit>,
}

impl BigUint {
    pub fn zero() -> Self {
        Self { digits: vec![0] }
    }

    pub fn one() -> Self {
        Self { digits: vec![1] }
    }

    pub fn is_zero(&self) -> bool {
        self.digits.len() == 1 && self.digits[0] == 0
    }

    pub fn is_one(&self) -> bool {
        self.digits.len() == 1 && self.digits[0] == 1
    }

    pub fn try_clone(&self) -> Result<Self, AllocError> {
        let mut digits = try_with_capacity(self.digits.len())?;
        digits.extend_from_slice(&self.digits);
        Ok(Self { digits })
    }

    pub fn try_from_u32(value: u32) -> Result<Self, AllocError> {
        Ok(Self {
            digits: vec![value],
        })
    }

    pub fn try_from_u64(value: u64) -> Result<Self, AllocError> {
        if value == 0 {
            return Ok(Self::zero());
        }
        let lo = value as BigDigit;
        let hi = (value >> BITS) as BigDigit;
        if hi == 0 {
            Ok(Self { digits: vec![lo] })
        } else {
            Ok(Self {
                digits: vec![lo, hi],
            })
        }
    }

    pub fn try_from_u128(value: u128) -> Result<Self, AllocError> {
        if value == 0 {
            return Ok(Self::zero());
        }
        let mut digits = try_with_capacity(4)?;
        let mut v = value;
        while v > 0 {
            digits.push(v as BigDigit);
            v >>= BITS;
        }
        Ok(Self { digits })
    }

    pub fn try_from_str_radix(s: &str, radix: u32) -> Result<Self, AllocError> {
        if radix != 10 {
            return Err(AllocError);
        }
        let s = s.trim();
        if s.is_empty() {
            return Err(AllocError);
        }
        let mut result = Self::zero();
        for ch in s.chars() {
            let digit = ch.to_digit(radix).ok_or(AllocError)?;
            result = result.try_mul_u32(10)?.try_add_u32(digit)?;
        }
        Ok(result)
    }

    fn normalize(&mut self) {
        while self.digits.len() > 1 && self.digits.last() == Some(&0) {
            self.digits.pop();
        }
        if self.digits.is_empty() {
            self.digits.push(0);
        }
    }

    pub fn bits(&self) -> u64 {
        if self.is_zero() {
            return 0;
        }
        let top = self.digits[self.digits.len() - 1];
        (self.digits.len() as u64 - 1) * u64::from(BITS) + (64 - top.leading_zeros() as u64)
    }

    pub fn cmp(&self, other: &Self) -> Ordering {
        cmp_slices(&self.digits, &other.digits)
    }

    pub fn try_add(&self, other: &Self) -> Result<Self, AllocError> {
        let max_len = self.digits.len().max(other.digits.len());
        let out_len = max_len + 1;
        let mut out = try_with_capacity(out_len)?;
        out.resize(out_len, 0);
        let mut carry: DoubleBigDigit = 0;
        for (i, out_digit) in out.iter_mut().enumerate().take(max_len) {
            carry += DoubleBigDigit::from(*self.digits.get(i).unwrap_or(&0))
                + DoubleBigDigit::from(*other.digits.get(i).unwrap_or(&0));
            *out_digit = carry as BigDigit;
            carry >>= BITS;
        }
        if carry != 0 {
            out[max_len] = carry as BigDigit;
        } else {
            out.truncate(max_len);
        }
        let mut result = Self { digits: out };
        result.normalize();
        Ok(result)
    }

    fn try_add_u32(&self, other: u32) -> Result<Self, AllocError> {
        self.try_add(&Self::try_from_u32(other)?)
    }

    pub fn try_sub(&self, other: &Self) -> Result<Self, AllocError> {
        if self.cmp(other) == Ordering::Less {
            return Err(AllocError);
        }
        let len = self.digits.len();
        let mut out = try_with_capacity(len)?;
        out.extend_from_slice(&self.digits);
        sub_in_place(&mut out, &other.digits);
        let mut result = Self { digits: out };
        result.normalize();
        Ok(result)
    }

    pub fn try_mul(&self, other: &Self) -> Result<Self, AllocError> {
        if self.is_zero() || other.is_zero() {
            return Ok(Self::zero());
        }
        let a = trim_leading_zeros(&self.digits);
        let b = trim_leading_zeros(&other.digits);
        let out_len = a.len() + b.len();
        let mut out = try_with_capacity(out_len)?;
        out.resize(out_len, 0);
        schoolbook_mul(&mut out, a, b);
        let mut result = Self { digits: out };
        result.normalize();
        Ok(result)
    }

    pub fn try_mul_u32(&self, other: u32) -> Result<Self, AllocError> {
        if other == 0 || self.is_zero() {
            return Ok(Self::zero());
        }
        let mut digits = try_with_capacity(self.digits.len())?;
        digits.extend_from_slice(&self.digits);
        mul_in_place(&mut digits, other as BigDigit)?;
        let mut result = Self { digits };
        result.normalize();
        Ok(result)
    }

    pub fn try_div_rem(&self, other: &Self) -> Result<(Self, Self), AllocError> {
        if other.is_zero() {
            return Err(AllocError);
        }
        if self.is_zero() {
            return Ok((Self::zero(), Self::zero()));
        }
        match self.cmp(other) {
            Ordering::Less => return Ok((Self::zero(), self.try_clone()?)),
            Ordering::Equal => return Ok((Self::one(), Self::zero())),
            Ordering::Greater => {}
        }
        if other.digits.len() == 1 {
            return div_rem_digit(self.try_clone()?, other.digits[0]);
        }
        let shift = other
            .digits
            .last()
            .expect("BUG: non-empty divisor")
            .leading_zeros();
        let (q, r) = if shift == 0 {
            div_rem_core(self.try_clone()?, &other.digits)?
        } else {
            let shifted_dividend = self.try_shl(shift)?;
            let shifted_divisor = other.try_shl(shift)?;
            let (q, r) = div_rem_core(shifted_dividend, &shifted_divisor.digits)?;
            (q, r.try_shr(shift)?)
        };
        Ok((q, r))
    }

    pub fn try_gcd(&self, other: &Self) -> Result<Self, AllocError> {
        let mut a = self.try_clone()?;
        let mut b = other.try_clone()?;
        while !b.is_zero() {
            let (_, r) = a.try_div_rem(&b)?;
            a = b;
            b = r;
        }
        Ok(a)
    }

    pub fn try_pow_u32(&self, exp: u32) -> Result<Self, AllocError> {
        if exp == 0 {
            return Ok(Self::one());
        }
        let mut base = self.try_clone()?;
        let mut result = Self::one();
        let mut e = exp;
        while e > 0 {
            if e & 1 == 1 {
                result = result.try_mul(&base)?;
            }
            e >>= 1;
            if e > 0 {
                base = base.try_mul(&base)?;
            }
        }
        Ok(result)
    }

    pub fn try_nth_root(&self, n: u32) -> Result<Self, AllocError> {
        if n == 0 {
            return Err(AllocError);
        }
        if self.is_zero() {
            return Ok(Self::zero());
        }
        if n == 1 {
            return self.try_clone();
        }
        let mut lo = Self::zero();
        let mut hi = self.try_add(&Self::one())?;
        while lo.cmp(&hi) == Ordering::Less {
            let two = Self::try_from_u32(2)?;
            let mid = {
                let sum = lo.try_add(&hi)?;
                let (q, _) = sum.try_div_rem(&two)?;
                q
            };
            let mid_pow = mid.try_pow_u32(n)?;
            match mid_pow.cmp(self) {
                Ordering::Less | Ordering::Equal => {
                    let next = mid.try_add(&Self::one())?;
                    let next_pow = next.try_pow_u32(n)?;
                    if next_pow.cmp(self) == Ordering::Greater {
                        return Ok(mid);
                    }
                    lo = next;
                }
                Ordering::Greater => {
                    hi = mid;
                }
            }
        }
        Ok(lo)
    }

    pub fn to_u32(&self) -> Option<u32> {
        if self.digits.len() == 1 {
            Some(self.digits[0])
        } else {
            None
        }
    }

    pub fn as_digits(&self) -> &[BigDigit] {
        &self.digits
    }

    pub fn try_to_string_dec(&self) -> Result<String, AllocError> {
        if self.is_zero() {
            return Ok("0".to_string());
        }
        let ten = BigUint::try_from_u32(10)?;
        let mut n = self.try_clone()?;
        let mut digits = Vec::new();
        while !n.is_zero() {
            let (q, r) = n.try_div_rem(&ten)?;
            digits.push(r.to_u32().expect("BUG: rem fits u32") as u8);
            n = q;
        }
        Ok(digits.iter().rev().map(|d| (b'0' + *d) as char).collect())
    }

    pub fn to_string_dec(&self) -> String {
        self.try_to_string_dec()
            .expect("BUG: infallible display context")
    }

    pub fn to_u128(&self) -> Option<u128> {
        if self.is_zero() {
            return Some(0);
        }
        if self.digits.len() > 4 {
            return None;
        }
        let mut result: u128 = 0;
        for (i, &d) in self.digits.iter().enumerate() {
            result |= u128::from(d) << (i * 32);
        }
        Some(result)
    }

    fn try_shl(&self, bits: u32) -> Result<Self, AllocError> {
        if bits == 0 || self.is_zero() {
            return self.try_clone();
        }
        let digit_shift = (bits / BITS) as usize;
        let bit_shift = bits % BITS;
        let extra = usize::from(bit_shift > 0);
        let new_len = self.digits.len() + digit_shift + extra;
        let mut out = try_with_capacity(new_len)?;
        out.resize(new_len, 0);
        if bit_shift == 0 {
            out[digit_shift..digit_shift + self.digits.len()].copy_from_slice(&self.digits);
        } else {
            let mut carry = 0u64;
            for (i, &d) in self.digits.iter().enumerate() {
                let v = (u64::from(d) << bit_shift) | carry;
                out[digit_shift + i] = v as BigDigit;
                carry = v >> BITS;
            }
            if carry != 0 {
                out[digit_shift + self.digits.len()] = carry as BigDigit;
            }
        }
        let mut result = Self { digits: out };
        result.normalize();
        Ok(result)
    }

    fn try_shr(&self, bits: u32) -> Result<Self, AllocError> {
        if bits == 0 || self.is_zero() {
            return self.try_clone();
        }
        let digit_shift = (bits / BITS) as usize;
        if digit_shift >= self.digits.len() {
            return Ok(Self::zero());
        }
        let bit_shift = bits % BITS;
        let mut out = try_with_capacity(self.digits.len())?;
        if bit_shift == 0 {
            out.extend_from_slice(&self.digits[digit_shift..]);
        } else {
            let mut carry = 0u64;
            for &d in self.digits.iter().rev().skip(digit_shift) {
                let v = u64::from(d);
                out.push(((v >> bit_shift) | carry) as BigDigit);
                carry = (v & ((1u64 << bit_shift) - 1)) << (BITS - bit_shift);
            }
            out.reverse();
        }
        let mut result = Self { digits: out };
        result.normalize();
        Ok(result)
    }
}

fn trim_leading_zeros(digits: &[BigDigit]) -> &[BigDigit] {
    match digits.iter().rposition(|&d| d != 0) {
        Some(i) => &digits[..=i],
        None => &digits[..1],
    }
}

fn cmp_slices(a: &[BigDigit], b: &[BigDigit]) -> Ordering {
    let a = trim_leading_zeros(a);
    let b = trim_leading_zeros(b);
    match a.len().cmp(&b.len()) {
        Ordering::Equal => a.iter().rev().cmp(b.iter().rev()),
        other => other,
    }
}

fn sub_in_place(a: &mut [BigDigit], b: &[BigDigit]) {
    let mut borrow: DoubleBigDigit = 0;
    for (i, &db) in b.iter().enumerate() {
        let ai = DoubleBigDigit::from(a[i]);
        let diff = ai
            .wrapping_sub(DoubleBigDigit::from(db))
            .wrapping_sub(borrow);
        borrow = if diff > ai { 1 } else { 0 };
        a[i] = diff as BigDigit;
    }
    for a in a.iter_mut().skip(b.len()) {
        if borrow == 0 {
            break;
        }
        let ai = DoubleBigDigit::from(*a);
        let diff = ai.wrapping_sub(borrow);
        borrow = if diff > ai { 1 } else { 0 };
        *a = diff as BigDigit;
    }
}

fn mul_in_place(digits: &mut Vec<BigDigit>, mul: BigDigit) -> Result<(), AllocError> {
    let mut carry: DoubleBigDigit = 0;
    for d in digits.iter_mut() {
        carry += DoubleBigDigit::from(*d) * DoubleBigDigit::from(mul);
        *d = carry as BigDigit;
        carry >>= BITS;
    }
    if carry != 0 {
        try_reserve_exact(digits, 1)?;
        digits.push(carry as BigDigit);
    }
    Ok(())
}

fn schoolbook_mul(out: &mut [BigDigit], a: &[BigDigit], b: &[BigDigit]) {
    for (i, &bc) in b.iter().enumerate() {
        if bc == 0 {
            continue;
        }
        let mut carry: DoubleBigDigit = 0;
        for (j, &ac) in a.iter().enumerate() {
            let idx = i + j;
            carry += DoubleBigDigit::from(out[idx])
                + DoubleBigDigit::from(ac) * DoubleBigDigit::from(bc);
            out[idx] = carry as BigDigit;
            carry >>= BITS;
        }
        if carry != 0 {
            out[i + a.len()] = carry as BigDigit;
        }
    }
}

fn div_wide(hi: BigDigit, lo: BigDigit, divisor: BigDigit) -> (BigDigit, BigDigit) {
    debug_assert!(hi < divisor);
    let lhs = (DoubleBigDigit::from(hi) << BITS) | DoubleBigDigit::from(lo);
    let rhs = DoubleBigDigit::from(divisor);
    ((lhs / rhs) as BigDigit, (lhs % rhs) as BigDigit)
}

fn div_rem_digit(mut a: BigUint, b: BigDigit) -> Result<(BigUint, BigUint), AllocError> {
    if b == 0 {
        return Err(AllocError);
    }
    let mut rem = 0;
    for d in a.digits.iter_mut().rev() {
        let (q, r) = div_wide(rem, *d, b);
        *d = q;
        rem = r;
    }
    a.normalize();
    Ok((a, BigUint::try_from_u32(rem)?))
}

fn add2_in_place(a: &mut [BigDigit], b: &[BigDigit]) -> BigDigit {
    let mut carry: DoubleBigDigit = 0;
    for (i, &db) in b.iter().enumerate() {
        carry += DoubleBigDigit::from(a[i]) + DoubleBigDigit::from(db);
        a[i] = carry as BigDigit;
        carry >>= BITS;
    }
    for a in a.iter_mut().skip(b.len()) {
        if carry == 0 {
            break;
        }
        carry += DoubleBigDigit::from(*a);
        *a = carry as BigDigit;
        carry >>= BITS;
    }
    carry as BigDigit
}

fn sub_mul_digit_same_len(a: &mut [BigDigit], b: &[BigDigit], c: BigDigit) -> BigDigit {
    const MAX: BigDigit = BigDigit::MAX;
    let mut offset_carry = MAX;
    for (x, y) in a.iter_mut().zip(b) {
        let offset_sum = ((DoubleBigDigit::from(MAX) << BITS) | DoubleBigDigit::from(*x))
            - DoubleBigDigit::from(MAX)
            + DoubleBigDigit::from(offset_carry)
            - DoubleBigDigit::from(*y) * DoubleBigDigit::from(c);
        offset_carry = (offset_sum >> BITS) as BigDigit;
        *x = offset_sum as BigDigit;
    }
    MAX - offset_carry
}

fn div_rem_core(mut a: BigUint, b: &[BigDigit]) -> Result<(BigUint, BigUint), AllocError> {
    debug_assert!(a.digits.len() >= b.len() && b.len() > 1);
    debug_assert!(b.last().unwrap().leading_zeros() == 0);

    let mut a0 = 0;
    let b0 = b[b.len() - 1];
    let b1 = b[b.len() - 2];
    let q_len = a.digits.len() - b.len() + 1;
    let mut q = try_with_capacity(q_len)?;
    q.resize(q_len, 0);

    for j in (0..q_len).rev() {
        let a1 = *a.digits.last().expect("BUG: dividend digit");
        let a2 = a.digits[a.digits.len() - 2];

        let (mut q0, mut r) = if a0 < b0 {
            let (q0, r) = div_wide(a0, a1, b0);
            (q0, r as DoubleBigDigit)
        } else {
            (BigDigit::MAX, a0 as DoubleBigDigit + a1 as DoubleBigDigit)
        };

        while r <= BigDigit::MAX as DoubleBigDigit
            && ((r << BITS) | DoubleBigDigit::from(a2))
                < q0 as DoubleBigDigit * b1 as DoubleBigDigit
        {
            q0 -= 1;
            r += b0 as DoubleBigDigit;
        }

        let mut borrow = sub_mul_digit_same_len(&mut a.digits[j..], b, q0);
        if borrow > a0 {
            q0 -= 1;
            borrow -= add2_in_place(&mut a.digits[j..], b);
        }
        debug_assert!(borrow == a0);
        q[j] = q0;
        a0 = a.digits.pop().expect("BUG: dividend digit pop");
    }

    a.digits.push(a0);
    a.normalize();
    debug_assert!(cmp_slices(&a.digits, b) == Ordering::Less);

    let mut q_uint = BigUint { digits: q };
    q_uint.normalize();
    Ok((q_uint, a))
}

impl fmt::Display for BigUint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.try_to_string_dec() {
            Ok(s) => write!(f, "{s}"),
            Err(_) => f.write_str("<out of memory>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_mul_small() {
        let a = BigUint::try_from_u32(12345).unwrap();
        let b = BigUint::try_from_u32(67890).unwrap();
        let c = a.try_mul(&b).unwrap();
        assert_eq!(c.to_u128(), Some(838_102_050));
    }

    #[test]
    fn div_rem_basic() {
        let a = BigUint::try_from_u32(100).unwrap();
        let b = BigUint::try_from_u32(7).unwrap();
        let (q, r) = a.try_div_rem(&b).unwrap();
        assert_eq!(q.to_u32(), Some(14));
        assert_eq!(r.to_u32(), Some(2));
    }

    #[test]
    fn try_div_rem_zero_divisor_returns_alloc_error() {
        let a = BigUint::try_from_u32(2).unwrap();
        let zero = BigUint::try_from_u32(0).unwrap();
        assert!(a.try_div_rem(&zero).is_err());
    }
}
