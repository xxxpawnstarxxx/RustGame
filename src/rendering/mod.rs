//! World rendering with Macroquad primitives and programmatically generated
//! textures only — no image assets.
//!
//! The renderer only *reads* simulation state. Terrain is drawn per chunk:
//! each visible chunk's tile colours are computed from world and environment
//! data into a small texture (one pixel per tile), cached, and rebuilt when
//! its inputs change (overlay, game hour for weather-dependent overlays, or a
//! navigation-relevant change in that chunk). Rebuilds are budgeted per frame
//! so large maps stay responsive. Structures, settlements, characters and
//! paths are drawn on top with shapes; a translucent wash shows night.

pub mod palette;

pub use palette::Overlay;

use crate::core::ids::EntityId;
use crate::simulation::GameState;
use crate::world::{DoorState, StructureKind, TilePos};
use macroquad::color::{BLACK, Color, WHITE};
use macroquad::input::{
    KeyCode, MouseButton, is_key_down, is_mouse_button_down, is_mouse_button_pressed,
    mouse_position, mouse_wheel,
};
use macroquad::math::vec2;
use macroquad::shapes::{
    draw_circle, draw_circle_lines, draw_line, draw_rectangle, draw_rectangle_lines,
};
use macroquad::text::{draw_text, measure_text};
use macroquad::texture::{DrawTextureParams, FilterMode, Image, Texture2D, draw_texture_ex};
use macroquad::time::get_frame_time;
use macroquad::window::{clear_background, screen_height, screen_width};
use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

const MAX_CHUNK_REBUILDS_PER_FRAME: usize = 24;

/// What a cached chunk texture was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChunkKey {
    overlay: Overlay,
    hour: u64,
    epoch: u64,
    world: u64,
}

struct CachedChunk {
    key: ChunkKey,
    texture: Texture2D,
}

/// Camera over the map, in tile units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub centre_x: f32,
    pub centre_y: f32,
    /// Screen pixels per tile.
    pub zoom: f32,
}

impl Camera {
    pub fn world_to_screen(&self, x: f32, y: f32) -> (f32, f32) {
        (
            (x - self.centre_x) * self.zoom + screen_width() / 2.0,
            (y - self.centre_y) * self.zoom + screen_height() / 2.0,
        )
    }

    pub fn screen_to_world(&self, sx: f32, sy: f32) -> (f32, f32) {
        (
            (sx - screen_width() / 2.0) / self.zoom + self.centre_x,
            (sy - screen_height() / 2.0) / self.zoom + self.centre_y,
        )
    }

    pub fn tile_at_screen(&self, sx: f32, sy: f32) -> TilePos {
        let (x, y) = self.screen_to_world(sx, sy);
        TilePos::new(x.floor() as i32, y.floor() as i32)
    }

    pub fn centre_tile(&self) -> TilePos {
        TilePos::new(self.centre_x.floor() as i32, self.centre_y.floor() as i32)
    }
}

/// Map clicks reported to the application.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MapInput {
    pub left_click: Option<TilePos>,
    /// Character under the cursor at the left click, if any.
    pub clicked_entity: Option<EntityId>,
    pub right_click: Option<TilePos>,
}

/// Renderer state (camera, overlay, texture cache).
pub struct MapView {
    pub camera: Camera,
    pub overlay: Overlay,
    pub show_night: bool,
    cache: HashMap<usize, CachedChunk>,
    last_mouse: (f32, f32),
    world_generation: u64,
    centred_on: Option<(u64, u32)>,
}

impl Default for MapView {
    fn default() -> Self {
        Self::new()
    }
}

impl MapView {
    pub fn new() -> Self {
        Self {
            camera: Camera {
                centre_x: 0.0,
                centre_y: 0.0,
                zoom: 4.0,
            },
            overlay: Overlay::Natural,
            show_night: true,
            cache: HashMap::new(),
            last_mouse: mouse_position(),
            world_generation: 0,
            centred_on: None,
        }
    }

