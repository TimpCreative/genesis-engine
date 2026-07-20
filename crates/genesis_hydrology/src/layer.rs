//! [`SimulationLayer`] integration for hydrology.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use genesis_core::data::{HydroFlags, WorldData};
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

use crate::budget::{
    FORMATION_END_YEAR, WaterBudget, condensed_fraction_at_year, inventory_volume_m3,
};
use crate::coastal::update_coastal;
use crate::erosion::apply_erosion;
use crate::events::{
    maybe_emit_flag_events, maybe_emit_formation_ocean_events, maybe_emit_glacial_maximum,
    maybe_emit_registry_events, maybe_emit_river_course_shift, maybe_emit_sea_level_milestone,
};
use crate::groundwater::{recharge_and_baseflow, total_groundwater_storage_m3, water_tables};
use crate::ice::update_ice;
use crate::lakes::{
    adjudicate_lakes, apply_returned_surplus, export_salt, registry_lake_volume_m3,
};
use crate::partition::partition_land;
use crate::regime::classify_regimes;
use crate::routing::{FlowAccumulation, RoutingSurface, hex_area_m2};
use crate::soil::update_soil;
use crate::solve::{
    global_mean_temperature_c, sea_level_for_land_fraction, solve_flooding,
    thermosteric_effective_volume_m3, volume_to_fill_to_level_m3,
};
use crate::state::HydrologyState;

/// Default Geological-era hydrology tick interval (Doc 08 §2.1 — matches climate).
pub const DEFAULT_GEOLOGICAL_HYDROLOGY_TICK_YEARS: i64 = 500_000;
/// Default Prehistoric-era hydrology tick interval (Doc 08 §2.1).
pub const DEFAULT_PREHISTORIC_HYDROLOGY_TICK_YEARS: i64 = 500_000;
/// Default Ancient-era hydrology tick interval (Doc 08 §2.1).
pub const DEFAULT_ANCIENT_HYDROLOGY_TICK_YEARS: i64 = 100_000;
/// Default Recent-era hydrology tick interval (Doc 08 §2.1).
pub const DEFAULT_RECENT_HYDROLOGY_TICK_YEARS: i64 = 1_000;
/// Default Formation-era hydrology tick interval (Doc 08 §2.1).
pub const DEFAULT_FORMATION_HYDROLOGY_TICK_YEARS: i64 = 5_000_000;

/// Hydrology simulation layer (Doc 08). Registers after climate.
pub struct HydrologyLayer {
    state: Rc<RefCell<HydrologyState>>,
    last_tick_year: Cell<WorldYear>,
}

impl HydrologyLayer {
    /// Creates a layer sharing `state` with the caller via `Rc`.
    pub fn attach(state: &mut HydrologyState) -> (Self, Rc<RefCell<HydrologyState>>) {
        let shared = Rc::new(RefCell::new(std::mem::take(state)));
        let layer = Self {
            state: Rc::clone(&shared),
            last_tick_year: Cell::new(WorldYear::FORMATION),
        };
        (layer, shared)
    }

    /// Recovers owned state from a shared handle after tick simulation.
    pub fn detach_state(shared: Rc<RefCell<HydrologyState>>) -> HydrologyState {
        Rc::try_unwrap(shared)
            .expect("hydrology state still borrowed")
            .into_inner()
    }
}

/// Returns true while the climate formation period is active (Doc 07 §3) —
/// hydrology matches climate's cadence (Doc 08 §2.1), and condensation is
/// staged over the same period. Local copy of the climate helper: the crate
/// depends on `genesis_core` only.
fn formation_period_active(year: i64, params: &WorldParameters) -> bool {
    !params.core.climate.skip_planetary_formation && year <= FORMATION_END_YEAR
}

