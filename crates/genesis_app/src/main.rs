// Environment variables read by the app:
// - GENESIS_TARGET_YEAR (i64): simulated year to advance to. Default 1_000_000.
// - GENESIS_SUBDIVISION_LEVEL (u8): ISEA3H subdivision level. Default 7 (valid 5–9).
// - GENESIS_SEED (u64): world seed. Default from WorldParameters::default().
// - GENESIS_SCREENSHOT_DIR: if set, writes elevation/temperature/precipitation PNGs then exits.
//   Examples:
//     cargo run -p genesis_app                       # 1M years (default)
//     GENESIS_TARGET_YEAR=10000000 cargo run -p genesis_app   # 10M years
//     GENESIS_SUBDIVISION_LEVEL=8 GENESIS_SCREENSHOT_DIR=screenshots/y1m_subdiv8 \
//       cargo run -p genesis_app --release

mod history;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use genesis_climate::ClimateState;
use genesis_core::{WorldParameters, WorldYear, create_world};
use genesis_render::{GenesisRenderPlugin, RenderMode, WorldResource};
use genesis_tectonics::TectonicsState;

use crate::history::generate_full_history;

#[derive(Resource)]
struct AutoScreenshots {
    dir: String,
    year: i64,
    step: u8,
    frames_until_next: u8,
}

fn auto_screenshot_system(
    mut commands: Commands,
    state: Option<ResMut<AutoScreenshots>>,
    mut current_mode: ResMut<genesis_render::CurrentRenderMode>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut state) = state else {
        return;
    };

    if state.frames_until_next > 0 {
        state.frames_until_next -= 1;
        return;
    }

    let (mode, label) = match state.step {
        0 => (RenderMode::Elevation, "elevation"),
        1 => (RenderMode::Temperature, "temperature"),
        2 => (RenderMode::Precipitation, "precipitation"),
        _ => {
            exit.write(AppExit::Success);
            return;
        }
    };

    current_mode.0 = mode;
    let path = format!("{}/year{}_{}.png", state.dir, state.year, label);
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));

    state.step += 1;
    // Give Bevy a couple frames to apply mode change & capture.
    state.frames_until_next = 3;
}

const DEFAULT_SUBDIVISION_LEVEL: u8 = 7;
const MIN_SUBDIVISION_LEVEL: u8 = 5;
const MAX_SUBDIVISION_LEVEL: u8 = 9;

fn subdivision_level_from_env() -> u8 {
    match std::env::var("GENESIS_SUBDIVISION_LEVEL") {
        Ok(s) => match s.parse::<u8>() {
            Ok(level) if (MIN_SUBDIVISION_LEVEL..=MAX_SUBDIVISION_LEVEL).contains(&level) => {
                info!("Using GENESIS_SUBDIVISION_LEVEL={} from environment", level);
                level
            }
            Ok(level) => {
                warn!(
                    "GENESIS_SUBDIVISION_LEVEL={} is outside supported range {}..={}; using default {}",
                    level, MIN_SUBDIVISION_LEVEL, MAX_SUBDIVISION_LEVEL, DEFAULT_SUBDIVISION_LEVEL
                );
                DEFAULT_SUBDIVISION_LEVEL
            }
            Err(e) => {
                warn!(
                    "GENESIS_SUBDIVISION_LEVEL='{}' could not be parsed ({}); using default {}",
                    s, e, DEFAULT_SUBDIVISION_LEVEL
                );
                DEFAULT_SUBDIVISION_LEVEL
            }
        },
        Err(_) => DEFAULT_SUBDIVISION_LEVEL,
    }
}

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

fn print_world_summary(world: &genesis_core::World, tectonics: &TectonicsState) {
    let summary = genesis_tectonics::summarize_world(world, tectonics);
    info!(
        "Genesis Engine geology smoke test: subdivision level {}, {} hexes, {} plates",
        world.data.grid.subdivision_level(),
        world.data.grid.cell_count(),
        tectonics.registry.count(),
    );
    info!("{summary}");
    eprintln!(
        "Genesis Engine geology smoke test: subdivision level {}, {} hexes, {} plates",
        world.data.grid.subdivision_level(),
        world.data.grid.cell_count(),
        tectonics.registry.count(),
    );
    eprintln!("{summary}");
}