    /// Drops all cached textures (after loading or regenerating a world).
    pub fn invalidate(&mut self) {
        self.cache.clear();
        self.world_generation += 1;
    }

    /// Centres the camera on the first settlement when a new world appears.
    fn centre_on_new_world(&mut self, game: &GameState) {
        let world = game.world();
        let identity = (world.meta.seed, world.map_info.width);
        if self.centred_on == Some(identity) {
            return;
        }
        self.centred_on = Some(identity);
        self.invalidate();
        let target = world
            .components
            .settlements
            .iter()
            .next()
            .map(|(_, s)| s.centre)
            .unwrap_or(TilePos::new(
                world.map_info.width as i32 / 2,
                world.map_info.height as i32 / 2,
            ));
        self.camera.centre_x = target.x as f32;
        self.camera.centre_y = target.y as f32;
        self.camera.zoom = 6.0;
    }

    /// Pans, zooms and reports clicks. Pointer and keys are ignored while
    /// the interface wants them.
    pub fn handle_input(
        &mut self,
        game: &GameState,
        pointer_over_ui: bool,
        keyboard_captured: bool,
    ) -> MapInput {
        self.centre_on_new_world(game);
        let mut input = MapInput::default();
        let dt = get_frame_time();
        let mouse = mouse_position();
        if !keyboard_captured {
            let speed = 600.0 / self.camera.zoom * dt;
            let (mut dx, mut dy) = (0.0, 0.0);
            if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
                dy -= speed;
            }
            if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
                dy += speed;
            }
            if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
                dx -= speed;
            }
            if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
                dx += speed;
            }
            self.camera.centre_x += dx;
            self.camera.centre_y += dy;
        }
        if !pointer_over_ui {
            let (_, wheel) = mouse_wheel();
            if wheel != 0.0 {
                let before = self.camera.screen_to_world(mouse.0, mouse.1);
                let factor = if wheel > 0.0 { 1.15 } else { 1.0 / 1.15 };
                self.camera.zoom = (self.camera.zoom * factor).clamp(0.25, 48.0);
                let after = self.camera.screen_to_world(mouse.0, mouse.1);
                self.camera.centre_x += before.0 - after.0;
                self.camera.centre_y += before.1 - after.1;
            }
            if is_mouse_button_down(MouseButton::Middle) {
                self.camera.centre_x -= (mouse.0 - self.last_mouse.0) / self.camera.zoom;
                self.camera.centre_y -= (mouse.1 - self.last_mouse.1) / self.camera.zoom;
            }
            if is_mouse_button_pressed(MouseButton::Left) {
                let tile = self.camera.tile_at_screen(mouse.0, mouse.1);
                input.left_click = Some(tile);
                input.clicked_entity = self.character_at(game, mouse);
            }
            if is_mouse_button_pressed(MouseButton::Right) {
                input.right_click = Some(self.camera.tile_at_screen(mouse.0, mouse.1));
            }
        }
        let world = game.world();
        self.camera.centre_x = self
            .camera
            .centre_x
            .clamp(0.0, world.map_info.width.max(1) as f32);
        self.camera.centre_y = self
            .camera
            .centre_y
            .clamp(0.0, world.map_info.height.max(1) as f32);
        self.last_mouse = mouse;
        input
    }

    fn character_at(&self, game: &GameState, mouse: (f32, f32)) -> Option<EntityId> {
        let world = game.world();
        let (wx, wy) = self.camera.screen_to_world(mouse.0, mouse.1);
        let reach = (8.0 / self.camera.zoom).max(0.6);
        world
            .components
            .movers
            .iter()
            .filter_map(|(id, m)| {
                let p = world.components.positions.get(id)?;
                let (x, y) = m.interpolated(p.tile);
                let d = ((x + 0.5 - wx).powi(2) + (y + 0.5 - wy).powi(2)).sqrt();
                (d <= reach).then_some((d, id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    /// Draws the whole scene.
    pub fn draw(
        &mut self,
        game: &GameState,
        selected_entity: Option<EntityId>,
        selected_tile: Option<TilePos>,
    ) {
        clear_background(Color::from_rgba(12, 14, 20, 255));
        let world = game.world();
        if world.derived.map.is_empty() {
            return;
        }
        self.draw_terrain(game);
        self.draw_structures(game);
        self.draw_settlements(game);
        self.draw_movers(game, selected_entity);
        if let Some(tile) = selected_tile {
            let (sx, sy) = self.camera.world_to_screen(tile.x as f32, tile.y as f32);
            draw_rectangle_lines(
                sx,
                sy,
                self.camera.zoom.max(3.0),
                self.camera.zoom.max(3.0),
                2.0,
                Color::new(1.0, 0.9, 0.2, 1.0),
            );
        }
        if self.show_night {
            let light = world.ambient.daylight.light_level;
            let darkness = (1.0 - light) * 0.6;
            if darkness > 0.01 {
                draw_rectangle(
                    0.0,
                    0.0,
                    screen_width(),
                    screen_height(),
                    Color::new(0.02, 0.03, 0.12, darkness),
                );
            }
        }
        draw_day_dial(game);
    }

    fn visible_chunks(&self, game: &GameState) -> Vec<(usize, TilePos)> {
        let map = &game.world().derived.map;
        let cs = map.chunk_size() as f32;
        let (x0, y0) = self.camera.screen_to_world(0.0, 0.0);
        let (x1, y1) = self.camera.screen_to_world(screen_width(), screen_height());
        let cx0 = (x0 / cs).floor().max(0.0) as i32;
        let cy0 = (y0 / cs).floor().max(0.0) as i32;
        let cx1 = ((x1 / cs).ceil() as i32).min(map.chunks_x() as i32);
        let cy1 = ((y1 / cs).ceil() as i32).min(map.chunks_y() as i32);
        let mut chunks = Vec::new();
        for cy in cy0..cy1 {
            for cx in cx0..cx1 {
                let chunk = crate::world::ChunkPos::new(cx, cy);
                if let Some(index) = map.chunk_index(chunk) {
                    chunks.push((index, map.chunk_origin(chunk)));
                }
            }
        }
        // Rebuild those nearest the centre first.
        let centre = self.camera.centre_tile();
        chunks.sort_by_key(|(_, o)| o.offset(cs as i32 / 2, cs as i32 / 2).chebyshev(centre));
        chunks
    }

    fn draw_terrain(&mut self, game: &GameState) {
        let world = game.world();
        let map = &world.derived.map;
        let calendar = game.calendar();
        let now = world.clock.now();
        let month = calendar.decompose(now).month + 1;
        let hour = if self.overlay.is_dynamic() {
            now.seconds() / calendar.seconds_per_hour()
        } else {
            0
        };
        let cs = map.chunk_size();
        let mut rebuilds = 0;
        for (index, origin) in self.visible_chunks(game) {
            let key = ChunkKey {
                overlay: self.overlay,
                hour,
                epoch: world
                    .navigation
                    .chunk_epochs
                    .get(index)
                    .copied()
                    .unwrap_or(0),
                world: self.world_generation,
            };
            let stale = self.cache.get(&index).is_none_or(|c| c.key != key);
            if stale && rebuilds < MAX_CHUNK_REBUILDS_PER_FRAME {
                rebuilds += 1;
                let mut image = Image::gen_image_color(cs as u16, cs as u16, BLACK);
                for y in 0..cs {
                    for x in 0..cs {
                        let pos = origin.offset(x as i32, y as i32);
                        image.set_pixel(
                            x,
                            y,
                            palette::tile_colour(world, game.content(), pos, self.overlay, month),
                        );
                    }
                }
                match self.cache.get_mut(&index) {
                    Some(cached) => {
                        cached.texture.update(&image);
                        cached.key = key;
                    }
                    None => {
                        let texture = Texture2D::from_image(&image);
                        texture.set_filter(FilterMode::Nearest);
                        self.cache.insert(index, CachedChunk { key, texture });
                    }
                }
            }
            if let Some(cached) = self.cache.get(&index) {
                let (sx, sy) = self
                    .camera
                    .world_to_screen(origin.x as f32, origin.y as f32);
                let size = cs as f32 * self.camera.zoom;
                draw_texture_ex(
                    &cached.texture,
                    sx,
                    sy,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(size + 0.5, size + 0.5)),
                        ..Default::default()
                    },
                );
            }
        }
        // Resource markers when close enough to read them.
        if self.camera.zoom >= 8.0 {
            for (pos, index) in self
                .visible_tiles(game)
                .filter_map(|p| map.tile(p).map(|t| (p, t.resource)))
            {
                if index != crate::world::map::NO_INDEX {
                    let (sx, sy) = self
                        .camera
                        .world_to_screen(pos.x as f32 + 0.5, pos.y as f32 + 0.5);
                    draw_circle(
                        sx,
                        sy,
                        self.camera.zoom * 0.15,
                        palette::resource_colour(index),
                    );
                }
            }
        }
    }

    fn visible_tiles<'a>(&self, game: &'a GameState) -> impl Iterator<Item = TilePos> + 'a {
        let (x0, y0) = self.camera.screen_to_world(0.0, 0.0);
        let (x1, y1) = self.camera.screen_to_world(screen_width(), screen_height());
        let map = &game.world().derived.map;
        let (x0, y0) = (x0.floor().max(0.0) as i32, y0.floor().max(0.0) as i32);
        let x1 = (x1.ceil() as i32).min(map.width() as i32);
        let y1 = (y1.ceil() as i32).min(map.height() as i32);
        (y0..y1).flat_map(move |y| (x0..x1).map(move |x| TilePos::new(x, y)))
    }

    fn draw_structures(&self, game: &GameState) {
        let z = self.camera.zoom;
        for (pos, tile) in game.world().structures.iter() {
            let (sx, sy) = self.camera.world_to_screen(pos.x as f32, pos.y as f32);
            if sx < -z || sy < -z || sx > screen_width() || sy > screen_height() {
                continue;
            }
            let (colour, inset) = match tile.kind {
                StructureKind::Wall => (Color::from_rgba(90, 70, 50, 255), 0.0),
                StructureKind::Floor => (Color::from_rgba(150, 130, 100, 255), 0.0),
                StructureKind::Fence => (Color::from_rgba(120, 95, 60, 255), 0.3),
                StructureKind::Door(state) | StructureKind::Gate(state) => (
                    match state {
                        DoorState::Open => Color::from_rgba(170, 140, 90, 255),
                        DoorState::Closed => Color::from_rgba(130, 90, 45, 255),
                        DoorState::Locked => Color::from_rgba(150, 40, 30, 255),
                    },
                    0.1,
                ),
            };
            draw_rectangle(
                sx + z * inset,
                sy + z * inset,
                z * (1.0 - 2.0 * inset),
                z * (1.0 - 2.0 * inset),
                colour,
            );
        }
    }

    fn draw_settlements(&self, game: &GameState) {
        for (_, s) in game.world().components.settlements.iter() {
            let (sx, sy) = self
                .camera
                .world_to_screen(s.centre.x as f32 + 0.5, s.centre.y as f32 + 0.5);
            let r = match s.kind {
                crate::world::SettlementKind::Town => 7.0,
                crate::world::SettlementKind::Village => 5.0,
                crate::world::SettlementKind::Hamlet => 3.5,
            };
            draw_circle(sx, sy, r, Color::from_rgba(200, 60, 40, 255));
            draw_circle_lines(sx, sy, r, 1.5, WHITE);
            let size = 18.0;
            let dims = measure_text(&s.name, None, size as u16, 1.0);
            draw_text(
                &s.name,
                sx - dims.width / 2.0 + 1.0,
                sy - r - 5.0,
                size,
                BLACK,
            );
            draw_text(&s.name, sx - dims.width / 2.0, sy - r - 6.0, size, WHITE);
        }
    }

    fn draw_movers(&self, game: &GameState, selected: Option<EntityId>) {
        let world = game.world();
        let z = self.camera.zoom;
        for (id, mover) in world.components.movers.iter() {
            let Some(pos) = world.components.positions.get(id) else {
                continue;
            };
            let (x, y) = mover.interpolated(pos.tile);
            let (sx, sy) = self.camera.world_to_screen(x + 0.5, y + 0.5);
            let r = (z * 0.35).max(3.0);
            if selected == Some(id) {
                // Planned route.
                let mut last = (sx, sy);
                for p in mover
                    .step
                    .iter()
                    .map(|s| s.to)
                    .chain(mover.path.iter().copied())
                {
                    let next = self
                        .camera
                        .world_to_screen(p.x as f32 + 0.5, p.y as f32 + 0.5);
                    draw_line(
                        last.0,
                        last.1,
                        next.0,
                        next.1,
                        2.0,
                        Color::new(1.0, 0.9, 0.3, 0.8),
                    );
                    last = next;
                }
                draw_circle_lines(sx, sy, r + 3.0, 2.0, Color::new(1.0, 0.9, 0.2, 1.0));
            }
            let colour = match mover.locomotion {
                crate::navigation::Locomotion::Fallen
                | crate::navigation::Locomotion::Stumbling => Color::from_rgba(220, 80, 60, 255),
                crate::navigation::Locomotion::Swimming | crate::navigation::Locomotion::Wading => {
                    Color::from_rgba(90, 160, 230, 255)
                }
                _ => Color::from_rgba(235, 225, 200, 255),
            };
            draw_circle(sx, sy, r, colour);
            draw_circle_lines(sx, sy, r, 1.0, BLACK);
        }
    }
}

