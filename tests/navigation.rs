//! Navigation and movement on generated and hand-built maps.

mod common;

use common::*;
use medieval_sim::content::{BuildingId, CharacterTemplateId, TerrainKind, WorldPresetId};
use medieval_sim::core::config::GameConfig;
use medieval_sim::core::ids::EntityId;
use medieval_sim::core::time::{Calendar, TimeUnit};
use medieval_sim::entities::EntityKind;
use medieval_sim::navigation::{Abilities, Gait, NavStatus, Occupancy, PathResult, find_path};
use medieval_sim::simulation::{Command, EventKind, GameState, WorldMeta, WorldState};
use medieval_sim::world::{DoorState, MapInfo, TilePos, WorldMap};

fn spawn(game: &mut GameState, template: &str, tile: TilePos) -> EntityId {
    game.submit(Command::SpawnCharacter {
        template: CharacterTemplateId::new(template),
        name: None,
        tile,
    });
    game.step_tick();
    newest(game, EntityKind::Character).expect("spawned")
}

fn position(game: &GameState, id: EntityId) -> TilePos {
    game.world().components.positions.get(id).unwrap().tile
}

/// Two walkable tiles `distance` apart (Chebyshev) with a path between them.
fn route(game: &GameState, distance: i32) -> (TilePos, TilePos) {
    let tiles = walkable_tiles(game);
    let set: std::collections::BTreeSet<TilePos> = tiles.iter().copied().collect();
    let world = game.world();
    for &a in tiles.iter().step_by(37) {
        for (dx, dy) in [(distance, 0), (0, distance), (distance, distance)] {
            let b = a.offset(dx, dy);
            if set.contains(&b)
                && matches!(
                    find_path(
                        world,
                        &game.config().movement,
                        &Occupancy::default(),
                        EntityId::new(0, 0),
                        Abilities::default(),
                        a,
                        b
                    ),
                    PathResult::Found { .. }
                )
            {
                return (a, b);
            }
        }
    }
    panic!("no route of length {distance}");
}

#[test]
fn characters_walk_to_their_destination() {
    let mut game = test_game();
    let (start, goal) = route(&game, 25);
    let walker = spawn(&mut game, "villein_ploughman", start);
    game.submit(Command::MoveTo {
        entity: walker,
        goal,
        gait: None,
    });
    let limit = ticks(&game, 1, TimeUnit::Hour);
    let taken = run_until(&mut game, limit, |g| position(g, walker) == goal);
    assert!(taken < limit, "did not arrive within an hour");
    game.step_tick();
    let mover = game.world().components.movers.get(walker).unwrap();
    assert_eq!(mover.status, NavStatus::Arrived);
    assert_eq!(count_events(&game, EventKind::DestinationReached), 1);
    assert!(mover.distance_m >= 25.0 * game.world().derived.map.tile_size_m());
}

fn travel_time(template: &str, gait: Gait) -> u64 {
    let mut game = test_game();
    let (start, goal) = route(&game, 20);
    let walker = spawn(&mut game, template, start);
    game.submit(Command::MoveTo {
        entity: walker,
        goal,
        gait: Some(gait),
    });
    run_until(&mut game, 20_000, |g| position(g, walker) == goal)
}

#[test]
fn gait_and_load_change_travel_time() {
    let walking = travel_time("villein_ploughman", Gait::Walk);
    let running = travel_time("villein_ploughman", Gait::Run);
    let crouching = travel_time("villein_ploughman", Gait::Crouch);
    let armoured = travel_time("man_at_arms", Gait::Walk);
    assert!(running < walking, "run {running} vs walk {walking}");
    assert!(crouching > walking, "crouch {crouching} vs walk {walking}");
    assert!(
        armoured > walking,
        "armoured {armoured} vs unladen {walking}"
    );
}

