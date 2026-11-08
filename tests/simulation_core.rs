//! Integration tests for the simulation core: scheduling, events, commands,
//! timers, routines, determinism and pacing.

mod common;

use common::*;
use medieval_sim::content::{CharacterTemplateId, RoutineId};
use medieval_sim::core::config::{ConfigurationManager, GameConfig};
use medieval_sim::core::ids::SystemId;
use medieval_sim::core::time::{TimeOfDay, TimeUnit};
use medieval_sim::entities::EntityKind;
use medieval_sim::persistence;
use medieval_sim::simulation::time::{DayPhase, Recurrence, TimerAction};
use medieval_sim::simulation::{
    Command, EventEnvelope, EventKind, EventSource, GameState, SimEvent, SimulationContext, System,
    SystemPhase, UpdateSchedule,
};

fn spawn_bare(game: &mut GameState, name: &str, routine: Option<&str>) {
    game.submit(Command::SpawnEntity {
        kind: EntityKind::Character,
        name: Some(name.into()),
        routine: routine.map(RoutineId::new),
    });
}

#[test]
fn shipped_configuration_matches_builtin_defaults() {
    let config = ConfigurationManager::load(&root().join("config")).unwrap();
    assert_eq!(config.config(), &GameConfig::default());
}

#[test]
fn commands_spawn_and_destroy_entities() {
    let mut game = test_game();
    game.step_tick();
    let created_before = count_events(&game, EventKind::EntityCreated);
    spawn_bare(&mut game, "Agnes", None);
    game.step_tick();
    let id = game
        .world()
        .entities
        .iter()
        .find(|(id, _)| game.world().name_of(*id) == Some("Agnes"))
        .map(|(id, _)| id)
        .unwrap();
    assert_eq!(game.world().entities.kind(id), Some(EntityKind::Character));
    assert_eq!(
        count_events(&game, EventKind::EntityCreated),
        created_before + 1
    );

    game.submit(Command::DestroyEntity { entity: id });
    game.step_tick();
    assert!(!game.world().entities.is_alive(id));
    assert!(game.world().name_of(id).is_none());
    assert_eq!(count_events(&game, EventKind::EntityDestroyed), 1);

    game.submit(Command::DestroyEntity { entity: id });
    let report = game.step_tick();
    assert_eq!(report.commands_rejected, 1);
    assert_eq!(game.scheduler().rejected_commands().count(), 1);
}

#[test]
fn unknown_definitions_are_rejected_before_spawning() {
    let mut game = test_game();
    let before = game.world().entities.len();
    spawn_bare(&mut game, "Nobody", Some("court_jester_of_atlantis"));
    game.submit(Command::SpawnCharacter {
        template: CharacterTemplateId::new("dragon_rider"),
        name: None,
        tile: walkable_tiles(&game)[0],
    });
    let report = game.step_tick();
    assert_eq!(report.commands_rejected, 2);
    assert_eq!(game.world().entities.len(), before);
}

#[test]
fn calendar_boundaries_and_day_phases_over_a_day() {
    let mut game = test_game();
    // 06:00 on 1 March is before sunrise (~06:17 with ~11.4 h of daylight).
    assert_eq!(game.world().ambient.daylight.phase, DayPhase::Dawn);
    let day = ticks(&game, 1, TimeUnit::Day);
    game.run_ticks(day);
    assert_eq!(count_events(&game, EventKind::HourStarted), 24);
    assert_eq!(count_events(&game, EventKind::DayStarted), 1);
    assert_eq!(count_events(&game, EventKind::DayPhaseChanged), 4);
    assert_eq!(game.world().ambient.daylight.phase, DayPhase::Dawn);
}

