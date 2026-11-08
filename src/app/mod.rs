//! Application shell: start-up, the frame loop, and wiring between the
//! simulation, renderer and interface.
//!
//! Frame order:
//!
//! 1. advance the simulation by the real frame time (fixed-step ticks),
//! 2. draw the world with Macroquad primitives,
//! 3. build and draw the egui interface (which may submit commands or change
//!    pacing for the *next* frame).

pub mod paths;

use crate::content::ContentRegistry;
use crate::core::config::ConfigurationManager;
use crate::debugging::DebugTools;
use crate::simulation::{Command, GameState};
use crate::{rendering, ui};
use anyhow::{Context, Result};
use paths::GamePaths;
use tracing_subscriber::EnvFilter;

/// Installs the tracing subscriber. `RUST_LOG` overrides the default filter.
pub fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // Ignore the error if a subscriber is already installed (e.g. in tests).
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// Loads configuration and content and creates a new world.
pub fn build_game(paths: &GamePaths) -> Result<GameState> {
    let config = ConfigurationManager::load(&paths.config)
        .with_context(|| format!("loading configuration from {}", paths.config.display()))?;
    let content = ContentRegistry::load(&paths.data, config.config())
        .with_context(|| format!("loading content from {}", paths.data.display()))?;
    let game = GameState::new(config.config().clone(), content)?;
    Ok(game)
}

/// Runs the windowed application until the player quits.
pub async fn run() -> Result<()> {
    let paths = GamePaths::locate()?;
    tracing::info!(root = %paths.root.display(), "starting");
    let mut game = build_game(&paths)?;
    tracing::info!(
        start = %game.calendar().format(game.world().clock.now()),
        period = %game.config().period.name,
        "world created"
    );

    let mut ui_state = ui::UiState::new(paths.saves.clone(), &game);
    let mut debug = DebugTools::new(paths.saves.clone());
    let mut view = rendering::MapView::new();

    loop {
        let frame = game.advance_frame(f64::from(macroquad::time::get_frame_time()));

        // Map input (deferring to the interface when it wants the pointer or keys).
        let input = view.handle_input(&game, ui_state.pointer_over_ui, ui_state.keyboard_captured);
        if let Some(tile) = input.left_click {
            ui_state.selected_tile = Some(tile);
            ui_state.selected_entity = input.clicked_entity;
        }
        if let (Some(goal), Some(entity)) = (input.right_click, ui_state.selected_entity)
            && game.world().components.movers.contains(entity)
        {
            game.submit(Command::MoveTo {
                entity,
                goal,
                gait: Some(ui_state.order_gait),
            });
        }

        view.draw(&game, ui_state.selected_entity, ui_state.selected_tile);

        egui_macroquad::ui(|ctx| {
            ui::draw(ctx, &mut game, &mut ui_state, &frame, &mut view);
            if ui_state.show_debug {
                debug.draw(ctx, &mut game, &mut ui_state.selected_entity);
            }
        });
        egui_macroquad::draw();
        if ui_state.world_replaced {
            ui_state.world_replaced = false;
            view.invalidate();
        }

        if ui_state.quit_requested {
            break;
        }
        macroquad::window::next_frame().await;
    }
    tracing::info!("exiting");
    Ok(())
}
