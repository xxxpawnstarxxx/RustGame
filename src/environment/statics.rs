//! Static per-cell aggregates of the map used by the environment systems.
//!
//! Derived deterministically from the map and configuration, so they are
//! rebuilt after loading rather than saved.

use crate::content::TerrainKind;
use crate::core::config::EnvironmentConfig;
use crate::world::TilePos;
use crate::world::map::{NO_INDEX, SoilKind, WorldMap};
use std::collections::BTreeMap;

/// Aggregates of one ground cell.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundCellStatic {
    pub land_tiles: u32,
    pub water_tiles: u32,
    pub dominant_soil: SoilKind,
    pub mean_elevation_m: f32,
    pub min_drainage_height_m: f32,
    pub base_fertility: f32,
    pub mean_moisture: f32,
    pub mean_temp_c: f32,
    /// Largest river discharge in or next to the cell.
    pub nearby_river_flow_m3s: f32,
    pub has_river: bool,
    pub marsh_fraction: f32,
    pub upland_fraction: f32,
    /// Mean initial canopy over the cell's tiles.
    pub initial_tree_density: f32,
    pub weather_cell: u32,
    pub wildlife_region: u32,
}

/// Aggregates of one weather cell.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherCellStatic {
    pub mean_elevation_m: f32,
    /// 0 at the northern edge, 1 at the southern.
    pub latitude_fraction: f32,
    /// Fraction of tiles that are sea, including a one-cell neighbourhood.
    pub coastal_fraction: f32,
    pub centre_u: f32,
    pub centre_v: f32,
    pub ground_cells: Vec<u32>,
}

/// Aggregates of one wildlife region.
#[derive(Debug, Clone, PartialEq)]
pub struct WildlifeRegionStatic {
    pub land_km2: f32,
    pub wetland_fraction: f32,
    pub upland_fraction: f32,
    pub ground_cells: Vec<u32>,
    pub neighbours: Vec<u32>,
}

/// All static environment aggregates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnvStatics {
    pub ground_cols: u32,
    pub ground_rows: u32,
    pub ground_cell_tiles: u32,
    pub weather_cols: u32,
    pub weather_rows: u32,
    pub weather_cell_tiles: u32,
    pub wildlife_cols: u32,
    pub wildlife_rows: u32,
    pub wildlife_region_tiles: u32,
    pub ground: Vec<GroundCellStatic>,
    pub weather: Vec<WeatherCellStatic>,
    pub wildlife: Vec<WildlifeRegionStatic>,
}

