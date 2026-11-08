//! Tile colours computed from world and environment data.

use crate::content::{ContentRegistry, TerrainKind};
use crate::environment::EnvironmentQuery;
use crate::simulation::WorldState;
use crate::world::map::NO_INDEX;
use crate::world::{Crossing, RoadKind, TilePos};
use macroquad::color::Color;

/// What the terrain layer shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Overlay {
    #[default]
    Natural,
    Elevation,
    SoilMoisture,
    Fertility,
    Temperature,
    Snow,
    Flood,
    Contamination,
    Resources,
}

impl Overlay {
    pub const ALL: [Self; 9] = [
        Self::Natural,
        Self::Elevation,
        Self::SoilMoisture,
        Self::Fertility,
        Self::Temperature,
        Self::Snow,
        Self::Flood,
        Self::Contamination,
        Self::Resources,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Natural => "Natural",
            Self::Elevation => "Elevation",
            Self::SoilMoisture => "Soil moisture",
            Self::Fertility => "Fertility",
            Self::Temperature => "Temperature",
            Self::Snow => "Snow",
            Self::Flood => "Flooding",
            Self::Contamination => "Water fouling",
            Self::Resources => "Resources",
        }
    }

    /// Whether colours change with the weather (and need hourly refresh).
    pub fn is_dynamic(self) -> bool {
        !matches!(self, Self::Elevation | Self::Resources)
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba(r, g, b, 255)
}

pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        1.0,
    )
}

fn scale(c: Color, f: f32) -> Color {
    Color::new(
        (c.r * f).min(1.0),
        (c.g * f).min(1.0),
        (c.b * f).min(1.0),
        1.0,
    )
}

/// Blue → green → yellow → red ramp for scalar overlays.
fn ramp(t: f32) -> Color {
    let stops = [
        rgb(40, 70, 160),
        rgb(60, 150, 90),
        rgb(220, 210, 80),
        rgb(190, 60, 40),
    ];
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let i = (t.floor() as usize).min(stops.len() - 2);
    mix(stops[i], stops[i + 1], t - i as f32)
}

/// Resource marker colour by palette index.
pub fn resource_colour(index: u16) -> Color {
    const COLOURS: [(u8, u8, u8); 6] = [
        (150, 70, 50),
        (225, 220, 200),
        (180, 110, 70),
        (70, 50, 35),
        (240, 240, 250),
        (200, 160, 60),
    ];
    let (r, g, b) = COLOURS[index as usize % COLOURS.len()];
    rgb(r, g, b)
}

