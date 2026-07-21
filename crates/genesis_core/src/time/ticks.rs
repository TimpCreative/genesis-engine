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

/// How often a layer reporting `tick_interval == 0` (dormant) is re-polled to
/// see whether world state has since given it work — e.g. biology waking when
/// oceans first form. Coarse, so idle re-polls are cheap; the layer still does no
/// `advance` work until it reports a positive interval.
const DORMANT_REPOLL_YEARS: i64 = 1_000_000;

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
        self.advance_to_with(target_year, world, rng, params, &mut |_| {});
    }

    /// Like [`Self::advance_to`], invoking `on_tick` with the world state after
    /// each processed tick (for progress reporting and history buffering).
    /// Purely observational — the callback receives a shared reference and
    /// cannot affect simulation state, so output is identical to `advance_to`.
    pub fn advance_to_with(
        &mut self,
        target_year: WorldYear,
        world: &mut WorldData,
        rng: &WorldRng,
        params: &WorldParameters,
        on_tick: &mut dyn FnMut(&WorldData),
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
                    } else {
                        // Dormant at this year — do no work, but re-poll later
                        // rather than park forever, so a layer can wake once world
                        // state gives it work (Doc 09 §3; limitation 3).
                        state.next_tick_year = world.current_year + DORMANT_REPOLL_YEARS;
                    }
                }
            }

            on_tick(world);
        }

        if world.current_year < target_year {
            world.current_year = target_year;
        }
    }

    fn init_next_ticks(&mut self, start: WorldYear, params: &WorldParameters) {
        for state in &mut self.layers {
            let interval = state.layer.tick_interval(start, params);
            // Active layers tick immediately at `start`; dormant ones are re-polled
            // on the coarse cadence rather than parked forever (limitation 3).
            state.next_tick_year = if interval > 0 {
                start
            } else {
                start + DORMANT_REPOLL_YEARS
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

/// Always dormant; used to verify the coordinator does not stall.
#[cfg(test)]
struct DormantLayer;

#[cfg(test)]
impl SimulationLayer for DormantLayer {
    fn name(&self) -> &str {
        "dormant"
    }

    fn tick_interval(&self, _current_time: WorldYear, _params: &WorldParameters) -> i64 {
        0
    }

    fn advance(&mut self, _world: &mut WorldData, _rng: &WorldRng) -> Vec<()> {
        panic!("dormant layer must not advance");
    }
}

/// Dormant until `wake_year`, then ticks — models biology (no work before oceans
/// exist, active afterward) to prove the coordinator re-polls dormant layers.
#[cfg(test)]
struct WakingLayer {
    wake_year: i64,
    interval: i64,
    tick_years: Arc<Mutex<Vec<WorldYear>>>,
}

#[cfg(test)]
impl SimulationLayer for WakingLayer {
    fn name(&self) -> &str {
        "waking"
    }

    fn tick_interval(&self, current_time: WorldYear, _params: &WorldParameters) -> i64 {
        if current_time.value() >= self.wake_year {
            self.interval
        } else {
            0
        }
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
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 4;
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut world = WorldData::new(grid, params);
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
                WorldYear(0),
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
    fn first_tick_occurs_at_world_start_year() {
        let layer = RecordingLayer::new("formation", 500_000);
        let recorded = Arc::clone(&layer.tick_years);

        let mut coord = TickCoordinator::new();
        coord.add_layer(Box::new(layer));

        let mut world = world_at(WorldYear::FORMATION);
        let rng = WorldRng::from_effective_seed(1);
        let params = WorldParameters::default();
        coord.advance_to(WorldYear(500_000), &mut world, &rng, &params);

        let years = recorded.lock().unwrap();
        assert_eq!(years.first().copied(), Some(WorldYear::FORMATION));
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
            vec![WorldYear(0), WorldYear(50), WorldYear(100)]
        );
        assert_eq!(
            *slow_log.lock().unwrap(),
            vec![WorldYear(0), WorldYear(100)]
        );
    }

    #[test]
    fn coordinator_does_not_stall_on_dormant_layer() {
        let mut coord = TickCoordinator::new();
        coord.add_layer(Box::new(DormantLayer));

        let mut world = world_at(WorldYear::FORMATION);
        let rng = WorldRng::from_effective_seed(1);
        let params = WorldParameters::default();
        let target = WorldYear(1_000_000);

        coord.advance_to(target, &mut world, &rng, &params);

        assert_eq!(world.current_year, target);
    }

    #[test]
    fn dormant_at_start_layer_wakes_when_it_gets_work() {
        // A layer that reports 0 at world start (dormant) must still be re-polled
        // and tick once world state gives it work — not parked at i64::MAX forever
        // (limitation 3).
        let waker = WakingLayer {
            wake_year: 3_000_000,
            interval: 500_000,
            tick_years: Arc::new(std::sync::Mutex::new(Vec::new())),
        };
        let log = Arc::clone(&waker.tick_years);

        let mut coord = TickCoordinator::new();
        coord.add_layer(Box::new(waker));

        let mut world = world_at(WorldYear::FORMATION);
        let rng = WorldRng::from_effective_seed(1);
        let params = WorldParameters::default();
        coord.advance_to(WorldYear(5_000_000), &mut world, &rng, &params);

        let years = log.lock().unwrap();
        assert!(
            !years.is_empty(),
            "a dormant-at-start layer must wake and tick once it has work"
        );
        assert!(
            years.iter().all(|y| y.value() >= 3_000_000),
            "no ticks before the wake year: {years:?}"
        );
    }
}
