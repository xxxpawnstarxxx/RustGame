//! The `Mover` component and the locomotion model.
//!
//! Speed = walking speed × gait multiplier × load factor × fatigue factor ×
//! mobility, divided by the tile's traversal cost. Swimming and climbing use
//! their own base speeds. Stamina drains with hard effort and recovers at a
//! walk or at rest; an exhausted character drops to a slower gait. Loads
//! above the soft limit slow movement, prevent running above 100 %, prevent
//! swimming above the swim limit and prevent movement at the hard limit.

use super::traversal::{Abilities, TraversalMode};
use crate::core::config::{GaitConfig, MovementConfig};
use crate::world::TilePos;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;

/// Pace chosen by (or for) a character.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Gait {
    Crouch,
    #[default]
    Walk,
    Run,
    Sprint,
}

impl Gait {
    pub const ALL: [Self; 4] = [Self::Crouch, Self::Walk, Self::Run, Self::Sprint];

    pub fn config(self, config: &MovementConfig) -> &GaitConfig {
        match self {
            Self::Crouch => &config.crouch,
            Self::Walk => &config.walk,
            Self::Run => &config.run,
            Self::Sprint => &config.sprint,
        }
    }

    fn slower(self) -> Self {
        match self {
            Self::Sprint => Self::Run,
            Self::Run | Self::Walk => Self::Walk,
            Self::Crouch => Self::Crouch,
        }
    }
}

impl fmt::Display for Gait {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// What the body is doing right now.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Locomotion {
    #[default]
    Standing,
    Walking,
    Wading,
    Swimming,
    Climbing,
    OpeningDoor,
    Waiting,
    Stumbling,
    Fallen,
}

/// Navigation state.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum NavStatus {
    #[default]
    Idle,
    /// Waiting for the navigation system to plan a route.
    Planning,
    Moving,
    Arrived,
    Failed,
}

/// Physical capabilities and current condition relevant to movement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MovementCapability {
    pub walk_speed_mps: f32,
    pub carry_capacity_kg: f32,
    pub can_swim: bool,
    pub can_climb: bool,
    /// 1 = unimpaired; reduced by injuries to locomotion parts.
    pub mobility: f32,
    /// Short-term reserve, 0 – 1.
    pub stamina: f32,
    /// Long-term tiredness, 0 – 1.
    pub fatigue: f32,
    /// Weight of carried items.
    pub carried_kg: f32,
}

impl MovementCapability {
    /// Load as a fraction of comfortable capacity.
    pub fn encumbrance(&self) -> f32 {
        self.carried_kg / self.carry_capacity_kg.max(1.0)
    }

    pub fn abilities(&self, config: &MovementConfig) -> Abilities {
        Abilities {
            swim: self.can_swim && self.carried_kg <= config.max_swim_load_kg,
            climb: self.can_climb && self.mobility > 0.5 && self.encumbrance() < 1.0,
        }
    }

    /// Fastest gait currently allowed, given the request.
    pub fn allowed_gait(&self, requested: Gait, config: &MovementConfig) -> Gait {
        let mut gait = requested;
        if self.encumbrance() > 1.0 || self.mobility < 0.5 {
            gait = gait.min(Gait::Walk);
        }
        while gait > Gait::Walk && self.stamina < config.exhausted_stamina {
            gait = gait.slower();
        }
        gait
    }

    /// Movement speed in m/s for a gait and traversal mode (before tile cost).
    pub fn speed_mps(&self, gait: Gait, mode: TraversalMode, config: &MovementConfig) -> f32 {
        let enc = self.encumbrance();
        if enc >= config.encumbrance_hard_limit {
            return 0.0;
        }
        let load = if enc <= config.encumbrance_soft_limit {
            1.0
        } else {
            let t = (enc - config.encumbrance_soft_limit)
                / (config.encumbrance_hard_limit - config.encumbrance_soft_limit);
            1.0 - 0.65 * t
        };
        let fatigue = 1.0 - 0.4 * self.fatigue.clamp(0.0, 1.0);
        let mobility = self.mobility.clamp(0.05, 1.0);
        match mode {
            TraversalMode::Walk | TraversalMode::Wade => {
                self.walk_speed_mps
                    * gait.config(config).speed_multiplier
                    * load
                    * fatigue
                    * mobility
            }
            TraversalMode::Swim => {
                let burden = (1.0 - self.carried_kg / config.max_swim_load_kg.max(1.0)).max(0.2);
                config.swim_speed_mps * burden * fatigue * mobility
            }
            TraversalMode::Climb => config.climb_speed_mps * load * fatigue * mobility,
        }
    }

    /// Applies `seconds` of effort (or rest when `mode` is `None`).
    pub fn exert(
        &mut self,
        gait: Gait,
        mode: Option<TraversalMode>,
        seconds: f32,
        config: &MovementConfig,
    ) {
        let (stamina, fatigue) = match mode {
            None => (
                config.rest_stamina_per_second,
                config.rest_fatigue_per_second,
            ),
            Some(TraversalMode::Swim) => (
                config.swim_stamina_per_second,
                config.sprint.fatigue_per_second,
            ),
            Some(TraversalMode::Climb) => (
                config.climb_stamina_per_second,
                config.run.fatigue_per_second,
            ),
            Some(_) => {
                let g = gait.config(config);
                (g.stamina_per_second, g.fatigue_per_second)
            }
        };
        let load = 1.0 + self.encumbrance().max(0.0);
        let stamina = if stamina < 0.0 {
            stamina * load
        } else {
            stamina
        };
        self.stamina = (self.stamina + stamina * seconds).clamp(0.0, 1.0);
        self.fatigue = (self.fatigue + fatigue * load * seconds).clamp(0.0, 1.0);
    }
}

