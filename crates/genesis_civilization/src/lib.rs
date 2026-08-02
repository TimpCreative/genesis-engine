//! Civilization simulation (Doc 10) — the fine-cadence layer.
//!
//! Civilizations evolve on human timescales (decades to millennia), far finer
//! than the 500k geological tick. The [`TickCoordinator`](genesis_core::time::TickCoordinator)
//! is multi-rate by design — each layer reports its own per-era `tick_interval`
//! — so civilization simply reports a fine cadence in the eras where intelligent
//! life exists, and the coordinator drives it at that rate while tectonics sits
//! effectively frozen (Doc 05 §A.1).
//!
//! **This slice is the cadence seam, not the simulation.** The layer is dormant
//! until sapience, then ticks at a human-scale rate, but its `advance` is an
//! inert scaffold: it records that it ran and deliberately **never reads the RNG
//! or mutates [`WorldData`]**, so registering it cannot perturb the physical or
//! biological trajectory (determinism preserved). Real civilization dynamics —
//! cultures, technology, settlements, nations — land on top of this seam.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use genesis_core::data::WorldData;
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

/// Ancient-era civilization tick interval (years): the rise of agriculture
/// through the first states — a millennium of resolution, ~500× finer than the
/// geological tick. (Doc 10 §2.1; refined as civ dynamics land.)
pub const CIV_ANCIENT_TICK_YEARS: i64 = 1_000;

/// Recent-era civilization tick interval (years): industrial-to-modern pace,
/// where a human generation matters — decade resolution.
pub const CIV_RECENT_TICK_YEARS: i64 = 25;

/// Civilization simulation state (Doc 10). Minimal scaffold: only observability
/// counters for now, so tests can prove the coordinator drives the layer at the
/// fine cadence. Real state (settlements, cultures, tech) grows here.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CivilizationState {
    /// Number of civilization ticks the coordinator has driven.
    pub ticks_run: u64,
    /// World year of the most recent tick (`FORMATION` if never ticked).
    pub last_tick_year: WorldYear,
}

impl CivilizationState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Civilization [`SimulationLayer`]. Registered last (after biology): it only
/// reads finished geography/climate/biology, so it never gates the physical
/// layers. Mirrors the other layers' `attach`/`detach_state` sharing so a
/// live-stepping worker can hold the state resident and read it through the
/// shared handle.
pub struct CivilizationLayer {
    state: Rc<RefCell<CivilizationState>>,
    /// Layer-local timestep clock (mirrors the physical layers); reset on
    /// `attach`, so the layer must be built once per session, not re-attached
    /// between steps (see Doc 05 §A.1).
    last_tick_year: Cell<WorldYear>,
}

impl CivilizationLayer {
    /// Creates a layer sharing `state` with the caller via `Rc`.
    pub fn attach(state: &mut CivilizationState) -> (Self, Rc<RefCell<CivilizationState>>) {
        let shared = Rc::new(RefCell::new(std::mem::take(state)));
        let layer = Self {
            state: Rc::clone(&shared),
            last_tick_year: Cell::new(WorldYear::FORMATION),
        };
        (layer, shared)
    }

    /// Recovers owned state from a shared handle after tick simulation.
    pub fn detach_state(shared: Rc<RefCell<CivilizationState>>) -> CivilizationState {
        Rc::try_unwrap(shared)
            .expect("civilization state still borrowed")
            .into_inner()
    }
}

impl SimulationLayer for CivilizationLayer {
    fn name(&self) -> &str {
        "civilization"
    }

    /// Dormant until intelligent life (Formation/Geological/Prehistoric report
    /// `0`, so the coordinator re-polls but does no work); then a human-scale
    /// cadence far finer than any geological layer. The Prehistoric→Ancient
    /// boundary is `sapience_emergence_year` (Doc 03 / `Era::for_year`), so this
    /// wakes exactly when sapience emerges.
    fn tick_interval(&self, current_time: WorldYear, params: &WorldParameters) -> i64 {
        match Era::for_year(current_time, params) {
            Era::Ancient => CIV_ANCIENT_TICK_YEARS,
            Era::Recent => CIV_RECENT_TICK_YEARS,
            // Pre-sapience: no civilization exists yet.
            Era::Formation | Era::Geological | Era::Prehistoric => 0,
        }
    }

