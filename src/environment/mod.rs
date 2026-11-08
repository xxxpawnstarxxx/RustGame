//! The natural environment: weather, ground water, vegetation and wildlife.
//!
//! The environment is simulated on three coarse grids laid over the tile map
//! (sizes in `config/environment.json`):
//!
//! | Grid | Default cell | Holds |
//! |------|--------------|-------|
//! | weather | 32 × 32 tiles | temperature, humidity, cloud, precipitation, wind; drought and flood status per region |
//! | ground | 4 × 4 tiles | soil moisture, surface water, snow, flooding, mud, fertility, contamination, grass, tree canopy |
//! | wildlife | 64 × 64 tiles | population, disease and hunting pressure per wild species |
//!
//! Four systems advance it — [`WeatherSystem`] and [`GroundSystem`] hourly,
//! [`VegetationSystem`] and [`WildlifeSystem`] daily — each integrating over
//! the elapsed game time it is given. Other systems read the results through
//! [`EnvironmentQuery`] (movement already uses it; agriculture, needs,
//! buildings and the economy will).

pub mod ground;
pub mod query;
pub mod statics;
pub mod systems;
pub mod vegetation;
pub mod weather;
pub mod wildlife;

pub use ground::GroundGrid;
pub use query::{
    BuildingStress, EnvironmentQuery, GrowingConditions, LocalConditions, RegionResources,
};
pub use statics::EnvStatics;
pub use systems::{GroundSystem, VegetationSystem, WeatherSystem, WildlifeSystem};
pub use weather::{RegionHydrology, Synoptic, WeatherCell, WeatherState};
pub use wildlife::{Population, WildlifeRegion, WildlifeState};

use crate::content::{ContentRegistry, WorldGenPresetDef};
use crate::core::config::GameConfig;
use crate::core::rng::SimRng;
use crate::core::time::{Calendar, GameInstant};
use crate::world::WorldMap;
use serde::{Deserialize, Serialize};

/// Persistent environment state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentState {
    /// False for worlds migrated from saves that predate the environment.
    pub initialized: bool,
    pub weather: WeatherState,
    pub ground: GroundGrid,
    pub wildlife: WildlifeState,
    /// Dedicated random stream so environment randomness is independent of
    /// other systems' draws.
    pub rng: SimRng,
}

impl Default for EnvironmentState {
    fn default() -> Self {
        Self {
            initialized: false,
            weather: WeatherState::default(),
            ground: GroundGrid::default(),
            wildlife: WildlifeState::default(),
            rng: SimRng::from_seed(0),
        }
    }
}

/// Stream number of the environment's random generator.
pub const ENVIRONMENT_RNG_STREAM: u64 = 101;

impl EnvironmentState {
    /// Initial conditions for a freshly generated map.
    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        map: &WorldMap,
        statics: &EnvStatics,
        preset: &WorldGenPresetDef,
        content: &ContentRegistry,
        config: &GameConfig,
        calendar: &Calendar,
        seed: u64,
        now: GameInstant,
    ) -> Self {
        let rng = SimRng::from_seed(seed).derive_stream(ENVIRONMENT_RNG_STREAM);
        let ground = GroundGrid::initialize(map, statics, &config.environment);
        let mut state = Self {
            initialized: true,
            weather: WeatherState::initialize(statics),
            ground,
            wildlife: WildlifeState::initialize(statics, preset, content),
            rng,
        };
        weather::step(
            &mut state,
            statics,
            preset,
            &config.environment,
            calendar,
            now,
            0.0,
            map.tile_size_m(),
        );
        wildlife::seed_populations(&mut state, statics, preset, content, &config.environment);
        state
    }
}
