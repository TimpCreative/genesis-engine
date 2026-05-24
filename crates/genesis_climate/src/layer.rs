//! [`SimulationLayer`] integration for climate.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use genesis_core::data::WorldData;
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

use crate::events::{emit_phase_transition_event, maybe_emit_cooling_milestone};
use crate::formation::{composition_at_year, cooling_temperature_c, sea_level_at_year};
use crate::state::{ClimateState, FormationSubPhase, formation_period_active};

/// Default Geological-era climate tick interval (Doc 07 §2.2).
pub const DEFAULT_GEOLOGICAL_CLIMATE_TICK_YEARS: i64 = 500_000;
/// Default Prehistoric-era climate tick interval (Doc 07 §2.2).
pub const DEFAULT_PREHISTORIC_CLIMATE_TICK_YEARS: i64 = 500_000;
/// Default Ancient-era climate tick interval (Doc 07 §2.2).
pub const DEFAULT_ANCIENT_CLIMATE_TICK_YEARS: i64 = 100_000;
/// Default Recent-era climate tick interval (Doc 07 §2.2).
pub const DEFAULT_RECENT_CLIMATE_TICK_YEARS: i64 = 1_000;
/// Default Formation-era climate tick interval (Doc 07 §3.2).
pub const DEFAULT_FORMATION_CLIMATE_TICK_YEARS: i64 = 5_000_000;

/// Climate simulation layer (Doc 07).
pub struct ClimateLayer {
    state: Rc<RefCell<ClimateState>>,
    last_tick_year: Cell<WorldYear>,
}

impl ClimateLayer {
    /// Creates a layer sharing `state` with the caller via `Rc`.
    pub fn attach(state: &mut ClimateState) -> (Self, Rc<RefCell<ClimateState>>) {
        let shared = Rc::new(RefCell::new(std::mem::take(state)));
        let layer = Self {
            state: Rc::clone(&shared),
            last_tick_year: Cell::new(WorldYear::FORMATION),
        };
        (layer, shared)
    }

    /// Recovers owned state from a shared handle after tick simulation.
    pub fn detach_state(shared: Rc<RefCell<ClimateState>>) -> ClimateState {
        Rc::try_unwrap(shared)
            .expect("climate state still borrowed")
            .into_inner()
    }
}

impl SimulationLayer for ClimateLayer {
    fn name(&self) -> &str {
        "climate"
    }

    fn tick_interval(&self, current_time: WorldYear, params: &WorldParameters) -> i64 {
        if formation_period_active(current_time.value(), params) {
            return DEFAULT_FORMATION_CLIMATE_TICK_YEARS;
        }

        let era = Era::for_year(current_time, params);
        match era {
            Era::Formation => DEFAULT_FORMATION_CLIMATE_TICK_YEARS,
            Era::Geological => DEFAULT_GEOLOGICAL_CLIMATE_TICK_YEARS,
            Era::Prehistoric => DEFAULT_PREHISTORIC_CLIMATE_TICK_YEARS,
            Era::Ancient => DEFAULT_ANCIENT_CLIMATE_TICK_YEARS,
            Era::Recent => DEFAULT_RECENT_CLIMATE_TICK_YEARS,
        }
    }

