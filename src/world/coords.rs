//! Tile and chunk coordinates.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The eight compass directions, clockwise from north. `y` grows southwards.
pub const DIRS8: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// The four orthogonal directions.
pub const DIRS4: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// A tile position. `x` grows eastwards, `y` southwards.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub struct TilePos {
    pub x: i32,
    pub y: i32,
}

impl TilePos {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy)
    }

    /// The eight neighbours, clockwise from north.
    pub fn neighbours8(self) -> [Self; 8] {
        DIRS8.map(|(dx, dy)| self.offset(dx, dy))
    }

    pub fn chebyshev(self, other: Self) -> i32 {
        (self.x - other.x).abs().max((self.y - other.y).abs())
    }

    /// Distance in tiles allowing diagonal moves (√2 per diagonal).
    pub fn octile(self, other: Self) -> f32 {
        let dx = (self.x - other.x).abs() as f32;
        let dy = (self.y - other.y).abs() as f32;
        dx.max(dy) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dy)
    }

    pub fn euclidean(self, other: Self) -> f32 {
        let dx = (self.x - other.x) as f32;
        let dy = (self.y - other.y) as f32;
        (dx * dx + dy * dy).sqrt()
    }

    /// Whether `other` is one of the eight neighbours.
    pub fn is_adjacent(self, other: Self) -> bool {
        self != other && self.chebyshev(other) == 1
    }

    pub fn is_diagonal_to(self, other: Self) -> bool {
        self.x != other.x && self.y != other.y
    }
}

impl fmt::Display for TilePos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// A chunk position (chunk grid coordinates, not tiles).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub struct ChunkPos {
    pub x: i32,
    pub y: i32,
}

impl ChunkPos {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        let a = TilePos::new(0, 0);
        let b = TilePos::new(3, 1);
        assert_eq!(a.chebyshev(b), 3);
        assert!((a.octile(b) - (3.0 + std::f32::consts::SQRT_2 - 1.0)).abs() < 1e-6);
        assert!(a.is_adjacent(TilePos::new(1, 1)));
        assert!(!a.is_adjacent(a));
    }
}