/// A 24-hour dial: daylight arc between sunrise and sunset, hand at the
/// current time (midnight at the bottom, noon at the top).
fn draw_day_dial(game: &GameState) {
    let calendar = game.calendar();
    let daylight = &game.world().ambient.daylight;
    let day_len = calendar.seconds_per_day() as f32;
    let radius = 46.0;
    let cx = screen_width() - radius - 20.0;
    let cy = screen_height() - radius - 34.0;
    let angle = |fraction: f32| PI / 2.0 + fraction * TAU;

    draw_circle(cx, cy, radius, Color::from_rgba(40, 46, 77, 230));
    let sunrise = daylight.sunrise_seconds as f32 / day_len;
    let sunset = daylight.sunset_seconds as f32 / day_len;
    let steps = 64;
    for i in 0..steps {
        let f = sunrise + (sunset - sunrise) * (i as f32 + 0.5) / steps as f32;
        let a = angle(f);
        let inner = radius * 0.35;
        draw_line(
            cx + inner * a.cos(),
            cy + inner * a.sin(),
            cx + (radius - 3.0) * a.cos(),
            cy + (radius - 3.0) * a.sin(),
            (TAU * radius / steps as f32) * (sunset - sunrise) + 1.5,
            Color::from_rgba(237, 204, 107, 255),
        );
    }
    for hour in 0..calendar.hours_per_day() {
        let a = angle(hour as f32 / calendar.hours_per_day() as f32);
        let inner = if hour % 6 == 0 {
            radius - 10.0
        } else {
            radius - 5.0
        };
        draw_line(
            cx + inner * a.cos(),
            cy + inner * a.sin(),
            cx + radius * a.cos(),
            cy + radius * a.sin(),
            2.0,
            WHITE,
        );
    }
    draw_circle_lines(cx, cy, radius, 2.0, WHITE);
    let now = game.world().clock.now();
    let a = angle(calendar.seconds_into_day(now) as f32 / day_len);
    let hand = Color::from_rgba(190, 50, 40, 255);
    draw_line(
        cx,
        cy,
        cx + (radius - 5.0) * a.cos(),
        cy + (radius - 5.0) * a.sin(),
        3.0,
        hand,
    );
    draw_circle(cx, cy, 3.0, hand);
    let label = calendar.format_time(now);
    let dims = measure_text(&label, None, 22, 1.0);
    draw_text(
        &label,
        cx - dims.width / 2.0,
        cy + radius + 22.0,
        22.0,
        WHITE,
    );
}
