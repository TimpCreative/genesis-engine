//! Biogenesis — the probabilistic origin of life at a marine vent (Doc 09 §3.1–3.2).

use genesis_core::data::{BedrockType, WATER_NONE, WorldData};
use genesis_core::events::{EventKind, EventLocation, Significance};
use genesis_core::grid::HexId;
use genesis_core::rng::WorldRng;
use genesis_core::time::WorldYear;
use rand::Rng;

use crate::events::emit;
use crate::state::{BiologyState, Milestone, Origin};

/// Minimum water depth for a hex to host a hydrothermal-vent community (m).
const DEEP_OCEAN_DEPTH_M: f32 = 1000.0;

/// Whether a hex is deep ocean (a candidate for a vent community).
fn is_deep_ocean(world: &WorldData, i: usize) -> bool {
    let water = world.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
    water.is_finite() && water - world.elevation_mean[i] >= DEEP_OCEAN_DEPTH_M
}

/// Whether a hex sits on, or beside, volcanically active crust — `Igneous`
/// bedrock, tectonics' marker for hotspot / ridge / arc volcanism. This is the
/// "adjacent to volcanism/hotspot" refinement of the vent origin (Doc 09 §3.1).
fn is_volcanic_adjacent(world: &WorldData, hex: HexId) -> bool {
    let volcanic = |j: usize| world.bedrock_type.get(j).copied() == Some(BedrockType::Igneous);
    volcanic(hex.0 as usize)
        || world
            .grid
            .neighbors(hex)
            .iter()
            .any(|nb| volcanic(nb.0 as usize))
}

/// Vent-bearing hexes in ascending `HexId` order (Doc 09 §3.2): **deep ocean
/// adjacent to volcanism/hotspot** (submarine hydrothermal vents, §3.1). Falls
/// back to plain deep ocean when no submarine volcanism has formed yet, so life
/// can still arise on a volcanically quiet young ocean.
fn suitable_vent_hexes(world: &WorldData) -> Vec<HexId> {
    let deep: Vec<HexId> = (0..world.cell_count() as usize)
        .filter(|&i| is_deep_ocean(world, i))
        .map(|i| HexId(i as u32))
        .collect();
    let vents: Vec<HexId> = deep
        .iter()
        .copied()
        .filter(|&h| is_volcanic_adjacent(world, h))
        .collect();
    if vents.is_empty() { deep } else { vents }
}

/// Attempts biogenesis this tick. Probability accrues so that the expected
/// origin lands near `life_emergence_year` (scaled by `biogenesis_rate_scale`);
/// as the ramp approaches the target the per-tick hazard rises toward certainty.
/// The first success in ascending `HexId` order plants the root lineage
/// (Doc 09 §3.1–3.2). `None`-op once life exists (single origin).
pub(crate) fn try_biogenesis(
    state: &mut BiologyState,
    world: &mut WorldData,
    rng: &WorldRng,
    tick_interval: i64,
) {
    let year = world.current_year;
    let suitable = suitable_vent_hexes(world);
    if suitable.is_empty() {
        return; // no oceans / no deep vents yet
    }
    let bio = &world.parameters.core.biology;
    let remaining = (bio.life_emergence_year.value() - year.value()).max(tick_interval);
    let hazard = ((tick_interval as f64) / (remaining as f64 * suitable.len() as f64))
        * f64::from(bio.biogenesis_rate_scale);
    let hazard = hazard.clamp(0.0, 1.0);

    let mut stream = rng.stream_at("biology.biogenesis", year.value() as u64);
    for hex in suitable {
        let roll: f64 = stream.gen_range(0.0..1.0);
        if roll < hazard {
            plant_origin(state, world, hex, year);
            return;
        }
    }
}

/// Plants the tree of life's root at `hex`/`year` with the prokaryote-analog
/// genome (Doc 09 §3.3 root).
fn plant_origin(state: &mut BiologyState, world: &mut WorldData, hex: HexId, year: WorldYear) {
    let chemosynthesis = state
        .graph
        .id_of("core:chemosynthesis")
        .expect("core trait");
    let unicellular = state.graph.id_of("core:unicellular").expect("core trait");
    state.root_genome.insert(chemosynthesis);
    state.root_genome.insert(unicellular);
    state.origin = Some(Origin { hex, year });
    state.milestones.insert(Milestone::LifeEmerged);

    // Minimal reflection into WorldData; province/biomass fields are populated
    // by later slices (P4-4/P4-5).
    let i = hex.0 as usize;
    if let Some(b) = world.biomass.get_mut(i) {
        *b = b.max(1.0);
    }

    emit(
        state,
        year,
        EventLocation::Hex(hex),
        Significance::Pivotal,
        EventKind::LifeEmerged { hex, year },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    /// An all-land world (so tests carve their own deep-ocean vents) with life
    /// targeted soon so the hazard resolves immediately.
    fn bare_world() -> WorldData {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        params.core.biology.life_emergence_year = WorldYear(1_000_000);
        let mut world = create_world(params).expect("world").data;
        for i in 0..world.cell_count() as usize {
            world.water_level_m[i] = WATER_NONE;
            world.elevation_mean[i] = 100.0;
            world.bedrock_type[i] = BedrockType::Sedimentary;
        }
        world
    }

    fn carve_deep_ocean(world: &mut WorldData, hex: HexId) {
        let i = hex.0 as usize;
        world.water_level_m[i] = 0.0;
        world.elevation_mean[i] = -3000.0;
    }

    #[test]
    fn origin_is_a_volcanic_adjacent_vent_when_one_exists() {
        let mut world = bare_world();
        for h in [HexId(4), HexId(9), HexId(15)] {
            carve_deep_ocean(&mut world, h);
        }
        world.bedrock_type[9] = BedrockType::Igneous; // a submarine hotspot

        let suitable = suitable_vent_hexes(&world);
        assert!(!suitable.is_empty());
        assert!(
            suitable.iter().all(|&h| is_volcanic_adjacent(&world, h)),
            "vents must be volcanic-adjacent: {suitable:?}"
        );

        world.current_year = WorldYear(1_000_000);
        let rng = WorldRng::from_effective_seed(1);
        let mut state = BiologyState::new();
        try_biogenesis(&mut state, &mut world, &rng, 500_000);
        let origin = state.origin().expect("life should emerge");
        assert!(
            is_volcanic_adjacent(&world, origin.hex),
            "origin {origin:?} must be at a volcanic-adjacent vent"
        );
    }

    #[test]
    fn falls_back_to_plain_deep_ocean_without_volcanism() {
        let mut world = bare_world();
        carve_deep_ocean(&mut world, HexId(4));
        // No Igneous bedrock anywhere → the vent filter would empty, so life still
        // arises on plain deep ocean rather than never emerging.
        assert_eq!(suitable_vent_hexes(&world), vec![HexId(4)]);
    }
}
