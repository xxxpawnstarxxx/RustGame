use crate::core::config::SimulationConfig;

/// Converts real frame time into fixed simulation ticks.
///
/// Runtime-only state: pausing or changing speed never changes simulation
/// *outcomes*, only how quickly ticks are produced, because every tick has
/// the same fixed game-time length.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeController {
    paused: bool,
    speed_index: usize,
    multipliers: Vec<f64>,
    base_ticks_per_second: f64,
    max_ticks_per_frame: u32,
    max_frame_delta: f64,
    accumulator: f64,
    queued_steps: u64,
    dropped_ticks: u64,
    ticks_last_frame: u32,
}

impl TimeController {
    pub fn new(config: &SimulationConfig) -> Self {
        let multipliers = if config.speed_multipliers.is_empty() {
            vec![1.0]
        } else {
            config.speed_multipliers.clone()
        };
        Self {
            paused: config.start_paused,
            speed_index: config.default_speed_index.min(multipliers.len() - 1),
            multipliers,
            base_ticks_per_second: config.base_ticks_per_real_second,
            max_ticks_per_frame: config.max_ticks_per_frame.max(1),
            max_frame_delta: config.max_frame_delta_seconds,
            accumulator: 0.0,
            queued_steps: 0,
            dropped_ticks: 0,
            ticks_last_frame: 0,
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Pauses or resumes. Resuming discards queued single steps.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if !paused {
            self.queued_steps = 0;
        }
    }

    pub fn toggle_pause(&mut self) {
        self.set_paused(!self.paused);
    }

    pub fn speed_index(&self) -> usize {
        self.speed_index
    }

    pub fn multipliers(&self) -> &[f64] {
        &self.multipliers
    }

    pub fn multiplier(&self) -> f64 {
        self.multipliers[self.speed_index]
    }

    /// Selects a speed (clamped to the available range).
    pub fn set_speed_index(&mut self, index: usize) {
        self.speed_index = index.min(self.multipliers.len() - 1);
    }

    pub fn faster(&mut self) {
        self.set_speed_index(self.speed_index + 1);
    }

    pub fn slower(&mut self) {
        self.set_speed_index(self.speed_index.saturating_sub(1));
    }

    /// Queues ticks to execute while paused (frame-by-frame stepping).
    /// Ignored while running.
    pub fn request_steps(&mut self, ticks: u64) {
        if self.paused {
            self.queued_steps = self.queued_steps.saturating_add(ticks);
        }
    }

    pub fn queued_steps(&self) -> u64 {
        self.queued_steps
    }

    /// Target tick rate at the current speed.
    pub fn target_ticks_per_second(&self) -> f64 {
        self.base_ticks_per_second * self.multiplier()
    }

    /// Total ticks dropped because a frame needed more than the cap.
    pub fn dropped_ticks(&self) -> u64 {
        self.dropped_ticks
    }

    pub fn ticks_last_frame(&self) -> u32 {
        self.ticks_last_frame
    }

    /// Fraction of the next tick already accumulated, in `[0, 1)`; renderers
    /// may use it to interpolate between tick states.
    pub fn interpolation_alpha(&self) -> f64 {
        self.accumulator.fract()
    }

    /// Returns how many ticks to execute for a frame of `real_dt` seconds.
    pub fn plan_frame(&mut self, real_dt: f64) -> u32 {
        let cap = u64::from(self.max_ticks_per_frame);
        let ticks = if self.paused {
            let steps = self.queued_steps.min(cap);
            self.queued_steps -= steps;
            steps
        } else {
            let dt = if real_dt.is_finite() {
                real_dt.clamp(0.0, self.max_frame_delta)
            } else {
                0.0
            };
            self.accumulator += dt * self.target_ticks_per_second();
            let wanted = self.accumulator.floor();
            self.accumulator -= wanted;
            let wanted = wanted as u64;
            if wanted > cap {
                self.dropped_ticks += wanted - cap;
                cap
            } else {
                wanted
            }
        };
        self.ticks_last_frame = ticks as u32;
        self.ticks_last_frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller() -> TimeController {
        TimeController::new(&SimulationConfig {
            base_ticks_per_real_second: 10.0,
            speed_multipliers: vec![1.0, 4.0],
            max_ticks_per_frame: 20,
            max_frame_delta_seconds: 1.0,
            ..SimulationConfig::default()
        })
    }

    #[test]
    fn accumulates_fractional_ticks() {
        let mut c = controller();
        let total: u32 = (0..10).map(|_| c.plan_frame(0.05)).sum();
        assert_eq!(total, 5);
    }

    #[test]
    fn speed_multiplies_tick_rate() {
        let mut c = controller();
        c.set_speed_index(1);
        assert_eq!(c.plan_frame(0.5), 20);
        c.set_speed_index(99);
        assert_eq!(c.speed_index(), 1);
    }

    #[test]
    fn pause_stops_ticks_but_allows_stepping() {
        let mut c = controller();
        c.set_paused(true);
        assert_eq!(c.plan_frame(1.0), 0);
        c.request_steps(3);
        assert_eq!(c.plan_frame(0.0), 3);
        assert_eq!(c.plan_frame(0.0), 0);
        c.request_steps(5);
        c.set_paused(false);
        assert_eq!(c.queued_steps(), 0);
    }

    #[test]
    fn caps_ticks_and_counts_drops() {
        let mut c = controller();
        c.set_speed_index(1);
        assert_eq!(c.plan_frame(1.0), 20);
        assert_eq!(c.dropped_ticks(), 20);
    }

    #[test]
    fn clamps_bad_frame_deltas() {
        let mut c = controller();
        assert_eq!(c.plan_frame(f64::NAN), 0);
        assert_eq!(c.plan_frame(-3.0), 0);
        assert_eq!(c.plan_frame(100.0), 10);
    }
}
