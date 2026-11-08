//! Medieval simulation — windowed entry point.

use macroquad::window::Conf;

fn window_conf() -> Conf {
    Conf {
        window_title: "Medieval Simulation".to_owned(),
        window_width: 1280,
        window_height: 800,
        window_resizable: true,
        high_dpi: true,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    medieval_sim::app::init_logging();
    if let Err(err) = medieval_sim::app::run().await {
        tracing::error!("{err:#}");
        eprintln!("medieval_sim: {err:#}");
        std::process::exit(1);
    }
}
