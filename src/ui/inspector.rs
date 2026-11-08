//! The right-hand inspector: selected tile, its environment, and the
//! selected entity, with the actions available on them.

use super::UiState;
use crate::content::TerrainKind;
use crate::core::time::TimeUnit;
use crate::entities::EntityKind;
use crate::environment::EnvironmentQuery;
use crate::navigation::Gait;
use crate::simulation::{Command, GameState, ItemLocation};
use crate::world::map::NO_INDEX;
use crate::world::{DoorState, StructureKind};
use egui_macroquad::egui;

fn row(ui: &mut egui::Ui, label: &str, value: impl Into<String>) {
    ui.label(label);
    ui.label(value.into());
    ui.end_row();
}

pub(super) fn panel(ctx: &egui::Context, game: &mut GameState, state: &mut UiState) {
    egui::SidePanel::right("inspector")
        .default_width(320.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(entity) = state.selected_entity {
                    entity_section(ui, game, state, entity);
                    ui.separator();
                }
                if state.selected_tile.is_some() {
                    tile_section(ui, game, state);
                }
                ui.separator();
                if ui.button("Clear selection").clicked() {
                    state.selected_tile = None;
                    state.selected_entity = None;
                }
            });
        });
}

fn tile_section(ui: &mut egui::Ui, game: &mut GameState, state: &mut UiState) {
    let Some(pos) = state.selected_tile else {
        return;
    };
    let world = game.world();
    let Some(tile) = world.derived.map.tile(pos).copied() else {
        ui.label("Outside the map");
        return;
    };
    ui.heading(format!("Tile {pos}"));
    let env = EnvironmentQuery::new(world);
    egui::Grid::new("tile_grid")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            row(
                ui,
                "Terrain",
                format!("{:?} ({:?})", tile.terrain, tile.landform),
            );
            row(
                ui,
                "Elevation",
                format!("{:.1} m, slope {:.1}°", tile.elevation_m, tile.slope_deg),
            );
            row(ui, "Soil", format!("{:?}", tile.soil));
            row(
                ui,
                "Above drainage",
                format!("{:.1} m", tile.drainage_height_m),
            );
            if tile.river_flow_m3s > 0.0 {
                row(ui, "River", format!("{:.3} m³/s", tile.river_flow_m3s));
            }
            if tile.road != crate::world::RoadKind::None
                || tile.crossing != crate::world::Crossing::None
            {
                row(ui, "Way", format!("{:?} {:?}", tile.road, tile.crossing));
            }
            if let Some((id, amount)) = world.derived.map.resource_at(pos) {
                let name = game
                    .content()
                    .resources()
                    .get(id)
                    .map_or(id.to_string(), |r| r.name.clone());
                row(ui, "Deposit", format!("{name} ({amount} units)"));
            }
            if let Some(s) = world.structures.get(pos) {
                row(
                    ui,
                    "Structure",
                    format!(
                        "{:?} of {}",
                        s.kind,
                        world.name_of(s.building).unwrap_or("building")
                    ),
                );
            }
        });

    if let Some(c) = env.conditions_at(pos) {
        ui.label(egui::RichText::new("Conditions").strong());
        egui::Grid::new("env_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                row(
                    ui,
                    "Temperature",
                    format!("{:.1} °C (feels {:.1})", c.temperature_c, c.wind_chill_c),
                );
                row(
                    ui,
                    "Wind",
                    format!("{:.1} m/s from {:.0}°", c.wind_mps, c.wind_from_deg),
                );
                row(
                    ui,
                    "Humidity / cloud",
                    format!("{:.0}% / {:.0}%", c.humidity * 100.0, c.cloud * 100.0),
                );
                row(
                    ui,
                    "Precipitation",
                    if c.precipitation_mm_h > 0.0 {
                        format!(
                            "{:.1} mm/h {}",
                            c.precipitation_mm_h,
                            if c.snowing { "snow" } else { "rain" }
                        )
                    } else {
                        "none".into()
                    },
                );
                if !tile.terrain.is_water() {
                    row(
                        ui,
                        "Soil moisture",
                        format!("{:.0}%", c.soil_moisture * 100.0),
                    );
                    row(
                        ui,
                        "Mud / standing water",
                        format!("{:.0}% / {:.0} mm", c.mud * 100.0, c.surface_water_mm),
                    );
                    row(ui, "Snow", format!("{:.1} cm", c.snow_cm));
                    row(ui, "Grass", format!("{:.0}%", c.grass * 100.0));
                    let species = world
                        .derived
                        .statics
                        .ground_cell(pos)
                        .map(|g| world.environment.ground.tree_species[g])
                        .filter(|s| *s != NO_INDEX)
                        .and_then(|s| world.derived.map.tree_palette.get(s as usize))
                        .and_then(|id| game.content().trees().get(id))
                        .map_or("—".to_string(), |t| t.name.clone());
                    row(
                        ui,
                        "Woodland",
                        format!("{:.0}% {species}", c.tree_density * 100.0),
                    );
                }
                if c.flood_depth_m > 0.0 {
                    row(ui, "Flooded", format!("{:.2} m", c.flood_depth_m));
                }
                row(
                    ui,
                    "Water fouling",
                    format!("{:.1}%", c.water_contamination * 100.0),
                );
                row(
                    ui,
                    "River stage",
                    format!(
                        "{:.2} m{}",
                        c.river_stage_m,
                        if c.in_drought { " · DROUGHT" } else { "" }
                    ),
                );
            });
    }

    if let Some(region) = world.derived.statics.wildlife_region(pos) {
        ui.collapsing(format!("Wildlife (region {region})"), |ui| {
            let wildlife = &world.environment.wildlife;
            egui::Grid::new("wildlife_grid")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for (i, id) in wildlife.species.iter().enumerate() {
                        let p = &wildlife.regions[region].populations[i];
                        let name = game
                            .content()
                            .animals()
                            .get(id)
                            .map_or(id.to_string(), |a| a.name.clone());
                        let sick = if p.infected > 0.0 {
                            format!(", {:.1} sick", p.infected)
                        } else {
                            String::new()
                        };
                        row(
                            ui,
                            &name,
                            format!("{:.1} of ~{:.1}{sick}", p.count, p.capacity),
                        );
                    }
                });
        });
    }

    ui.separator();
    ui.label(egui::RichText::new("Actions here").strong());
    let content = game.content();
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("spawn_template")
            .selected_text(state.spawn_template.as_ref().map_or("—".into(), |t| {
                content
                    .characters()
                    .get(t)
                    .map_or(t.to_string(), |d| d.name.clone())
            }))
            .show_ui(ui, |ui| {
                for def in content.characters().iter() {
                    ui.selectable_value(&mut state.spawn_template, Some(def.id.clone()), &def.name);
                }
            });
    });
    let mut commands = Vec::new();
    if let Some(template) = &state.spawn_template
        && ui.button("Spawn character here").clicked()
    {
        commands.push(Command::SpawnCharacter {
            template: template.clone(),
            name: None,
            tile: pos,
        });
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("building")
            .selected_text(state.building.as_ref().map_or("—".into(), |b| {
                content
                    .buildings()
                    .get(b)
                    .map_or(b.to_string(), |d| d.name.clone())
            }))
            .show_ui(ui, |ui| {
                for def in content.buildings().iter() {
                    ui.selectable_value(&mut state.building, Some(def.id.clone()), &def.name);
                }
            });
        if let Some(building) = &state.building
            && ui.button("Build (top-left here)").clicked()
        {
            commands.push(Command::PlaceBuilding {
                building: building.clone(),
                origin: pos,
            });
        }
    });
    if let Some(s) = world.structures.get(pos) {
        if let StructureKind::Door(current) | StructureKind::Gate(current) = s.kind {
            ui.horizontal(|ui| {
                for state_choice in [DoorState::Open, DoorState::Closed, DoorState::Locked] {
                    if ui
                        .selectable_label(current == state_choice, format!("{state_choice:?}"))
                        .clicked()
                    {
                        commands.push(Command::SetDoor {
                            tile: pos,
                            state: state_choice,
                        });
                    }
                }
            });
        }
        if ui.button("Demolish building").clicked() {
            commands.push(Command::DestroyEntity { entity: s.building });
        }
    }
    if !matches!(tile.terrain, TerrainKind::Ocean | TerrainKind::Shallows)
        && ui.button("Fell a quarter of the trees").clicked()
    {
        commands.push(Command::FellTrees {
            tile: pos,
            fraction: 0.25,
        });
    }
    ui.horizontal(|ui| {
        let wildlife = &world.environment.wildlife;
        egui::ComboBox::from_id_salt("hunt")
            .selected_text(
                state
                    .hunt_animal
                    .as_ref()
                    .map_or("—".into(), |a| a.to_string()),
            )
            .show_ui(ui, |ui| {
                for id in &wildlife.species {
                    ui.selectable_value(&mut state.hunt_animal, Some(id.clone()), id.as_str());
                }
            });
        if let Some(animal) = &state.hunt_animal
            && ui.button("Hunt one").clicked()
        {
            commands.push(Command::HuntWildlife {
                tile: pos,
                animal: animal.clone(),
                count: 1,
            });
        }
    });
    for command in commands {
        game.submit(command);
    }
}

