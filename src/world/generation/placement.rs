//! Natural resources, settlement sites and the road network.
//!
//! Geography drives placement: deposits follow terrain and elevation;
//! settlements seek fresh water, fertile flat land and flood-safe ground at a
//! minimum spacing; roads follow the cheapest ground between settlements,
//! avoiding steep slopes, marsh and woodland, and ford or bridge rivers.

use super::terrain::{distance_field, quantile};
use super::{Fields, SettlementSite};
use crate::content::{ContentRegistry, TerrainKind, WorldGenPresetDef};
use crate::core::rng::SimRng;
use crate::world::coords::TilePos;
use crate::world::map::{Crossing, NO_INDEX, RoadKind, SettlementKind, SoilKind};
use crate::world::noise::{Noise2, hash01};
use pathfinding::prelude::astar;

/// Places deposits per the preset's distribution rules (earlier rules win).
pub(super) fn resources(
    f: &mut Fields,
    preset: &WorldGenPresetDef,
    content: &ContentRegistry,
    seed: u64,
) -> Vec<crate::content::ResourceId> {
    let palette: Vec<_> = content.resources().ids().cloned().collect();
    let scale = f.w.max(f.h) as f64;
    for (k, rule) in preset.resources.iter().enumerate() {
        let Some(index) = palette.iter().position(|id| id == &rule.resource) else {
            continue;
        };
        let noise = Noise2::new(seed, 100 + k as u64);
        let freq = f64::from(rule.cluster_frequency);
        let eligible: Vec<usize> = (0..f.len())
            .filter(|&i| {
                f.resource[i] == NO_INDEX
                    && rule.terrains.contains(&f.terrain[i])
                    && f.elevation[i] >= rule.min_elevation_m
                    && f.elevation[i] <= rule.max_elevation_m
            })
            .collect();
        if eligible.is_empty() || rule.abundance <= 0.0 {
            continue;
        }
        let values: Vec<f32> = eligible
            .iter()
            .map(|&i| {
                let (x, y) = f.xy(i);
                noise.fbm(
                    x as f64 / scale * freq,
                    y as f64 / scale * freq,
                    3,
                    0.5,
                    2.0,
                ) as f32
            })
            .collect();
        let threshold = quantile(values.clone(), 1.0 - rule.abundance);
        for (i, value) in eligible.into_iter().zip(values) {
            if value >= threshold {
                let (x, y) = f.xy(i);
                let t = hash01(seed, x as i32, y as i32, 200 + k as u64);
                let [lo, hi] = rule.amount;
                f.resource[i] = index as u16;
                f.resource_amount[i] = lo
                    + ((f64::from(hi - lo) + 1.0) * t)
                        .floor()
                        .min(f64::from(hi - lo)) as u32;
            }
        }
    }
    palette
}

fn mean_in_radius(sat: &[f64], f: &Fields, x: usize, y: usize, r: usize) -> f32 {
    let x0 = x.saturating_sub(r);
    let y0 = y.saturating_sub(r);
    let x1 = (x + r).min(f.w - 1);
    let y1 = (y + r).min(f.h - 1);
    let w = f.w + 1;
    let at = |xx: usize, yy: usize| sat[yy * w + xx];
    let sum = at(x1 + 1, y1 + 1) - at(x0, y1 + 1) - at(x1 + 1, y0) + at(x0, y0);
    (sum / ((x1 - x0 + 1) * (y1 - y0 + 1)) as f64) as f32
}

fn summed_area(f: &Fields, value: impl Fn(usize) -> f64) -> Vec<f64> {
    let w = f.w + 1;
    let mut sat = vec![0f64; w * (f.h + 1)];
    for y in 0..f.h {
        let mut row = 0.0;
        for x in 0..f.w {
            row += value(f.idx(x, y));
            sat[(y + 1) * w + x + 1] = sat[y * w + x + 1] + row;
        }
    }
    sat
}

