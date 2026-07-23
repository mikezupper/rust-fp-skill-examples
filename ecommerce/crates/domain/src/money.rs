//! Money.
//!
//! Integer minor units, always. `f64` for currency is a correctness bug, not a
//! style preference: 0.1 + 0.2 != 0.3, and a rounding error in a total is a
//! financial defect. The workspace denies `clippy::float_arithmetic` in this crate
//! so the shortcut is not available.

use std::iter::Sum;
use std::ops::Add;

use serde::{Deserialize, Serialize};

/// A non-negative amount in the smallest currency unit (US cents).
///
/// Non-negativity is an invariant of the type, not a check performed by callers:
/// the inner field is private and the only constructors either take an unsigned
/// value or fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cents(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    #[error("monetary amount {0} is negative")]
    Negative(i64),
    #[error("monetary amount {0} exceeds the representable maximum")]
    Overflow(i64),
    /// Returned instead of wrapping or saturating. A total that silently wraps to a
    /// small number is worse than a failed checkout.
    #[error("monetary arithmetic overflowed")]
    ArithmeticOverflow,
}

impl Cents {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(amount: u32) -> Self {
        Self(amount)
    }

    /// The boundary parser: signed, unbounded input (a database column, a JSON
    /// number) becomes a `Cents` or an error. There is no third option.
    pub fn try_from_i64(amount: i64) -> Result<Self, MoneyError> {
        match u32::try_from(amount) {
            Ok(v) => Ok(Self(v)),
            Err(_) if amount < 0 => Err(MoneyError::Negative(amount)),
            Err(_) => Err(MoneyError::Overflow(amount)),
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn as_i64(self) -> i64 {
        self.0 as i64
    }

    /// Checked multiplication by a count. Returns an error rather than wrapping.
    pub fn checked_mul(self, factor: u32) -> Result<Self, MoneyError> {
        self.0
            .checked_mul(factor)
            .map(Self)
            .ok_or(MoneyError::ArithmeticOverflow)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(MoneyError::ArithmeticOverflow)
    }
}

/// Saturating `Add` so `Cents` composes with `Sum`. Every path that can realistically
/// overflow uses `checked_add`/`checked_mul` instead; this exists so iterator chains
/// read naturally, and saturation is the safe direction (a capped total is visible,
/// a wrapped total is not).
impl Add for Cents {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl Sum for Cents {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Add::add)
    }
}

impl<'a> Sum<&'a Cents> for Cents {
    fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
        iter.copied().sum()
    }
}

impl std::fmt::Display for Cents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{:02}", self.0 / 100, self.0 % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn negative_amounts_are_rejected() {
        assert!(matches!(
            Cents::try_from_i64(-1),
            Err(MoneyError::Negative(-1))
        ));
    }

    #[test]
    fn oversized_amounts_are_rejected() {
        assert!(matches!(
            Cents::try_from_i64(i64::MAX),
            Err(MoneyError::Overflow(_))
        ));
    }

    proptest! {
        /// Round-trip: every value that parses renders back to the same integer.
        #[test]
        fn i64_round_trips(raw in 0i64..=i64::from(u32::MAX)) {
            let cents = Cents::try_from_i64(raw).expect("in range");
            prop_assert_eq!(cents.as_i64(), raw);
        }

        /// Invariant: a sum is never less than any of its parts.
        #[test]
        fn sum_dominates_parts(parts in prop::collection::vec(0u32..1_000_000, 0..32)) {
            let cents: Vec<Cents> = parts.iter().copied().map(Cents::new).collect();
            let total: Cents = cents.iter().sum();
            for part in &cents {
                prop_assert!(total >= *part);
            }
        }

        /// Commutativity: summation order does not change the total.
        #[test]
        fn sum_is_order_independent(parts in prop::collection::vec(0u32..1_000_000, 0..32)) {
            let forward: Cents = parts.iter().copied().map(Cents::new).sum();
            let backward: Cents = parts.iter().rev().copied().map(Cents::new).sum();
            prop_assert_eq!(forward, backward);
        }
    }
}