impl HydrologyLayer {
    /// Zeroes the drainage-network derivations for an inactive tick (§2.1:
    /// full activation requires standing water and nonzero precipitation;
    /// gate §15 #4 — zero rivers before oceans).
    fn clear_drainage_fields(world: &mut WorldData) {
        let n = world.cell_count() as usize;
        for i in 0..n {
            world.flow_direction[i] = None;
            world.river_discharge_m3_yr[i] = 0.0;
            world.discharge_seasonality[i] = 1.0;
            world.hydro_flags[i] = HydroFlags::NONE;
            if world.water_body_id[i] == genesis_core::WaterBodyId::NONE {
                world.water_table_depth_m[i] = crate::groundwater::WATER_TABLE_ARID_OFFSET_M as f32;
            } else {
                world.water_table_depth_m[i] = 0.0;
            }
        }
    }
}

impl SimulationLayer for HydrologyLayer {
    fn name(&self) -> &str {
        "hydrology"
    }

    fn tick_interval(&self, current_time: WorldYear, params: &WorldParameters) -> i64 {
        if formation_period_active(current_time.value(), params) {
            return DEFAULT_FORMATION_HYDROLOGY_TICK_YEARS;
        }

        let era = Era::for_year(current_time, params);
        match era {
            Era::Formation => DEFAULT_FORMATION_HYDROLOGY_TICK_YEARS,
            Era::Geological => DEFAULT_GEOLOGICAL_HYDROLOGY_TICK_YEARS,
            Era::Prehistoric => DEFAULT_PREHISTORIC_HYDROLOGY_TICK_YEARS,
            Era::Ancient => DEFAULT_ANCIENT_HYDROLOGY_TICK_YEARS,
            Era::Recent => DEFAULT_RECENT_HYDROLOGY_TICK_YEARS,
        }
    }

    fn advance(&mut self, world: &mut WorldData, _rng: &WorldRng) -> Vec<()> {
        let tick_start = std::time::Instant::now();
        let mut state = self.state.borrow_mut();
        let year = world.current_year;
        let interval_years = (year - self.last_tick_year.get()) as f64;
        let skip_formation = world.parameters.core.climate.skip_planetary_formation;
        let event_granularity = world.parameters.core.climate.event_granularity;

        // §2.2 step 1 — inventory update: condensation fraction (§3.3) and
        // the budget partition (§3.2); the ocean term is the remainder, so
        // the accounting identity holds by construction.
        let condensed_fraction = condensed_fraction_at_year(year.value(), skip_formation);
        let budget = WaterBudget::partition(
            inventory_volume_m3(&world.parameters),
            condensed_fraction,
            state.prev_lake_volume_m3,
            state.ice_volume_m3,
            state.groundwater_storage_m3,
        );
        state.atmosphere_reserve_m3 = budget.atmosphere_reserve_m3;

        // §2.2 step 2 — flooding solve (§3.4): sea level, the ocean mask,
        // and the candidate-sea list (written by §5.2, not here).
        //
        // Doc 06-CAL datum pin: when the calibration layer is active the land/ocean
        // line is the calibrated terrain's sea level = 0, not the GEL budget, so
        // flood to the volume that fills everything below 0. GEL still drives the
        // budget partition (ice / groundwater / atmosphere) above.
        let ocean_volume_m3 = if world.parameters.core.terrain.enabled {
            // Datum pin (Doc 06-CAL): solve sea level as the (1 − land_fraction)
            // elevation quantile, so the land fraction is exactly the target,
            // independent of the GEL budget, thermosteric drift, or which era
            // last calibrated the terrain. During condensation the level rises
            // from the deepest cell up to that quantile, so oceans form on a
            // timeline. Fed to the bathtub solve as the equivalent volume, with
            // thermosteric expansion divided out so the solved level is exact
            // (GEL still drives the ice/groundwater/atmosphere partition;
            // deliberate bounded eustasy returns in Phase 2).
            let land = f64::from(world.parameters.core.terrain.land_fraction);
            let target_level = sea_level_for_land_fraction(world, land);
            let deepest = world
                .elevation_mean
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min);
            let level =
                f64::from(deepest) + (target_level - f64::from(deepest)) * condensed_fraction;
            let vol = volume_to_fill_to_level_m3(world, level);
            let thermo = thermosteric_effective_volume_m3(1.0, global_mean_temperature_c(world));
            if thermo > 0.0 { vol / thermo } else { vol }
        } else {
            budget.ocean_volume_m3
        };
        let outcome = solve_flooding(world, ocean_volume_m3);

