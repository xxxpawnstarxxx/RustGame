//! Stable, platform-independent hashing (FNV-1a, 64-bit).
//!
//! Used for content fingerprints, generated-map fingerprints and save-file
//! checksums. `std`'s `DefaultHasher` is not guaranteed stable across Rust
//! releases, so it must never be used for anything persisted.

/// Incremental FNV-1a 64-bit hasher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv64(u64);

impl Default for Fnv64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub const fn new() -> Self {
        Self(Self::OFFSET)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    pub fn write_u8(&mut self, value: u8) {
        self.write(&[value]);
    }

    pub fn write_u32(&mut self, value: u32) {
        self.write(&value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    pub fn write_f32(&mut self, value: f32) {
        self.write_u32(value.to_bits());
    }

    pub fn write_str(&mut self, value: &str) {
        self.write_u64(value.len() as u64);
        self.write(value.as_bytes());
    }

    pub const fn finish(&self) -> u64 {
        self.0
    }
}

/// One-shot FNV-1a 64-bit hash.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = Fnv64::new();
    h.write(bytes);
    h.finish()
}

/// Formats a hash as 16 lowercase hex digits.
pub fn hex64(value: u64) -> String {
    format!("{value:016x}")
}

/// Stateless 64-bit mixing function (SplitMix64 finaliser). Used to derive
/// per-tile pseudo-random values from coordinates and a seed.
pub const fn mix64(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(hex64(1), "0000000000000001");
    }

    #[test]
    fn mixing_spreads_inputs() {
        assert_ne!(mix64(1), mix64(2));
        assert_eq!(mix64(42), mix64(42));
    }
}
