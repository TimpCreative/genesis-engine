//! Emergent biomes (Doc 09 §4.6).
//!
//! A biome is the climate regime × the dominant producer strategy actually
//! present. P4-7 assigns from `climate_regime` + water + precipitation, gated on
//! life existing (no producers ⇒ barren) — the "biomes only where life is"
//! spirit. A world whose producers never colonized land keeps ocean-only life;
//! land-colonization gating (§3.4) is a refinement.

use genesis_core::data::{BiomeId, ClimateRegimePlaceholder as Regime, WATER_NONE, WorldData};

/// Biome display names, indexed by `BiomeId.0`. The set is engine content.
pub const BIOME_NAMES: &[&str] = &[
    "Ocean",            // 0
    "Lake",             // 1
    "Ice cap",          // 2
    "Tundra",           // 3
    "Boreal forest",    // 4
    "Temperate forest", // 5
    "Grassland",        // 6
    "Cold desert",      // 7
    "Hot desert",       // 8
    "Mediterranean",    // 9
    "Savanna",          // 10
    "Tropical forest",  // 11
    "Barren",           // 12
];

const OCEAN: u16 = 0;
const LAKE: u16 = 1;
const ICE_CAP: u16 = 2;
const TUNDRA: u16 = 3;
const BOREAL: u16 = 4;
const TEMPERATE_FOREST: u16 = 5;
const GRASSLAND: u16 = 6;
const COLD_DESERT: u16 = 7;
const HOT_DESERT: u16 = 8;
const MEDITERRANEAN: u16 = 9;
const SAVANNA: u16 = 10;
const TROPICAL_FOREST: u16 = 11;
const BARREN: u16 = 12;

/// Barren-biome id (no producers) — read by [`crate::population`].
pub const BARREN_ID: u16 = BARREN;

/// Display name for a biome id (Doc 09 §4.6).
pub fn biome_name(biome: BiomeId) -> &'static str {
    BIOME_NAMES.get(biome.0 as usize).copied().unwrap_or("—")
}

/// Precipitation (mm/yr) above which a temperate/tropical belt is forest, not
/// grassland/savanna.
const FOREST_PRECIP_MM: f32 = 800.0;

fn is_wet(world: &WorldData, i: usize) -> bool {
    let water = world.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
    water.is_finite() && water > world.elevation_mean[i]
}

fn land_biome(regime: Regime, precip: f32) -> u16 {
    let wet = precip >= FOREST_PRECIP_MM;
    match regime {
        Regime::Tropical => {
            if wet {
                TROPICAL_FOREST
            } else {
                SAVANNA
            }
        }
        Regime::Subtropical => {
            if wet {
                TROPICAL_FOREST
            } else {
                SAVANNA
            }
        }
        Regime::HotDesert => HOT_DESERT,
        Regime::ColdDesert => COLD_DESERT,
        Regime::Mediterranean => MEDITERRANEAN,
        Regime::Temperate | Regime::ContinentalCool => {
            if wet {
                TEMPERATE_FOREST
            } else {
                GRASSLAND
            }
        }
        Regime::Boreal => BOREAL,
        Regime::Tundra => TUNDRA,
        Regime::Polar => ICE_CAP,
        Regime::Unset => BARREN,
    }
}

/// Assigns `world.biome` from climate + water for the current geography. Marine
/// hexes are `Ocean`/`Lake` (life is marine-first); land hexes take their climate
/// biome once life has colonized land (`land_colonized`), else `Barren` (§3.4).
pub fn assign_biomes(world: &mut WorldData, land_colonized: bool) {
    let n = world.cell_count() as usize;
    for i in 0..n {
        let biome = if is_wet(world, i) {
            let freshwater = world
                .water_body_id
                .get(i)
                .copied()
                .and_then(|id| world.water_bodies.get(&id))
                .map(|b| {
                    use genesis_core::data::WaterBodyKind::{Lake, SaltLake};
                    matches!(b.kind, Lake | SaltLake)
                })
                .unwrap_or(false);
            if freshwater { LAKE } else { OCEAN }
        } else if world.ice_mask.get(i).copied().unwrap_or(false) {
            ICE_CAP
        } else if !land_colonized {
            BARREN
        } else {
            let regime = world
                .climate_regime
                .get(i)
                .copied()
                .unwrap_or(Regime::Unset);
            land_biome(regime, world.precipitation[i])
        };
        world.biome[i] = BiomeId(biome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{create_world, data::BiomeId};

    #[test]
    fn ocean_is_marine_land_is_climate() {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world").data;
        // Hex 0 ocean, hex 1 tropical land.
        world.elevation_mean[0] = -2000.0;
        world.water_level_m[0] = 0.0;
        world.elevation_mean[1] = 200.0;
        world.water_level_m[1] = WATER_NONE;
        world.climate_regime[1] = Regime::Tropical;
        world.precipitation[1] = 2500.0;
        assign_biomes(&mut world, true);
        assert_eq!(world.biome[0], BiomeId(OCEAN));
        assert_eq!(world.biome[1], BiomeId(TROPICAL_FOREST));
        assert_eq!(biome_name(world.biome[1]), "Tropical forest");
    }

    #[test]
    fn no_land_biomes_before_life() {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world").data;
        world.elevation_mean[1] = 200.0;
        world.water_level_m[1] = WATER_NONE;
        world.climate_regime[1] = Regime::Temperate;
        assign_biomes(&mut world, false);
        assert_eq!(world.biome[1], BiomeId(BARREN));
    }
}