        debug_assert!(
            budget.is_conserved(),
            "Doc 08 §3.2 conservation violated: error {} m³",
            budget.conservation_error_m3()
        );

        // Skip-formation worlds start with oceans present; the formation
        // narrative is suppressed (matches climate's skip behavior).
        if !state.formation_events_initialized {
            if skip_formation {
                state.oceans_begin_emitted = true;
                state.oceans_stabilized_emitted = true;
            }
            state.formation_events_initialized = true;
        }
        maybe_emit_formation_ocean_events(
            &mut state,
            condensed_fraction,
            outcome.wet_cell_count,
            outcome.sea_level_m as f32,
            year,
            event_granularity,
        );
        maybe_emit_sea_level_milestone(
            &mut state,
            outcome.sea_level_m as f32,
            year,
            event_granularity,
        );

        // §2.1: full activation requires standing water and precipitation.
        let active = outcome.wet_cell_count > 0 && world.precipitation.iter().any(|&p| p > 0.0);
        if !active {
            Self::clear_drainage_fields(world);
            state.prev_lake_volume_m3 = registry_lake_volume_m3(world);
            state.groundwater_storage_m3 =
                total_groundwater_storage_m3(world, &state.aquifer_storage_m);
            self.last_tick_year.set(year);
            log_slow_hydrology_tick(year, tick_start);
            return Vec::new();
        }

        // §2.2 step 3 — drainage network (§4): routing surface (with the
        // depression tree), flow directions, and the water partition.
        // Preserve persistent glacial landform flags across the per-tick clear.
        let n_cells = world.cell_count() as usize;
        let mut persistent = vec![HydroFlags::NONE; n_cells];
        for (i, slot) in persistent.iter_mut().enumerate() {
            let f = world.hydro_flags[i];
            if f.contains(HydroFlags::CARVED_TROUGH) {
                *slot |= HydroFlags::CARVED_TROUGH;
            }
            if f.contains(HydroFlags::FJORD) {
                *slot |= HydroFlags::FJORD;
            }
            if f.contains(HydroFlags::DELTA) {
                *slot |= HydroFlags::DELTA;
            }
        }
        world.hydro_flags.copy_from_slice(&persistent);
        let surface = RoutingSurface::build(world, &outcome.candidates);
        surface.write_flow_directions(world);
        let mut partition = partition_land(world, &surface);

        // §2.2 step 4 — groundwater (§6): recharge into the per-hex aquifer,
        // baseflow release, and the karst diversion (flags KARST from the
        // partition, SPRING at resurgences).
        let mut candidate_karst_inflow = vec![0.0; outcome.candidates.len()];
        let baseflow = recharge_and_baseflow(
            world,
            &surface,
            &mut state.aquifer_storage_m,
            &mut partition.runoff_m3_yr,
            &partition.recharge_m3_yr,
            interval_years,
            &mut candidate_karst_inflow,
        );

        // §4.3 accumulation: runoff + baseflow, descending filled order.
        let mut acc = FlowAccumulation::accumulate(
            world,
            &surface,
            &partition.runoff_m3_yr,
            &baseflow.baseflow_m3_yr,
            &partition.recharge_m3_yr,
        );
        for (extra, slot) in candidate_karst_inflow
            .iter()
            .zip(acc.candidate_inflow_m3_yr.iter_mut())
        {
            *slot += extra;
        }

        // §2.2 step 5 — lake balance (§5): depression adjudication bottom-up,
        // candidate seas, salt; surplus returns as the closed-form ΔL.
        let lake_outcome = adjudicate_lakes(
            world,
            &surface,
            &mut acc,
            &outcome.candidates,
            interval_years,
        );
        apply_returned_surplus(world, lake_outcome.returned_to_ocean_m3);
        export_salt(world);