/// Chooses settlement sites by a geographic suitability score.
pub(super) fn settlements(
    f: &mut Fields,
    preset: &WorldGenPresetDef,
    seed: u64,
) -> Vec<SettlementSite> {
    let s = &preset.settlements;
    if s.count == 0 {
        return Vec::new();
    }
    let radius = s.water_radius_tiles.max(1) as u16;
    let fresh = distance_field(
        f,
        |i| matches!(f.terrain[i], TerrainKind::River | TerrainKind::Lake),
        radius,
    );
    let coast = distance_field(
        f,
        |i| matches!(f.terrain[i], TerrainKind::Ocean | TerrainKind::Shallows),
        radius,
    );
    let fertility = summed_area(f, |i| f64::from(f.fertility[i]));
    let woods = summed_area(f, |i| f64::from(f.tree_density[i]));
    let deposits = summed_area(f, |i| if f.resource[i] != NO_INDEX { 1.0 } else { 0.0 });

    let mut candidates = Vec::new();
    let margin = (f.w.min(f.h) / 12).clamp(4, 24);
    for y in (margin..f.h.saturating_sub(margin)).step_by(2) {
        for x in (margin..f.w.saturating_sub(margin)).step_by(2) {
            let i = f.idx(x, y);
            if !matches!(f.terrain[i], TerrainKind::Lowland | TerrainKind::Hills)
                || f.slope[i] > 6.0
                || f.hand[i] < 1.0
                || fresh[i].min(coast[i]) > radius
            {
                continue;
            }
            let water = 1.0 - f32::from(fresh[i].min(coast[i])) / f32::from(radius);
            let score = 1.2 * mean_in_radius(&fertility, f, x, y, 12)
                + 0.6 * water
                + 0.35 * (1.0 - f.slope[i] / 6.0)
                + if fresh[i] <= 3 { 0.25 } else { 0.0 }
                + if coast[i] <= radius { 0.2 } else { 0.0 }
                + 4.0 * mean_in_radius(&deposits, f, x, y, 10).min(0.05)
                + if f.hand[i] > 3.0 { 0.1 } else { 0.0 }
                - 0.3 * mean_in_radius(&woods, f, x, y, 6);
            candidates.push((score, i));
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut picked: Vec<(TilePos, f32)> = Vec::new();
    for (score, i) in candidates {
        let (x, y) = f.xy(i);
        let pos = TilePos::new(x as i32, y as i32);
        if picked
            .iter()
            .all(|(p, _)| p.chebyshev(pos) >= s.min_spacing_tiles as i32)
        {
            picked.push((pos, score));
            if picked.len() as u32 == s.count {
                break;
            }
        }
    }
    if (picked.len() as u32) < s.count {
        tracing::warn!(
            placed = picked.len(),
            requested = s.count,
            "fewer suitable settlement sites than requested"
        );
    }

    let mut names = s.names.clone();
    let mut rng = SimRng::from_seed(seed).derive_stream(7);
    for i in (1..names.len()).rev() {
        let j = rng.range_u32(0..i as u32 + 1) as usize;
        names.swap(i, j);
    }
    let villages = ((picked.len() as u32).saturating_sub(s.town_count) * 3).div_ceil(5);
    picked
        .into_iter()
        .enumerate()
        .map(|(rank, (centre, score))| {
            let rank = rank as u32;
            let kind = if rank < s.town_count {
                SettlementKind::Town
            } else if rank < s.town_count + villages {
                SettlementKind::Village
            } else {
                SettlementKind::Hamlet
            };
            SettlementSite {
                name: names[rank as usize].clone(),
                kind,
                centre,
                score,
            }
        })
        .collect()
}

/// Clears woodland around settlements (fields, commons and crofts).
pub(super) fn clear_around_settlements(f: &mut Fields, sites: &[SettlementSite]) {
    for site in sites {
        let r = match site.kind {
            SettlementKind::Town => 22,
            SettlementKind::Village => 16,
            SettlementKind::Hamlet => 9,
        };
        for dy in -r..=r {
            for dx in -r..=r {
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                if d > r as f32 {
                    continue;
                }
                if let Some(i) = f.index(site.centre.x + dx, site.centre.y + dy) {
                    let keep = (d / r as f32).powi(2);
                    f.tree_density[i] *= keep;
                    if f.tree_density[i] < 0.08 {
                        f.tree_density[i] = 0.0;
                        f.tree_species[i] = NO_INDEX;
                    }
                }
            }
        }
    }
}

/// Connects settlements with a minimum spanning tree plus extra links, each
/// routed along the cheapest ground.
pub(super) fn roads(f: &mut Fields, preset: &WorldGenPresetDef, sites: &[SettlementSite]) {
    if sites.len() < 2 {
        return;
    }
    let r = &preset.roads;
    let mut edges = Vec::new();
    for a in 0..sites.len() {
        for b in a + 1..sites.len() {
            edges.push((sites[a].centre.euclidean(sites[b].centre), a, b));
        }
    }
    edges.sort_by(|x, y| x.0.total_cmp(&y.0).then((x.1, x.2).cmp(&(y.1, y.2))));

    let mut parent: Vec<usize> = (0..sites.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut chosen = Vec::new();
    let mut spare = Vec::new();
    for &(len, a, b) in &edges {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra == rb {
            spare.push((len, a, b));
        } else {
            parent[ra] = rb;
            chosen.push((len, a, b));
        }
    }
    let mean = chosen.iter().map(|e| e.0).sum::<f32>() / chosen.len().max(1) as f32;
    chosen.extend(
        spare
            .into_iter()
            .filter(|e| e.0 < mean * 2.5)
            .take(r.extra_connections as usize),
    );

    for (_, a, b) in chosen {
        let kind =
            if sites[a].kind != SettlementKind::Hamlet && sites[b].kind != SettlementKind::Hamlet {
                RoadKind::Road
            } else {
                RoadKind::Track
            };
        let start = sites[a].centre;
        let goal = sites[b].centre;
        let cost = |i: usize| -> Option<u32> {
            let base = match f.terrain[i] {
                TerrainKind::Ocean
                | TerrainKind::Shallows
                | TerrainKind::Lake
                | TerrainKind::Cliff => return None,
                TerrainKind::River => r.river_crossing_cost,
                TerrainKind::Marsh => 10.0 * r.marsh_cost,
                TerrainKind::Mountain => 45.0,
                TerrainKind::Hills => 16.0,
                TerrainKind::Beach => 14.0,
                TerrainKind::Lowland => 10.0,
            };
            let c = base + r.slope_cost * f.slope[i] + 10.0 * r.forest_cost * f.tree_density[i];
            let c = if f.road[i] != RoadKind::None {
                c * 0.3
            } else {
                c
            };
            Some(c.max(3.0).round() as u32)
        };
        let result = astar(
            &start,
            |p: &TilePos| {
                let mut next = Vec::with_capacity(8);
                for n in p.neighbours8() {
                    if let Some(j) = f.index(n.x, n.y)
                        && let Some(c) = cost(j)
                    {
                        let c = if p.is_diagonal_to(n) {
                            c * 141 / 100
                        } else {
                            c
                        };
                        next.push((n, c));
                    }
                }
                next
            },
            |p: &TilePos| (p.octile(goal) * 3.0) as u32,
            |p: &TilePos| *p == goal,
        );
        let Some((path, _)) = result else {
            tracing::debug!(from = %start, to = %goal, "no road route");
            continue;
        };
        for p in path {
            if let Some(i) = f.index(p.x, p.y) {
                if f.road[i] < kind {
                    f.road[i] = kind;
                }
                if f.terrain[i] == TerrainKind::River && f.crossing[i] == Crossing::None {
                    f.crossing[i] = if f.flow[i] <= preset.hydrology.ford_max_m3s {
                        Crossing::Ford
                    } else {
                        Crossing::Bridge
                    };
                }
                // Road beds are cleared of trees.
                f.tree_density[i] = 0.0;
                f.tree_species[i] = NO_INDEX;
                if f.soil[i] == SoilKind::Peat {
                    f.fertility[i] *= 0.5;
                }
            }
        }
    }
}