impl EnvStatics {
    pub fn build(map: &WorldMap, config: &EnvironmentConfig) -> Self {
        let gc = config.ground_cell_tiles.max(1);
        let wc = config.weather_cell_tiles.max(gc);
        let rc = config.wildlife_region_tiles.max(gc);
        let (w, h) = (map.width(), map.height());
        let ground_cols = w.div_ceil(gc);
        let ground_rows = h.div_ceil(gc);
        let weather_cols = w.div_ceil(wc);
        let weather_rows = h.div_ceil(wc);
        let wildlife_cols = w.div_ceil(rc);
        let wildlife_rows = h.div_ceil(rc);
        let tile_km2 = map.tile_size_m().powi(2) / 1e6;

        let mut ground = Vec::with_capacity((ground_cols * ground_rows) as usize);
        for gy in 0..ground_rows {
            for gx in 0..ground_cols {
                let mut soils: BTreeMap<SoilKind, u32> = BTreeMap::new();
                let (mut land, mut water, mut marsh, mut upland) = (0u32, 0u32, 0u32, 0u32);
                let (mut elev, mut fert, mut moist, mut temp, mut trees) =
                    (0f32, 0f32, 0f32, 0f32, 0f32);
                let mut min_hand = f32::MAX;
                let mut has_river = false;
                let mut count = 0u32;
                for ty in gy * gc..((gy + 1) * gc).min(h) {
                    for tx in gx * gc..((gx + 1) * gc).min(w) {
                        let Some(t) = map.tile(TilePos::new(tx as i32, ty as i32)) else {
                            continue;
                        };
                        count += 1;
                        elev += t.elevation_m;
                        moist += t.moisture;
                        temp += t.mean_temp_c;
                        trees += if t.tree_species != NO_INDEX {
                            t.tree_density
                        } else {
                            0.0
                        };
                        if t.terrain.is_water() {
                            water += 1;
                            has_river |= t.terrain == TerrainKind::River;
                        } else {
                            land += 1;
                            fert += t.fertility;
                            *soils.entry(t.soil).or_default() += 1;
                            min_hand = min_hand.min(t.drainage_height_m);
                            marsh += u32::from(t.terrain == TerrainKind::Marsh);
                            upland += u32::from(matches!(
                                t.terrain,
                                TerrainKind::Hills | TerrainKind::Mountain
                            ));
                        }
                    }
                }
                let n = count.max(1) as f32;
                let dominant_soil = soils
                    .iter()
                    .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
                    .map_or(SoilKind::None, |(s, _)| *s);
                let cx = (gx * gc + gc / 2).min(w - 1);
                let cy = (gy * gc + gc / 2).min(h - 1);
                ground.push(GroundCellStatic {
                    land_tiles: land,
                    water_tiles: water,
                    dominant_soil,
                    mean_elevation_m: elev / n,
                    min_drainage_height_m: if land > 0 { min_hand } else { 0.0 },
                    base_fertility: fert / land.max(1) as f32,
                    mean_moisture: moist / n,
                    mean_temp_c: temp / n,
                    nearby_river_flow_m3s: 0.0,
                    has_river,
                    marsh_fraction: marsh as f32 / n,
                    upland_fraction: upland as f32 / n,
                    initial_tree_density: trees / n,
                    weather_cell: (cy / wc) * weather_cols + cx / wc,
                    wildlife_region: (cy / rc) * wildlife_cols + cx / rc,
                });
            }
        }
        // Largest discharge within a one-cell neighbourhood.
        let river_flow: Vec<f32> = (0..ground_rows)
            .flat_map(|gy| (0..ground_cols).map(move |gx| (gx, gy)))
            .map(|(gx, gy)| {
                let mut best = 0f32;
                for ty in gy * gc..((gy + 1) * gc).min(h) {
                    for tx in gx * gc..((gx + 1) * gc).min(w) {
                        if let Some(t) = map.tile(TilePos::new(tx as i32, ty as i32)) {
                            best = best.max(t.river_flow_m3s);
                        }
                    }
                }
                best
            })
            .collect();
        for gy in 0..ground_rows as i32 {
            for gx in 0..ground_cols as i32 {
                let mut best = 0f32;
                for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (x, y) = (gx + dx, gy + dy);
                    if x >= 0 && y >= 0 && x < ground_cols as i32 && y < ground_rows as i32 {
                        best = best.max(river_flow[(y as u32 * ground_cols + x as u32) as usize]);
                    }
                }
                ground[(gy as u32 * ground_cols + gx as u32) as usize].nearby_river_flow_m3s = best;
            }
        }

