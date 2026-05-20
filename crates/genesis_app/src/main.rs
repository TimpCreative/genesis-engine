use bevy::prelude::*;
use genesis_core::{WorldParameters, create_world};
use genesis_render::{GenesisRenderPlugin, WorldResource};

fn main() {
    let mut parameters = WorldParameters::default();
    // Level 6 (~7.3k hexes) keeps first-frame mesh build reasonable for the smoke test.
    parameters.core.grid.subdivision_level = 6;

    let world = create_world(parameters).expect("default world creates successfully");

    info!(
        "Genesis Engine Phase 0 smoke test: subdivision level {}, {} hexes",
        world.data.grid.subdivision_level(),
        world.data.grid.cell_count(),
    );

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Genesis Engine — Phase 0 Smoke Test".to_string(),
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
