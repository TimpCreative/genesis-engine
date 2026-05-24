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

    /// Manual P2-2 report: `cargo test -p genesis_app p2_2_formation_metrics_report -- --ignored --nocapture`
    #[test]
    #[ignore = "manual P2-2 verification report"]
    fn p2_2_formation_metrics_report() {
        use genesis_core::events::{EventKind, Significance};
        use genesis_core::parameters::WorldParameters;

        let targets = [
            1_000_000_i64,
            100_000_000,
            300_000_000,
            500_000_000,
            1_000_000_000,
            4_500_000_000,
        ];

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 7;

        for &year in &targets {
            let mut world = create_world(params.clone()).expect("world");
            let mut tectonics = TectonicsState::new();
            let mut climate = ClimateState::new();
            generate_full_history(
                &mut world,
                &mut tectonics,
                &mut climate,
                WorldYear(year),
                |_| {},
            )
            .expect("history");

            let summary = genesis_tectonics::summarize_world(&world, &tectonics);
            let notable = world
                .branch_tree
                .root()
                .event_log
                .iter_significant(Significance::Notable)
                .count();
            let cooling = world
                .branch_tree
                .root()
                .event_log
                .iter()
                .filter(|e| matches!(e.kind, EventKind::PlanetaryCoolingMilestone { .. }))
                .count();
            let oceans_begin = world
                .branch_tree
                .root()
                .event_log
                .iter()
                .filter(|e| matches!(e.kind, EventKind::OceansBeginForming { .. }))
                .count();
            let oceans_stable = world
                .branch_tree
                .root()
                .event_log
                .iter()
                .filter(|e| matches!(e.kind, EventKind::OceansStabilized { .. }))
                .count();
            let formation_done = world
                .branch_tree
                .root()
                .event_log
                .iter()
                .filter(|e| matches!(e.kind, EventKind::FormationComplete { .. }))
                .count();

            eprintln!("=== YEAR {year} ===");
            eprintln!("summarize_world: {summary}");
            eprintln!(
                "formation: temp_c={} sea_m={} co2_ppm={} sub_phase={:?} complete={}",
                world.data.global_temperature_c,
                world.data.sea_level_m,
                climate.atmospheric_composition.co2_ppm,
                climate.formation_sub_phase,
                climate.formation_complete
            );
            eprintln!(
                "events (Notable+): total_notable={notable} cooling_milestones={cooling} oceans_begin={oceans_begin} oceans_stable={oceans_stable} formation_complete={formation_done}"
            );
        }
    }

    /// Manual P2-3 report: `cargo test -p genesis_app p2_3_distance_to_ocean_stats -- --ignored --nocapture`
    #[test]
    #[ignore = "manual P2-3 distance-to-ocean verification"]
    fn p2_3_distance_to_ocean_stats() {
        use genesis_core::parameters::WorldParameters;

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 7;

        let mut world = create_world(params).expect("world");
        let mut tectonics = TectonicsState::new();
        let mut climate = ClimateState::new();
        generate_full_history(
            &mut world,
            &mut tectonics,
            &mut climate,
            WorldYear(1_000_000_000),
            |_| {},
        )
        .expect("history");

        let dist = &world.data.distance_to_ocean_km;
        let mut min_nonzero = f32::INFINITY;
        let mut max_finite = 0.0_f32;
        let mut count_zero = 0_u64;
        let mut count_deep_interior = 0_u64;

        for &d in dist {
            if d == 0.0 {
                count_zero += 1;
            }
            if d.is_finite() && d > 0.0 {
                min_nonzero = min_nonzero.min(d);
            }
            if d.is_finite() {
                max_finite = max_finite.max(d);
            }
            if d > 1000.0 {
                count_deep_interior += 1;
            }
        }

        eprintln!("=== distance_to_ocean_km at 1B years (subdiv=7) ===");
        eprintln!("min_nonzero_km: {min_nonzero}");
        eprintln!("max_finite_km: {max_finite}");
        eprintln!("count_at_zero (ocean): {count_zero}");
        eprintln!("count_gt_1000km (deep interior): {count_deep_interior}");
        eprintln!(
            "count_infinity: {}",
            dist.iter().filter(|d| d.is_infinite()).count()
        );

        assert!(count_zero > 0, "expected some ocean hexes at 1B");
        assert!(
            max_finite > 0.0 && max_finite < f32::INFINITY,
            "expected finite interior distances"
        );
    }

    /// Manual P2-5 report: `cargo test -p genesis_app p2_5_wind_field_stats -- --ignored --nocapture`
    #[test]
    #[ignore = "manual P2-5 wind field verification"]
    fn p2_5_wind_field_stats() {
        use genesis_core::parameters::WorldParameters;

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 7;

        let mut world = create_world(params).expect("world");
        let mut tectonics = TectonicsState::new();
        let mut climate = ClimateState::new();
        generate_full_history(
            &mut world,
            &mut tectonics,
            &mut climate,
            WorldYear(1_000_000_000),
            |_| {},
        )
        .expect("history");

        let data = &world.data;
        let n = data.cell_count() as usize;

        let mut min_speed = f32::INFINITY;
        let mut max_speed = 0.0_f32;
        let mut sum_low_elev = 0.0_f64;
        let mut count_low_elev = 0_u64;
        let mut sum_high_elev = 0.0_f64;
        let mut count_high_elev = 0_u64;

        for i in 0..n {
            let speed = data.wind_speed_m_s[i];
            let elev = data.elevation_mean[i];
            if speed.is_finite() && speed > 0.0 {
                min_speed = min_speed.min(speed);
                max_speed = max_speed.max(speed);
            }
            if elev < 1000.0 {
                sum_low_elev += f64::from(speed);
                count_low_elev += 1;
            }
            if elev > 4000.0 {
                sum_high_elev += f64::from(speed);
                count_high_elev += 1;
            }
        }

        let distinct_directions: std::collections::BTreeSet<i32> = data
            .wind_direction_rad
            .iter()
            .filter(|&&d| d > 0.0)
            .map(|&d| (d * 100.0).round() as i32)
            .collect();

        let mean_low = if count_low_elev > 0 {
            sum_low_elev / count_low_elev as f64
        } else {
            0.0
        };
        let mean_high = if count_high_elev > 0 {
            sum_high_elev / count_high_elev as f64
        } else {
            0.0
        };

        eprintln!("=== wind field at 1B years (subdiv=7) ===");
        eprintln!("min_speed_m_s: {min_speed}");
        eprintln!("max_speed_m_s: {max_speed}");
        eprintln!("mean_speed_below_1000m_elev: {mean_low}");
        eprintln!("mean_speed_above_4000m_elev: {mean_high}");
        eprintln!(
            "distinct_directions (0.01 rad bins): {}",
            distinct_directions.len()
        );

        assert!(max_speed > 0.0 && max_speed < 30.0);
        assert!(distinct_directions.len() >= 4);
    }

    /// Manual P2-6 report: `cargo test -p genesis_app p2_6_temperature_field_stats -- --ignored --nocapture`
    #[test]
    #[ignore = "manual P2-6 temperature field verification"]
    fn p2_6_temperature_field_stats() {
        use genesis_core::parameters::WorldParameters;

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 7;

        let mut world = create_world(params).expect("world");
        let mut tectonics = TectonicsState::new();
        let mut climate = ClimateState::new();
        generate_full_history(
            &mut world,
            &mut tectonics,
            &mut climate,
            WorldYear(1_000_000_000),
            |_| {},
        )
        .expect("history");

        let data = &world.data;
        let n = data.cell_count() as usize;
        let sea = data.sea_level_m;

        let mut sum_all = 0.0_f64;
        let mut min_temp = f32::INFINITY;
        let mut max_temp = f32::NEG_INFINITY;

        let mut sum_tropical = 0.0_f64;
        let mut count_tropical = 0_u64;
        let mut sum_polar = 0.0_f64;
        let mut count_polar = 0_u64;
        let mut sum_sea_level = 0.0_f64;
        let mut count_sea_level = 0_u64;
        let mut sum_high_elev = 0.0_f64;
        let mut count_high_elev = 0_u64;

        let mut sum_range_equator = 0.0_f64;
        let mut count_range_equator = 0_u64;
        let mut sum_range_mid = 0.0_f64;
        let mut count_range_mid = 0_u64;
        let mut sum_range_polar = 0.0_f64;
        let mut count_range_polar = 0_u64;

        for i in 0..n {
            let t = data.temperature_mean[i];
            let elev = data.elevation_mean[i];
            let range = data.temperature_range[i];
            let (lat, _) = data.grid.center_lat_lon(genesis_core::HexId(i as u32));
            let abs_lat_deg = lat.abs().to_degrees();

            sum_all += f64::from(t);
            min_temp = min_temp.min(t);
            max_temp = max_temp.max(t);

            if abs_lat_deg < 23.0 {
                sum_tropical += f64::from(t);
                count_tropical += 1;
            }
            if abs_lat_deg > 60.0 {
                sum_polar += f64::from(t);
                count_polar += 1;
            }
            if elev < sea + 100.0 {
                sum_sea_level += f64::from(t);
                count_sea_level += 1;
            }
            if elev > 3000.0 {
                sum_high_elev += f64::from(t);
                count_high_elev += 1;
            }

            if abs_lat_deg < 10.0 {
                sum_range_equator += f64::from(range);
                count_range_equator += 1;
            } else if (40.0..50.0).contains(&abs_lat_deg) {
                sum_range_mid += f64::from(range);
                count_range_mid += 1;
            } else if abs_lat_deg > 60.0 {
                sum_range_polar += f64::from(range);
                count_range_polar += 1;
            }
        }

        let global_mean = sum_all / n as f64;
        let mean_tropical = if count_tropical > 0 {
            sum_tropical / count_tropical as f64
        } else {
            0.0
        };
        let mean_polar = if count_polar > 0 {
            sum_polar / count_polar as f64
        } else {
            0.0
        };
        let mean_sea_level = if count_sea_level > 0 {
            sum_sea_level / count_sea_level as f64
        } else {
            0.0
        };
        let mean_high_elev = if count_high_elev > 0 {
            sum_high_elev / count_high_elev as f64
        } else {
            0.0
        };
        let mean_range_equator = if count_range_equator > 0 {
            sum_range_equator / count_range_equator as f64
        } else {
            0.0
        };
        let mean_range_mid = if count_range_mid > 0 {
            sum_range_mid / count_range_mid as f64
        } else {
            0.0
        };
        let mean_range_polar = if count_range_polar > 0 {
            sum_range_polar / count_range_polar as f64
        } else {
            0.0
        };

        eprintln!("=== temperature field at 1B years (subdiv=7) ===");
        eprintln!("global_mean_c: {global_mean}");
        eprintln!("min_c: {min_temp}");
        eprintln!("max_c: {max_temp}");
        eprintln!("mean_tropical_c (|lat|<23°): {mean_tropical}");
        eprintln!("mean_polar_c (|lat|>60°): {mean_polar}");
        eprintln!("mean_sea_level_c (elev < sea+100m): {mean_sea_level}");
        eprintln!("mean_high_elev_c (elev > 3000m): {mean_high_elev}");
        eprintln!("mean_range_equator_c: {mean_range_equator}");
        eprintln!("mean_range_45deg_c: {mean_range_mid}");
        eprintln!("mean_range_polar_c: {mean_range_polar}");

        assert!(min_temp >= -60.0);
        assert!(max_temp <= 50.0);
        assert!(mean_tropical > mean_polar);
    }

    /// Manual P2-7 report: `cargo test -p genesis_app p2_7_ocean_basin_stats -- --ignored --nocapture`
    #[test]
    #[ignore = "manual P2-7 ocean basin verification"]
    fn p2_7_ocean_basin_stats() {
        use genesis_core::parameters::WorldParameters;

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 7;

        let mut world = create_world(params).expect("world");
        let mut tectonics = TectonicsState::new();
        let mut climate = ClimateState::new();
        generate_full_history(
            &mut world,
            &mut tectonics,
            &mut climate,
            WorldYear(1_000_000_000),
            |_| {},
        )
        .expect("history");

        let basins = &climate.ocean_basins.basins;
        let count = basins.len();

        eprintln!("=== ocean basins at 1B years (subdiv=7) ===");
        eprintln!("total_basin_count: {count}");

        if let Some(largest) = basins.first() {
            let lat_span_deg = (largest.lat_max_rad - largest.lat_min_rad).to_degrees();
            eprintln!("largest_basin_hex_count: {}", largest.hex_count);
            eprintln!("largest_basin_lat_span_deg: {lat_span_deg}");
        }
        if let Some(smallest) = basins.last() {
            eprintln!("smallest_basin_hex_count: {}", smallest.hex_count);
        }
        if basins.len() >= 5 {
            eprintln!("fifth_largest_hex_count: {}", basins[4].hex_count);
        }

        assert!(count > 0, "expected at least one ocean basin");
        assert!(basins.first().is_some_and(|b| b.hex_count > 0));
        assert!(
            basins[0].hex_count >= basins.last().map(|b| b.hex_count).unwrap_or(0),
            "basins should be sorted largest-first"
        );
    }

    /// Manual P2-8 report: `cargo test -p genesis_app p2_8_ocean_current_stats -- --ignored --nocapture`
    #[test]
    #[ignore = "manual P2-8 ocean current verification"]
    fn p2_8_ocean_current_stats() {
        use genesis_climate::ocean_currents::MAX_CURRENT_SPEED_M_S;
        use genesis_core::parameters::WorldParameters;
        use genesis_core::BasinId;

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 7;

        let mut world = create_world(params).expect("world");
        let mut tectonics = TectonicsState::new();
        let mut climate = ClimateState::new();
        generate_full_history(
            &mut world,
            &mut tectonics,
            &mut climate,
            WorldYear(1_000_000_000),
            |_| {},
        )
        .expect("history");

        let data = &world.data;
        let sea = data.sea_level_m;
        let n = data.cell_count() as usize;

        let largest_id = climate
            .ocean_basins
            .basins
            .first()
            .map(|b| b.id)
            .unwrap_or(BasinId::NONE);

        let mut min_speed = f32::INFINITY;
        let mut max_speed = 0.0_f32;
        let mut sum_speed = 0.0_f64;
        let mut ocean_count = 0_u64;
        let mut fast_count = 0_u64;

        let mut basin_sum = 0.0_f64;
        let mut basin_count = 0_u64;

        for i in 0..n {
            if data.elevation_mean[i] >= sea {
                continue;
            }
            let [e, north] = data.ocean_current_vec[i];
            let speed =
                (f64::from(e) * f64::from(e) + f64::from(north) * f64::from(north)).sqrt() as f32;

            ocean_count += 1;
            min_speed = min_speed.min(speed);
            max_speed = max_speed.max(speed);
            sum_speed += f64::from(speed);
            if speed > 0.1 {
                fast_count += 1;
            }

            if data.basin_id[i] == largest_id {
                basin_sum += f64::from(speed);
                basin_count += 1;
            }
        }

        let mean_speed = if ocean_count > 0 {
            sum_speed / f64::from(ocean_count as u32)
        } else {
            0.0
        };
        let mean_basin_speed = if basin_count > 0 {
            basin_sum / f64::from(basin_count as u32)
        } else {
            0.0
        };

        let coastal =
            genesis_climate::ocean_currents::compute_coastal_temperature_adjustments(data);
        let mut adj_count = 0_u64;
        let mut min_adj = f32::INFINITY;
        let mut max_adj = f32::NEG_INFINITY;
        let mut sum_abs_adj = 0.0_f64;

        for &adj in coastal.values() {
            adj_count += 1;
            min_adj = min_adj.min(adj);
            max_adj = max_adj.max(adj);
            sum_abs_adj += f64::from(adj.abs());
        }
        let mean_abs_adj = if adj_count > 0 {
            sum_abs_adj / f64::from(adj_count as u32)
        } else {
            0.0
        };

        eprintln!("=== ocean currents at 1B years (subdiv=7) ===");
        eprintln!("ocean_hex_count: {ocean_count}");
        eprintln!("min_speed_m_s: {min_speed}");
        eprintln!("max_speed_m_s: {max_speed}");
        eprintln!("mean_speed_m_s: {mean_speed}");
        eprintln!("hexes_speed_gt_0.1: {fast_count}");
        eprintln!("largest_basin_mean_speed_m_s: {mean_basin_speed}");
        eprintln!("coastal_adjustment_count: {adj_count}");
        eprintln!("coastal_adj_min_c: {min_adj}");
        eprintln!("coastal_adj_max_c: {max_adj}");
        eprintln!("coastal_adj_mean_abs_c: {mean_abs_adj}");

        assert!(max_speed <= MAX_CURRENT_SPEED_M_S);
        assert!(min_speed >= 0.0);
        if fast_count > 0 {
            assert!(adj_count > 0 || mean_abs_adj == 0.0);
        }
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

        // plate assignment is independent of climate; elevation/sea level differ
        // because formation sea level affects erosion (P2-2).
        assert_eq!(
            world_tectonics_only.data.plate_id,
            world_combined.data.plate_id
        );
        assert_eq!(
            world_tectonics_only.data.plate_origin,
            world_combined.data.plate_origin
        );
    }
}
