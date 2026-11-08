//! Environment: seasonal climate, water balance, vegetation and wildlife over
//! a simulated year, plus felling and hunting.

mod common;

use common::*;
use medieval_sim::content::AnimalId;
use medieval_sim::core::time::TimeUnit;
use medieval_sim::environment::EnvironmentQuery;
use medieval_sim::simulation::{Command, EventKind, GameState};

/// A test world ticking once per game hour.
fn hourly_game() -> GameState {
    let mut config = test_config();
    config.simulation.game_seconds_per_tick = 3600;
    game_with(config, 1340)
}

fn mean(values: impl Iterator<Item = f32>) -> f32 {
    let (sum, n) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    sum / n.max(1) as f32
}

#[derive(Default, Clone, Copy)]
struct Monthly {
    temp: f32,
    grass: f32,
    moisture: f32,
    samples: f32,
}

#[test]
fn a_year_of_weather_vegetation_and_wildlife() {
    let mut game = hourly_game();
    let initial_wildlife: Vec<f32> = (0..game.world().environment.wildlife.species.len())
        .map(|s| game.world().environment.wildlife.total(s))
        .collect();
    let mut months = [Monthly::default(); 12];
    let hours = ticks(&game, 365, TimeUnit::Day);
    for h in 0..hours {
        game.step_tick();
        if h % 24 != 12 {
            continue;
        }
        let world = game.world();
        let month = game.calendar().decompose(world.clock.now()).month as usize;
        let env = &world.environment;
        let land: Vec<usize> = (0..env.ground.len())
            .filter(|&i| world.derived.statics.ground[i].land_tiles > 0)
            .collect();
        let m = &mut months[month];
        m.temp += mean(env.weather.cells.iter().map(|c| c.temperature_c));
        m.grass += mean(land.iter().map(|&i| env.ground.grass[i]));
        m.moisture += mean(land.iter().map(|&i| env.ground.soil_moisture[i]));
        m.samples += 1.0;
    }
    let avg = |m: usize, f: fn(&Monthly) -> f32| f(&months[m]) / months[m].samples.max(1.0);
    let (jan, feb, jun, jul, aug) = (0, 1, 5, 6, 7);
    let jan_t = avg(jan, |m| m.temp);
    let jul_t = avg(jul, |m| m.temp);
    assert!(
        jul_t - jan_t > 8.0,
        "July {jul_t:.1} °C vs January {jan_t:.1} °C"
    );
    assert!((-3.0..10.0).contains(&jan_t), "January mean {jan_t:.1} °C");
    assert!((12.0..22.0).contains(&jul_t), "July mean {jul_t:.1} °C");
    assert!(
        avg(jun, |m| m.grass) > avg(feb, |m| m.grass),
        "grass grows in summer"
    );
    assert!(
        avg(aug, |m| m.moisture) < avg(jan, |m| m.moisture),
        "summer soils are drier"
    );

    assert!(
        count_events(&game, EventKind::PrecipitationStarted) > 50,
        "it rains"
    );
    assert!(count_events(&game, EventKind::SeasonChanged) >= 4);

    let wildlife = &game.world().environment.wildlife;
    for (s, id) in wildlife.species.iter().enumerate() {
        let total = wildlife.total(s);
        let capacity: f32 = wildlife
            .regions
            .iter()
            .map(|r| r.populations[s].capacity)
            .sum();
        assert!(total.is_finite() && total >= 0.0, "{id}: {total}");
        assert!(
            total <= capacity * 3.0 + 1.0,
            "{id}: {total} far above capacity {capacity}"
        );
        if initial_wildlife[s] > 1.0 {
            assert!(
                total > 0.05 * initial_wildlife[s],
                "{id} collapsed from {} to {total}",
                initial_wildlife[s]
            );
        }
    }
}

#[test]
fn queries_report_local_conditions() {
    let mut game = hourly_game();
    game.run_ticks(48);
    let world = game.world();
    let query = EnvironmentQuery::new(world);
    let land = walkable_tiles(&game)[0];
    let c = query.conditions_at(land).expect("conditions on the map");
    assert!(c.temperature_c > -30.0 && c.temperature_c < 40.0);
    assert!((0.0..=1.0).contains(&c.soil_moisture));
    assert!(c.wind_chill_c <= c.temperature_c + 1e-3);
    let g = query.growing_conditions(land).unwrap();
    assert!((0.0..=1.0).contains(&g.fertility));
    assert!(query.building_stress(land).is_some());
    assert!(query.region_resources(0).is_some());
    assert!(query.movement_penalty(land, &game.config().movement) >= 0.0);
}

#[test]
fn felling_and_hunting_change_the_environment() {
    let mut game = test_game();
    game.step_tick();
    let world = game.world();
    let wooded = world
        .derived
        .map
        .positions()
        .find(|p| {
            world
                .derived
                .statics
                .ground_cell(*p)
                .is_some_and(|g| world.environment.ground.tree_density[g] > 0.3)
        })
        .expect("some woodland");
    let cell = world.derived.statics.ground_cell(wooded).unwrap();
    let before = world.environment.ground.tree_density[cell];
    game.submit(Command::FellTrees {
        tile: wooded,
        fraction: 0.5,
    });
    game.step_tick();
    let after = game.world().environment.ground.tree_density[cell];
    assert!((after - before * 0.5).abs() < 1e-4);
    assert_eq!(count_events(&game, EventKind::TreesFelled), 1);

    let rabbit = AnimalId::new("rabbit");
    let world = game.world();
    let s = world.environment.wildlife.species_index(&rabbit).unwrap();
    let (region, tile) = world
        .derived
        .map
        .positions()
        .find_map(|p| {
            let r = world.derived.statics.wildlife_region(p)?;
            (world.environment.wildlife.regions[r].populations[s].count >= 3.0).then_some((r, p))
        })
        .expect("rabbits somewhere");
    let before = world.environment.wildlife.regions[region].populations[s].count;
    game.submit(Command::HuntWildlife {
        tile,
        animal: rabbit,
        count: 2,
    });
    game.step_tick();
    let pop = &game.world().environment.wildlife.regions[region].populations[s];
    assert!((before - pop.count - 2.0).abs() < 1e-3);
    assert!(pop.hunting_pressure > 0.0);
    assert_eq!(count_events(&game, EventKind::WildlifeHunted), 1);
}
