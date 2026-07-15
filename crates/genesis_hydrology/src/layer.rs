//! Hydrology simulation layer: recomputes surface flow each tick.
//!
//! Registers after tectonics and climate so flow reflects this tick's terrain
//! and precipitation. Stateless: flow is fully derived from `WorldData`.

use genesis_core::data::WorldData;
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

use crate::flow::{compute_flow_accumulation, compute_flow_directions};

/// Hydrology ticks at the climate cadence per era (Doc 07 §2.2 pattern).
pub const DEFAULT_FORMATION_HYDROLOGY_TICK_YEARS: i64 = 5_000_000;
pub const DEFAULT_GEOLOGICAL_HYDROLOGY_TICK_YEARS: i64 = 500_000;
pub const DEFAULT_PREHISTORIC_HYDROLOGY_TICK_YEARS: i64 = 500_000;
pub const DEFAULT_ANCIENT_HYDROLOGY_TICK_YEARS: i64 = 100_000;
pub const DEFAULT_RECENT_HYDROLOGY_TICK_YEARS: i64 = 1_000;

/// Stateless flow-recomputation layer.
#[derive(Default)]
pub struct HydrologyLayer;

impl SimulationLayer for HydrologyLayer {
    fn name(&self) -> &str {
        "hydrology"
    }

    fn tick_interval(&self, current_time: WorldYear, params: &WorldParameters) -> i64 {
        match Era::for_year(current_time, params) {
            Era::Formation => DEFAULT_FORMATION_HYDROLOGY_TICK_YEARS,
            Era::Geological => DEFAULT_GEOLOGICAL_HYDROLOGY_TICK_YEARS,
            Era::Prehistoric => DEFAULT_PREHISTORIC_HYDROLOGY_TICK_YEARS,
            Era::Ancient => DEFAULT_ANCIENT_HYDROLOGY_TICK_YEARS,
            Era::Recent => DEFAULT_RECENT_HYDROLOGY_TICK_YEARS,
        }
    }

    fn advance(&mut self, world: &mut WorldData, _rng: &WorldRng) -> Vec<()> {
        compute_flow_directions(world);
        compute_flow_accumulation(world);
        crate::soil::compute_soil_fertility(world);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters as Params;
    use genesis_core::time::TickCoordinator;

    #[test]
    fn layer_populates_flow_fields() {
        let mut params = Params::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = ((i * 13) % 400) as f32 - 100.0;
        }
        world.data.sea_level_m = 0.0;

        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(HydrologyLayer));
        let p = world.data.parameters.clone();
        coordinator.advance_to(
            genesis_core::WorldYear(1_000_000),
            &mut world.data,
            &world.rng,
            &p,
        );

        let flowing = world
            .data
            .flow_direction
            .iter()
            .filter(|d| d.is_some())
            .count();
        assert!(flowing > 0, "some land hexes should drain somewhere");
        let max_volume = world
            .data
            .flow_volume
            .iter()
            .copied()
            .fold(0.0f32, f32::max);
        assert!(max_volume > 0.0, "accumulated flow should be positive");
    }
}