    fn advance(&mut self, world: &mut WorldData, _rng: &WorldRng) -> Vec<()> {
        {
            let mut state = self.state.borrow_mut();
            let params = &world.parameters;
            let current_year_value = world.current_year.value();

            if params.core.climate.skip_planetary_formation {
                if !state.formation_complete {
                    state.formation_complete = true;
                    state.formation_sub_phase = FormationSubPhase::Complete;
                }
            } else if formation_period_active(current_year_value, params)
                && !state.formation_complete
            {
                let new_phase = FormationSubPhase::for_year(current_year_value);
                let prev_phase = state.formation_sub_phase;

                world.global_temperature_c = cooling_temperature_c(current_year_value);
                world.sea_level_m = sea_level_at_year(current_year_value);
                state.atmospheric_composition = composition_at_year(current_year_value);

                if new_phase != prev_phase {
                    emit_phase_transition_event(
                        &mut state,
                        world,
                        prev_phase,
                        new_phase,
                        world.current_year,
                        params.core.climate.event_granularity,
                    );
                    state.formation_sub_phase = new_phase;
                }

                if new_phase == FormationSubPhase::Complete {
                    state.formation_complete = true;
                }

                maybe_emit_cooling_milestone(
                    &mut state,
                    world.global_temperature_c,
                    world.current_year,
                    params.core.climate.event_granularity,
                );
            }
        }

        let dist_start = std::time::Instant::now();
        crate::ocean_distance::compute_distance_to_ocean(world);
        let dist_elapsed = dist_start.elapsed();
        if dist_elapsed.as_millis() > 50 {
            eprintln!(
                "[climate] ocean_distance tick at year {} took {}ms",
                world.current_year.value(),
                dist_elapsed.as_millis()
            );
        }

        {
            let mut state = self.state.borrow_mut();
            state.circulation_cells = crate::circulation::compute_circulation(world);

            let wind_start = std::time::Instant::now();
            crate::wind::compute_wind_field(world, &state.circulation_cells);
            let wind_elapsed = wind_start.elapsed();
            if wind_elapsed.as_millis() > 50 {
                eprintln!(
                    "[climate] wind tick at year {} took {}ms",
                    world.current_year.value(),
                    wind_elapsed.as_millis()
                );
            }

            let basins_start = std::time::Instant::now();
            state.ocean_basins = crate::ocean_basins::identify_ocean_basins(world);
            let basins_elapsed = basins_start.elapsed();
            if basins_elapsed.as_millis() > 50 {
                eprintln!(
                    "[climate] ocean_basins tick at year {} took {}ms",
                    world.current_year.value(),
                    basins_elapsed.as_millis()
                );
            }

            let currents_start = std::time::Instant::now();
            crate::ocean_currents::compute_ocean_currents(world, &state.ocean_basins);
            let currents_elapsed = currents_start.elapsed();
            if currents_elapsed.as_millis() > 100 {
                eprintln!(
                    "[climate] ocean_currents tick at year {} took {}ms",
                    world.current_year.value(),
                    currents_elapsed.as_millis()
                );
            }

            let temp_start = std::time::Instant::now();
            crate::temperature::compute_temperature_field(world, &state);
            let temp_elapsed = temp_start.elapsed();
            if temp_elapsed.as_millis() > 50 {
                eprintln!(
                    "[climate] temperature tick at year {} took {}ms",
                    world.current_year.value(),
                    temp_elapsed.as_millis()
                );
            }

            let precip_start = std::time::Instant::now();
            crate::precipitation::compute_precipitation_field(world, &state);
            let precip_elapsed = precip_start.elapsed();
            if precip_elapsed.as_millis() > 100 {
                eprintln!(
                    "[climate] precipitation tick at year {} took {}ms",
                    world.current_year.value(),
                    precip_elapsed.as_millis()
                );
            }

            let era = Era::for_year(world.current_year, &world.parameters);
            if state.formation_complete && !state.circulation_logged_once && era != Era::Formation {
                eprintln!(
                    "[climate] circulation: {} cells per hemisphere ({}h rotation), gradient ~{}°C",
                    state.circulation_cells.cells_per_hemisphere,
                    world.parameters.core.planet.rotation_period_hours,
                    state.circulation_cells.equator_pole_temp_diff_c,
                );
                state.circulation_logged_once = true;
            }
        }

        self.last_tick_year.set(world.current_year);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::TickCoordinator;
    use genesis_core::{HexId, WorldYear, create_world};

    #[test]
    fn climate_layer_ticks_at_formation_interval_during_formation_period() {
        let state = ClimateState::default();
        let params = WorldParameters::default();

        let mut state_owned = state;
        let (layer, _shared) = ClimateLayer::attach(&mut state_owned);

        let interval = layer.tick_interval(WorldYear(100), &params);
        assert_eq!(interval, DEFAULT_FORMATION_CLIMATE_TICK_YEARS);
    }

    #[test]
    fn climate_layer_ticks_at_geological_interval_after_formation() {
        let state = ClimateState::default();
        let params = WorldParameters::default();

        let mut state_owned = state;
        let (layer, _shared) = ClimateLayer::attach(&mut state_owned);

        let interval = layer.tick_interval(WorldYear(600_000_000), &params);
        assert_eq!(interval, DEFAULT_GEOLOGICAL_CLIMATE_TICK_YEARS);
    }

    #[test]
    fn formation_completes_by_end_of_formation_era() {
        // Climate-only coordinator: fast, no tectonic ticks to 500M.
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        let mut climate = ClimateState::new();

        let (layer, shared) = ClimateLayer::attach(&mut climate);
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(layer));

        let params = world.data.parameters.clone();
        coordinator.advance_to(WorldYear(500_000_000), &mut world.data, &world.rng, &params);
        drop(coordinator);

        let climate = ClimateLayer::detach_state(shared);

        assert!(climate.formation_complete);
        assert_eq!(climate.formation_sub_phase, FormationSubPhase::Complete);
        assert!(
            (world.data.sea_level_m - 0.0).abs() < 50.0,
            "sea level should be near modern; got {}",
            world.data.sea_level_m
        );
        assert!(
            (world.data.global_temperature_c - 15.0).abs() < 20.0,
            "temperature should be near equilibrium; got {}",
            world.data.global_temperature_c
        );
    }

