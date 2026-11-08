//! Developer tools: inspectors for events, systems, timers and entities, and
//! actions for exercising the simulation (spawning, timers, quick save/load).
//!
//! Debug tools follow the same rule as the player interface: they read state
//! freely but change it only through commands (or by replacing the whole
//! world when quick-loading).

use crate::core::ids::EntityId;
use crate::core::time::TimeUnit;
use crate::entities::EntityKind;
use crate::persistence::SaveManager;
use crate::simulation::time::{Recurrence, TimerAction};
use crate::simulation::{Command, EventKind, GameState};
use egui_macroquad::egui;
use std::path::PathBuf;

const QUICKSAVE_SLOT: &str = "quicksave";
const MAX_LISTED_EVENTS: usize = 300;

/// State of the debug windows.
#[derive(Debug, Clone)]
pub struct DebugTools {
    saves_dir: PathBuf,
    show_events: bool,
    show_systems: bool,
    show_timers: bool,
    show_entities: bool,
    event_filter: Option<EventKind>,
    kind_filter: Option<EntityKind>,
    spawned: u32,
    selected: Option<EntityId>,
    status: String,
}

impl DebugTools {
    pub fn new(saves_dir: PathBuf) -> Self {
        Self {
            saves_dir,
            show_events: false,
            show_systems: false,
            show_timers: false,
            show_entities: false,
            event_filter: None,
            kind_filter: None,
            spawned: 0,
            selected: None,
            status: String::new(),
        }
    }

