//! Seeded gradient noise for world generation.
//!
//! A self-contained 2D gradient ("Perlin-style") noise with fractal sums.
//! Lattice gradients come from hashing integer coordinates with the seed, so
//! the output depends only on `(seed, layer, x, y)` — never on evaluation
//! order — which keeps generation reproducible.

use crate::core::hash::mix64;

/// Twelve unit gradients evenly spaced around the circle.
const GRADIENTS: [(f64, f64); 12] = [
    (1.0, 0.0),
    (0.866_025, 0.5),
    (0.5, 0.866_025),
    (0.0, 1.0),
    (-0.5, 0.866_025),
    (-0.866_025, 0.5),
    (-1.0, 0.0),
    (-0.866_025, -0.5),
    (-0.5, -0.866_025),
    (0.0, -1.0),
    (0.5, -0.866_025),
    (0.866_025, -0.5),
];

/// One independent noise layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Noise2 {
    seed: u64,
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

impl Noise2 {
    /// A layer derived from a world seed and a layer number.
    pub fn new(seed: u64, layer: u64) -> Self {
        Self {
            seed: mix64(seed ^ mix64(layer.wrapping_add(0x5bd1_e995))),
        }
    }

    fn gradient(&self, ix: i64, iy: i64) -> (f64, f64) {
        let h = mix64(
            self.seed
                ^ (ix as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ (iy as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f),
        );
        GRADIENTS[(h % 12) as usize]
    }

    /// Single-octave noise, approximately in `[-1, 1]`.
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        let x0 = x.floor();
        let y0 = y.floor();
        let (ix, iy) = (x0 as i64, y0 as i64);
        let (fx, fy) = (x - x0, y - y0);
        let dot = |gx: i64, gy: i64, dx: f64, dy: f64| {
            let (a, b) = self.gradient(gx, gy);
            a * dx + b * dy
        };
        let n00 = dot(ix, iy, fx, fy);
        let n10 = dot(ix + 1, iy, fx - 1.0, fy);
        let n01 = dot(ix, iy + 1, fx, fy - 1.0);
        let n11 = dot(ix + 1, iy + 1, fx - 1.0, fy - 1.0);
        let (u, v) = (fade(fx), fade(fy));
        (lerp(lerp(n00, n10, u), lerp(n01, n11, u), v) * 1.414).clamp(-1.0, 1.0)
    }

    /// Fractal Brownian motion, normalised to approximately `[-1, 1]`.
    pub fn fbm(&self, x: f64, y: f64, octaves: u32, persistence: f64, lacunarity: f64) -> f64 {
        let mut sum = 0.0;
        let mut amplitude = 1.0;
        let mut frequency = 1.0;
        let mut norm = 0.0;
        for octave in 0..octaves.max(1) {
            // Offset each octave so lattice points do not line up.
            let shift = f64::from(octave) * 17.31;
            sum += amplitude * self.sample(x * frequency + shift, y * frequency - shift);
            norm += amplitude;
            amplitude *= persistence;
            frequency *= lacunarity;
        }
        sum / norm
    }

    /// Ridged multifractal noise in `[0, 1]`: sharp crests along zero lines.
    pub fn ridged(&self, x: f64, y: f64, octaves: u32) -> f64 {
        let mut sum = 0.0;
        let mut amplitude = 0.5;
        let mut frequency = 1.0;
        let mut norm = 0.0;
        let mut weight = 1.0;
        for octave in 0..octaves.max(1) {
            let shift = f64::from(octave) * 23.17;
            let n = 1.0
                - self
                    .sample(x * frequency + shift, y * frequency + shift)
                    .abs();
            let n = n * n * weight;
            weight = (n * 2.0).clamp(0.0, 1.0);
            sum += n * amplitude;
            norm += amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        (sum / norm).clamp(0.0, 1.0)
    }
}

/// Deterministic value in `[0, 1)` for a tile and salt.
pub fn hash01(seed: u64, x: i32, y: i32, salt: u64) -> f64 {
    let h = mix64(seed ^ mix64((x as u32 as u64) << 32 | (y as u32 as u64)) ^ mix64(salt));
    (h >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_bounded() {
        let a = Noise2::new(7, 1);
        let b = Noise2::new(7, 1);
        let c = Noise2::new(7, 2);
        let mut differs = false;
        for i in 0..200 {
            let (x, y) = (f64::from(i) * 0.37, f64::from(i) * 0.11);
            assert_eq!(a.fbm(x, y, 5, 0.5, 2.0), b.fbm(x, y, 5, 0.5, 2.0));
            assert!((-1.0..=1.0).contains(&a.fbm(x, y, 5, 0.5, 2.0)));
            assert!((0.0..=1.0).contains(&a.ridged(x, y, 4)));
            differs |= a.sample(x, y) != c.sample(x, y);
        }
        assert!(differs);
        let h = hash01(1, 2, 3, 4);
        assert!((0.0..1.0).contains(&h));
        assert_eq!(h, hash01(1, 2, 3, 4));
    }
}
