//! [`SimulationLayer`] integration for hydrology.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use genesis_core::data::WorldData;
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

use crate::budget::{
    FORMATION_END_YEAR, WaterBudget, condensed_fraction_at_year, groundwater_capacity_m3,
    inventory_volume_m3, relax_groundwater,
};
use crate::events::maybe_emit_formation_ocean_events;
use crate::solve::solve_flooding;
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
        let mut state = self.state.borrow_mut();
        let year = world.current_year;
        let interval_years = (year - self.last_tick_year.get()) as f64;
        let skip_formation = world.parameters.core.climate.skip_planetary_formation;
        let event_granularity = world.parameters.core.climate.event_granularity;

        // §3.3 Formation condensation: piecewise-constant per stage.
        let condensed_fraction = condensed_fraction_at_year(year.value(), skip_formation);

        // §3.3 groundwater: simple relaxation toward capacity once condensed
        // water exists (aridity-equilibrium refinement is Slice 2's §6).
        if condensed_fraction > 0.0 {
            state.groundwater_storage_m3 = relax_groundwater(
                state.groundwater_storage_m3,
                groundwater_capacity_m3(&world.parameters),
                interval_years,
            );
        }

        // §3.2 partition: the ocean term is the remainder, so the accounting
        // identity holds by construction.
        let budget = WaterBudget::partition(
            inventory_volume_m3(&world.parameters),
            condensed_fraction,
            state.prev_lake_volume_m3,
            state.ice_volume_m3,
            state.groundwater_storage_m3,
        );
        state.atmosphere_reserve_m3 = budget.atmosphere_reserve_m3;

        // §3.4 flooding solve: writes sea_level_m, water_level_m,
        // water_body_id, and the water_bodies registry.
        let outcome = solve_flooding(world, budget.ocean_volume_m3);

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

        self.last_tick_year.set(year);
        Vec::new()
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
        world.data.current_year = WorldYear(300_000_000);

        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        let hydrology = HydrologyLayer::detach_state(shared);

        // Condensation stage: 90% of the 1000 GEL inventory minus groundwater
        // stands in the basin; basin cells wet, plateau cells dry.
        assert!(world.data.sea_level_m > -2000.0 && world.data.sea_level_m < 1000.0);
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
        assert!(hydrology.groundwater_storage_m3 > 0.0);
        assert!(hydrology.oceans_begin_emitted);
        assert!(!hydrology.oceans_stabilized_emitted);
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
}
