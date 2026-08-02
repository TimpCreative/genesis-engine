//! [`SimulationLayer`] integration for biology (Doc 09).
//!
//! **P4-3: the layer is live.** It is dormant during Formation (no oceans) and
//! ticks at the geological biology cadence afterward (Doc 05 §B.1; the full
//! milestone-triggered cadence is later). Each tick it attempts biogenesis until
//! life exists, then advances the microbial era (Doc 09 §3).

use std::cell::RefCell;
use std::rc::Rc;

use genesis_core::data::WorldData;
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

use crate::biogenesis::try_biogenesis;
use crate::microbial::microbial_step;
use crate::state::BiologyState;

/// Geological-era biology tick interval (Doc 05 §B.1). Refined to the full
/// era-based cadence (finer post-sapience) in a later slice.
pub const DEFAULT_BIOLOGY_TICK_YEARS: i64 = 500_000;

/// How often the geography-derived biology fields (provinces, richness, biomes,
/// biomass) are recomputed once life exists — coarser than the tick to bound
/// per-tick cost. Must be a multiple of the biology tick interval.
pub const HEAVY_FIELD_STRIDE_YEARS: i64 = 5_000_000;

/// Biology simulation layer (Doc 09). Registered after hydrology (Layer 1).
pub struct BiologyLayer {
    state: Rc<RefCell<BiologyState>>,
}

impl BiologyLayer {
    /// Creates a layer sharing `state` with the caller via `Rc`.
    pub fn attach(state: &mut BiologyState) -> (Self, Rc<RefCell<BiologyState>>) {
        let shared = Rc::new(RefCell::new(std::mem::take(state)));
        let layer = Self {
            state: Rc::clone(&shared),
        };
        (layer, shared)
    }

    /// Recovers owned state from a shared handle after tick simulation.
    pub fn detach_state(shared: Rc<RefCell<BiologyState>>) -> BiologyState {
        Rc::try_unwrap(shared)
            .expect("biology state still borrowed")
            .into_inner()
    }
}

impl SimulationLayer for BiologyLayer {
    fn name(&self) -> &str {
        "biology"
    }

    /// **Dormant during Formation** — biology has no work before oceans exist, so
    /// it reports `0` and the coordinator re-polls it until oceans form (the
    /// coordinator now wakes dormant-at-start layers, limitation 3). Afterward it
    /// ticks at the geological cadence (Doc 05 §B.1).
    fn tick_interval(&self, current_time: WorldYear, params: &WorldParameters) -> i64 {
        match Era::for_year(current_time, params) {
            Era::Formation => 0,
            _ => DEFAULT_BIOLOGY_TICK_YEARS,
        }
    }

    fn advance(&mut self, world: &mut WorldData, rng: &WorldRng) -> Vec<()> {
        let interval = self.tick_interval(world.current_year, &world.parameters);
        let mut state = self.state.borrow_mut();
        if state.origin.is_none() {
            try_biogenesis(&mut state, world, rng, interval);
        } else {
            microbial_step(&mut state, world, rng);
            // Refresh the geography-derived biology fields (provinces §5.1,
            // energy/diversity §4.4/§5.2, biomes §4.6, biomass §5.2, guild
            // occupancy §4.3) at most every ~5 My — coarser than the tick to bound
            // cost. A dirty-flag then skips even that recompute when the geography
            // and climate inputs are unchanged since the last one (limitations 9 & 13).
            if world.current_year.value() % HEAVY_FIELD_STRIDE_YEARS == 0 {
                // Land biomes appear only once life has colonized land — proxied
                // by multicellularity (§3.4); before that, continents stay barren
                // and only the ocean is alive (limitation 17).
                let land_colonized = state
                    .milestones
                    .contains(&crate::state::Milestone::Multicellularity);
                let sig = terrain_signature(world, land_colonized);
                if state.heavy_signature != Some(sig) {
                    // Reborrow so `guilds` (shared) and `provinces` (mut) are seen
                    // as disjoint fields, not two borrows through the `RefMut`.
                    let s = &mut *state;
                    s.provinces = crate::province::label_provinces(world);
                    crate::richness::compute_primary_productivity(world);
                    crate::richness::compute_richness(world, &mut s.provinces);
                    crate::biome::assign_biomes(world, land_colonized);
                    crate::population::compute_biomass(world);
                    crate::population::compute_guild_occupancy(&s.guilds, &mut s.provinces);
                    s.heavy_signature = Some(sig);
                }
            }
        }
        Vec::new()
    }
}

/// A deterministic signature of the geography/climate inputs the heavy biology
/// fields depend on (elevation, water, climate regime, precipitation, temperature,
/// soil, plus the land-colonization gate). Equal signatures mean a heavy recompute
/// would reproduce the same fields, so it can be skipped (limitations 9 & 13). A
/// single O(n) FNV-1a pass — far cheaper than the flood-fill + richness + biome +
/// biomass + occupancy recompute it guards.
fn terrain_signature(world: &WorldData, land_colonized: bool) -> u64 {
    #[inline]
    fn mix(h: u64, x: u64) -> u64 {
        (h ^ x).wrapping_mul(0x0000_0100_0000_01B3) // FNV-1a 64-bit prime
    }
    let mut h: u64 = 0xCBF2_9CE4_8422_2325; // FNV offset basis
    h = mix(h, land_colonized as u64);
    for i in 0..world.cell_count() as usize {
        h = mix(h, world.elevation_mean[i].to_bits() as u64);
        h = mix(h, world.water_level_m[i].to_bits() as u64);
        h = mix(h, world.climate_regime[i] as u64);
        h = mix(h, world.precipitation[i].to_bits() as u64);
        h = mix(h, world.temperature_mean[i].to_bits() as u64);
        h = mix(h, world.temperature_range[i].to_bits() as u64);
        h = mix(h, world.soil_fertility[i].to_bits() as u64);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_is_dormant_in_formation_and_active_after() {
        let mut state = BiologyState::new();
        let (layer, shared) = BiologyLayer::attach(&mut state);
        let params = WorldParameters::default();
        // Dormant (0) during Formation — the coordinator re-polls until oceans
        // form (limitation 3) — and active at the geological cadence afterward.
        assert_eq!(layer.tick_interval(WorldYear(0), &params), 0);
        assert_eq!(
            layer.tick_interval(WorldYear(1_000_000_000), &params),
            DEFAULT_BIOLOGY_TICK_YEARS
        );
        assert_eq!(layer.name(), "biology");
        // The coordinator owns and drops the boxed layer before the caller
        // detaches; mirror that here so the `Rc` is uniquely owned.
        drop(layer);
        let _ = BiologyLayer::detach_state(shared);
    }

    #[test]
    fn terrain_signature_is_stable_and_input_sensitive() {
        use genesis_core::create_world;

        let mut world = create_world(WorldParameters::default())
            .expect("world")
            .data;
        let base = terrain_signature(&world, false);
        // Deterministic: same inputs ⇒ same signature (the dirty-flag would skip).
        assert_eq!(base, terrain_signature(&world, false));
        // The land-colonization gate is part of the signature.
        assert_ne!(base, terrain_signature(&world, true));
        // A single changed input hex changes the signature (so no stale skip).
        world.elevation_mean[0] += 1.0;
        assert_ne!(base, terrain_signature(&world, false));
    }
}
