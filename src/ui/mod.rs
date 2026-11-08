//! Player-facing egui interface.
//!
//! The interface reads simulation state and changes it only by submitting
//! commands, adjusting pacing, or (world window) saving, loading and
//! generating whole worlds.
//!
//! Map: `WASD`/arrows pan, wheel zooms, middle-drag pans, left-click selects
//! a tile or character, right-click sends the selected character there.
//! Keys: `Space` pause, `1`–`9` speed, `+`/`-` faster/slower, `.` step one
//! tick while paused, `F3` debug tools.

mod inspector;
mod world_window;

use crate::content::{AnimalId, BuildingId, CharacterTemplateId};
use crate::core::ids::EntityId;
use crate::core::time::TimeUnit;
use crate::environment::EnvironmentQuery;
use crate::navigation::Gait;
use crate::persistence::SaveManager;
use crate::rendering::{MapView, Overlay};
use crate::simulation::{FrameReport, GameState};
use crate::world::TilePos;
use egui_macroquad::egui;
use std::path::PathBuf;

pub use world_window::NewWorldForm;

/// Interface state that is not part of the simulation.
pub struct UiState {
    pub show_debug: bool,
    pub show_world_window: bool,
    pub quit_requested: bool,
    /// Set by the interface each frame so map input can defer to it.
    pub pointer_over_ui: bool,
    pub keyboard_captured: bool,
    pub selected_tile: Option<TilePos>,
    pub selected_entity: Option<EntityId>,
    /// Gait used for right-click orders.
    pub order_gait: Gait,
    pub spawn_template: Option<CharacterTemplateId>,
    pub building: Option<BuildingId>,
    pub hunt_animal: Option<AnimalId>,
    pub saves: SaveManager,
    pub save_slot: String,
    pub new_world: NewWorldForm,
    pub confirm_delete: Option<String>,
    pub status: String,
    /// Set when the world was replaced, so the renderer drops its caches.
    pub world_replaced: bool,
}

impl UiState {
    pub fn new(saves_dir: PathBuf, game: &GameState) -> Self {
        Self {
            show_debug: false,
            show_world_window: false,
            quit_requested: false,
            pointer_over_ui: false,
            keyboard_captured: false,
            selected_tile: None,
            selected_entity: None,
            order_gait: Gait::Walk,
            spawn_template: game.content().characters().ids().next().cloned(),
            building: game.content().buildings().ids().next().cloned(),
            hunt_animal: game.world().environment.wildlife.species.first().cloned(),
            saves: SaveManager::new(saves_dir, game.config().simulation.save_backups),
            save_slot: "slot1".into(),
            new_world: NewWorldForm::from_game(game),
            confirm_delete: None,
            status: String::new(),
            world_replaced: false,
        }
    }
}

const SPEED_KEYS: [egui::Key; 9] = [
    egui::Key::Num1,
    egui::Key::Num2,
    egui::Key::Num3,
    egui::Key::Num4,
    egui::Key::Num5,
    egui::Key::Num6,
    egui::Key::Num7,
    egui::Key::Num8,
    egui::Key::Num9,
];

fn handle_shortcuts(ctx: &egui::Context, game: &mut GameState, state: &mut UiState) {
    if ctx.wants_keyboard_input() {
        return;
    }
    let time = game.time_mut();
    ctx.input(|input| {
        if input.key_pressed(egui::Key::Space) {
            time.toggle_pause();
        }
        for (index, key) in SPEED_KEYS.iter().enumerate() {
            if input.key_pressed(*key) {
                time.set_speed_index(index);
            }
        }
        if input.key_pressed(egui::Key::Plus) || input.key_pressed(egui::Key::Equals) {
            time.faster();
        }
        if input.key_pressed(egui::Key::Minus) {
            time.slower();
        }
        if input.key_pressed(egui::Key::Period) {
            time.request_steps(1);
        }
        if input.key_pressed(egui::Key::F3) {
            state.show_debug = !state.show_debug;
        }
    });
}