/// The step currently being taken.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub to: TilePos,
    /// 0 – 1.
    pub progress: f32,
    pub mode: TraversalMode,
    pub cost: f32,
    pub diagonal: bool,
}

/// Component: an entity that moves itself across the map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mover {
    pub capability: MovementCapability,
    /// Requested gait.
    pub gait: Gait,
    /// Gait actually used (after stamina and load limits).
    pub effective_gait: Gait,
    pub locomotion: Locomotion,
    pub status: NavStatus,
    pub goal: Option<TilePos>,
    /// Remaining tiles after the current step.
    pub path: VecDeque<TilePos>,
    pub step: Option<Step>,
    /// Seconds of enforced pause (opening a door, recovering from a stumble or fall).
    pub delay_s: f32,
    /// Seconds spent blocked by other characters.
    pub wait_s: f32,
    /// Navigation epoch when the current path was planned.
    pub path_epoch: u64,
    /// Replan before taking the next step.
    pub repath: bool,
    /// Replan once the current (sidestep) step completes.
    pub repath_after_step: bool,
    /// Total distance travelled.
    pub distance_m: f32,
}

impl Mover {
    pub fn new(capability: MovementCapability) -> Self {
        Self {
            capability,
            gait: Gait::Walk,
            effective_gait: Gait::Walk,
            locomotion: Locomotion::Standing,
            status: NavStatus::Idle,
            goal: None,
            path: VecDeque::new(),
            step: None,
            delay_s: 0.0,
            wait_s: 0.0,
            path_epoch: 0,
            repath: false,
            repath_after_step: false,
            distance_m: 0.0,
        }
    }

    /// Requests travel to `goal`; planning happens on the next navigation update.
    pub fn travel_to(&mut self, goal: TilePos) {
        self.goal = Some(goal);
        self.path.clear();
        self.status = NavStatus::Planning;
        self.repath = false;
        self.repath_after_step = false;
        self.wait_s = 0.0;
    }

    /// Stops after the current step.
    pub fn stop(&mut self) {
        self.goal = None;
        self.path.clear();
        self.status = NavStatus::Idle;
        self.repath = false;
        self.repath_after_step = false;
    }

    /// Visual position between tiles for rendering (tile units).
    pub fn interpolated(&self, from: TilePos) -> (f32, f32) {
        match &self.step {
            Some(step) => {
                let t = step.progress.clamp(0.0, 1.0);
                (
                    from.x as f32 + (step.to.x - from.x) as f32 * t,
                    from.y as f32 + (step.to.y - from.y) as f32 * t,
                )
            }
            None => (from.x as f32, from.y as f32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability() -> MovementCapability {
        MovementCapability {
            walk_speed_mps: 1.35,
            carry_capacity_kg: 30.0,
            can_swim: true,
            can_climb: true,
            mobility: 1.0,
            stamina: 1.0,
            fatigue: 0.0,
            carried_kg: 0.0,
        }
    }

    #[test]
    fn gaits_and_loads_change_speed() {
        let config = MovementConfig::default();
        let mut c = capability();
        let walk = c.speed_mps(Gait::Walk, TraversalMode::Walk, &config);
        let sprint = c.speed_mps(Gait::Sprint, TraversalMode::Walk, &config);
        assert!((walk - 1.35).abs() < 1e-6);
        assert!(sprint > 5.0);
        assert!(c.speed_mps(Gait::Crouch, TraversalMode::Walk, &config) < walk);
        c.carried_kg = 30.0;
        assert!(c.speed_mps(Gait::Walk, TraversalMode::Walk, &config) < walk);
        assert!(!c.abilities(&config).swim, "too heavy to swim");
        c.carried_kg = 45.0;
        assert_eq!(c.speed_mps(Gait::Walk, TraversalMode::Walk, &config), 0.0);
    }

    #[test]
    fn exhaustion_forces_slower_gaits() {
        let config = MovementConfig::default();
        let mut c = capability();
        assert_eq!(c.allowed_gait(Gait::Sprint, &config), Gait::Sprint);
        for _ in 0..30 {
            c.exert(Gait::Sprint, Some(TraversalMode::Walk), 1.0, &config);
        }
        assert!(c.stamina < config.exhausted_stamina);
        assert_eq!(c.allowed_gait(Gait::Sprint, &config), Gait::Walk);
        for _ in 0..60 {
            c.exert(Gait::Walk, None, 1.0, &config);
        }
        assert_eq!(c.allowed_gait(Gait::Sprint, &config), Gait::Sprint);
        c.mobility = 0.3;
        assert_eq!(c.allowed_gait(Gait::Run, &config), Gait::Walk);
    }
}