        let mut weather = Vec::with_capacity((weather_cols * weather_rows) as usize);
        let sea_fraction = |x0: u32, y0: u32, x1: u32, y1: u32| {
            let (mut sea, mut n) = (0u32, 0u32);
            for ty in y0..y1.min(h) {
                for tx in x0..x1.min(w) {
                    n += 1;
                    if let Some(t) = map.tile(TilePos::new(tx as i32, ty as i32))
                        && matches!(t.terrain, TerrainKind::Ocean | TerrainKind::Shallows)
                    {
                        sea += 1;
                    }
                }
            }
            sea as f32 / n.max(1) as f32
        };
        for wy in 0..weather_rows {
            for wx in 0..weather_cols {
                let mut elev = 0f32;
                let mut n = 0u32;
                for ty in wy * wc..((wy + 1) * wc).min(h) {
                    for tx in wx * wc..((wx + 1) * wc).min(w) {
                        if let Some(t) = map.tile(TilePos::new(tx as i32, ty as i32)) {
                            elev += t.elevation_m.max(0.0);
                            n += 1;
                        }
                    }
                }
                let coastal = sea_fraction(
                    (wx * wc).saturating_sub(wc),
                    (wy * wc).saturating_sub(wc),
                    (wx + 2) * wc,
                    (wy + 2) * wc,
                );
                weather.push(WeatherCellStatic {
                    mean_elevation_m: elev / n.max(1) as f32,
                    latitude_fraction: ((wy * wc + wc / 2) as f32 / h as f32).min(1.0),
                    coastal_fraction: coastal,
                    centre_u: (wx * wc + wc / 2) as f32 / w.max(h) as f32,
                    centre_v: (wy * wc + wc / 2) as f32 / w.max(h) as f32,
                    ground_cells: Vec::new(),
                });
            }
        }
        let mut wildlife: Vec<WildlifeRegionStatic> = (0..wildlife_cols * wildlife_rows)
            .map(|_| WildlifeRegionStatic {
                land_km2: 0.0,
                wetland_fraction: 0.0,
                upland_fraction: 0.0,
                ground_cells: Vec::new(),
                neighbours: Vec::new(),
            })
            .collect();
        for (i, g) in ground.iter().enumerate() {
            weather[g.weather_cell as usize].ground_cells.push(i as u32);
            let r = &mut wildlife[g.wildlife_region as usize];
            r.ground_cells.push(i as u32);
            r.land_km2 += g.land_tiles as f32 * tile_km2;
            r.wetland_fraction += g.marsh_fraction + if g.has_river { 0.2 } else { 0.0 };
            r.upland_fraction += g.upland_fraction;
        }
        for (i, r) in wildlife.iter_mut().enumerate() {
            let n = r.ground_cells.len().max(1) as f32;
            r.wetland_fraction = (r.wetland_fraction / n).min(1.0);
            r.upland_fraction /= n;
            let (x, y) = (
                (i as u32 % wildlife_cols) as i32,
                (i as u32 / wildlife_cols) as i32,
            );
            for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < wildlife_cols as i32 && ny < wildlife_rows as i32 {
                    r.neighbours.push(ny as u32 * wildlife_cols + nx as u32);
                }
            }
        }
        Self {
            ground_cols,
            ground_rows,
            ground_cell_tiles: gc,
            weather_cols,
            weather_rows,
            weather_cell_tiles: wc,
            wildlife_cols,
            wildlife_rows,
            wildlife_region_tiles: rc,
            ground,
            weather,
            wildlife,
        }
    }

    /// Ground cell index of a tile.
    pub fn ground_cell(&self, pos: TilePos) -> Option<usize> {
        if pos.x < 0 || pos.y < 0 || self.ground_cell_tiles == 0 {
            return None;
        }
        let (x, y) = (
            pos.x as u32 / self.ground_cell_tiles,
            pos.y as u32 / self.ground_cell_tiles,
        );
        (x < self.ground_cols && y < self.ground_rows).then(|| (y * self.ground_cols + x) as usize)
    }

    /// Weather cell (= hydrological region) index of a tile.
    pub fn weather_cell(&self, pos: TilePos) -> Option<usize> {
        self.ground_cell(pos)
            .map(|g| self.ground[g].weather_cell as usize)
    }

    /// Wildlife region index of a tile.
    pub fn wildlife_region(&self, pos: TilePos) -> Option<usize> {
        self.ground_cell(pos)
            .map(|g| self.ground[g].wildlife_region as usize)
    }

    /// Ground cell neighbours (4-connected).
    pub fn ground_neighbours(&self, cell: usize) -> impl Iterator<Item = usize> + '_ {
        let (x, y) = (
            (cell as u32 % self.ground_cols) as i32,
            (cell as u32 / self.ground_cols) as i32,
        );
        [(0, -1), (1, 0), (0, 1), (-1, 0)]
            .into_iter()
            .filter_map(move |(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                (nx >= 0 && ny >= 0 && nx < self.ground_cols as i32 && ny < self.ground_rows as i32)
                    .then(|| (ny as u32 * self.ground_cols + nx as u32) as usize)
            })
    }

    /// Tiles covered by a ground cell, as a bounding box `(x0, y0, x1, y1)` (exclusive).
    pub fn ground_cell_bounds(&self, cell: usize) -> (i32, i32, i32, i32) {
        let gc = self.ground_cell_tiles as i32;
        let x = (cell as u32 % self.ground_cols) as i32 * gc;
        let y = (cell as u32 / self.ground_cols) as i32 * gc;
        (x, y, x + gc, y + gc)
    }
}
