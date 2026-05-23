// Environment variables read by the app:
// - GENESIS_TARGET_YEAR (i64): simulated year to advance to. Default 1_000_000.
//   Examples:
//     cargo run -p genesis_app                       # 1M years (default)
//     GENESIS_TARGET_YEAR=10000000 cargo run -p genesis_app   # 10M years
//     GENESIS_TARGET_YEAR=100000000 cargo run -p genesis_app  # 100M years

mod history;

use bevy::prelude::*;
use genesis_climate::ClimateState;
use genesis_core::{WorldParameters, WorldYear, create_world};
use genesis_render::{GenesisRenderPlugin, WorldResource};
use genesis_tectonics::TectonicsState;

use crate::history::generate_full_history;

fn target_year_from_env() -> WorldYear {
    const DEFAULT_TARGET_YEAR: i64 = 1_000_000;

    match std::env::var("GENESIS_TARGET_YEAR") {
        Ok(s) => match s.parse::<i64>() {
            Ok(year) if year >= 0 => {
                info!("Using GENESIS_TARGET_YEAR={} from environment", year);
                WorldYear(year)
            }
            Ok(year) => {
                warn!(
                    "GENESIS_TARGET_YEAR={} is negative; using default {}",
                    year, DEFAULT_TARGET_YEAR
                );
                WorldYear(DEFAULT_TARGET_YEAR)
            }
            Err(e) => {
                warn!(
                    "GENESIS_TARGET_YEAR='{}' could not be parsed ({}); using default {}",
                    s, e, DEFAULT_TARGET_YEAR
                );
                WorldYear(DEFAULT_TARGET_YEAR)
            }
        },
        Err(_) => WorldYear(DEFAULT_TARGET_YEAR),
    }
}

fn main() {
    let mut parameters = WorldParameters::default();
    // Level 7 (~21.9k hexes) per Doc 06 §9.1 Phase 1 performance target.
    parameters.core.grid.subdivision_level = 7;

    let mut world = create_world(parameters).expect("default world creates successfully");
    let mut tectonics = TectonicsState::new();
    let mut climate = ClimateState::new();

    let target_year = target_year_from_env();
    generate_full_history(
        &mut world,
        &mut tectonics,
        &mut climate,
        target_year,
        |_| {},
    )
    .expect("tectonic and climate history generation");

    let summary = genesis_tectonics::summarize_world(&world, &tectonics);
    info!(
        "Genesis Engine geology smoke test: subdivision level {}, {} hexes, {} plates",
        world.data.grid.subdivision_level(),
        world.data.grid.cell_count(),
        tectonics.registry.count(),
    );
    info!("{summary}");
    // Log subscriber is not active until Bevy starts; stderr carries startup diagnostics.
    eprintln!(
        "Genesis Engine geology smoke test: subdivision level {}, {} hexes, {} plates",
        world.data.grid.subdivision_level(),
        world.data.grid.cell_count(),
        tectonics.registry.count(),
    );
    eprintln!("{summary}");

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
    use genesis_climate::ClimateState;
    use genesis_core::parameters::{WorldParameters, WorldSeed};
    use genesis_core::{WorldYear, create_world};
    use genesis_render::GenesisRenderPlugin;
    use genesis_tectonics::{TectonicsState, generate_full_history_with_tectonics};

    use crate::history::generate_full_history;

    #[test]
    fn app_plugins_build_without_panicking() {
        App::new()
            .add_plugins(MinimalPlugins)
            .add_plugins(GenesisRenderPlugin)
            .finish();
    }

    #[test]
    fn empty_climate_layer_does_not_change_tectonic_world_at_1m() {
        let mut params = WorldParameters::default();
        params.core.seed = WorldSeed::from_integer(42);
        params.core.grid.subdivision_level = 5;

        let mut world_tectonics_only = create_world(params.clone()).expect("world");
        let mut world_combined = create_world(params).expect("world");
        let mut tectonics_only = TectonicsState::new();
        let mut tectonics_combined = TectonicsState::new();
        let mut climate = ClimateState::new();

        generate_full_history_with_tectonics(
            &mut world_tectonics_only,
            &mut tectonics_only,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("tectonics only");
        generate_full_history(
            &mut world_combined,
            &mut tectonics_combined,
            &mut climate,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("combined");

        assert_eq!(
            world_tectonics_only.data.elevation_mean,
            world_combined.data.elevation_mean
        );
        assert_eq!(
            world_tectonics_only.data.plate_id,
            world_combined.data.plate_id
        );
        assert_eq!(
            world_tectonics_only.data.plate_origin,
            world_combined.data.plate_origin
        );
        assert_eq!(
            world_tectonics_only.data.sea_level_m,
            world_combined.data.sea_level_m
        );
    }
}
