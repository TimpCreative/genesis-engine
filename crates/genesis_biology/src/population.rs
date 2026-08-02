//! Population dynamics and guild occupancy (Doc 09 §5.2–§5.3, §4.3).
//!
//! P4-6, simplified: per-hex living biomass is a productivity × richness proxy
//! (the Biomass map layer's data), and each province's occupied guilds are
//! filled by the guaranteed-then-contingent cascade (§4.3) up to the count its
//! richness supports (§4.4). Full carrying-capacity relaxation and the trophic
//! food web (§5.2–§5.3) are a refinement.

use genesis_core::data::WorldData;

use crate::guild::GuildRoster;
use crate::province::{ProvinceRegistry, Realm};

/// Producer base × the trophic-pyramid sum (~1 + 0.1 + 0.01 + …). Total standing
/// biomass is **producer-dominated and set by the energy base** (primary
/// productivity), not by diversity — which redistributes biomass among species
/// rather than adding to it (Doc 09 §5.2).
const TROPHIC_TOTAL_SCALE: f32 = 1100.0; // tons/hex at full productivity

/// Fills `world.biomass` — total standing biomass from the producer base
/// (Doc 09 §5.2), zero on barren hexes.
pub fn compute_biomass(world: &mut WorldData) {
    let n = world.cell_count() as usize;
    for i in 0..n {
        let barren = world.biome[i].0 == crate::biome::BARREN_ID;
        world.biomass[i] = if barren {
            0.0
        } else {
            world.primary_productivity[i] * TROPHIC_TOTAL_SCALE
        };
    }
}

/// The guaranteed-then-contingent priority order for filling a realm's guilds
/// (Doc 09 §4.3): producers and decomposers are guaranteed where energy exists;
/// herbivores/predators cascade off producer biomass.
pub(crate) fn cascade_order(realm: Realm) -> &'static [&'static str] {
    match realm {
        Realm::Terrestrial | Realm::Freshwater => {
            &["producer", "decomposer", "herbivore", "apex_predator"]
        }
        Realm::Marine => &[
            "phytoplankton",
            "marine_decomposer",
            "filter_feeder",
            "nekton_predator",
        ],
    }
}

/// Fills each province's `occupied_guilds` from its realm's cascade, up to the
/// guild count its richness supports (Doc 09 §4.3–§4.4).
pub fn compute_guild_occupancy(roster: &GuildRoster, registry: &mut ProvinceRegistry) {
    for province in registry.provinces_mut() {
        let order = cascade_order(province.realm);
        let budget = (province.occupied_guild_count as usize).max(if province.richness > 0.0 {
            1
        } else {
            0
        });
        let mut occupied = Vec::new();
        for &name in order {
            if occupied.len() >= budget {
                break;
            }
            if let Some(g) = roster.iter().find(|g| g.name == name) {
                occupied.push(g.id);
            }
        }
        province.occupied_guilds = occupied;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::assign_biomes;
    use crate::core_graph::core_morphospace;
    use crate::guild::core_guilds;
    use crate::province::label_provinces;
    use crate::richness::{compute_primary_productivity, compute_richness};
    use genesis_core::create_world;
    use genesis_core::data::{ClimateRegimePlaceholder as Regime, WATER_NONE};
    use genesis_core::parameters::WorldParameters;

    #[test]
    fn rich_land_gets_biomass_and_guilds() {
        let mut world = create_world(WorldParameters::default())
            .expect("world")
            .data;
        for i in 0..world.cell_count() as usize {
            world.water_level_m[i] = WATER_NONE;
            world.elevation_mean[i] = 200.0;
            world.temperature_mean[i] = 24.0;
            world.precipitation[i] = 2500.0;
            world.soil_fertility[i] = 1.0;
            world.temperature_range[i] = 2.0;
            world.climate_regime[i] = Regime::Tropical;
        }
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let mut reg = label_provinces(&mut world);
        compute_primary_productivity(&mut world);
        compute_richness(&mut world, &mut reg);
        assign_biomes(&mut world, true);
        compute_biomass(&mut world);
        compute_guild_occupancy(&roster, &mut reg);

        assert!(world.biomass[0] > 0.0, "rich land should carry biomass");
        let province = reg.iter().next().unwrap();
        assert!(
            !province.occupied_guilds.is_empty(),
            "a rich province should occupy guilds"
        );
        // Producer is guaranteed and first.
        let producer = roster.iter().find(|g| g.name == "producer").unwrap();
        assert_eq!(province.occupied_guilds[0], producer.id);
    }
}