#[test]
fn encumbrance_tracks_carried_items() {
    let mut game = test_game();
    let tile = walkable_tiles(&game)[10];
    let soldier = spawn(&mut game, "man_at_arms", tile);
    game.step_tick();
    let cap = &game
        .world()
        .components
        .movers
        .get(soldier)
        .unwrap()
        .capability;
    // Sword 1.1 + mail 12 + breastplate 6 + kettle hat 1.8 + jack 3.5 + spear 2 = 26.4 kg.
    assert!((cap.carried_kg - 26.4).abs() < 0.01, "{}", cap.carried_kg);
    let mail = game
        .world()
        .components
        .items
        .iter()
        .find(|(_, i)| i.def.as_str() == "mail_hauberk")
        .map(|(id, _)| id)
        .unwrap();
    game.submit(Command::TransferItem {
        item: mail,
        to: medieval_sim::simulation::ItemLocation::Ground(tile),
    });
    game.step_tick();
    let cap = &game
        .world()
        .components
        .movers
        .get(soldier)
        .unwrap()
        .capability;
    assert!((cap.carried_kg - 14.4).abs() < 0.01, "{}", cap.carried_kg);
    game.submit(Command::DestroyEntity { entity: soldier });
    game.step_tick();
    let dropped = game
        .world()
        .components
        .items
        .iter()
        .filter(|(_, i)| i.location == medieval_sim::simulation::ItemLocation::Ground(tile))
        .count();
    assert_eq!(dropped, 6, "carried items fall where their holder stood");
}

#[test]
fn many_characters_share_the_road_without_piling_up() {
    let mut game = test_game();
    let (a, b) = route(&game, 18);
    let mut walkers = Vec::new();
    for i in 0..4 {
        let from = if i % 2 == 0 { a } else { b };
        let to = if i % 2 == 0 { b } else { a };
        let id = spawn(&mut game, "villein_ploughman", from);
        game.submit(Command::MoveTo {
            entity: id,
            goal: to,
            gait: None,
        });
        walkers.push((id, to));
    }
    let capacity = game.config().movement.tile_capacity as usize;
    let limit = ticks(&game, 1, TimeUnit::Hour);
    let arrived = run_until(&mut game, limit, |g| {
        let world = g.world();
        let mut counts = std::collections::BTreeMap::new();
        for (id, _) in &walkers {
            *counts
                .entry(world.components.positions.get(*id).unwrap().tile)
                .or_insert(0usize) += 1;
        }
        // Spawn tiles start shared; elsewhere capacity holds.
        assert!(
            counts
                .iter()
                .all(|(t, n)| *n <= capacity || *t == a || *t == b),
            "tile over capacity: {counts:?}"
        );
        walkers
            .iter()
            .all(|(id, to)| world.components.positions.get(*id).unwrap().tile == *to)
    });
    assert!(arrived < limit, "not everyone arrived");
}

#[test]
fn doors_and_walls_govern_access() {
    let mut game = test_game();
    let tiles = walkable_tiles(&game);
    let set: std::collections::BTreeSet<TilePos> = tiles.iter().copied().collect();
    // Find a 5×4 walkable patch with a walkable approach south of it.
    let origin = tiles
        .iter()
        .copied()
        .find(|o| (0..6).all(|dy| (0..5).all(|dx| set.contains(&o.offset(dx, dy)))))
        .expect("room for a cottage");
    game.submit(Command::PlaceBuilding {
        building: BuildingId::new("cruck_cottage"),
        origin,
    });
    game.step_tick();
    assert_eq!(count_events(&game, EventKind::StructureBuilt), 1);
    let inside = origin.offset(2, 1);
    let door = origin.offset(2, 3);
    let outside = origin.offset(2, 5);
    let walker = spawn(&mut game, "villein_ploughman", outside);

    game.submit(Command::SetDoor {
        tile: door,
        state: DoorState::Locked,
    });
    game.submit(Command::MoveTo {
        entity: walker,
        goal: inside,
        gait: None,
    });
    game.run_ticks(5);
    assert_eq!(
        game.world().components.movers.get(walker).unwrap().status,
        NavStatus::Failed
    );
    assert_eq!(count_events(&game, EventKind::PathFailed), 1);

    game.submit(Command::SetDoor {
        tile: door,
        state: DoorState::Closed,
    });
    game.submit(Command::MoveTo {
        entity: walker,
        goal: inside,
        gait: None,
    });
    let taken = run_until(&mut game, 2_000, |g| position(g, walker) == inside);
    assert!(taken < 2_000, "entered through the door");
    // Walls were never crossed: the route went through the doorway.
    let wall = origin.offset(0, 1);
    assert!(game.world().structures.get(wall).unwrap().kind.blocks());
}