    /// Draws all enabled debug windows. `selected` is the shared entity
    /// selection (clicking an entity here selects it on the map too).
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        game: &mut GameState,
        selected: &mut Option<EntityId>,
    ) {
        self.selected = *selected;
        self.draw_windows(ctx, game);
        *selected = self.selected;
    }

    fn draw_windows(&mut self, ctx: &egui::Context, game: &mut GameState) {
        self.draw_toolbar(ctx, game);
        if self.show_events {
            let mut open = true;
            egui::Window::new("Events")
                .open(&mut open)
                .default_pos([10.0, 120.0])
                .default_size([520.0, 360.0])
                .show(ctx, |ui| self.events_ui(ui, game));
            self.show_events = open;
        }
        if self.show_systems {
            let mut open = true;
            egui::Window::new("Systems")
                .open(&mut open)
                .default_pos([540.0, 120.0])
                .show(ctx, |ui| systems_ui(ui, game));
            self.show_systems = open;
        }
        if self.show_timers {
            let mut open = true;
            egui::Window::new("Timers")
                .open(&mut open)
                .default_pos([540.0, 360.0])
                .show(ctx, |ui| timers_ui(ui, game));
            self.show_timers = open;
        }
        if self.show_entities {
            let mut open = true;
            egui::Window::new("Entities")
                .open(&mut open)
                .default_pos([10.0, 500.0])
                .default_size([420.0, 260.0])
                .show(ctx, |ui| self.entities_ui(ui, game));
            self.show_entities = open;
        }
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context, game: &mut GameState) {
        egui::TopBottomPanel::top("debug_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Debug").strong());
                ui.toggle_value(&mut self.show_events, "Events");
                ui.toggle_value(&mut self.show_systems, "Systems");
                ui.toggle_value(&mut self.show_timers, "Timers");
                ui.toggle_value(&mut self.show_entities, "Entities");
                ui.separator();
                let world = game.world();
                ui.label(format!(
                    "tick {} · {} entities · {} timers · {} queued events · {} queued commands",
                    world.clock.tick(),
                    world.entities.len(),
                    world.timers.len(),
                    world.events.len(),
                    world.commands.len()
                ));
                ui.separator();
                if ui.button("Quick save").clicked() {
                    self.quick_save(game);
                }
                if ui.button("Quick load").clicked() {
                    self.quick_load(game);
                }
                if !self.status.is_empty() {
                    ui.label(&self.status);
                }
            });
        });
    }

    fn quick_save(&mut self, game: &GameState) {
        let manager = SaveManager::new(&self.saves_dir, game.config().simulation.save_backups);
        self.status = match manager.save(
            QUICKSAVE_SLOT,
            game.world(),
            game.content(),
            game.calendar(),
        ) {
            Ok(meta) => format!("saved tick {}", meta.tick),
            Err(err) => format!("save failed: {err}"),
        };
    }

    fn quick_load(&mut self, game: &mut GameState) {
        let manager = SaveManager::new(&self.saves_dir, game.config().simulation.save_backups);
        self.status = match manager.load(QUICKSAVE_SLOT, game.content(), game.config()) {
            Ok(loaded) => {
                let tick = loaded.world.clock.tick();
                match game.replace_world(loaded.world) {
                    Ok(()) => {
                        self.selected = None;
                        format!("loaded tick {tick}")
                    }
                    Err(err) => format!("load failed: {err}"),
                }
            }
            Err(err) => format!("load failed: {err}"),
        };
    }

    fn events_ui(&mut self, ui: &mut egui::Ui, game: &GameState) {
        let log = game.event_log();
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} dispatched · {} retained · {} deferral ticks",
                log.total(),
                log.len(),
                log.deferral_ticks()
            ));
            egui::ComboBox::from_id_salt("event_filter")
                .selected_text(
                    self.event_filter
                        .map_or("All kinds".to_string(), |k| k.to_string()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.event_filter, None, "All kinds");
                    for kind in EventKind::ALL {
                        ui.selectable_value(&mut self.event_filter, Some(*kind), kind.to_string());
                    }
                });
        });
        ui.collapsing("Counts by kind", |ui| {
            egui::Grid::new("event_counts")
                .striped(true)
                .show(ui, |ui| {
                    for (kind, count) in log.counts() {
                        ui.label(kind.to_string());
                        ui.label(count.to_string());
                        ui.end_row();
                    }
                });
        });
        ui.separator();
        let calendar = game.calendar();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let filter = self.event_filter;
                for logged in log
                    .recent()
                    .filter(|e| filter.is_none_or(|k| e.envelope.event.kind() == k))
                    .take(MAX_LISTED_EVENTS)
                {
                    let env = &logged.envelope;
                    let handlers = if logged.handled_by.is_empty() {
                        "—".to_string()
                    } else {
                        logged
                            .handled_by
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "{} t{} {} [{}] {:?} → {}",
                            env.id,
                            env.tick,
                            calendar.format(env.time),
                            env.source,
                            env.event,
                            handlers
                        ))
                        .monospace()
                        .small(),
                    );
                }
            });
    }

    fn entities_ui(&mut self, ui: &mut egui::Ui, game: &mut GameState) {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("entity_filter")
                .selected_text(
                    self.kind_filter
                        .map_or("All kinds".to_string(), |k| k.to_string()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.kind_filter, None, "All kinds");
                    for kind in EntityKind::ALL {
                        ui.selectable_value(&mut self.kind_filter, Some(kind), kind.to_string());
                    }
                });
            // Exercises delayed actions: a pedlar arrives at the first settlement in an hour.
            let arrival = game
                .world()
                .components
                .settlements
                .iter()
                .next()
                .map(|(_, s)| s.centre);
            let template = game.content().characters().ids().next().cloned();
            if let (Some(tile), Some(template)) = (arrival, template)
                && ui.button("Arrival in 1 hour").clicked()
            {
                self.spawned += 1;
                let delay = game.calendar().duration(1, TimeUnit::Hour);
                game.submit(Command::ScheduleTimer {
                    label: format!("arrival #{}", self.spawned),
                    delay: Some(delay),
                    recurrence: Recurrence::Once,
                    action: Box::new(TimerAction::Command(Command::SpawnCharacter {
                        template,
                        name: None,
                        tile,
                    })),
                    repeat_limit: None,
                });
            }
            let selected = self
                .selected
                .filter(|id| game.world().entities.is_alive(*id));
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("Destroy selected"))
                .clicked()
                && let Some(entity) = selected
            {
                game.submit(Command::DestroyEntity { entity });
                self.selected = None;
            }
        });
        ui.separator();
        let world = game.world();
        let content = game.content();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("entities").striped(true).show(ui, |ui| {
                    ui.strong("Id");
                    ui.strong("Kind");
                    ui.strong("Name");
                    ui.strong("Scheduled activity");
                    ui.end_row();
                    let filter = self.kind_filter;
                    for (id, kind) in world
                        .entities
                        .iter()
                        .filter(|(_, k)| filter.is_none_or(|f| f == *k))
                    {
                        if ui
                            .selectable_label(self.selected == Some(id), id.to_string())
                            .clicked()
                        {
                            self.selected = Some(id);
                        }
                        ui.label(kind.to_string());
                        ui.label(world.name_of(id).unwrap_or("—"));
                        let activity = world
                            .components
                            .routines
                            .get(id)
                            .and_then(|r| r.current.as_ref())
                            .map_or("—".to_string(), |a| {
                                content
                                    .activities()
                                    .get(a)
                                    .map_or(a.to_string(), |d| d.name.clone())
                            });
                        ui.label(activity);
                        ui.end_row();
                    }
                });
            });
    }
}