fn entity_section(
    ui: &mut egui::Ui,
    game: &mut GameState,
    state: &mut UiState,
    entity: crate::core::ids::EntityId,
) {
    let world = game.world();
    let Some(kind) = world.entities.kind(entity) else {
        state.selected_entity = None;
        return;
    };
    ui.heading(format!(
        "{} {}",
        world.name_of(entity).unwrap_or("Unnamed"),
        entity
    ));
    let calendar = game.calendar();
    let content = game.content();
    let mut commands = Vec::new();
    egui::Grid::new("entity_grid")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            row(ui, "Kind", kind.to_string());
            if let Some(p) = world.components.positions.get(entity) {
                row(ui, "Tile", p.tile.to_string());
            }
            if let Some(c) = world.components.characters.get(entity) {
                let years = world.clock.now().duration_since(c.birth).seconds()
                    / calendar.duration(1, TimeUnit::Year).seconds().max(1);
                row(ui, "Age", format!("{years}"));
                if let Some(p) = &c.profession {
                    row(
                        ui,
                        "Profession",
                        content
                            .professions()
                            .get(p)
                            .map_or(p.to_string(), |d| d.name.clone()),
                    );
                }
                if let Some(home) = c.home {
                    row(ui, "Home", world.name_of(home).unwrap_or("?"));
                }
                for s in &c.skills {
                    row(
                        ui,
                        &content
                            .skills()
                            .get(&s.skill)
                            .map_or(s.skill.to_string(), |d| d.name.clone()),
                        s.level.to_string(),
                    );
                }
            }
            if let Some(r) = world.components.routines.get(entity) {
                let activity = r.current.as_ref().map_or("—".into(), |a| {
                    content
                        .activities()
                        .get(a)
                        .map_or(a.to_string(), |d| d.name.clone())
                });
                row(ui, "Scheduled", activity);
            }
            if let Some(m) = world.components.movers.get(entity) {
                let cap = &m.capability;
                row(
                    ui,
                    "Movement",
                    format!("{:?} · {:?}", m.status, m.locomotion),
                );
                row(
                    ui,
                    "Gait",
                    format!("{} (using {})", m.gait, m.effective_gait),
                );
                row(
                    ui,
                    "Stamina / fatigue",
                    format!("{:.0}% / {:.0}%", cap.stamina * 100.0, cap.fatigue * 100.0),
                );
                row(
                    ui,
                    "Load",
                    format!(
                        "{:.1} of {:.0} kg ({:.0}%)",
                        cap.carried_kg,
                        cap.carry_capacity_kg,
                        cap.encumbrance() * 100.0
                    ),
                );
                row(
                    ui,
                    "Mobility",
                    format!(
                        "{:.0}%{}{}",
                        cap.mobility * 100.0,
                        if cap.can_swim { " · swims" } else { "" },
                        if cap.can_climb { " · climbs" } else { "" }
                    ),
                );
                if let Some(goal) = m.goal {
                    row(
                        ui,
                        "Heading for",
                        format!("{goal}, {} steps left", m.path.len()),
                    );
                }
                row(ui, "Travelled", format!("{:.0} m", m.distance_m));
            }
            if let Some(s) = world.components.settlements.get(entity) {
                row(ui, "Settlement", format!("{:?}", s.kind));
                if let Some(f) = s.faction {
                    row(ui, "Held by", world.name_of(f).unwrap_or("?"));
                }
            }
            if let Some(f) = world.components.factions.get(entity) {
                row(ui, "Faction", format!("{:?}", f.kind));
                if let Some(p) = f.parent {
                    row(ui, "Under", world.name_of(p).unwrap_or("?"));
                }
                for r in &f.relations {
                    row(
                        ui,
                        world.name_of(r.other).unwrap_or("?"),
                        format!("{:+}", r.value),
                    );
                }
            }
            if let Some(item) = world.components.items.get(entity) {
                row(
                    ui,
                    "Item",
                    format!(
                        "{} × {}",
                        content
                            .items()
                            .get(&item.def)
                            .map_or(item.def.to_string(), |d| d.name.clone()),
                        item.quantity
                    ),
                );
            }
        });
    let carried: Vec<String> = world
        .components
        .items
        .iter()
        .filter(|(_, i)| i.location == ItemLocation::Carried(entity))
        .map(|(_, i)| {
            format!(
                "{} × {}",
                content
                    .items()
                    .get(&i.def)
                    .map_or(i.def.to_string(), |d| d.name.clone()),
                i.quantity
            )
        })
        .collect();
    if !carried.is_empty() {
        ui.collapsing(format!("Carrying ({})", carried.len()), |ui| {
            for c in carried {
                ui.label(c);
            }
        });
    }
    if world.components.movers.contains(entity) {
        ui.horizontal(|ui| {
            ui.label("Gait:");
            for gait in Gait::ALL {
                if ui
                    .selectable_label(state.order_gait == gait, gait.to_string())
                    .clicked()
                {
                    state.order_gait = gait;
                    commands.push(Command::SetGait { entity, gait });
                }
            }
        });
        ui.label(egui::RichText::new("Right-click the map to send this character there.").small());
        if ui.button("Stop").clicked() {
            commands.push(Command::StopMoving { entity });
        }
    }
    if kind != EntityKind::Faction && ui.button("Remove from world").clicked() {
        commands.push(Command::DestroyEntity { entity });
        state.selected_entity = None;
    }
    for command in commands {
        game.submit(command);
    }
}