/// Colour of one tile under an overlay. `month` is 1-based.
pub fn tile_colour(
    world: &WorldState,
    content: &ContentRegistry,
    pos: TilePos,
    overlay: Overlay,
    month: u32,
) -> Color {
    let map = &world.derived.map;
    let Some(tile) = map.tile(pos) else {
        return rgb(0, 0, 0);
    };
    let env = EnvironmentQuery::new(world);
    let conditions = env.conditions_at(pos);
    match overlay {
        Overlay::Elevation => {
            if tile.elevation_m < 0.0 {
                return mix(
                    rgb(20, 40, 110),
                    rgb(60, 110, 190),
                    1.0 + tile.elevation_m / 25.0,
                );
            }
            return ramp(tile.elevation_m / 220.0);
        }
        Overlay::SoilMoisture if !tile.terrain.is_water() => {
            return ramp(1.0 - conditions.map_or(0.0, |c| c.soil_moisture));
        }
        Overlay::Fertility if !tile.terrain.is_water() => {
            let fertility = world
                .derived
                .statics
                .ground_cell(pos)
                .and_then(|g| world.environment.ground.fertility.get(g))
                .copied()
                .unwrap_or(tile.fertility);
            return ramp(1.0 - fertility);
        }
        Overlay::Temperature => {
            return ramp((conditions.map_or(10.0, |c| c.temperature_c) + 5.0) / 30.0);
        }
        _ => {}
    }

    // Natural colours.
    let mut c = match tile.terrain {
        TerrainKind::Ocean => mix(
            rgb(25, 50, 110),
            rgb(45, 85, 150),
            1.0 + tile.elevation_m / 20.0,
        ),
        TerrainKind::Shallows => rgb(70, 120, 165),
        TerrainKind::Lake => rgb(55, 100, 160),
        TerrainKind::River => rgb(70, 125, 185),
        TerrainKind::Beach => rgb(205, 190, 140),
        TerrainKind::Marsh => rgb(95, 115, 70),
        TerrainKind::Lowland => rgb(110, 150, 70),
        TerrainKind::Hills => rgb(115, 135, 70),
        TerrainKind::Mountain => rgb(125, 120, 100),
        TerrainKind::Cliff => rgb(120, 115, 110),
    };
    if !tile.terrain.is_water() {
        if let Some(cond) = &conditions {
            // Grass vigour: lush green when growing, straw-coloured when sparse.
            let grass_tint = mix(rgb(150, 140, 90), c, 0.4 + 0.6 * cond.grass);
            c = if matches!(
                tile.terrain,
                TerrainKind::Lowland | TerrainKind::Hills | TerrainKind::Marsh
            ) {
                grass_tint
            } else {
                c
            };
            let species = world
                .derived
                .statics
                .ground_cell(pos)
                .map(|g| world.environment.ground.tree_species[g])
                .filter(|s| *s != NO_INDEX)
                .or((tile.tree_species != NO_INDEX).then_some(tile.tree_species));
            let canopy = match species
                .and_then(|s| map.tree_palette.get(s as usize))
                .and_then(|id| content.trees().get(id))
            {
                Some(def) if !def.deciduous => rgb(35, 70, 50),
                Some(def) if def.in_leaf(month) => rgb(45, 90, 40),
                Some(_) => rgb(95, 85, 70),
                None => rgb(45, 90, 40),
            };
            c = mix(c, canopy, cond.tree_density * 0.9);
            c = mix(c, rgb(110, 90, 60), cond.mud * 0.35);
        }
        // Hill shading from the north-west.
        let neighbour = |dx: i32, dy: i32| {
            map.tile(pos.offset(dx, dy))
                .map_or(tile.elevation_m, |t| t.elevation_m.max(0.0))
        };
        let gradient = (neighbour(-1, -1) - neighbour(1, 1)) / (2.0 * map.tile_size_m());
        c = scale(c, (1.0 - gradient * 1.5).clamp(0.7, 1.25));
    } else if tile.terrain == TerrainKind::River {
        c = mix(c, rgb(40, 80, 160), (tile.river_flow_m3s * 10.0).min(1.0));
    }
    match tile.crossing {
        Crossing::Bridge => c = rgb(120, 90, 55),
        Crossing::Ford => c = mix(c, rgb(150, 130, 95), 0.5),
        Crossing::None => match tile.road {
            RoadKind::Road => c = mix(c, rgb(170, 145, 105), 0.85),
            RoadKind::Track => c = mix(c, rgb(150, 125, 90), 0.6),
            RoadKind::None => {}
        },
    }
    if let Some(cond) = &conditions {
        if !tile.terrain.is_water() {
            c = mix(c, rgb(240, 242, 248), (cond.snow_cm / 8.0).min(0.95));
        }
        c = mix(c, rgb(80, 120, 170), (cond.flood_depth_m * 2.0).min(0.8));
    }
    match overlay {
        Overlay::Snow => mix(
            scale(c, 0.5),
            rgb(255, 255, 255),
            conditions.map_or(0.0, |x| x.snow_cm / 20.0),
        ),
        Overlay::Flood => mix(
            scale(c, 0.5),
            rgb(40, 120, 255),
            conditions.map_or(0.0, |x| x.flood_depth_m.max(x.surface_water_mm / 200.0)),
        ),
        Overlay::Contamination => mix(
            scale(c, 0.5),
            rgb(160, 120, 30),
            conditions.map_or(0.0, |x| x.water_contamination * 5.0),
        ),
        Overlay::Resources => {
            if tile.resource != NO_INDEX {
                resource_colour(tile.resource)
            } else {
                scale(c, 0.45)
            }
        }
        _ => c,
    }
}
