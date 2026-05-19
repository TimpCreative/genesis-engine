//! Simulation layer trait and tick coordinator skeleton.

use crate::WorldData;
use crate::parameters::WorldParameters;
use crate::rng::WorldRng;
use crate::time::WorldYear;

/// A simulation subsystem that advances on a year-based tick schedule.
pub trait SimulationLayer {
    fn name(&self) -> &str;

    /// Tick interval in years at `current_time`; 0 if dormant.
    fn tick_interval(&self, current_time: WorldYear, params: &WorldParameters) -> i64;

    /// Advances one tick. Returns events generated.
    // TODO(step-5): replace `Vec<()>` with `Vec<Event>`.
    fn advance(&mut self, world: &mut WorldData, rng: &WorldRng) -> Vec<()>;
}

struct LayerState {
    layer: Box<dyn SimulationLayer>,
    next_tick_year: WorldYear,
}

/// Coordinates multiple [`SimulationLayer`] instances until a target year.
pub struct TickCoordinator {
    layers: Vec<LayerState>,
}

impl Default for TickCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl TickCoordinator {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn add_layer(&mut self, layer: Box<dyn SimulationLayer>) {
        self.layers.push(LayerState {
            layer,
            next_tick_year: WorldYear::FORMATION,
        });
    }

    /// Advances simulation until `world.current_year >= target_year`.
    ///
    /// Layers due at the same year run in registration order. Time advances to
    /// the earliest pending tick year among all layers each step.
    pub fn advance_to(
        &mut self,
        target_year: WorldYear,
        world: &mut WorldData,
        rng: &WorldRng,
        params: &WorldParameters,
    ) {
        self.init_next_ticks(world.current_year, params);

        while world.current_year < target_year {
            let Some(next_year) = self.earliest_next_tick(target_year) else {
                break;
            };
            if next_year > target_year {
                break;
            }

            world.current_year = next_year;

            for state in &mut self.layers {
                if state.next_tick_year == next_year {
                    let interval = state.layer.tick_interval(world.current_year, params);
                    if interval > 0 {
                        let _ = state.layer.advance(world, rng);
                        state.next_tick_year = world.current_year + interval;
                    }
                }
            }
        }
    }

    fn init_next_ticks(&mut self, start: WorldYear, params: &WorldParameters) {
        for state in &mut self.layers {
            let interval = state.layer.tick_interval(start, params);
            state.next_tick_year = if interval > 0 {
                start + interval
            } else {
                WorldYear(i64::MAX)
            };
        }
    }

    fn earliest_next_tick(&self, target_year: WorldYear) -> Option<WorldYear> {
        self.layers
            .iter()
            .map(|s| s.next_tick_year)
            .filter(|&y| y <= target_year)
            .min()
    }
}

#[cfg(test)]
use std::sync::{Arc, Mutex};

/// Test layer recording tick years in a shared vector.
#[cfg(test)]
struct RecordingLayer {
    name: &'static str,
    interval: i64,
    tick_years: Arc<Mutex<Vec<WorldYear>>>,
}

#[cfg(test)]
impl RecordingLayer {
    fn new(name: &'static str, interval: i64) -> Self {
        Self {
            name,
            interval,
            tick_years: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[cfg(test)]
impl SimulationLayer for RecordingLayer {
    fn name(&self) -> &str {
        self.name
    }

    fn tick_interval(&self, _current_time: WorldYear, _params: &WorldParameters) -> i64 {
        self.interval
    }

    fn advance(&mut self, world: &mut WorldData, _rng: &WorldRng) -> Vec<()> {
        self.tick_years.lock().unwrap().push(world.current_year);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::WorldData;
    use crate::grid::HexGrid;
    use crate::rng::WorldRng;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn world_at(year: WorldYear) -> WorldData {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut world = WorldData::new(grid);
        world.current_year = year;
        world
    }

    #[test]
    fn advance_to_ticks_every_100_years() {
        let layer = RecordingLayer::new("test", 100);
        let recorded = Arc::clone(&layer.tick_years);

        let mut coord = TickCoordinator::new();
        coord.add_layer(Box::new(layer));

        let mut world = world_at(WorldYear::FORMATION);
        let rng = WorldRng::from_effective_seed(1);
        let params = WorldParameters::default();
        coord.advance_to(WorldYear(1000), &mut world, &rng, &params);

        let years = recorded.lock().unwrap().clone();
        assert_eq!(
            years,
            vec![
                WorldYear(100),
                WorldYear(200),
                WorldYear(300),
                WorldYear(400),
                WorldYear(500),
                WorldYear(600),
                WorldYear(700),
                WorldYear(800),
                WorldYear(900),
                WorldYear(1000),
            ]
        );
    }

    #[test]
    fn multiple_layers_tick_in_registration_order() {
        let fast = RecordingLayer::new("fast", 50);
        let slow = RecordingLayer::new("slow", 100);
        let fast_log = Arc::clone(&fast.tick_years);
        let slow_log = Arc::clone(&slow.tick_years);

        let mut coord = TickCoordinator::new();
        coord.add_layer(Box::new(fast));
        coord.add_layer(Box::new(slow));

        let mut world = world_at(WorldYear::FORMATION);
        let rng = WorldRng::from_effective_seed(1);
        let params = WorldParameters::default();
        coord.advance_to(WorldYear(100), &mut world, &rng, &params);

        assert_eq!(
            *fast_log.lock().unwrap(),
            vec![WorldYear(50), WorldYear(100)]
        );
        assert_eq!(*slow_log.lock().unwrap(), vec![WorldYear(100)]);
    }
}