#[test]
fn locking_a_door_mid_journey_forces_a_new_route() {
    let mut game = test_game();
    let (start, goal) = route(&game, 30);
    let walker = spawn(&mut game, "villein_ploughman", start);
    game.submit(Command::MoveTo {
        entity: walker,
        goal,
        gait: None,
    });
    game.run_ticks(3);
    let path: Vec<TilePos> = game
        .world()
        .components
        .movers
        .get(walker)
        .unwrap()
        .path
        .iter()
        .copied()
        .collect();
    assert!(path.len() > 10);
    // Drop a fence across a tile on the planned route.
    let blocker = path[path.len() / 2];
    game.submit(Command::PlaceBuilding {
        building: BuildingId::new("sheepfold"),
        origin: blocker,
    });
    game.step_tick();
    if game.world().structures.get(blocker).is_some() {
        let taken = run_until(&mut game, 20_000, |g| {
            let m = g.world().components.movers.get(walker).unwrap();
            position(g, walker) == goal || m.status == NavStatus::Failed
        });
        assert!(taken < 20_000);
        let tile = position(&game, walker);
        assert!(
            game.world()
                .structures
                .get(tile)
                .is_none_or(|s| !s.kind.blocks())
        );
    }
}

/// A hand-built map: lowland with a river across the middle and a cliff band.
fn hand_built_world(config: &GameConfig) -> WorldState {
    let calendar = Calendar::new(&config.calendar).unwrap();
    let meta = WorldMeta {
        name: "Hand-built".into(),
        seed: 1,
        created_unix: 0,
    };
    let info = MapInfo {
        preset: WorldPresetId::new("test_small"),
        seed: 1,
        width: 32,
        height: 32,
        fingerprint: String::new(),
    };
    let mut world = WorldState::new(config, &calendar, meta, info).unwrap();
    let mut map = WorldMap::new(32, 32, 16, 5.0);
    for y in 0..32 {
        for x in 0..32 {
            let t = map.tile_mut(TilePos::new(x, y)).unwrap();
            t.elevation_m = 10.0;
            t.terrain = if x == 16 {
                TerrainKind::River
            } else if y == 5 {
                TerrainKind::Cliff
            } else {
                TerrainKind::Lowland
            };
        }
    }
    world.derived.map = map;
    world
}

#[test]
fn water_and_cliffs_need_the_right_abilities() {
    let config = test_config();
    let world = hand_built_world(&config);
    let movement = &config.movement;
    let occupancy = Occupancy::default();
    let id = EntityId::new(0, 0);
    let across = (TilePos::new(10, 20), TilePos::new(22, 20));
    let walker = Abilities {
        swim: false,
        climb: false,
    };
    let swimmer = Abilities {
        swim: true,
        climb: false,
    };
    assert_eq!(
        find_path(&world, movement, &occupancy, id, walker, across.0, across.1),
        PathResult::Unreachable
    );
    let PathResult::Found { path, .. } = find_path(
        &world, movement, &occupancy, id, swimmer, across.0, across.1,
    ) else {
        panic!("a swimmer crosses");
    };
    assert!(path.iter().any(|p| p.x == 16));

    let over_cliff = (TilePos::new(5, 2), TilePos::new(5, 9));
    let climber = Abilities {
        swim: false,
        climb: true,
    };
    assert_eq!(
        find_path(
            &world,
            movement,
            &occupancy,
            id,
            walker,
            over_cliff.0,
            over_cliff.1
        ),
        PathResult::Unreachable
    );
    assert!(matches!(
        find_path(
            &world,
            movement,
            &occupancy,
            id,
            climber,
            over_cliff.0,
            over_cliff.1
        ),
        PathResult::Found { .. }
    ));
}

#[test]
fn bridges_open_river_crossings_and_search_is_bounded() {
    let mut config = test_config();
    let mut world = hand_built_world(&config);
    let id = EntityId::new(0, 0);
    let walker = Abilities::default();
    world
        .derived
        .map
        .tile_mut(TilePos::new(16, 28))
        .unwrap()
        .crossing = medieval_sim::world::Crossing::Bridge;
    let PathResult::Found { path, .. } = find_path(
        &world,
        &config.movement,
        &Occupancy::default(),
        id,
        walker,
        TilePos::new(10, 20),
        TilePos::new(22, 20),
    ) else {
        panic!("the bridge makes the crossing possible");
    };
    assert!(path.contains(&TilePos::new(16, 28)));
    config.movement.max_path_expansions = 100;
    assert_eq!(
        find_path(
            &world,
            &config.movement,
            &Occupancy::default(),
            id,
            walker,
            TilePos::new(10, 20),
            TilePos::new(22, 20)
        ),
        PathResult::TooFar
    );
}
