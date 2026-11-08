//! Relief, hydrology, climate normals, terrain classes, soils and woodland.

// Generation walks several parallel per-tile arrays by index at once.
#![allow(clippy::needless_range_loop)]

use super::Fields;
use crate::content::{CoastSide, ContentRegistry, TerrainKind, WorldGenPresetDef};
use crate::world::map::{Landform, NO_FLOW, NO_INDEX, SoilKind};
use crate::world::noise::Noise2;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

const SECONDS_PER_YEAR: f32 = 31_557_600.0;

fn smoothstep(lo: f64, hi: f64, x: f64) -> f64 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Value at quantile `q` (0 – 1) of `values`.
pub(super) fn quantile(mut values: Vec<f32>, q: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    let index = ((values.len() - 1) as f32 * q.clamp(0.0, 1.0)).round() as usize;
    values[index]
}

/// Multi-source breadth-first Chebyshev distance (in tiles), capped at `limit`.
pub(super) fn distance_field(
    f: &Fields,
    is_source: impl Fn(usize) -> bool,
    limit: u16,
) -> Vec<u16> {
    let mut dist = vec![u16::MAX; f.len()];
    let mut queue = VecDeque::new();
    for i in 0..f.len() {
        if is_source(i) {
            dist[i] = 0;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        let d = dist[i];
        if d >= limit {
            continue;
        }
        for (j, _) in f.neighbours(i) {
            if dist[j] == u16::MAX {
                dist[j] = d + 1;
                queue.push_back(j);
            }
        }
    }
    dist
}

/// Fractal relief, coast gradient and conversion to metres.
pub(super) fn elevation(f: &mut Fields, preset: &WorldGenPresetDef, seed: u64) {
    let s = &preset.elevation;
    let base = Noise2::new(seed, 1);
    let warp_x = Noise2::new(seed, 2);
    let warp_y = Noise2::new(seed, 3);
    let ridge = Noise2::new(seed, 4);
    let mask = Noise2::new(seed, 5);
    let scale = f.w.max(f.h) as f64;
    let freq = f64::from(s.base_frequency);
    let ridge_freq = f64::from(s.ridge_frequency);
    let warp = f64::from(s.warp_strength);

    let mut raw = vec![0f32; f.len()];
    for y in 0..f.h {
        for x in 0..f.w {
            let u = x as f64 / scale;
            let v = y as f64 / scale;
            let wx = u + warp * warp_x.fbm(u * 2.0, v * 2.0, 3, 0.5, 2.0);
            let wy = v + warp * warp_y.fbm(u * 2.0 + 5.2, v * 2.0 + 1.3, 3, 0.5, 2.0);
            let b = base.fbm(
                wx * freq,
                wy * freq,
                s.octaves,
                f64::from(s.persistence),
                f64::from(s.lacunarity),
            );
            let r = ridge.ridged(wx * ridge_freq, wy * ridge_freq, 4);
            let m = smoothstep(
                0.4,
                0.75,
                (mask.fbm(u * 1.5, v * 1.5, 2, 0.5, 2.0) + 1.0) / 2.0,
            );
            let (fu, fv) = (x as f64 / f.w as f64, y as f64 / f.h as f64);
            let t = match s.coast {
                CoastSide::None => 0.5,
                CoastSide::North => fv,
                CoastSide::South => 1.0 - fv,
                CoastSide::West => fu,
                CoastSide::East => 1.0 - fu,
                CoastSide::Island => {
                    let d = ((fu - 0.5).powi(2) + (fv - 0.5).powi(2)).sqrt() / 0.5;
                    1.0 - d.min(1.0)
                }
            };
            let e = (b + 1.0) / 2.0
                + f64::from(s.ridge_strength) * m * r
                + f64::from(s.coast_strength) * (t - 0.5);
            raw[f.idx(x, y)] = e as f32;
        }
    }
    let (lo, hi) = raw
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let span = (hi - lo).max(1e-6);
    for v in &mut raw {
        *v = (*v - lo) / span;
    }
    let sea = if s.sea_coverage > 0.0 {
        quantile(raw.clone(), s.sea_coverage)
    } else {
        -1.0
    };
    for (i, e) in raw.iter().enumerate() {
        f.elevation[i] = if *e < sea {
            -s.sea_depth_m * ((sea - e) / sea.max(1e-6)).clamp(0.02, 1.0)
        } else {
            let t = ((e - sea) / (1.0 - sea).max(1e-6)).clamp(0.0, 1.0);
            0.5 + t.powf(s.height_exponent) * s.max_m
        };
    }
}

/// Depression filling, lakes, flow routing, catchments, rivers and
/// height-above-drainage.
pub(super) fn hydrology(f: &mut Fields, preset: &WorldGenPresetDef, tile_area_km2: f32) {
    let hy = &preset.hydrology;
    let n = f.len();

    // Priority-flood (Barnes et al.) with a small gradient so every filled
    // cell drains. Seeds: the sea, plus map edges (water leaves the map).
    let mut filled = f.elevation.clone();
    let mut closed = vec![false; n];
    let mut heap = BinaryHeap::new();
    let key = |v: f32| Reverse(OrdF32(v));
    for i in 0..n {
        let (x, y) = f.xy(i);
        if f.elevation[i] < 0.0 || x == 0 || y == 0 || x == f.w - 1 || y == f.h - 1 {
            closed[i] = true;
            heap.push((key(filled[i]), Reverse(i)));
        }
    }
    while let Some((Reverse(OrdF32(level)), Reverse(i))) = heap.pop() {
        for (j, _) in f.neighbours(i) {
            if closed[j] {
                continue;
            }
            closed[j] = true;
            if filled[j] <= level {
                filled[j] = level + 1e-3;
            }
            heap.push((key(filled[j]), Reverse(j)));
        }
    }

    // Lakes: connected depressions whose deepest point exceeds the minimum depth.
    let depth: Vec<f32> = (0..n).map(|i| filled[i] - f.elevation[i]).collect();
    let mut label = vec![u32::MAX; n];
    let mut next_label = 0u32;
    for start in 0..n {
        if label[start] != u32::MAX || depth[start] <= 0.05 || f.elevation[start] < 0.0 {
            continue;
        }
        let mut members = vec![start];
        label[start] = next_label;
        let mut cursor = 0;
        let mut deepest = 0f32;
        while cursor < members.len() {
            let i = members[cursor];
            cursor += 1;
            deepest = deepest.max(depth[i]);
            for (j, _) in f.neighbours(i) {
                if label[j] == u32::MAX && depth[j] > 0.05 && f.elevation[j] >= 0.0 {
                    label[j] = next_label;
                    members.push(j);
                }
            }
        }
        if deepest >= hy.lake_min_depth_m {
            for i in members {
                if depth[i] > 0.5 {
                    f.terrain[i] = TerrainKind::Lake;
                }
            }
        }
        next_label += 1;
    }

    // D8 steepest descent on the filled surface.
    for i in 0..n {
        if f.elevation[i] < 0.0 {
            f.flow_dir[i] = NO_FLOW;
            continue;
        }
        let mut best = NO_FLOW;
        let mut best_drop = 0f32;
        for (d, (j, dist)) in f.neighbours_dir(i) {
            let drop = (filled[i] - filled[j]) / dist;
            if drop > best_drop {
                best_drop = drop;
                best = d;
            }
        }
        f.flow_dir[i] = best;
    }

    // Accumulate catchment from high to low.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| filled[*b].total_cmp(&filled[*a]).then(a.cmp(b)));
    let mut acc: Vec<f32> = (0..n).map(|i| tile_area_km2 * f.precip[i]).collect();
    for &i in &order {
        if let Some(j) = f.downstream(i) {
            acc[j] += acc[i];
        }
    }
    let runoff = hy.annual_rainfall_mm / 1000.0 * hy.runoff_coefficient * 1e6 / SECONDS_PER_YEAR;
    for i in 0..n {
        f.catchment[i] = acc[i];
        if f.elevation[i] >= 0.0
            && f.terrain[i] != TerrainKind::Lake
            && acc[i] >= hy.river_threshold_km2
        {
            f.terrain[i] = TerrainKind::River;
            f.flow[i] = acc[i] * runoff;
        }
    }

    // Widen large rivers onto adjacent low ground.
    let channel: Vec<usize> = (0..n)
        .filter(|&i| f.terrain[i] == TerrainKind::River && f.flow[i] > hy.wide_river_m3s)
        .collect();
    for i in channel {
        let extra = ((f.flow[i] / hy.wide_river_m3s).log2().floor() as i32 + 1).clamp(1, 3);
        let (x, y) = f.xy(i);
        for dy in -extra..=extra {
            for dx in -extra..=extra {
                if dx.abs() + dy.abs() > extra {
                    continue;
                }
                let Some(j) = f.index(x as i32 + dx, y as i32 + dy) else {
                    continue;
                };
                if f.elevation[j] >= 0.0 && !f.terrain[j].is_water() && filled[j] <= filled[i] + 1.0
                {
                    f.terrain[j] = TerrainKind::River;
                    f.flow[j] = f.flow[j].max(f.flow[i]);
                }
            }
        }
    }

    // Height above nearest drainage, following flow paths downhill.
    let mut drain_level = vec![0f32; n];
    for &i in order.iter().rev() {
        drain_level[i] = if f.terrain[i].is_water() || f.elevation[i] < 0.0 {
            filled[i]
        } else {
            match f.downstream(i) {
                Some(j) => drain_level[j],
                None => filled[i],
            }
        };
        f.hand[i] = (filled[i] - drain_level[i]).max(0.0);
    }
    f.filled = filled;
}

/// Slope in degrees from the steepest neighbour difference.
pub(super) fn slopes(f: &mut Fields, tile_size_m: f32) {
    for i in 0..f.len() {
        let e = f.elevation[i].max(0.0);
        let mut steepest = 0f32;
        for (j, dist) in f.neighbours(i) {
            let d = (e - f.elevation[j].max(0.0)).abs() / (dist * tile_size_m);
            steepest = steepest.max(d);
        }
        f.slope[i] = steepest.atan().to_degrees();
    }
}

/// Relative precipitation from orography and noise (before routing).
pub(super) fn precipitation(f: &mut Fields, preset: &WorldGenPresetDef, seed: u64) {
    let c = &preset.climate;
    let noise = Noise2::new(seed, 11);
    // Wind blows *towards* the opposite of where it comes from.
    let from = c.prevailing_wind_from_deg.to_radians();
    let (ux, uy) = (from.sin(), -from.cos()); // unit vector pointing upwind
    let scale = f.w.max(f.h) as f64;
    for y in 0..f.h {
        for x in 0..f.w {
            let i = f.idx(x, y);
            let upwind = f
                .index(
                    x as i32 + (ux * 8.0).round() as i32,
                    y as i32 + (uy * 8.0).round() as i32,
                )
                .map_or(f.elevation[i], |j| f.elevation[j]);
            let rise = f.elevation[i].max(0.0) - upwind.max(0.0);
            let oro = if rise > 0.0 {
                c.orographic_factor * rise / 100.0
            } else {
                c.orographic_factor * rise / 200.0
            };
            let n = noise.fbm(x as f64 / scale * 3.0, y as f64 / scale * 3.0, 3, 0.5, 2.0) as f32;
            f.precip[i] = (1.0 + oro + 0.15 * n).clamp(0.4, 2.5);
        }
    }
}

/// Mean annual temperature and the wetness index.
pub(super) fn climate(f: &mut Fields, preset: &WorldGenPresetDef, seed: u64) {
    let c = &preset.climate;
    let base = c.months.iter().map(|m| m.mean_temp_c).sum::<f32>() / c.months.len().max(1) as f32;
    let noise = Noise2::new(seed, 12);
    let scale = f.w.max(f.h) as f64;
    let (pmin, pmax) = f
        .precip
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let pspan = (pmax - pmin).max(1e-6);
    for y in 0..f.h {
        for x in 0..f.w {
            let i = f.idx(x, y);
            let v = y as f32 / f.h as f32;
            f.temp[i] = base + c.north_south_gradient_c * (v - 0.5)
                - c.lapse_rate_c_per_km * f.elevation[i].max(0.0) / 1000.0;
            let p = (f.precip[i] - pmin) / pspan;
            let wet_low = 1.0 - smoothstep(0.0, 12.0, f64::from(f.hand[i])) as f32;
            let n = (noise.fbm(x as f64 / scale * 6.0, y as f64 / scale * 6.0, 2, 0.5, 2.0) as f32
                + 1.0)
                / 2.0;
            f.moisture[i] = if f.terrain[i].is_water() || f.elevation[i] < 0.0 {
                1.0
            } else {
                (0.4 * p + 0.4 * wet_low + 0.2 * n).clamp(0.0, 1.0)
            };
        }
    }
}

/// Terrain classes and landforms.
pub(super) fn classify(f: &mut Fields, preset: &WorldGenPresetDef) {
    let s = &preset.elevation;
    let ocean = distance_field(f, |i| f.elevation[i] < 0.0, 3);
    for i in 0..f.len() {
        let e = f.elevation[i];
        if e < 0.0 {
            f.terrain[i] = if e < -2.0 {
                TerrainKind::Ocean
            } else {
                TerrainKind::Shallows
            };
            continue;
        }
        if f.terrain[i].is_water() {
            continue;
        }
        f.terrain[i] = if f.slope[i] > s.cliff_slope_deg {
            TerrainKind::Cliff
        } else if e > s.mountain_m {
            TerrainKind::Mountain
        } else if e > s.hill_m || f.slope[i] > 12.0 {
            TerrainKind::Hills
        } else if f.hand[i] < 1.2 && f.moisture[i] > 0.62 && f.slope[i] < 2.5 {
            TerrainKind::Marsh
        } else if ocean[i] <= 2 && e < 3.0 && f.slope[i] < 4.0 {
            TerrainKind::Beach
        } else {
            TerrainKind::Lowland
        };
    }

    // Local relief: highest elevation within a square window.
    let radius = 8usize;
    let mut row_max = vec![0f32; f.len()];
    for y in 0..f.h {
        for x in 0..f.w {
            let lo = x.saturating_sub(radius);
            let hi = (x + radius).min(f.w - 1);
            row_max[f.idx(x, y)] = (lo..=hi)
                .map(|xx| f.elevation[f.idx(xx, y)])
                .fold(f32::MIN, f32::max);
        }
    }
    for y in 0..f.h {
        for x in 0..f.w {
            let i = f.idx(x, y);
            let lo = y.saturating_sub(radius);
            let hi = (y + radius).min(f.h - 1);
            let window_max = (lo..=hi)
                .map(|yy| row_max[f.idx(x, yy)])
                .fold(f32::MIN, f32::max);
            let relief = window_max - f.elevation[i];
            f.landform[i] = match f.terrain[i] {
                TerrainKind::Mountain | TerrainKind::Cliff => Landform::Mountain,
                TerrainKind::Hills => Landform::Hill,
                _ if f.elevation[i] >= 0.0 && f.hand[i] < 8.0 && relief > 30.0 => Landform::Valley,
                _ => Landform::Plain,
            };
        }
    }
}

/// Soil types and natural fertility.
pub(super) fn soils(f: &mut Fields, seed: u64) {
    let noise = Noise2::new(seed, 21);
    let fert_noise = Noise2::new(seed, 22);
    let river = distance_field(
        f,
        |i| matches!(f.terrain[i], TerrainKind::River | TerrainKind::Lake),
        8,
    );
    let scale = f.w.max(f.h) as f64;
    for y in 0..f.h {
        for x in 0..f.w {
            let i = f.idx(x, y);
            let n = noise.fbm(x as f64 / scale * 5.0, y as f64 / scale * 5.0, 3, 0.5, 2.0);
            let soil = match f.terrain[i] {
                t if t.is_water() => SoilKind::None,
                TerrainKind::Beach => SoilKind::Sand,
                TerrainKind::Marsh => SoilKind::Peat,
                TerrainKind::Cliff | TerrainKind::Mountain => SoilKind::Stony,
                TerrainKind::Hills if f.slope[i] > 15.0 || n < -0.3 => SoilKind::Stony,
                TerrainKind::Hills if f.moisture[i] > 0.78 => SoilKind::Peat,
                _ if f.hand[i] < 2.5 && river[i] <= 6 => SoilKind::Alluvial,
                _ if n < -0.35 => SoilKind::Sand,
                _ if n > 0.35 => SoilKind::Clay,
                _ => SoilKind::Loam,
            };
            f.soil[i] = soil;
            let variation =
                fert_noise.fbm(x as f64 / scale * 9.0, y as f64 / scale * 9.0, 2, 0.5, 2.0) as f32
                    * 0.08;
            let slope_factor = (1.0 - f.slope[i] / 40.0).clamp(0.0, 1.0);
            f.fertility[i] = (soil.base_fertility() * slope_factor + variation).clamp(0.0, 1.0)
                * if soil == SoilKind::None { 0.0 } else { 1.0 };
        }
    }
}

/// Initial woodland: species by suitability, extent by target coverage.
pub(super) fn woodland(
    f: &mut Fields,
    preset: &WorldGenPresetDef,
    content: &ContentRegistry,
    seed: u64,
) {
    let species: Vec<_> = content.trees().iter().collect();
    if species.is_empty() {
        return;
    }
    let noise = Noise2::new(seed, 31);
    let freq = f64::from(preset.vegetation.forest_frequency);
    let scale = f.w.max(f.h) as f64;
    let mut potential = vec![f32::MIN; f.len()];
    let mut best_species = vec![NO_INDEX; f.len()];
    let mut eligible = Vec::new();
    for y in 0..f.h {
        for x in 0..f.w {
            let i = f.idx(x, y);
            let soil_factor = match f.soil[i] {
                SoilKind::None => continue,
                SoilKind::Stony => 0.6,
                SoilKind::Peat => 0.75,
                SoilKind::Sand => 0.8,
                _ => 1.0,
            };
            if matches!(f.terrain[i], TerrainKind::Cliff | TerrainKind::Beach) {
                continue;
            }
            let mut best = (0f32, NO_INDEX);
            for (k, sp) in species.iter().enumerate() {
                let patch = noise.sample(
                    x as f64 / scale * 12.0 + k as f64 * 31.0,
                    y as f64 / scale * 12.0,
                ) as f32;
                let suit =
                    sp.suitability(f.moisture[i], f.temp[i], f.elevation[i]) * (1.0 + 0.3 * patch);
                if suit > best.0 {
                    best = (suit, k as u16);
                }
            }
            if best.1 == NO_INDEX {
                continue;
            }
            let n = (noise.fbm(
                x as f64 / scale * freq,
                y as f64 / scale * freq,
                4,
                0.5,
                2.0,
            ) as f32
                + 1.0)
                / 2.0;
            potential[i] = 0.55 * best.0 * soil_factor + 0.45 * n;
            best_species[i] = best.1;
            eligible.push(potential[i]);
        }
    }
    let coverage = preset.vegetation.forest_coverage;
    if coverage <= 0.0 || eligible.is_empty() {
        return;
    }
    let threshold = quantile(eligible, 1.0 - coverage);
    for i in 0..f.len() {
        if best_species[i] != NO_INDEX && potential[i] >= threshold {
            f.tree_species[i] = best_species[i];
            f.tree_density[i] = (0.45
                + 0.55 * (potential[i] - threshold) / (1.0 - threshold).max(0.05))
            .clamp(0.3, 1.0);
        }
    }
}

/// Total-order wrapper for heap keys.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OrdF32(f32);

impl Eq for OrdF32 {}

impl PartialOrd for OrdF32 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrdF32 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}