fn systems_ui(ui: &mut egui::Ui, game: &GameState) {
    let report = game.scheduler().last_report();
    ui.label(format!(
        "last tick {}: {} systems, {} events, {} timers, {} commands ({} rejected)",
        report.tick,
        report.systems_run,
        report.events_dispatched,
        report.timers_fired,
        report.commands_applied,
        report.commands_rejected
    ));
    let time = game.time();
    ui.label(format!(
        "pacing: {} · {}× · {:.0} ticks/s target · {} dropped",
        if time.is_paused() {
            "paused"
        } else {
            "running"
        },
        time.multiplier(),
        time.target_ticks_per_second(),
        time.dropped_ticks()
    ));
    ui.separator();
    egui::Grid::new("systems").striped(true).show(ui, |ui| {
        for header in [
            "System", "Phase", "Schedule", "Runs", "Last run", "Last µs", "Events",
        ] {
            ui.strong(header);
        }
        ui.end_row();
        for entry in game.systems().iter() {
            let state = game.world().systems.get(entry.id());
            ui.label(entry.id().to_string());
            ui.label(format!("{:?}", entry.phase()));
            ui.label(match entry.interval_ticks() {
                Some(n) => format!("{} ({n} ticks)", entry.schedule()),
                None => entry.schedule().to_string(),
            });
            ui.label(state.map_or(0, |s| s.run_count).to_string());
            ui.label(
                state
                    .and_then(|s| s.last_run_tick)
                    .map_or("never".to_string(), |t| format!("t{t}")),
            );
            ui.label(entry.stats().last_update_micros.to_string());
            ui.label(entry.stats().events_handled.to_string());
            ui.end_row();
        }
    });
    let rejected: Vec<_> = game.scheduler().rejected_commands().take(10).collect();
    if !rejected.is_empty() {
        ui.separator();
        ui.strong("Recently rejected commands");
        for r in rejected {
            ui.label(format!("t{}: {} — {}", r.tick, r.reason, r.command));
        }
    }
}

fn timers_ui(ui: &mut egui::Ui, game: &mut GameState) {
    let calendar = game.calendar();
    let mut cancel = None;
    if game.world().timers.is_empty() {
        ui.label("No pending timers.");
    }
    egui::Grid::new("timers").striped(true).show(ui, |ui| {
        for timer in game.world().timers.iter() {
            ui.label(timer.id.to_string());
            ui.label(&timer.label);
            ui.label(calendar.format(timer.fire_at));
            ui.label(format!("{:?}", timer.recurrence));
            ui.label(format!("fired {}", timer.times_fired));
            if ui.small_button("Cancel").clicked() {
                cancel = Some(timer.id);
            }
            ui.end_row();
        }
    });
    if let Some(timer) = cancel {
        game.submit(Command::CancelTimer { timer });
    }
}
