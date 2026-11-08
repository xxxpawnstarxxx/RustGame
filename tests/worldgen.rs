//! World generation: reproducibility and geographic rules.

mod common;

use common::*;
use medieval_sim::content::TerrainKind;
use medieval_sim::world::map::NO_INDEX;
use medieval_sim::world::{Crossing, GeneratedWorld, RoadKind, generate};

fn generated(preset: &str, seed: u64) -> GeneratedWorld {
    let config = test_config();
    let content = shipped_content(&config);
    generate(content.worldgen().get(preset).unwrap(), seed, &content).unwrap()
}

#[test]
fn generation_is_reproducible() {
    let a = generated("test_small", 7);
    let b = generated("test_small", 7);
    let c = generated("test_small", 8);
    assert_eq!(a.info, b.info);
    assert_eq!(a.map, b.map);
    assert_eq!(a.sites, b.sites);
    assert_ne!(a.info.fingerprint, c.info.fingerprint);
}

#[test]
fn default_preset_has_varied_geography() {
    let g = generated("lowland_england", 1340);
    let mut counts = std::collections::BTreeMap::new();
    let mut roads = 0;
    let mut bridges_or_fords = 0;
    let mut wooded = 0;
    for pos in g.map.positions() {
        let t = g.map.tile(pos).unwrap();
        *counts.entry(t.terrain).or_insert(0usize) += 1;
        roads += usize::from(t.road != RoadKind::None);
        bridges_or_fords += usize::from(t.crossing != Crossing::None);
        wooded += usize::from(t.tree_species != NO_INDEX);
    }
    let total = g.map.tile_count() as f32;
    for terrain in [
        TerrainKind::Ocean,
        TerrainKind::River,
        TerrainKind::Lowland,
        TerrainKind::Hills,
        TerrainKind::Marsh,
        TerrainKind::Beach,
    ] {
        assert!(
            counts.get(&terrain).copied().unwrap_or(0) > 0,
            "no {terrain:?} in {counts:?}"
        );
    }
    let sea = (counts[&TerrainKind::Ocean] + counts[&TerrainKind::Shallows]) as f32 / total;
    assert!((0.05..0.2).contains(&sea), "sea fraction {sea}");
    let woods = wooded as f32 / total;
    assert!((0.08..0.35).contains(&woods), "woodland fraction {woods}");
    assert!(
        roads > 300 && bridges_or_fords > 0,
        "roads {roads}, crossings {bridges_or_fords}"
    );
    assert!(g.sites.len() >= 4);
    assert!(g.map.resource_palette.len() >= 4);
}

#[test]
fn rivers_drain_downhill_to_water_or_the_edge() {
    let g = generated("lowland_england", 1340);
    let map = &g.map;
    for pos in map
        .positions()
        .filter(|p| map.tile(*p).unwrap().terrain == TerrainKind::River)
        .step_by(7)
    {
        let mut current = pos;
        let mut steps = 0;
        loop {
            let tile = map.tile(current).unwrap();
            let Some(next) = tile.downstream(current) else {
                break;
            };
            let Some(down) = map.tile(next) else { break };
            assert!(
                down.elevation_m <= tile.elevation_m + 2.0,
                "river climbs from {current} ({}) to {next} ({})",
                tile.elevation_m,
                down.elevation_m
            );
            if matches!(down.terrain, TerrainKind::Ocean | TerrainKind::Shallows) {
                break;
            }
            current = next;
            steps += 1;
            assert!(steps < 5_000, "flow path from {pos} loops");
        }
    }
}

#[test]
fn settlements_respect_geographic_rules() {
    let config = test_config();
    let content = shipped_content(&config);
    let preset = content.worldgen().get("lowland_england").unwrap();
    let g = generate(preset, 1340, &content).unwrap();
    let rules = &preset.settlements;
    for (i, site) in g.sites.iter().enumerate() {
        let tile = g.map.tile(site.centre).unwrap();
        assert!(
            matches!(tile.terrain, TerrainKind::Lowland | TerrainKind::Hills),
            "{} on {:?}",
            site.name,
            tile.terrain
        );
        assert!(
            tile.slope_deg <= 6.0,
            "{} on a {:.1}° slope",
            site.name,
            tile.slope_deg
        );
        assert!(
            tile.drainage_height_m >= 1.0,
            "{} is on the flood plain",
            site.name
        );
        let r = rules.water_radius_tiles as i32;
        let water = (-r..=r).any(|dy| {
            (-r..=r).any(|dx| {
                g.map
                    .tile(site.centre.offset(dx, dy))
                    .is_some_and(|t| t.terrain.is_water())
            })
        });
        assert!(water, "{} has no water within {r} tiles", site.name);
        for other in &g.sites[i + 1..] {
            assert!(site.centre.chebyshev(other.centre) >= rules.min_spacing_tiles as i32);
        }
        assert_ne!(
            tile.road,
            RoadKind::None,
            "{} is not on the road network",
            site.name
        );
    }
}

#[test]
fn resources_follow_their_terrain_rules() {
    let config = test_config();
    let content = shipped_content(&config);
    let preset = content.worldgen().get("lowland_england").unwrap();
    let g = generate(preset, 1340, &content).unwrap();
    let mut found = 0;
    for pos in g.map.positions() {
        let Some((id, amount)) = g.map.resource_at(pos) else {
            continue;
        };
        found += 1;
        let rule = preset.resources.iter().find(|r| &r.resource == id).unwrap();
        let tile = g.map.tile(pos).unwrap();
        assert!(
            rule.terrains.contains(&tile.terrain),
            "{id} on {:?}",
            tile.terrain
        );
        assert!((rule.amount[0]..=rule.amount[1]).contains(&amount));
    }
    assert!(found > 100);
}

#[test]
fn woodland_only_grows_on_land() {
    let g = generated("lowland_england", 99);
    for pos in g.map.positions() {
        let t = g.map.tile(pos).unwrap();
        if t.tree_species != NO_INDEX {
            assert!(!t.terrain.is_water() && t.terrain != TerrainKind::Cliff);
            assert!((0.0..=1.0).contains(&t.tree_density));
        }
    }
}
