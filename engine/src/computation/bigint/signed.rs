//! Fallible signed arbitrary-precision integers.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use super::alloc::AllocError;
use super::biguint::BigUint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sign {
    Minus = -1,
    Zero = 0,
    Plus = 1,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BigInt {
    sign: Sign,
    magnitude: BigUint,
}

impl BigInt {
    pub fn zero() -> Self {
        Self {
            sign: Sign::Zero,
            magnitude: BigUint::zero(),
        }
    }

    pub fn one() -> Self {
        Self::from_i64(1)
    }

    pub fn from_i64(value: i64) -> Self {
        if value == 0 {
            return Self::zero();
        }
        let sign = if value < 0 { Sign::Minus } else { Sign::Plus };
        Self {
            sign,
            magnitude: BigUint::try_from_u64(value.unsigned_abs())
                .expect("BUG: i64 fits in BigUint"),
        }
    }

    pub fn from_i128(value: i128) -> Self {
        if value == 0 {
            return Self::zero();
        }
        let sign = if value < 0 { Sign::Minus } else { Sign::Plus };
        let abs = if value == i128::MIN {
            BigUint::try_from_u128(1u128 << 127).expect("BUG: i128::MIN magnitude")
        } else {
            BigUint::try_from_u128(value.unsigned_abs()).expect("BUG: i128 fits in BigUint")
        };
        Self {
            sign,
            magnitude: abs,
        }
    }

    pub fn try_from_u32(value: u32) -> Result<Self, AllocError> {
        Ok(Self {
            sign: if value == 0 { Sign::Zero } else { Sign::Plus },
            magnitude: BigUint::try_from_u32(value)?,
        })
    }

    pub fn try_from_str_radix(s: &str, radix: u32) -> Result<Self, AllocError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(AllocError);
        }
        if let Some(rest) = s.strip_prefix('-') {
            let mag = BigUint::try_from_str_radix(rest, radix)?;
            if mag.is_zero() {
                Ok(Self::zero())
            } else {
                Ok(Self {
                    sign: Sign::Minus,
                    magnitude: mag,
                })
            }
        } else if let Some(rest) = s.strip_prefix('+') {
            let mag = BigUint::try_from_str_radix(rest, radix)?;
            Ok(Self {
                sign: if mag.is_zero() {
                    Sign::Zero
                } else {
                    Sign::Plus
                },
                magnitude: mag,
            })
        } else {
            let mag = BigUint::try_from_str_radix(s, radix)?;
            Ok(Self {
                sign: if mag.is_zero() {
                    Sign::Zero
                } else {
                    Sign::Plus
                },
                magnitude: mag,
            })
        }
    }

    pub fn sign(&self) -> Sign {
        self.sign
    }

    pub fn is_zero(&self) -> bool {
        self.magnitude.is_zero()
    }

    pub fn is_negative(&self) -> bool {
        matches!(self.sign, Sign::Minus)
    }

    pub fn is_positive(&self) -> bool {
        matches!(self.sign, Sign::Plus)
    }

    pub fn magnitude(&self) -> &BigUint {
        &self.magnitude
    }

    pub fn try_clone(&self) -> Result<Self, AllocError> {
        Ok(Self {
            sign: self.sign,
            magnitude: self.magnitude.try_clone()?,
        })
    }

    pub fn try_abs(&self) -> Result<Self, AllocError> {
        Ok(Self {
            sign: if self.is_zero() {
                Sign::Zero
            } else {
                Sign::Plus
            },
            magnitude: self.magnitude.try_clone()?,
        })
    }

    pub fn try_neg(&self) -> Result<Self, AllocError> {
        if self.is_zero() {
            Ok(Self::zero())
        } else {
            Ok(Self {
                sign: match self.sign {
                    Sign::Plus => Sign::Minus,
                    Sign::Minus => Sign::Plus,
                    Sign::Zero => Sign::Zero,
                },
                magnitude: self.magnitude.try_clone()?,
            })
        }
    }

    pub fn try_add(&self, other: &Self) -> Result<Self, AllocError> {
        match (self.sign, other.sign) {
            (Sign::Zero, _) => other.try_clone(),
            (_, Sign::Zero) => self.try_clone(),
            (Sign::Plus, Sign::Plus) => Ok(Self {
                sign: Sign::Plus,
                magnitude: self.magnitude.try_add(&other.magnitude)?,
            }),
            (Sign::Minus, Sign::Minus) => Ok(Self {
                sign: Sign::Minus,
                magnitude: self.magnitude.try_add(&other.magnitude)?,
            }),
            (Sign::Plus, Sign::Minus) => match self.magnitude.cmp(&other.magnitude) {
                Ordering::Less => Ok(Self {
                    sign: Sign::Minus,
                    magnitude: other.magnitude.try_sub(&self.magnitude)?,
                }),
                Ordering::Greater => Ok(Self {
                    sign: Sign::Plus,
                    magnitude: self.magnitude.try_sub(&other.magnitude)?,
                }),
                Ordering::Equal => Ok(Self::zero()),
            },
            (Sign::Minus, Sign::Plus) => match self.magnitude.cmp(&other.magnitude) {
                Ordering::Less => Ok(Self {
                    sign: Sign::Plus,
                    magnitude: other.magnitude.try_sub(&self.magnitude)?,
                }),
                Ordering::Greater => Ok(Self {
                    sign: Sign::Minus,
                    magnitude: self.magnitude.try_sub(&other.magnitude)?,
                }),
                Ordering::Equal => Ok(Self::zero()),
            },
        }
    }

    pub fn try_sub(&self, other: &Self) -> Result<Self, AllocError> {
        self.try_add(&other.try_neg()?)
    }

    pub fn try_mul(&self, other: &Self) -> Result<Self, AllocError> {
        let magnitude = self.magnitude.try_mul(&other.magnitude)?;
        let sign = match (self.sign, other.sign) {
            (Sign::Zero, _) | (_, Sign::Zero) => Sign::Zero,
            (Sign::Plus, Sign::Plus) | (Sign::Minus, Sign::Minus) => Sign::Plus,
            (Sign::Plus, Sign::Minus) | (Sign::Minus, Sign::Plus) => Sign::Minus,
        };
        Ok(Self { sign, magnitude })
    }

    pub fn try_div_rem(&self, other: &Self) -> Result<(Self, Self), AllocError> {
        if other.is_zero() {
            return Err(AllocError);
        }
        let (q_mag, r_mag) = self.magnitude.try_div_rem(&other.magnitude)?;
        let q_sign = match (self.sign, other.sign) {
            (Sign::Zero, _) | (_, Sign::Zero) => Sign::Zero,
            (Sign::Plus, Sign::Plus) | (Sign::Minus, Sign::Minus) => Sign::Plus,
            (Sign::Plus, Sign::Minus) | (Sign::Minus, Sign::Plus) => Sign::Minus,
        };
        let q = Self {
            sign: if q_mag.is_zero() { Sign::Zero } else { q_sign },
            magnitude: q_mag,
        };
        let r = Self {
            sign: if r_mag.is_zero() {
                Sign::Zero
            } else if self.is_negative() {
                Sign::Minus
            } else {
                Sign::Plus
            },
            magnitude: r_mag,
        };
        Ok((q, r))
    }

    pub fn try_div_trunc(&self, other: &Self) -> Result<Self, AllocError> {
        self.try_div_rem(other).map(|(q, _)| q)
    }

    pub fn try_rem(&self, other: &Self) -> Result<Self, AllocError> {
        self.try_div_rem(other).map(|(_, r)| r)
    }

    pub fn try_gcd(&self, other: &Self) -> Result<Self, AllocError> {
        Ok(Self {
            sign: Sign::Plus,
            magnitude: self.magnitude.try_gcd(&other.magnitude)?,
        })
    }

    pub fn try_pow_u32(&self, exp: u32) -> Result<Self, AllocError> {
        let magnitude = self.magnitude.try_pow_u32(exp)?;
        let sign = if exp.is_multiple_of(2) || !self.is_negative() {
            if magnitude.is_zero() {
                Sign::Zero
            } else {
                Sign::Plus
            }
        } else {
            Sign::Minus
        };
        Ok(Self { sign, magnitude })
    }

    pub fn try_nth_root(&self, n: u32) -> Result<Self, AllocError> {
        if self.is_negative() && n.is_multiple_of(2) {
            return Err(AllocError);
        }
        let root_mag = self.magnitude.try_nth_root(n)?;
        let sign = if self.is_negative() && !root_mag.is_zero() {
            Sign::Minus
        } else if root_mag.is_zero() {
            Sign::Zero
        } else {
            Sign::Plus
        };
        Ok(Self {
            sign,
            magnitude: root_mag,
        })
    }

    pub fn to_i32(&self) -> Option<i32> {
        if self.magnitude.as_digits().len() > 1 {
            return None;
        }
        let u = self.magnitude.to_u32()?;
        match self.sign {
            Sign::Zero => Some(0),
            Sign::Plus => i32::try_from(u).ok(),
            Sign::Minus => i32::try_from(u).ok().map(|v| -v),
        }
    }

    pub fn to_i128(&self) -> Option<i128> {
        let u = self.magnitude.to_u128()?;
        match self.sign {
            Sign::Zero => Some(0),
            Sign::Plus => Some(u as i128),
            Sign::Minus => Some(-(u as i128)),
        }
    }

    pub fn to_u32(&self) -> Option<u32> {
        if self.is_negative() {
            None
        } else {
            self.magnitude.to_u32()
        }
    }

    pub fn to_u8(&self) -> Option<u8> {
        self.to_u32().and_then(|v| u8::try_from(v).ok())
    }

    pub fn to_usize(&self) -> Option<usize> {
        self.magnitude
            .to_u128()
            .and_then(|v| usize::try_from(v).ok())
    }

    pub fn bits(&self) -> u64 {
        self.magnitude.bits()
    }
}

impl fmt::Debug for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(std::cmp::Ord::cmp(self, other))
    }
}

impl Ord for BigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.sign, other.sign) {
            (Sign::Zero, Sign::Zero) => Ordering::Equal,
            (Sign::Zero, Sign::Plus) => Ordering::Less,
            (Sign::Zero, Sign::Minus) => Ordering::Greater,
            (Sign::Plus, Sign::Zero) => Ordering::Greater,
            (Sign::Minus, Sign::Zero) => Ordering::Less,
            (Sign::Plus, Sign::Plus) => self.magnitude.cmp(&other.magnitude),
            (Sign::Minus, Sign::Minus) => other.magnitude.cmp(&self.magnitude),
            (Sign::Plus, Sign::Minus) => Ordering::Greater,
            (Sign::Minus, Sign::Plus) => Ordering::Less,
        }
    }
}

impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return write!(f, "0");
        }
        if self.is_negative() {
            write!(f, "-")?;
        }
        write!(f, "{}", self.magnitude)
    }
}

impl FromStr for BigInt {
    type Err = AllocError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from_str_radix(s, 10)
    }
}