#[test]
fn routines_follow_the_schedule_and_honour_sunday() {
    let mut game = test_game();
    spawn_bare(&mut game, "Walter", Some("peasant_labourer"));
    game.step_tick();
    game.step_tick();
    let walter = newest(&game, EntityKind::Character).unwrap();
    let activity = |game: &GameState| {
        game.world()
            .components
            .routines
            .get(walter)
            .and_then(|r| r.current.clone())
            .map(|a| a.to_string())
    };
    assert_eq!(activity(&game).as_deref(), Some("field_work"));
    game.run_ticks(ticks(&game, 4, TimeUnit::Day) + ticks(&game, 2, TimeUnit::Hour));
    let date = game.calendar().decompose(game.world().clock.now());
    assert_eq!(game.calendar().weekday_name(date.weekday), "Sunday");
    assert_eq!(activity(&game).as_deref(), Some("mass"));
}

#[test]
fn timers_delay_and_repeat_actions() {
    let mut game = test_game();
    let hour = game.calendar().duration(1, TimeUnit::Hour);
    game.submit(Command::ScheduleTimer {
        label: "arrival".into(),
        delay: Some(hour),
        recurrence: Recurrence::Once,
        action: Box::new(TimerAction::Command(Command::SpawnEntity {
            kind: EntityKind::Character,
            name: Some("Late arrival".into()),
            routine: None,
        })),
        repeat_limit: None,
    });
    game.submit(Command::ScheduleTimer {
        label: "noon bell".into(),
        delay: None,
        recurrence: Recurrence::Daily {
            at: TimeOfDay::hm(12, 0),
        },
        action: Box::new(TimerAction::Emit(SimEvent::HourStarted { hour: 12 })),
        repeat_limit: Some(3),
    });
    game.step_tick();
    assert_eq!(game.world().timers.len(), 2);
    let people = |g: &GameState| {
        g.world()
            .entities
            .iter()
            .filter(|(_, k)| *k == EntityKind::Character)
            .count()
    };

    game.run_ticks(ticks(&game, 1, TimeUnit::Hour) - 1);
    assert_eq!(people(&game), 0, "fires exactly at the delay, not before");
    game.step_tick();
    assert_eq!(people(&game), 1);

    game.run_ticks(ticks(&game, 5, TimeUnit::Day));
    let bells = game
        .event_log()
        .recent()
        .filter(|e| matches!(e.envelope.source, EventSource::Timer(_)))
        .count();
    assert_eq!(bells, 3, "repeat limit honoured");
    assert!(game.world().timers.is_empty());
}

/// Runs hourly and checks it is told how much time has passed.
struct HourlyProbe;
impl System for HourlyProbe {
    fn id(&self) -> SystemId {
        SystemId::new("hourly_probe")
    }
    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::Interval {
            amount: 1,
            unit: TimeUnit::Hour,
        }
    }
    fn update(&mut self, ctx: &mut SimulationContext<'_>) {
        assert!(ctx.elapsed() <= ctx.calendar.duration(1, TimeUnit::Hour));
    }
}

/// Runs only after entities are created and emits a follow-up event.
struct CreationWatcher;
impl System for CreationWatcher {
    fn id(&self) -> SystemId {
        SystemId::new("creation_watcher")
    }
    fn phase(&self) -> SystemPhase {
        SystemPhase::PostUpdate
    }
    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::EventDriven
    }
    fn subscriptions(&self) -> &[EventKind] {
        &[EventKind::EntityCreated]
    }
    fn update(&mut self, _ctx: &mut SimulationContext<'_>) {}
    fn handle_event(&mut self, event: &EventEnvelope, ctx: &mut SimulationContext<'_>) {
        if let SimEvent::EntityCreated {
            entity,
            kind: EntityKind::Character,
        } = event.event
        {
            ctx.emit(SimEvent::SettlementFounded {
                settlement: entity,
                founder: None,
            });
        }
    }
}

