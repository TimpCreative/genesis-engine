use bevy::prelude::*;
use genesis_core::{WorldParameters, WorldYear, create_world};
use genesis_render::{GenesisRenderPlugin, WorldResource};
use genesis_tectonics::{TectonicsState, generate_full_history_with_tectonics};

fn main() {
    let mut parameters = WorldParameters::default();
    // Level 6 (~7.3k hexes) keeps first-frame mesh build reasonable for the smoke test.
    parameters.core.grid.subdivision_level = 6;

    let mut world = create_world(parameters).expect("default world creates successfully");
    let mut tectonics = TectonicsState::new();

    // Formation at year 0 plus two Geological ticks (500k + 500k) for visible plate motion.
    generate_full_history_with_tectonics(&mut world, &mut tectonics, WorldYear(1_000_000), |_| {})
        .expect("tectonic formation and geological ticks");

    info!(
        "Genesis Engine geology smoke test: subdivision level {}, {} hexes, {} plates",
        world.data.grid.subdivision_level(),
        world.data.grid.cell_count(),
        tectonics.registry.count(),
    );
    info!("{}", genesis_tectonics::summarize_world(&world, &tectonics));

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Genesis Engine — Geology Smoke Test".to_string(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(GenesisRenderPlugin)
        .insert_resource(WorldResource(world))
        .run();
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use genesis_render::GenesisRenderPlugin;

    #[test]
    fn app_plugins_build_without_panicking() {
        App::new()
            .add_plugins(MinimalPlugins)
            .add_plugins(GenesisRenderPlugin)
            .finish();
    }
}