        // Standing-water partition check: the solve's effective ocean volume
        // is exactly the ocean component plus the candidate bathtub volumes
        // (§3.4); after adjudication the ocean holds the returned surplus
        // and the candidates hold what they kept.
        debug_assert!(
            {
                // Check against the volume we actually flooded with: under the
                // Doc 06-CAL datum pin this is the volume-below-0, not the GEL budget
                // term; the invariant (accounted standing water == flooded
                // volume) holds either way.
                let effective = thermosteric_effective_volume_m3(
                    ocean_volume_m3,
                    global_mean_temperature_c(world),
                );
                let accounted = world
                    .water_bodies
                    .values()
                    .filter(|b| b.kind == genesis_core::data::WaterBodyKind::Ocean)
                    .map(|b| b.volume_km3 * 1.0e9)
                    .sum::<f64>()
                    + lake_outcome.candidate_kept_m3;
                effective <= 0.0
                    || (accounted - effective).abs()
                        <= crate::budget::CONSERVATION_TOLERANCE_REL * effective
            },
            "Doc 08 §3.4 standing-water partition violated"
        );

        // Write total annual discharge (§4.3: surface runoff + baseflow).
        for i in 0..world.cell_count() as usize {
            world.river_discharge_m3_yr[i] = if surface.is_water(world, i as u32) {
                0.0
            } else {
                acc.discharge_m3_yr[i] as f32
            };
        }

        // §6.4 water tables, springs, oases (needs perennial channels, which
        // need the accumulation); then §7 regimes, seasonality, ephemeral
        // and permafrost flags.
        water_tables(world, &surface, &acc.baseflow_m3_yr, &acc.recharge_m3_yr);
        classify_regimes(world, &surface, &acc);

        // §2.2 steps 7–10 — ice/carving, erosion, soil, coastal.
        let n = world.cell_count() as usize;
        if state.alluvium_depth_m.len() != n {
            state.alluvium_depth_m = vec![0.0; n];
        }
        if state.prev_ice_mask.len() != n {
            state.prev_ice_mask = vec![false; n];
        }
        world.hydro_elevation_delta_m.fill(0.0);
        let base_rate = world.parameters.core.geology.base_erosion_rate_per_year;
        let (ice_volume, glacial_load) = update_ice(
            world,
            &surface,
            &mut state.prev_ice_mask,
            base_rate,
            interval_years,
        );
        state.ice_volume_m3 = ice_volume;
        let _erosion = apply_erosion(
            world,
            &surface,
            &mut state.alluvium_depth_m,
            &glacial_load,
            interval_years,
        );
        update_soil(world, &surface, &state.alluvium_depth_m, interval_years);
        update_coastal(world, &surface, &state.alluvium_depth_m);

        // §2.2 step 11 — events (§13).
        maybe_emit_registry_events(&mut state, world, year, event_granularity);
        maybe_emit_flag_events(&mut state, world, year, event_granularity);
        maybe_emit_river_course_shift(&mut state, world, year, event_granularity);
        let planet_area = hex_area_m2(&world.grid) * n as f64;
        maybe_emit_glacial_maximum(&mut state, ice_volume, planet_area, year, event_granularity);

        // Close-out: the budget terms the next tick's solve debits (§3.4).
        state.prev_lake_volume_m3 = registry_lake_volume_m3(world);
        state.groundwater_storage_m3 =
            total_groundwater_storage_m3(world, &state.aquifer_storage_m);

        self.last_tick_year.set(year);
        log_slow_hydrology_tick(year, tick_start);
        Vec::new()
    }
}

/// Slow-step log threshold in ms; `GENESIS_SLOW_TICK_STEP_MS` overrides it
/// (Doc 08 §14 — same env var as tectonics).
fn slow_tick_threshold_ms() -> u128 {
    static THRESHOLD: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        std::env::var("GENESIS_SLOW_TICK_STEP_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50)
    })
}