    #[test]
    fn circulation_cells_computed_after_formation() {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        let mut climate = ClimateState::new();

        let (layer, shared) = ClimateLayer::attach(&mut climate);
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(layer));

        let params = world.data.parameters.clone();
        coordinator.advance_to(WorldYear(500_000_000), &mut world.data, &world.rng, &params);
        drop(coordinator);

        let mut climate = ClimateLayer::detach_state(shared);

        assert!(climate.formation_complete);
        assert_eq!(climate.circulation_cells.cells_per_hemisphere, 3);
        assert_eq!(climate.circulation_cells.cells.len(), 3);
        assert!(
            climate.circulation_cells.equator_pole_temp_diff_c >= 40.0,
            "gradient should be strong after cooling; got {}",
            climate.circulation_cells.equator_pole_temp_diff_c
        );

        let intensity_at_500m = climate.circulation_cells.cells[0].intensity;
        let gradient_at_500m = climate.circulation_cells.equator_pole_temp_diff_c;

        world.data.current_year = WorldYear(1_000_000_000);
        let (mut layer, shared) = ClimateLayer::attach(&mut climate);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        climate = ClimateLayer::detach_state(shared);

        assert!(
            climate.circulation_cells.equator_pole_temp_diff_c >= gradient_at_500m,
            "gradient at 1B should be >= 500M"
        );
        assert!(
            climate.circulation_cells.cells[0].intensity >= intensity_at_500m,
            "intensity at 1B should be >= 500M"
        );
    }

    #[test]
    fn wind_field_populated_after_formation() {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        let mut climate = ClimateState::new();

        let (layer, shared) = ClimateLayer::attach(&mut climate);
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(layer));

        let params = world.data.parameters.clone();
        coordinator.advance_to(WorldYear(500_000_000), &mut world.data, &world.rng, &params);
        drop(coordinator);

        let _climate = ClimateLayer::detach_state(shared);

        let speeds = &world.data.wind_speed_m_s;
        assert!(
            speeds.iter().any(|&s| s > 0.0),
            "expected some non-zero wind speeds after formation"
        );
        assert!(
            speeds.iter().all(|&s| s >= 0.0 && s < 30.0),
            "wind speeds should be in [0, 30) m/s"
        );

        let distinct_directions: std::collections::BTreeSet<i32> = world
            .data
            .wind_direction_rad
            .iter()
            .filter(|&&d| d > 0.0)
            .map(|&d| (d * 100.0).round() as i32)
            .collect();
        assert!(
            distinct_directions.len() >= 4,
            "expected at least 4 distinct wind directions, got {}",
            distinct_directions.len()
        );
    }

    #[test]
    fn temperature_field_populated_with_reasonable_values() {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        let mut climate = ClimateState::new();

        let (layer, shared) = ClimateLayer::attach(&mut climate);
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(layer));

        let params = world.data.parameters.clone();
        coordinator.advance_to(WorldYear(500_000_000), &mut world.data, &world.rng, &params);
        drop(coordinator);

        let _climate = ClimateLayer::detach_state(shared);

        for &t in &world.data.temperature_mean {
            assert!(
                t >= -60.0 && t <= 50.0,
                "temperature {t}°C out of expected [-60, 50] range"
            );
        }

        let mut equator_temp = None;
        let mut polar_temp = None;
        for i in 0..world.data.cell_count() as usize {
            let (lat, _) = world.data.grid.center_lat_lon(HexId(i as u32));
            let abs_lat_deg = lat.abs().to_degrees();
            if abs_lat_deg < 5.0 && equator_temp.is_none() {
                equator_temp = Some(world.data.temperature_mean[i]);
            }
            if abs_lat_deg > 70.0 && polar_temp.is_none() {
                polar_temp = Some(world.data.temperature_mean[i]);
            }
        }

        let eq = equator_temp.expect("found equator hex");
        let pole = polar_temp.expect("found polar hex");
        assert!(
            eq > pole,
            "equator ({eq}°C) should be warmer than pole ({pole}°C)"
        );
    }

    #[test]
    fn ocean_basins_identified_after_formation() {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        let mut climate = ClimateState::new();

        let (layer, shared) = ClimateLayer::attach(&mut climate);
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(layer));

        let params = world.data.parameters.clone();
        coordinator.advance_to(WorldYear(500_000_000), &mut world.data, &world.rng, &params);
        drop(coordinator);

        let mut climate = ClimateLayer::detach_state(shared);

        // `create_world` leaves elevation at 0; at sea level 0 there is no ocean until
        // tectonics shapes terrain. Sculpt mixed ocean/land to verify basin wiring.
        world.data.sea_level_m = 0.0;
        for (i, elev) in world.data.elevation_mean.iter_mut().enumerate() {
            *elev = if i % 4 == 0 { 200.0 } else { -100.0 };
        }

        let (mut layer, shared) = ClimateLayer::attach(&mut climate);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        climate = ClimateLayer::detach_state(shared);

        let basin_count = climate.ocean_basins.basins.len();
        assert!(
            basin_count >= 1,
            "expected at least one basin, got {basin_count}"
        );

        let mut ocean_hexes = 0_u64;
        for (i, &elev) in world.data.elevation_mean.iter().enumerate() {
            if elev < world.data.sea_level_m {
                ocean_hexes += 1;
                assert_ne!(
                    world.data.basin_id[i],
                    genesis_core::BasinId::NONE,
                    "ocean hex {i} missing basin_id"
                );
            } else {
                assert_eq!(
                    world.data.basin_id[i],
                    genesis_core::BasinId::NONE,
                    "land hex {i} should be NONE"
                );
            }
        }

        assert!(ocean_hexes > 0, "expected some ocean hexes");
        if !climate.ocean_basins.basins.is_empty() {
            let largest = climate.ocean_basins.basins[0].hex_count;
            assert!(
                f64::from(largest) >= f64::from(ocean_hexes as u32) * 0.3,
                "largest basin {largest} should cover >=30% of {ocean_hexes} ocean hexes"
            );
        }
    }

    #[test]
    fn ocean_currents_produce_gyres_in_large_basins() {
        use crate::ocean_currents::MAX_CURRENT_SPEED_M_S;

        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        let mut climate = ClimateState::new();

        let (layer, shared) = ClimateLayer::attach(&mut climate);
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(layer));

        let params = world.data.parameters.clone();
        coordinator.advance_to(WorldYear(500_000_000), &mut world.data, &world.rng, &params);
        drop(coordinator);

        let mut climate = ClimateLayer::detach_state(shared);

        world.data.sea_level_m = 0.0;
        for elev in world.data.elevation_mean.iter_mut() {
            *elev = -100.0;
        }

        let (mut layer, shared) = ClimateLayer::attach(&mut climate);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        climate = ClimateLayer::detach_state(shared);

        let largest_basin = climate
            .ocean_basins
            .basins
            .first()
            .expect("expected at least one basin");
        assert!(
            largest_basin.hex_count >= crate::ocean_currents::MIN_BASIN_SIZE_FOR_GYRE,
            "largest basin {} hexes should qualify for gyre",
            largest_basin.hex_count
        );

        let sea = world.data.sea_level_m;
        let mut basin_fast = 0_u32;
        let mut basin_total = 0_u32;

        for i in 0..world.data.cell_count() as usize {
            if world.data.elevation_mean[i] >= sea {
                continue;
            }

            let [e, n] = world.data.ocean_current_vec[i];
            assert!(
                e.abs() <= MAX_CURRENT_SPEED_M_S && n.abs() <= MAX_CURRENT_SPEED_M_S,
                "current [{e}, {n}] exceeds clamp"
            );
            assert!(e >= -MAX_CURRENT_SPEED_M_S && n >= -MAX_CURRENT_SPEED_M_S);

            if world.data.basin_id[i] == largest_basin.id {
                basin_total += 1;
                let speed = (e * e + n * n).sqrt();
                if speed > 0.1 {
                    basin_fast += 1;
                }
            }
        }

        assert!(basin_total > 0, "expected ocean hexes in largest basin");
        let fast_fraction = f64::from(basin_fast) / f64::from(basin_total);
        assert!(
            fast_fraction > 0.5,
            "expected majority of largest-basin hexes with speed > 0.1 m/s; got {basin_fast}/{basin_total} ({fast_fraction:.1})"
        );
    }

    #[test]
    fn precipitation_field_has_realistic_distribution() {
        use crate::precipitation::{PRECIPITATION_MAX_MM, PRECIPITATION_MIN_MM};

        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        let mut climate = ClimateState::new();

        // Mixed ocean/land with relief so orographic and coastal modifiers can produce
        // wet and dry extremes (flat elevation=0 worlds cap base precip below 1500 mm).
        world.data.sea_level_m = 0.0;
        for i in 0..world.data.cell_count() as usize {
            let hex = HexId(i as u32);
            let (lat, _) = world.data.grid.center_lat_lon(hex);
            let abs_lat_deg = lat.abs().to_degrees();
            world.data.elevation_mean[i] = if abs_lat_deg < 25.0 {
                match i % 11 {
                    0 => 4000.0,
                    1 | 2 => 800.0,
                    3..=5 => -500.0,
                    _ => 200.0,
                }
            } else if abs_lat_deg < 40.0 {
                if i % 4 == 0 { 1500.0 } else { -200.0 }
            } else if i % 3 == 0 {
                500.0
            } else {
                -300.0
            };
        }

        let (layer, shared) = ClimateLayer::attach(&mut climate);
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(layer));

        let params = world.data.parameters.clone();
        coordinator.advance_to(
            WorldYear(1_000_000_000),
            &mut world.data,
            &world.rng,
            &params,
        );
        drop(coordinator);

        let _climate = ClimateLayer::detach_state(shared);

        let sea = world.data.sea_level_m;
        let mut land_count = 0_u64;
        let mut desert_count = 0_u64;
        let mut wet_count = 0_u64;
        let mut sum_tropical = 0.0_f64;
        let mut count_tropical = 0_u64;
        let mut sum_subtropical = 0.0_f64;
        let mut count_subtropical = 0_u64;

        for i in 0..world.data.cell_count() as usize {
            let elev = world.data.elevation_mean[i];
            if elev < sea {
                continue;
            }

            land_count += 1;
            let p = world.data.precipitation[i];
            assert!(
                (PRECIPITATION_MIN_MM..=PRECIPITATION_MAX_MM).contains(&p),
                "precipitation {p} out of range"
            );

            if p < 250.0 {
                desert_count += 1;
            }
            if p > 1500.0 {
                wet_count += 1;
            }

            let (lat, _) = world.data.grid.center_lat_lon(HexId(i as u32));
            let abs_lat_deg = lat.abs().to_degrees();
            if abs_lat_deg < 23.0 {
                sum_tropical += f64::from(p);
                count_tropical += 1;
            } else if (23.0..40.0).contains(&abs_lat_deg) {
                sum_subtropical += f64::from(p);
                count_subtropical += 1;
            }
        }

        assert!(land_count > 0, "expected land hexes");
        let desert_fraction = desert_count as f64 / land_count as f64;
        let wet_fraction = wet_count as f64 / land_count as f64;
        assert!(
            desert_fraction >= 0.10,
            "expected >=10% desert hexes (<250mm), got {desert_fraction:.1} ({desert_count}/{land_count})"
        );
        assert!(
            wet_fraction >= 0.10,
            "expected >=10% wet hexes (>1500mm), got {wet_fraction:.1} ({wet_count}/{land_count})"
        );

        let mean_tropical = sum_tropical / count_tropical as f64;
        let mean_subtropical = sum_subtropical / count_subtropical as f64;
        assert!(
            count_tropical > 0 && count_subtropical > 0,
            "expected tropical and subtropical land hexes"
        );
        assert!(
            mean_tropical > mean_subtropical,
            "tropical mean {mean_tropical} should exceed subtropical {mean_subtropical}"
        );
    }
}
