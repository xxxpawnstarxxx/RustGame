//! World creation and save-slot management.

use super::UiState;
use crate::content::WorldPresetId;
use crate::persistence::{SaveManager, SlotStatus};
use crate::simulation::{GameState, NewWorldOptions};
use egui_macroquad::egui;

/// Inputs of the "new world" form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorldForm {
    pub name: String,
    pub seed: String,
    pub preset: WorldPresetId,
}

impl NewWorldForm {
    pub fn from_game(game: &GameState) -> Self {
        let options = NewWorldOptions::from_config(game.config());
        Self {
            name: options.name,
            seed: options.seed.to_string(),
            preset: options.preset,
        }
    }
}

fn format_unix(seconds: u64) -> String {
    // Civil date from Unix days (Howard Hinnant's algorithm), UTC.
    let days = (seconds / 86_400) as i64;
    let secs = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        secs / 3600,
        secs % 3600 / 60
    )
}

pub(super) fn window(ctx: &egui::Context, game: &mut GameState, state: &mut UiState) {
    let mut open = state.show_world_window;
    egui::Window::new("World & saves")
        .open(&mut open)
        .default_width(460.0)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("Save").strong());
            ui.horizontal(|ui| {
                ui.label("Slot");
                ui.text_edit_singleline(&mut state.save_slot);
                if ui.button("Save").clicked() {
                    state.status = match state.saves.save(
                        &state.save_slot,
                        game.world(),
                        game.content(),
                        game.calendar(),
                    ) {
                        Ok(meta) => {
                            format!("Saved '{}' ({} KB)", meta.slot, meta.world_bytes / 1024)
                        }
                        Err(err) => format!("Save failed: {err}"),
                    };
                }
            });
            ui.separator();
            ui.label(egui::RichText::new("Saved worlds").strong());
            let slots = state.saves.list();
            if slots.is_empty() {
                ui.label("No saves yet.");
            }
            egui::Grid::new("slots")
                .striped(true)
                .num_columns(3)
                .show(ui, |ui| {
                    for info in slots {
                        match (&info.status, &info.metadata) {
                            (SlotStatus::Ok, Some(meta)) => {
                                ui.label(egui::RichText::new(&info.slot).strong());
                                ui.label(format!(
                                    "{} · {}\nsaved {} · seed {} · {} people · {} backups",
                                    meta.world_name,
                                    meta.game_date,
                                    format_unix(meta.saved_unix),
                                    meta.seed,
                                    meta.counts.characters,
                                    info.backups
                                ));
                            }
                            (status, _) => {
                                ui.label(&info.slot);
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    format!("{status:?} — loading will try backups"),
                                );
                            }
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Load").clicked() {
                                state.status = load(game, &state.saves, &info.slot);
                                state.world_replaced = true;
                                state.selected_entity = None;
                                state.selected_tile = None;
                            }
                            if state.confirm_delete.as_deref() == Some(info.slot.as_str()) {
                                if ui.button("Confirm delete").clicked() {
                                    state.status = match state.saves.delete(&info.slot) {
                                        Ok(()) => format!("Deleted '{}'", info.slot),
                                        Err(err) => format!("Delete failed: {err}"),
                                    };
                                    state.confirm_delete = None;
                                }
                            } else if ui.button("Delete").clicked() {
                                state.confirm_delete = Some(info.slot.clone());
                            }
                        });
                        ui.end_row();
                    }
                });
            ui.separator();
            ui.label(egui::RichText::new("New world").strong());
            egui::Grid::new("new_world").num_columns(2).show(ui, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut state.new_world.name);
                ui.end_row();
                ui.label("Seed");
                ui.text_edit_singleline(&mut state.new_world.seed);
                ui.end_row();
                ui.label("Landscape");
                egui::ComboBox::from_id_salt("preset")
                    .selected_text(
                        game.content()
                            .worldgen()
                            .get(&state.new_world.preset)
                            .map_or(state.new_world.preset.to_string(), |p| p.name.clone()),
                    )
                    .show_ui(ui, |ui| {
                        for preset in game.content().worldgen().iter() {
                            ui.selectable_value(
                                &mut state.new_world.preset,
                                preset.id.clone(),
                                &preset.name,
                            )
                            .on_hover_text(&preset.description);
                        }
                    });
                ui.end_row();
            });
            if ui.button("Generate new world").clicked() {
                match state.new_world.seed.trim().parse::<u64>() {
                    Ok(seed) => {
                        let options = NewWorldOptions {
                            name: state.new_world.name.clone(),
                            seed,
                            preset: state.new_world.preset.clone(),
                            created_unix: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |d| d.as_secs()),
                        };
                        state.status = match game.regenerate(&options) {
                            Ok(()) => format!("Generated '{}' (seed {seed})", options.name),
                            Err(err) => format!("Generation failed: {err}"),
                        };
                        state.world_replaced = true;
                        state.selected_entity = None;
                        state.selected_tile = None;
                    }
                    Err(_) => state.status = "The seed must be a whole number".into(),
                }
            }
        });
    state.show_world_window = open;
}

fn load(game: &mut GameState, saves: &SaveManager, slot: &str) -> String {
    let loaded = match saves.load(slot, game.content(), game.config()) {
        Ok(loaded) => loaded,
        Err(err) => return format!("Load failed: {err}"),
    };
    let mut notes = Vec::new();
    if let Some(path) = &loaded.recovered_from {
        notes.push(format!("recovered from backup {}", path.display()));
    }
    if let Some(v) = loaded.migrated_from {
        notes.push(format!("migrated from format {v}"));
    }
    notes.extend(loaded.warnings.iter().cloned());
    match game.replace_world(loaded.world) {
        Ok(()) if notes.is_empty() => format!("Loaded '{slot}'"),
        Ok(()) => format!("Loaded '{slot}': {}", notes.join("; ")),
        Err(err) => format!("Load failed: {err}"),
    }
}