fn main() {
    let mut parameters = WorldParameters::default();
    // Default level 7 (~21.9k hexes) per Doc 06 §9.1; override via GENESIS_SUBDIVISION_LEVEL.
    let subdivision_level = subdivision_level_from_env();
    parameters.core.grid.subdivision_level = subdivision_level;
    eprintln!("GENESIS_SUBDIVISION_LEVEL={subdivision_level}");
    if let Ok(seed_str) = std::env::var("GENESIS_SEED") {
        match seed_str.parse::<u64>() {
            Ok(seed) => {
                parameters.core.seed = genesis_core::parameters::WorldSeed::from_integer(seed);
                eprintln!("GENESIS_SEED={seed}");
            }
            Err(_) => eprintln!("GENESIS_SEED={seed_str} is not a valid u64; using default seed"),
        }
    }

    let mut world = create_world(parameters).expect("default world creates successfully");
    let mut tectonics = TectonicsState::new();
    let mut climate = ClimateState::new();

    let requested_year = target_year_from_env();
    // If the user requests year 0, still run Formation once to populate plate/climate fields.
    let simulate_year = if requested_year.value() == 0 {
        WorldYear(1)
    } else {
        requested_year
    };
    // Print progress every 100M simulated years so long generations are
    // visibly alive (a 4.5B-year run is minutes of otherwise silent compute).
    let mut last_reported_year: i64 = 0;
    generate_full_history(
        &mut world,
        &mut tectonics,
        &mut climate,
        simulate_year,
        |p| {
            const REPORT_EVERY_YEARS: i64 = 100_000_000;
            let current = p.current_year.value();
            if current - last_reported_year >= REPORT_EVERY_YEARS {
                last_reported_year = current;
                eprintln!(
                    "[genesis] simulated year {:.2}B / {:.2}B",
                    current as f64 / 1e9,
                    p.target_year.value() as f64 / 1e9
                );
            }
        },
    )
    .expect("tectonic and climate history generation");
    if requested_year.value() == 0 {
        world.data.current_year = requested_year;
    }

    print_world_summary(&world, &tectonics);

    let screenshot_dir = std::env::var("GENESIS_SCREENSHOT_DIR").ok();
    if let Some(dir) = screenshot_dir.as_deref() {
        let _ = std::fs::create_dir_all(dir);
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Genesis Engine — Elevation (press M to cycle)".to_string(),
            resolution: (1280, 720).into(),
            ..default()
        }),
        ..default()
    }))
    .add_plugins(GenesisRenderPlugin)
    .insert_resource(WorldResource(world));

    if let Some(dir) = screenshot_dir {
        app.insert_resource(AutoScreenshots {
            dir,
            year: requested_year.value(),
            step: 0,
            frames_until_next: 3,
        })
        .add_systems(Update, auto_screenshot_system);
    }

    app.run();
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
        let significant_basin_count = basins
            .iter()
            .filter(|b| b.hex_count >= SIGNIFICANT_BASIN_MIN_HEXES)
            .count();
        eprintln!("significant_basin_count: {significant_basin_count}");

        // Diagnostic: plate surface utilization and continental elevation dispersion.
        let mut plate_populated_sum: f64 = 0.0;
        let mut plate_populated_count: u64 = 0;
        for plate in tectonics.registry.iter() {
            plate_populated_sum += f64::from(plate.surface.populated_count());
            plate_populated_count += 1;
        }
        let mean_populated = if plate_populated_count > 0 {
            plate_populated_sum / plate_populated_count as f64
        } else {
            0.0
        };
        eprintln!("avg_plate_surface_populated_count: {mean_populated}");

        let mut cont_sum: f64 = 0.0;
        let mut cont_sum_sq: f64 = 0.0;
        let mut cont_n: u64 = 0;
        for (i, &plate_id) in world.data.plate_id.iter().enumerate() {
            if plate_id == genesis_core::PlateId::NONE {
                continue;
            }
            let Some(plate) = tectonics.registry.get(plate_id) else {
                continue;
            };
            if plate.plate_type != genesis_tectonics::PlateType::Continental {
                continue;
            }
            let e = f64::from(world.data.elevation_mean[i]);
            cont_sum += e;
            cont_sum_sq += e * e;
            cont_n += 1;
        }
        let cont_mean = if cont_n > 0 {
            cont_sum / cont_n as f64
        } else {
            0.0
        };
        let cont_var = if cont_n > 0 {
            (cont_sum_sq / cont_n as f64) - cont_mean * cont_mean
        } else {
            0.0
        };
        let cont_stddev = cont_var.max(0.0).sqrt();
        eprintln!("continental_elevation_mean_m: {cont_mean}");
        eprintln!("continental_elevation_stddev_m: {cont_stddev}");

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

        const MAJOR_BASIN_MIN_HEXES: u32 = 50;
        const MICRO_BASIN_MAX_HEXES: u32 = 4;
        const SIGNIFICANT_BASIN_MIN_HEXES: u32 = MICRO_BASIN_MAX_HEXES + 1;
        let major_ocean_basin_count = basins
            .iter()
            .filter(|b| !b.is_inland && b.hex_count >= MAJOR_BASIN_MIN_HEXES)
            .count();
        let inland_lake_basin_count = basins.iter().filter(|b| b.is_inland).count();
        let micro_basin_count = basins
            .iter()
            .filter(|b| b.hex_count <= MICRO_BASIN_MAX_HEXES)
            .count();
        let sea = world.data.sea_level_m;
        let total_ocean_hexes: u64 = world
            .data
            .elevation_mean
            .iter()
            .filter(|&&e| e < sea)
            .count() as u64;
        let largest_ocean_component_fraction = if total_ocean_hexes > 0 {
            f64::from(basins.first().map(|b| b.hex_count).unwrap_or(0)) / total_ocean_hexes as f64
        } else {
            0.0
        };
        eprintln!("major_ocean_basin_count: {major_ocean_basin_count}");
        eprintln!("inland_lake_basin_count: {inland_lake_basin_count}");
        eprintln!("micro_basin_count: {micro_basin_count}");
        eprintln!("largest_ocean_component_fraction: {largest_ocean_component_fraction}");

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
        use genesis_core::BasinId;
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

    /// Manual P2-9 report: `cargo test -p genesis_app p2_9_precipitation_stats -- --ignored --nocapture`
    #[test]
    #[ignore = "manual P2-9 precipitation verification"]
    fn p2_9_precipitation_stats() {
        use genesis_core::parameters::WorldParameters;
        use std::f32::consts::PI;

        fn upwind_neighbor(
            data: &genesis_core::data::WorldData,
            hex: genesis_core::HexId,
            wind_dir: f32,
        ) -> Option<genesis_core::HexId> {
            let grid = &data.grid;
            let hex_pos = grid.cell_center_direction(hex);
            let north_pole = [0.0_f64, 0.0, 1.0];
            let cross = |a: [f64; 3], b: [f64; 3]| {
                [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ]
            };
            let normalize = |v: [f64; 3]| {
                let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                if mag < 1e-9 {
                    [1.0, 0.0, 0.0]
                } else {
                    [v[0] / mag, v[1] / mag, v[2] / mag]
                }
            };
            let east = normalize(cross(north_pole, hex_pos));
            let north = normalize(cross(hex_pos, east));
            let bearing = wind_dir + PI;
            let target_east = bearing.sin();
            let target_north = bearing.cos();
            let mut best = None;
            let mut best_alignment = -1.0_f64;
            for &neighbor in grid.neighbors(hex) {
                let n_pos = grid.cell_center_direction(neighbor);
                let to_n = [
                    n_pos[0] - hex_pos[0],
                    n_pos[1] - hex_pos[1],
                    n_pos[2] - hex_pos[2],
                ];
                let east_comp = to_n[0] * east[0] + to_n[1] * east[1] + to_n[2] * east[2];
                let north_comp = to_n[0] * north[0] + to_n[1] * north[1] + to_n[2] * north[2];
                let mag = (east_comp * east_comp + north_comp * north_comp).sqrt();
                if mag < 1e-9 {
                    continue;
                }
                let alignment = (east_comp / mag) * f64::from(target_east)
                    + (north_comp / mag) * f64::from(target_north);
                if alignment > best_alignment {
                    best_alignment = alignment;
                    best = Some(neighbor);
                }
            }
            best
        }

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

        let mut land_count = 0_u64;
        let mut min_precip = f32::INFINITY;
        let mut max_precip = 0.0_f32;
        let mut sum_land = 0.0_f64;

        let mut sum_tropical = 0.0_f64;
        let mut count_tropical = 0_u64;
        let mut sum_subtropical = 0.0_f64;
        let mut count_subtropical = 0_u64;
        let mut sum_temperate = 0.0_f64;
        let mut count_temperate = 0_u64;
        let mut sum_polar = 0.0_f64;
        let mut count_polar = 0_u64;

        let mut desert_count = 0_u64;
        let mut wet_count = 0_u64;

        let mut rain_shadow_sum = 0.0_f64;
        let mut rain_shadow_count = 0_u64;

        for i in 0..n {
            let elev = data.elevation_mean[i];
            if elev < sea {
                continue;
            }

            let hex = genesis_core::HexId(i as u32);
            let p = data.precipitation[i];
            let (lat, _) = data.grid.center_lat_lon(hex);
            let abs_lat_deg = lat.abs().to_degrees();

            land_count += 1;
            min_precip = min_precip.min(p);
            max_precip = max_precip.max(p);
            sum_land += f64::from(p);

            if p < 250.0 {
                desert_count += 1;
            }
            if p > 1500.0 {
                wet_count += 1;
            }

            if abs_lat_deg < 23.0 {
                sum_tropical += f64::from(p);
                count_tropical += 1;
            } else if abs_lat_deg < 40.0 {
                sum_subtropical += f64::from(p);
                count_subtropical += 1;
            } else if abs_lat_deg < 60.0 {
                sum_temperate += f64::from(p);
                count_temperate += 1;
            } else {
                sum_polar += f64::from(p);
                count_polar += 1;
            }

            let wind_dir = data.wind_direction_rad[i];
            if let Some(upwind) = upwind_neighbor(data, hex, wind_dir) {
                let elev_upwind = data.elevation_mean[upwind.0 as usize];
                if elev_upwind > elev + 1000.0 {
                    rain_shadow_sum += f64::from(p);
                    rain_shadow_count += 1;
                }
            }
        }

        let mean_land = sum_land / land_count as f64;
        let mean_tropical = if count_tropical > 0 {
            sum_tropical / count_tropical as f64
        } else {
            0.0
        };
        let mean_subtropical = if count_subtropical > 0 {
            sum_subtropical / count_subtropical as f64
        } else {
            0.0
        };
        let mean_temperate = if count_temperate > 0 {
            sum_temperate / count_temperate as f64
        } else {
            0.0
        };
        let mean_polar = if count_polar > 0 {
            sum_polar / count_polar as f64
        } else {
            0.0
        };
        let mean_rain_shadow = if rain_shadow_count > 0 {
            rain_shadow_sum / rain_shadow_count as f64
        } else {
            0.0
        };

        eprintln!("=== precipitation at 1B years (subdiv=7) ===");
        eprintln!("land_hex_count: {land_count}");
        eprintln!("min_precip_mm: {min_precip}");
        eprintln!("max_precip_mm: {max_precip}");
        eprintln!("mean_land_precip_mm: {mean_land}");
        eprintln!("mean_tropical_mm: {mean_tropical}");
        eprintln!("mean_subtropical_mm: {mean_subtropical}");
        eprintln!("mean_temperate_mm: {mean_temperate}");
        eprintln!("mean_polar_mm: {mean_polar}");
        eprintln!("hexes_lt_250mm: {desert_count}");
        eprintln!("hexes_gt_1500mm: {wet_count}");
        eprintln!("rain_shadow_hex_count: {rain_shadow_count}");
        eprintln!("mean_rain_shadow_precip_mm: {mean_rain_shadow}");
        eprintln!("mean_all_land_precip_mm: {mean_land}");

        assert!(min_precip >= 0.0);
        assert!(max_precip <= 12000.0);
        assert!(land_count > 0);
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
    }
}