#[test]
fn interval_and_event_driven_scheduling() {
    let mut game = test_game();
    game.register_system(Box::new(HourlyProbe)).unwrap();
    game.register_system(Box::new(CreationWatcher)).unwrap();
    assert!(
        game.register_system(Box::new(HourlyProbe)).is_err(),
        "duplicate ids rejected"
    );

    game.run_ticks(ticks(&game, 3, TimeUnit::Hour));
    let runs = |game: &GameState, id: &'static str| {
        game.world()
            .systems
            .get(&SystemId::new(id))
            .map_or(0, |s| s.run_count)
    };
    assert_eq!(runs(&game, "hourly_probe"), 3);
    assert_eq!(runs(&game, "ambient_time"), 180);
    assert_eq!(runs(&game, "weather"), 3);
    // Woken once by the world-creation events on the first tick.
    let baseline = runs(&game, "creation_watcher");
    assert_eq!(baseline, 1);

    spawn_bare(&mut game, "Hugh", None);
    game.step_tick();
    game.step_tick();
    assert_eq!(runs(&game, "creation_watcher"), baseline + 1);
    game.run_ticks(10);
    assert_eq!(
        runs(&game, "creation_watcher"),
        baseline + 1,
        "no further events, no further runs"
    );

    let recent: Vec<_> = game
        .event_log()
        .recent()
        .filter(|e| {
            matches!(
                e.envelope.event,
                SimEvent::EntityCreated {
                    kind: EntityKind::Character,
                    ..
                } | SimEvent::SettlementFounded { founder: None, .. }
            )
        })
        .collect();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[1].envelope.event.kind(), EventKind::EntityCreated);
    assert!(
        recent[1]
            .handled_by
            .contains(&SystemId::new("creation_watcher"))
    );
    assert!(
        recent[0].envelope.id > recent[1].envelope.id,
        "cascade follows its cause"
    );
}

#[test]
fn event_budget_defers_without_losing_events() {
    let mut config = test_config();
    config.simulation.max_events_per_tick = 2;
    let mut game = game_with(config, 1340);
    let initial = game.world().events.len() as u64;
    for i in 0..5 {
        spawn_bare(&mut game, &format!("V{i}"), None);
    }
    let report = game.step_tick();
    assert_eq!(report.events_dispatched, 2);
    assert!(report.events_deferred > 0);
    game.run_ticks(40);
    assert!(game.world().events.is_empty());
    assert!(count_events(&game, EventKind::EntityCreated) >= 5);
    assert!(game.event_log().total() >= initial + 5);
}

fn scripted_run(game: &mut GameState) {
    let tiles = walkable_tiles(game);
    game.submit(Command::SpawnCharacter {
        template: CharacterTemplateId::new("villein_ploughman"),
        name: None,
        tile: tiles[0],
    });
    game.submit(Command::SpawnCharacter {
        template: CharacterTemplateId::new("monk"),
        name: None,
        tile: tiles[tiles.len() / 2],
    });
    game.run_ticks(ticks(game, 2, TimeUnit::Hour));
    let walker = newest(game, EntityKind::Character).unwrap();
    game.submit(Command::MoveTo {
        entity: walker,
        goal: tiles[tiles.len() / 3],
        gait: None,
    });
    game.run_ticks(ticks(game, 1, TimeUnit::Day));
}

#[test]
fn identical_inputs_produce_identical_worlds() {
    let mut a = test_game();
    let mut b = test_game();
    scripted_run(&mut a);
    scripted_run(&mut b);
    assert_eq!(a.world(), b.world());
    assert_eq!(
        persistence::world_to_json(a.world()).unwrap(),
        persistence::world_to_json(b.world()).unwrap()
    );
}

#[test]
fn different_seeds_produce_different_worlds() {
    let a = game_with(test_config(), 1);
    let b = game_with(test_config(), 2);
    assert_ne!(
        a.world().map_info.fingerprint,
        b.world().map_info.fingerprint
    );
}

#[test]
fn pausing_and_speed_change_only_pacing() {
    let mut game = test_game();
    game.time_mut().set_paused(true);
    assert_eq!(game.advance_frame(0.2).ticks, 0);
    game.time_mut().request_steps(4);
    assert_eq!(game.advance_frame(0.0).ticks, 4);
    assert_eq!(game.world().clock.tick(), 4);
    game.time_mut().set_paused(false);
    game.time_mut().set_speed_index(2);
    assert_eq!(game.advance_frame(0.1).ticks, 30);
}
