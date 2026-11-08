//! Deterministic, serialisable random number generation.
//!
//! All randomness that can influence simulation outcomes must come from a
//! [`SimRng`] stored in the persistent world state. The generator is ChaCha8
//! seeded from the configured world seed; its full position is serialised, so a
//! loaded save continues the exact same random sequence.
//!
//! Independent subsystems can take their own stream via
//! [`SimRng::derive_stream`], which keeps their sequences stable even when
//! another subsystem changes how many numbers it draws.

use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Seeded simulation random number generator.
#[derive(Debug, Clone)]
pub struct SimRng {
    inner: ChaCha8Rng,
}

impl SimRng {
    /// Creates a generator from a 64-bit world seed (stream 0).
    pub fn from_seed(seed: u64) -> Self {
        Self {
            inner: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    /// Returns a fresh generator with the same seed but a different stream.
    /// The derived generator starts at the beginning of its stream.
    pub fn derive_stream(&self, stream: u64) -> Self {
        let mut inner = ChaCha8Rng::from_seed(self.inner.get_seed());
        inner.set_stream(stream);
        Self { inner }
    }

    /// Uniform integer in `range`. Returns `range.start` for an empty range.
    pub fn range_u32(&mut self, range: std::ops::Range<u32>) -> u32 {
        if range.is_empty() {
            return range.start;
        }
        self.inner.random_range(range)
    }

    /// Uniform float in `[low, high)`. Returns `low` if the interval is empty.
    pub fn range_f32(&mut self, low: f32, high: f32) -> f32 {
        if low.is_nan() || high.is_nan() || high <= low {
            return low;
        }
        self.inner.random_range(low..high)
    }

    /// Uniform float in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        self.inner.random::<f64>()
    }

    /// Returns `true` with probability `p` (clamped to `[0, 1]`; NaN is 0).
    pub fn chance(&mut self, p: f64) -> bool {
        if p.is_nan() || p <= 0.0 {
            return false;
        }
        self.inner.random_bool(p.min(1.0))
    }

    /// Picks a uniformly random element, or `None` for an empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        let index = self.inner.random_range(0..items.len());
        items.get(index)
    }

    /// Direct access to the underlying engine for `rand` distribution APIs.
    pub fn engine(&mut self) -> &mut ChaCha8Rng {
        &mut self.inner
    }

    fn state(&self) -> RngState {
        let pos = self.inner.get_word_pos();
        RngState {
            seed: self.inner.get_seed(),
            stream: self.inner.get_stream(),
            word_pos_hi: (pos >> 64) as u64,
            word_pos_lo: pos as u64,
        }
    }
}

impl PartialEq for SimRng {
    fn eq(&self, other: &Self) -> bool {
        self.state() == other.state()
    }
}

/// Serialised generator position. The 128-bit word position is split into two
/// 64-bit halves so it round-trips through JSON numbers exactly.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RngState {
    seed: [u8; 32],
    stream: u64,
    word_pos_hi: u64,
    word_pos_lo: u64,
}

impl Serialize for SimRng {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.state().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SimRng {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let state = RngState::deserialize(deserializer)?;
        let mut inner = ChaCha8Rng::from_seed(state.seed);
        inner.set_stream(state.stream);
        inner.set_word_pos((u128::from(state.word_pos_hi) << 64) | u128::from(state.word_pos_lo));
        Ok(Self { inner })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = SimRng::from_seed(42);
        let mut b = SimRng::from_seed(42);
        for _ in 0..100 {
            assert_eq!(a.range_u32(0..1000), b.range_u32(0..1000));
        }
    }

    #[test]
    fn serialisation_resumes_sequence() {
        let mut rng = SimRng::from_seed(7);
        for _ in 0..37 {
            rng.unit();
        }
        let json = serde_json::to_string(&rng).unwrap();
        let mut restored: SimRng = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, rng);
        for _ in 0..50 {
            assert_eq!(restored.range_u32(0..u32::MAX), rng.range_u32(0..u32::MAX));
        }
    }

    #[test]
    fn derived_streams_differ_but_are_reproducible() {
        let base = SimRng::from_seed(1);
        let mut s1 = base.derive_stream(1);
        let mut s2 = base.derive_stream(2);
        let mut s1_again = base.derive_stream(1);
        let a: Vec<u32> = (0..8).map(|_| s1.range_u32(0..u32::MAX)).collect();
        let b: Vec<u32> = (0..8).map(|_| s2.range_u32(0..u32::MAX)).collect();
        let c: Vec<u32> = (0..8).map(|_| s1_again.range_u32(0..u32::MAX)).collect();
        assert_ne!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        let mut rng = SimRng::from_seed(3);
        assert_eq!(rng.range_u32(5..5), 5);
        assert_eq!(rng.range_f32(2.0, 1.0), 2.0);
        assert!(!rng.chance(f64::NAN));
        assert!(rng.chance(2.0));
        assert!(rng.pick::<u8>(&[]).is_none());
    }
}