    fn advance(&mut self, world: &mut WorldData, _rng: &WorldRng) -> Vec<()> {
        // Inert scaffold (Doc 10): record the tick and nothing more. Reads no
        // RNG and mutates no `WorldData`, so adding this layer to the coordinator
        // cannot change the physical/biology outcome — the seam is safe to wire
        // in before the simulation exists.
        let mut state = self.state.borrow_mut();
        state.ticks_run += 1;
        state.last_tick_year = world.current_year;
        self.last_tick_year.set(world.current_year);
        Vec::new()
    }
}

pub fn hello() -> &'static str {
    "genesis_civilization"
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::grid::HexGrid;
    use genesis_core::time::TickCoordinator;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn params_with_sapience(sapience: i64) -> WorldParameters {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 4;
        // Era ordering requires life < sapience (`Era::for_year` tests `y < life`
        // before `y < sapience`); keep life before sapience for the test span.
        params.core.biology.life_emergence_year = WorldYear((sapience / 2).max(1));
        params.core.civilization.sapience_emergence_year = Some(WorldYear(sapience));
        params
    }

    fn world_at(year: i64, params: &WorldParameters) -> WorldData {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut world = WorldData::new(grid, params.clone());
        world.current_year = WorldYear(year);
        world
    }

    #[test]
    fn hello_returns_crate_name() {
        assert_eq!(hello(), "genesis_civilization");
    }

    #[test]
    fn dormant_until_sapience_then_human_scale() {
        let mut state = CivilizationState::new();
        let (layer, _shared) = CivilizationLayer::attach(&mut state);
        let params = params_with_sapience(4_490_000_000);

        // Pre-sapience eras: dormant.
        assert_eq!(layer.tick_interval(WorldYear(0), &params), 0, "formation");
        assert_eq!(
            layer.tick_interval(WorldYear(1_000_000_000), &params),
            0,
            "geological"
        );
        assert_eq!(
            layer.tick_interval(WorldYear(4_000_000_000), &params),
            0,
            "prehistoric (pre-sapience)"
        );
        // Ancient (post-sapience): millennium cadence.
        assert_eq!(
            layer.tick_interval(WorldYear(4_491_000_000), &params),
            CIV_ANCIENT_TICK_YEARS
        );
        // Recent: decade cadence, finer still.
        let recent = params.core.time.default_user_year.value() - 1_000;
        assert_eq!(
            layer.tick_interval(WorldYear(recent), &params),
            CIV_RECENT_TICK_YEARS
        );
        assert!(CIV_RECENT_TICK_YEARS < CIV_ANCIENT_TICK_YEARS);
        assert!(
            CIV_ANCIENT_TICK_YEARS < 500_000,
            "far finer than the geological tick"
        );
    }

    /// The multi-rate proof: the coordinator drives civ at its fine cadence
    /// across the sapience boundary — sub-500k simulation works today, which is
    /// exactly what intelligent species will need.
    #[test]
    fn coordinator_drives_civ_at_fine_cadence() {
        let sapience = 1_000_000; // small so the test span is cheap
        let params = params_with_sapience(sapience);
        let mut state = CivilizationState::new();
        let (layer, shared) = CivilizationLayer::attach(&mut state);
        let mut coord = TickCoordinator::new();
        coord.add_layer(Box::new(layer));

        let mut world = world_at(0, &params);
        let rng = WorldRng::from_effective_seed(1);
        // Advance from before sapience to 20k years into the Ancient era.
        let target = WorldYear(sapience + 20_000);
        coord.advance_to(target, &mut world, &rng, &params);

        let civ = shared.borrow();
        // ~20 ticks at the 1000-year Ancient cadence (dormant before sapience).
        assert!(
            (18..=22).contains(&civ.ticks_run),
            "civ should tick ~20× over 20 ky of Ancient era, got {}",
            civ.ticks_run
        );
        assert!(
            civ.last_tick_year.value() >= sapience,
            "no civ tick before sapience"
        );
    }

    #[test]
    fn advance_does_not_mutate_world_or_rng() {
        // Determinism guard: the scaffold must be inert on the simulation.
        let params = params_with_sapience(1_000);
        let mut world = world_at(2_000, &params); // Ancient era
        world
            .elevation_mean
            .iter_mut()
            .enumerate()
            .for_each(|(i, e)| *e = i as f32);
        let before = world.elevation_mean.clone();

        let mut state = CivilizationState::new();
        let (mut layer, shared) = CivilizationLayer::attach(&mut state);
        let rng = WorldRng::from_effective_seed(7);
        layer.advance(&mut world, &rng);

        assert_eq!(
            world.elevation_mean, before,
            "civ advance must not touch terrain"
        );
        assert_eq!(shared.borrow().ticks_run, 1);
    }
}