fn log_slow_hydrology_tick(year: WorldYear, tick_start: std::time::Instant) {
    let elapsed = tick_start.elapsed();
    let threshold_ms = slow_tick_threshold_ms();
    if elapsed.as_millis() >= threshold_ms {
        eprintln!(
            "[hydrology] slow tick at year {} took {}ms (threshold {}ms)",
            year.value(),
            elapsed.as_millis(),
            threshold_ms
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::data::WATER_NONE;
    use genesis_core::time::TickCoordinator;
    use genesis_core::{HexGrid, HexId, create_world};

    const EARTH_RADIUS_KM: f64 = 6371.0;

    /// Grows a connected basin of `target` cells from hex 0 via BFS
    /// (adjacency-grown, so the basin is one component by construction).
    fn connected_basin(data: &WorldData, target: usize) -> Vec<bool> {
        let n = data.cell_count() as usize;
        let mut basin = vec![false; n];
        let mut queue = std::collections::VecDeque::new();
        basin[0] = true;
        queue.push_back(0_u32);
        let mut count = 1;
        while count < target {
            let cell = queue.pop_front().expect("grid is connected");
            for neighbor in data.grid.neighbors(HexId(cell)) {
                let j = neighbor.0 as usize;
                if !basin[j] {
                    basin[j] = true;
                    count += 1;
                    queue.push_back(neighbor.0);
                    if count >= target {
                        break;
                    }
                }
            }
        }
        basin
    }

    #[test]
    fn hydrology_layer_matches_climate_cadence() {
        let mut state = HydrologyState::new();
        let (layer, _shared) = HydrologyLayer::attach(&mut state);
        let params = WorldParameters::default();

        assert_eq!(
            layer.tick_interval(WorldYear(100_000_000), &params),
            DEFAULT_FORMATION_HYDROLOGY_TICK_YEARS
        );
        assert_eq!(
            layer.tick_interval(WorldYear(600_000_000), &params),
            DEFAULT_GEOLOGICAL_HYDROLOGY_TICK_YEARS
        );
        assert_eq!(
            layer.tick_interval(WorldYear(3_000_000_000), &params),
            DEFAULT_PREHISTORIC_HYDROLOGY_TICK_YEARS
        );
        assert_eq!(
            layer.tick_interval(WorldYear(4_490_000_000), &params),
            DEFAULT_ANCIENT_HYDROLOGY_TICK_YEARS
        );
        assert_eq!(
            layer.tick_interval(WorldYear(4_499_999_000), &params),
            DEFAULT_RECENT_HYDROLOGY_TICK_YEARS
        );
    }

    #[test]
    fn advance_derives_sea_level_and_ocean_mask() {
        let mut params = WorldParameters::default();
        params.core.hydrology.water_inventory_gel_m = 1000.0;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let basin = connected_basin(&world.data, n / 2);
        for (i, &in_basin) in basin.iter().enumerate() {
            world.data.elevation_mean[i] = if in_basin { -2000.0 } else { 1000.0 };
        }
        world.data.precipitation.fill(1200.0);
        world.data.temperature_mean.fill(10.0);
        world.data.current_year = WorldYear(300_000_000);

        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        let hydrology = HydrologyLayer::detach_state(shared);

        // Mid-ramp at 300 My: partial inventory stands in the basin minus
        // groundwater recharge; basin cells wet, plateau dry.
        assert!(world.data.sea_level_m > -2000.0 && world.data.sea_level_m < 1000.0);
        let frac = condensed_fraction_at_year(300_000_000, false);
        assert!(frac > 0.0 && frac < 1.0);
        assert_eq!(
            world.data.water_bodies.len(),
            1,
            "connected basin is one body"
        );
        for (i, &in_basin) in basin.iter().enumerate() {
            if in_basin {
                assert_eq!(world.data.water_level_m[i], world.data.sea_level_m);
                assert_ne!(world.data.water_body_id[i], genesis_core::WaterBodyId::NONE);
            } else {
                assert_eq!(world.data.water_level_m[i], WATER_NONE);
                assert_eq!(world.data.water_body_id[i], genesis_core::WaterBodyId::NONE);
            }
        }
        // §6.1: recharge banks per-hex aquifer storage; the global term is
        // its deterministic sum.
        assert!(hydrology.groundwater_storage_m3 > 0.0);
        assert_eq!(hydrology.aquifer_storage_m.len(), n);
        assert!(hydrology.oceans_begin_emitted);
        assert!(!hydrology.oceans_stabilized_emitted);
    }

    /// Doc 08 §3.3: dry at ~100 My; oceans present mid-ramp; full by Formation end.
    #[test]
    fn condensation_timeline_matches_temperature_gate() {
        assert_eq!(condensed_fraction_at_year(100_000_000, false), 0.0);

        let mut params = WorldParameters::default();
        params.core.hydrology.water_inventory_gel_m = 1000.0;
        params.core.grid.subdivision_level = 4;
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut data = WorldData::new(grid, params);
        let n = data.cell_count() as usize;
        let basin = connected_basin(&data, n / 2);
        for (i, &in_basin) in basin.iter().enumerate() {
            data.elevation_mean[i] = if in_basin { -2000.0 } else { 1000.0 };
        }
        data.precipitation.fill(1200.0);
        data.temperature_mean.fill(10.0);
        let rng = genesis_core::rng::WorldRng::from_effective_seed(1);

        data.current_year = WorldYear(100_000_000);
        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut data, &rng);
        assert!(
            data.water_bodies.is_empty(),
            "no standing water at 100 My while T is above onset"
        );

        data.current_year = WorldYear(300_000_000);
        layer.advance(&mut data, &rng);
        let mid = condensed_fraction_at_year(300_000_000, false);
        assert!(mid > 0.0 && mid < 1.0);
        assert!(
            !data.water_bodies.is_empty(),
            "oceans present mid Condensation-era ramp"
        );

        data.current_year = WorldYear(FORMATION_END_YEAR);
        layer.advance(&mut data, &rng);
        drop(layer);
        let hydrology = HydrologyLayer::detach_state(shared);
        assert_eq!(condensed_fraction_at_year(FORMATION_END_YEAR, false), 1.0);
        assert!(hydrology.oceans_begin_emitted);
        assert!(hydrology.oceans_stabilized_emitted);
    }

    #[test]
    fn formation_run_emits_ocean_events_once() {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let basin = connected_basin(&world.data, n / 2);
        for (i, &in_basin) in basin.iter().enumerate() {
            world.data.elevation_mean[i] = if in_basin { -2000.0 } else { 1000.0 };
        }

        let mut hydrology = HydrologyState::new();
        let (layer, shared) = HydrologyLayer::attach(&mut hydrology);
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(layer));

        let params = world.data.parameters.clone();
        coordinator.advance_to(WorldYear(500_000_000), &mut world.data, &world.rng, &params);
        drop(coordinator);
        let mut hydrology = HydrologyLayer::detach_state(shared);

        assert!(hydrology.oceans_begin_emitted);
        assert!(hydrology.oceans_stabilized_emitted);
        crate::events::flush_events_to_branch(&mut world, &mut hydrology);
        let ocean_events: Vec<_> = world
            .branch_tree
            .root()
            .event_log
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    genesis_core::events::EventKind::OceansBeginForming { .. }
                        | genesis_core::events::EventKind::OceansStabilized { .. }
                )
            })
            .collect();
        assert_eq!(
            ocean_events.len(),
            2,
            "both formation ocean events, once each"
        );
    }

    #[test]
    fn skip_formation_suppresses_ocean_events() {
        let mut params = WorldParameters::default();
        params.core.climate.skip_planetary_formation = true;
        // Legacy GEL-flood test on a uniform synthetic field: a flat −100 m
        // world has no land-fraction quantile to solve, so exercise the legacy
        // budget flood rather than the Doc 06-CAL datum solve.
        params.core.terrain.enabled = false;
        params.core.grid.subdivision_level = 4;
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut data = WorldData::new(grid, params);
        data.elevation_mean.fill(-100.0);
        data.current_year = WorldYear(1_000_000_000);
        let rng = genesis_core::rng::WorldRng::from_effective_seed(1);

        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut data, &rng);
        drop(layer);
        let hydrology = HydrologyLayer::detach_state(shared);

        assert!(hydrology.pending_events.is_empty());
        assert!(
            data.water_body_id
                .iter()
                .all(|&b| b != genesis_core::WaterBodyId::NONE)
        );
    }

    /// Gate §15 #4 (cheap form): zero rivers before oceans — a Molten-era
    /// world with precipitation set must still derive no drainage at all.
    #[test]
    fn no_rivers_before_oceans_form() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 4;
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut data = WorldData::new(grid, params);
        let n = data.cell_count() as usize;
        data.elevation_mean[0] = -500.0;
        for i in 1..n {
            data.elevation_mean[i] = 100.0 + (i % 50) as f32;
        }
        data.precipitation.fill(2000.0);
        data.temperature_mean.fill(15.0);
        data.current_year = WorldYear(10_000_000); // Hot Formation: fraction 0.
        let rng = genesis_core::rng::WorldRng::from_effective_seed(1);

        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut data, &rng);
        drop(layer);
        let _ = HydrologyLayer::detach_state(shared);

        assert!(data.water_bodies.is_empty(), "no standing water in Molten");
        assert!(data.flow_direction.iter().all(Option::is_none));
        assert!(data.river_discharge_m3_yr.iter().all(|&d| d == 0.0));
        assert!(data.hydro_flags.iter().all(|f| f.is_empty()));
    }

    /// Gate §15 #4 (cheap form): on an active world, every channel hex
    /// continues strictly downstream on the filled surface or terminates in
    /// water/a retained sink, and discharge never decreases along an edge.
    #[test]
    fn rivers_flow_downhill_and_discharge_grows() {
        let mut params = WorldParameters::default();
        params.core.hydrology.water_inventory_gel_m = 1000.0;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let basin = connected_basin(&world.data, n / 2);
        for (i, &in_basin) in basin.iter().enumerate() {
            // Small-relief plateau (< BASIN_MIN_DEPTH_M pits): the fill
            // routes every pit through and no lakes form, so the surface
            // rebuilt below matches the tick's exactly.
            world.data.elevation_mean[i] = if in_basin {
                -2000.0
            } else {
                500.0 + (i % 13) as f32 * 5.0
            };
        }
        world.data.precipitation.fill(2000.0);
        world.data.temperature_mean.fill(5.0);
        world.data.current_year = WorldYear(1_000_000_000);

        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        let _ = HydrologyLayer::detach_state(shared);

        let surface = RoutingSurface::build(&world.data, &[]);
        for i in 0..n {
            if world.data.water_body_id[i] != genesis_core::WaterBodyId::NONE {
                continue;
            }
            match world.data.flow_direction[i] {
                None => {
                    // Terminal sinks only: a retained basin bottom, or a
                    // dried candidate-sea floor below sea level (§5.2 — it
                    // re-floods as a candidate next solve).
                    assert!(
                        surface.depression_of[i] != crate::routing::NONE
                            || world.data.elevation_mean[i] < world.data.sea_level_m,
                        "hex {i}: no flow direction and no water and no depression"
                    );
                }
                Some(direction) => {
                    let target = world.data.grid.neighbors(HexId(i as u32))[direction.index()];
                    let j = target.0 as usize;
                    if world.data.water_body_id[j] == genesis_core::WaterBodyId::NONE {
                        assert!(
                            surface.filled_m[j] < surface.filled_m[i],
                            "hex {i}: flow must descend the filled surface"
                        );
                        assert!(
                            world.data.river_discharge_m3_yr[j]
                                >= world.data.river_discharge_m3_yr[i] * 0.999,
                            "hex {i} -> {j}: discharge must not drop ({} -> {})",
                            world.data.river_discharge_m3_yr[i],
                            world.data.river_discharge_m3_yr[j]
                        );
                    }
                }
            }
        }
    }
}