/// Draws the interface for one frame.
pub fn draw(
    ctx: &egui::Context,
    game: &mut GameState,
    state: &mut UiState,
    frame: &FrameReport,
    view: &mut MapView,
) {
    handle_shortcuts(ctx, game, state);
    top_bar(ctx, game, state, frame, view);
    if state.selected_tile.is_some() || state.selected_entity.is_some() {
        inspector::panel(ctx, game, state);
    }
    if state.show_world_window {
        world_window::window(ctx, game, state);
    }
    state.pointer_over_ui = ctx.is_pointer_over_area() || ctx.wants_pointer_input();
    state.keyboard_captured = ctx.wants_keyboard_input();
}

fn top_bar(
    ctx: &egui::Context,
    game: &mut GameState,
    state: &mut UiState,
    frame: &FrameReport,
    view: &mut MapView,
) {
    egui::TopBottomPanel::top("time_bar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            let now = game.world().clock.now();
            let ambient = game.world().ambient.clone();
            let calendar = game.calendar();
            ui.label(egui::RichText::new(&game.world().meta.name).strong());
            ui.separator();
            ui.label(egui::RichText::new(calendar.format_date(now)).strong());
            ui.label(
                egui::RichText::new(calendar.format_time(now))
                    .monospace()
                    .strong(),
            );
            ui.label(format!("{} · {}", ambient.season, ambient.daylight.phase));
            if let Some(c) =
                EnvironmentQuery::new(game.world()).conditions_at(view.camera.centre_tile())
            {
                let sky = if c.precipitation_mm_h > 0.0 {
                    if c.snowing {
                        "snow"
                    } else if c.precipitation_mm_h > 1.5 {
                        "heavy rain"
                    } else {
                        "rain"
                    }
                } else if c.cloud > 0.7 {
                    "overcast"
                } else if c.cloud > 0.35 {
                    "cloudy"
                } else {
                    "clear"
                };
                ui.label(format!(
                    "{:.0} °C · {sky} · wind {:.0} m/s from {:.0}°",
                    c.temperature_c, c.wind_mps, c.wind_from_deg
                ));
            }
            ui.separator();

            let ticks_per_hour = game
                .world()
                .clock
                .ticks_for(calendar.duration(1, TimeUnit::Hour));
            let time = game.time_mut();
            let label = if time.is_paused() { "Resume" } else { "Pause" };
            if ui.button(label).clicked() {
                time.toggle_pause();
            }
            for index in 0..time.multipliers().len() {
                let text = format!("{}×", time.multipliers()[index]);
                if ui
                    .selectable_label(time.speed_index() == index, text)
                    .clicked()
                {
                    time.set_speed_index(index);
                }
            }
            if time.is_paused() {
                if ui.button("Step tick").clicked() {
                    time.request_steps(1);
                }
                if ui.button("Step hour").clicked() {
                    time.request_steps(ticks_per_hour);
                }
            }
            ui.label(format!("{} ticks/frame", frame.ticks));
            if frame.dropped_ticks > 0 {
                ui.colored_label(egui::Color32::LIGHT_RED, "behind");
            }
            ui.separator();
            egui::ComboBox::from_id_salt("overlay")
                .selected_text(view.overlay.label())
                .show_ui(ui, |ui| {
                    for overlay in Overlay::ALL {
                        ui.selectable_value(&mut view.overlay, overlay, overlay.label());
                    }
                });
            ui.checkbox(&mut view.show_night, "Night");
            ui.separator();
            ui.toggle_value(&mut state.show_world_window, "World & saves");
            ui.checkbox(&mut state.show_debug, "Debug (F3)");
            if ui.button("Quit").clicked() {
                state.quit_requested = true;
            }
        });
        if !state.status.is_empty() {
            ui.label(egui::RichText::new(&state.status).small());
        }
    });
}
