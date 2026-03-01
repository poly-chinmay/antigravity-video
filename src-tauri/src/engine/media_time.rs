//! MediaTime - Integer-precision time representation.
//!
//! # Design Decision
//!
//! All timeline timing uses integer nanoseconds to avoid floating-point
//! precision issues that accumulate over many operations.
//!
//! # Invariants
//!
//! - MediaTime internally stores nanoseconds as i64
//! - All arithmetic is integer-based (no float math)
//! - Conversion to/from f64 happens ONLY at API boundaries

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::ops::{Add, Neg, Sub};

/// Integer-precision time representation.
///
/// Uses nanoseconds internally to avoid floating-point precision loss.
///
/// # Examples
///
/// ```
/// let t1 = MediaTime::from_seconds(10.5);
/// let t2 = MediaTime::from_seconds(5.0);
/// let duration = t1 - t2; // 5.5 seconds
/// assert_eq!(duration.to_seconds(), 5.5);
/// ```
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Hash)]
pub struct MediaTime {
    /// Time in nanoseconds. Signed to allow negative deltas.
    nanos: i64,
}

// =============================================================================
// CONSTANTS
// =============================================================================

/// Nanoseconds per second
const NANOS_PER_SECOND: i64 = 1_000_000_000;

/// Nanoseconds per millisecond
const NANOS_PER_MILLI: i64 = 1_000_000;

// =============================================================================
// CONSTRUCTION
// =============================================================================

impl MediaTime {
    /// Create a MediaTime representing zero.
    pub const ZERO: MediaTime = MediaTime { nanos: 0 };

    /// Create from nanoseconds directly.
    #[inline]
    pub const fn from_nanos(nanos: i64) -> Self {
        Self { nanos }
    }

    /// Create from seconds (f64).
    ///
    /// # Note
    /// This is the ONLY place where float-to-integer conversion occurs.
    /// Use sparingly and only at API boundaries.
    #[inline]
    pub fn from_seconds(seconds: f64) -> Self {
        Self {
            nanos: (seconds * NANOS_PER_SECOND as f64).round() as i64,
        }
    }

    /// Create from milliseconds.
    #[inline]
    pub const fn from_millis(millis: i64) -> Self {
        Self {
            nanos: millis * NANOS_PER_MILLI,
        }
    }

    /// Get raw nanoseconds value.
    #[inline]
    pub const fn as_nanos(&self) -> i64 {
        self.nanos
    }

    /// Convert to seconds (f64).
    ///
    /// # Note
    /// This is the ONLY place where integer-to-float conversion occurs.
    /// Use sparingly and only at API boundaries.
    #[inline]
    pub fn to_seconds(&self) -> f64 {
        self.nanos as f64 / NANOS_PER_SECOND as f64
    }

    /// Convert to milliseconds.
    #[inline]
    pub const fn to_millis(&self) -> i64 {
        self.nanos / NANOS_PER_MILLI
    }

    /// Check if this time is zero.
    #[inline]
    pub const fn is_zero(&self) -> bool {
        self.nanos == 0
    }

    /// Check if this time is positive (> 0).
    #[inline]
    pub const fn is_positive(&self) -> bool {
        self.nanos > 0
    }

    /// Check if this time is negative (< 0).
    #[inline]
    pub const fn is_negative(&self) -> bool {
        self.nanos < 0
    }

    /// Get absolute value.
    #[inline]
    pub const fn abs(&self) -> Self {
        Self {
            nanos: self.nanos.abs(),
        }
    }

    /// Return the maximum of two times.
    #[inline]
    pub fn max(self, other: Self) -> Self {
        if self.nanos >= other.nanos {
            self
        } else {
            other
        }
    }

    /// Return the minimum of two times.
    #[inline]
    pub fn min(self, other: Self) -> Self {
        if self.nanos <= other.nanos {
            self
        } else {
            other
        }
    }
}

// =============================================================================
// ARITHMETIC OPERATIONS
// =============================================================================

impl Add for MediaTime {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            nanos: self.nanos.saturating_add(rhs.nanos),
        }
    }
}

impl Sub for MediaTime {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            nanos: self.nanos.saturating_sub(rhs.nanos),
        }
    }
}

impl Neg for MediaTime {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self::Output {
        Self { nanos: -self.nanos }
    }
}

// =============================================================================
// COMPARISON OPERATIONS
// =============================================================================

impl PartialEq for MediaTime {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.nanos == other.nanos
    }
}

impl Eq for MediaTime {}

impl PartialOrd for MediaTime {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MediaTime {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.nanos.cmp(&other.nanos)
    }
}

// =============================================================================
// DISPLAY
// =============================================================================

impl std::fmt::Display for MediaTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.3}s", self.to_seconds())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_seconds_roundtrip() {
        let original = 10.5;
        let mt = MediaTime::from_seconds(original);
        let back = mt.to_seconds();
        assert!((original - back).abs() < 0.000000001);
    }

    #[test]
    fn test_addition() {
        let a = MediaTime::from_seconds(5.0);
        let b = MediaTime::from_seconds(3.0);
        let c = a + b;
        assert_eq!(c.to_seconds(), 8.0);
    }

    #[test]
    fn test_subtraction() {
        let a = MediaTime::from_seconds(10.0);
        let b = MediaTime::from_seconds(3.0);
        let c = a - b;
        assert_eq!(c.to_seconds(), 7.0);
    }

    #[test]
    fn test_comparison() {
        let a = MediaTime::from_seconds(5.0);
        let b = MediaTime::from_seconds(10.0);
        assert!(a < b);
        assert!(b > a);
        assert!(a == MediaTime::from_seconds(5.0));
    }

    #[test]
    fn test_zero() {
        assert!(MediaTime::ZERO.is_zero());
        assert!(!MediaTime::from_seconds(1.0).is_zero());
    }

    #[test]
    fn test_no_precision_loss() {
        // Perform 1000 operations that would accumulate float error
        let mut t = MediaTime::ZERO;
        let delta = MediaTime::from_nanos(1); // 1 nanosecond

        for _ in 0..1_000_000 {
            t = t + delta;
        }

        // Should be exactly 1 millisecond
        assert_eq!(t.as_nanos(), 1_000_000);
    }
}
