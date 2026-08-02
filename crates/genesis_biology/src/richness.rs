//! Primary productivity, the biotic-richness scalar, and the saturation cap
//! (Doc 09 §4.4–§4.5, §5.2).
//!
//! `primary_productivity` is the energy base (climate + soil + water); the
//! richness scalar `R` derives from it plus climatic stability, province area,
//! and disturbance — so the latitudinal diversity gradient is **emergent** (the
//! equator comes out rich because it is productive, stable, large-area, and
//! undisturbed — not because latitude is hardcoded; Doc 09 §4.4). `R` fills the
//! `biotic_richness` array (the Diversity map layer's eventual data) and drives
//! how many guilds a province supports and how many species each holds (§4.5),
//! materialized lazily later.

use genesis_core::data::{BedrockType, WATER_NONE, WorldData};
use genesis_core::grid::HexId;

use crate::province::ProvinceRegistry;

const MARINE_BASE: f32 = 0.5;
const TEMP_PEAK_C: f32 = 22.0;
const TEMP_WIDTH_C: f32 = 22.0;
const MOISTURE_SCALE_MM: f32 = 800.0;
const STABILITY_SCALE_C: f32 = 15.0;
const AREA_SCALE_HEXES: f32 = 50.0;
const ICE_DISTURBANCE: f32 = 0.6;
/// Basal microbial richness floor for any non-ice hex — life (microbes, basal
/// producers) blankets essentially all land and ocean quickly, so almost nothing
/// is truly lifeless; only ice stays barren (Doc 09 §3.4). Tiny, so it does not
/// flatten the emergent latitudinal gradient.
const MICROBIAL_FLOOR: f32 = 0.02;
/// Recently-resurfaced volcanic crust (`Igneous` bedrock) is a young, frequently
/// disturbed substrate — the `age_since_disturbance` term beyond ice (Doc 09 §4.4).
const VOLCANIC_DISTURBANCE: f32 = 0.4;
/// Depth (m) by which marine nutrients decay from shelf-rich to abyssal-poor.
const ABYSSAL_DEPTH_M: f32 = 2000.0;
/// Open-ocean (oligotrophic) nutrient floor, relative to a productive shelf.
const DEEP_NUTRIENT_FLOOR: f32 = 0.35;

/// Guilds a maximally-rich province supports (Doc 09 §4.4, ~40 at the equator).
const MAX_OCCUPIED_GUILDS: f32 = 40.0;
const GUILD_OCCUPANCY_K: f32 = 0.4;

/// Gaussian temperature suitability, peaking at [`TEMP_PEAK_C`].
fn temp_factor(t: f32) -> f32 {
    let z = (t - TEMP_PEAK_C) / TEMP_WIDTH_C;
    (-z * z).exp()
}

/// Saturating moisture suitability from annual precipitation.
fn moisture_factor(precip_mm: f32) -> f32 {
    1.0 - (-precip_mm.max(0.0) / MOISTURE_SCALE_MM).exp()
}

/// Climatic stability: low interannual temperature range breeds specialists.
fn stability_factor(temp_range_c: f32) -> f32 {
    1.0 / (1.0 + temp_range_c.max(0.0) / STABILITY_SCALE_C)
}

/// Larger, better-connected provinces hold more (area proxy).
fn area_factor(hex_count: usize) -> f32 {
    1.0 - (-(hex_count as f32) / AREA_SCALE_HEXES).exp()
}

fn is_wet(world: &WorldData, i: usize) -> bool {
    let water = world.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
    water.is_finite() && water > world.elevation_mean[i]
}

/// Marine nutrient availability from water depth (Doc 09 §5.2): shallow
/// continental shelves and upwelling coasts are nutrient-rich (~1.0), the deep
/// open ocean is an oligotrophic desert (→ [`DEEP_NUTRIENT_FLOOR`]). A coarse
/// upwelling proxy — the fuller nutrient/current model is later.
fn marine_nutrient_factor(depth_m: f32) -> f32 {
    let x = (depth_m.max(0.0) / ABYSSAL_DEPTH_M).clamp(0.0, 1.0);
    (1.0 - x) + DEEP_NUTRIENT_FLOOR * x
}

/// Energy base per hex ∈ [0,1] (Doc 09 §5.2). Terrestrial = temperature ×
/// moisture × soil; marine = a plankton base × temperature × surface light ×
/// nutrient supply (shelf/upwelling rich, deep ocean poor).
fn productivity_at(world: &WorldData, i: usize) -> f32 {
    let temp = world.temperature_mean[i];
    if is_wet(world, i) {
        let (lat, _) = world.grid.center_lat_lon(HexId(i as u32));
        let light = (lat as f32).cos().clamp(0.0, 1.0);
        let depth = world.water_level_m[i] - world.elevation_mean[i];
        let nutrient = marine_nutrient_factor(depth);
        (MARINE_BASE * temp_factor(temp) * light * nutrient).clamp(0.0, 1.0)
    } else {
        let precip = world.precipitation[i];
        let fertility = world
            .soil_fertility
            .get(i)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        (temp_factor(temp) * moisture_factor(precip) * fertility).clamp(0.0, 1.0)
    }
}

/// Fills `world.primary_productivity` (Doc 09 §5.2).
pub fn compute_primary_productivity(world: &mut WorldData) {
    let n = world.cell_count() as usize;
    for i in 0..n {
        world.primary_productivity[i] = productivity_at(world, i);
    }
}

/// Disturbance ∈ [0,1] suppressing richness (Doc 09 §4.4 `age_since_disturbance`).
/// Independent sources compound: glacial ice cover and recently-resurfaced
/// volcanic crust (`Igneous`). Higher ⇒ younger, less-accumulated diversity.
fn disturbance(world: &WorldData, i: usize) -> f32 {
    let ice = if world.ice_mask.get(i).copied().unwrap_or(false) {
        ICE_DISTURBANCE
    } else {
        0.0
    };
    let volcanic = if world.bedrock_type.get(i).copied() == Some(BedrockType::Igneous) {
        VOLCANIC_DISTURBANCE
    } else {
        0.0
    };
    (1.0 - (1.0 - ice) * (1.0 - volcanic)).clamp(0.0, 1.0)
}

/// Number of guilds a province of richness `r` supports (Doc 09 §4.4) — a
/// saturating function of R.
pub fn occupied_guild_count(r: f32) -> u32 {
    (MAX_OCCUPIED_GUILDS * (1.0 - (-r / GUILD_OCCUPANCY_K).exp())).round() as u32
}

/// Species a guild holds at richness `r` (Doc 09 §4.5): climbs then flattens
/// (competitive exclusion) — `s_max × (1 − exp(−R / k))`.
pub fn species_in_guild(r: f32, s_max: u32, k: f32) -> u32 {
    (s_max as f32 * (1.0 - (-r.max(0.0) / k).exp())).round() as u32
}

/// Fills `world.biotic_richness` and each province's aggregated `richness` /
/// `occupied_guild_count` (Doc 09 §4.4, §5.1). Requires `primary_productivity`
/// to be current (call [`compute_primary_productivity`] first).
pub fn compute_richness(world: &mut WorldData, registry: &mut ProvinceRegistry) {
    for province in registry.provinces_mut() {
        let area = area_factor(province.hexes.len());
        let mut sum = 0.0f32;
        for &hex in &province.hexes {
            let i = hex.0 as usize;
            let prod = world.primary_productivity[i];
            let stability =
                stability_factor(world.temperature_range.get(i).copied().unwrap_or(0.0));
            let r = (prod * stability * area * (1.0 - disturbance(world, i))).clamp(0.0, 1.0);
            // Basal microbial floor on **every** hex: microbes colonize even ice
            // caps, high peaks, and deserts, so no corner of a living planet is
            // truly lifeless (only the gradient's *magnitude* differs). Ice hexes
            // get a fraction of the floor — present, but barely.
            let ice = world.ice_mask.get(i).copied().unwrap_or(false);
            let floor = if ice {
                MICROBIAL_FLOOR * 0.25
            } else {
                MICROBIAL_FLOOR
            };
            let r = r.max(floor);
            world.biotic_richness[i] = r;
            sum += r;
        }
        let mean = if province.hexes.is_empty() {
            0.0
        } else {
            sum / province.hexes.len() as f32
        };
        province.richness = mean;
        province.occupied_guild_count = occupied_guild_count(mean);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{create_world, data::WATER_NONE};

    #[test]
    fn saturation_cap_climbs_then_flattens() {
        // Monotonic, and the marginal gain shrinks (competitive exclusion, §4.5).
        let low = species_in_guild(0.1, 100, 0.4);
        let mid = species_in_guild(0.5, 100, 0.4);
        let high = species_in_guild(1.0, 100, 0.4);
        assert!(low < mid && mid < high);
        assert!((high - mid) < (mid - low), "gains must diminish");
        assert!(high <= 100);
    }

    #[test]
    fn productivity_favors_warm_wet_fertile_land() {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world").data;
        let n = world.cell_count() as usize;
        // Hex 0: warm, wet, fertile. Hex 1: cold, dry, barren.
        world.water_level_m[0] = WATER_NONE;
        world.elevation_mean[0] = 200.0;
        world.temperature_mean[0] = 22.0;
        world.precipitation[0] = 2000.0;
        world.soil_fertility[0] = 1.0;
        world.water_level_m[1] = WATER_NONE;
        world.elevation_mean[1] = 200.0;
        world.temperature_mean[1] = -20.0;
        world.precipitation[1] = 10.0;
        world.soil_fertility[1] = 0.05;
        compute_primary_productivity(&mut world);
        assert!(world.primary_productivity[0] > world.primary_productivity[1]);
        assert!(world.primary_productivity[0] > 0.3);
        let _ = n;
    }

    #[test]
    fn shallow_seas_out_produce_the_deep_ocean() {
        // Same warmth and light, different depth: a continental shelf is far more
        // productive than the abyss (nutrient upwelling, Doc 09 §5.2).
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world").data;
        // Hex 0: shallow shelf. Hex 1: deep abyss. Both warm ocean.
        world.water_level_m[0] = 0.0;
        world.elevation_mean[0] = -50.0; // 50 m deep shelf
        world.temperature_mean[0] = 22.0;
        world.water_level_m[1] = 0.0;
        world.elevation_mean[1] = -4000.0; // abyss
        world.temperature_mean[1] = 22.0;
        compute_primary_productivity(&mut world);
        assert!(
            world.primary_productivity[0] > world.primary_productivity[1],
            "shelf {} should beat abyss {}",
            world.primary_productivity[0],
            world.primary_productivity[1]
        );
    }

    #[test]
    fn volcanic_and_ice_both_suppress_richness() {
        // A freshly-resurfaced volcanic hex and an ice-covered hex both score
        // lower than an equivalent undisturbed one (disturbance term, §4.4).
        let base = disturbance_free_land();
        let plain = disturbance(&base.0, base.1);
        let mut volcanic = base.0.clone();
        volcanic.bedrock_type[base.1] = BedrockType::Igneous;
        let mut icy = base.0.clone();
        icy.ice_mask[base.1] = true;
        assert!(disturbance(&volcanic, base.1) > plain);
        assert!(disturbance(&icy, base.1) > plain);
        // Compounding: ice + volcanic disturbs more than either alone.
        let mut both = base.0.clone();
        both.bedrock_type[base.1] = BedrockType::Igneous;
        both.ice_mask[base.1] = true;
        assert!(disturbance(&both, base.1) >= disturbance(&icy, base.1));
    }

    /// A plain land world + the index of an undisturbed hex.
    fn disturbance_free_land() -> (WorldData, usize) {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world").data;
        for i in 0..world.cell_count() as usize {
            world.ice_mask[i] = false;
            world.bedrock_type[i] = BedrockType::Sedimentary;
        }
        (world, 0)
    }

    #[test]
    fn every_hex_gets_at_least_a_microbial_floor() {
        use crate::province::label_provinces;
        // Even a cold, infertile, ice-capped high peak is not fully lifeless —
        // microbes reach every corner of a living planet.
        let mut world = create_world(WorldParameters::default())
            .expect("world")
            .data;
        for i in 0..world.cell_count() as usize {
            world.water_level_m[i] = WATER_NONE;
            world.elevation_mean[i] = 4000.0;
            world.temperature_mean[i] = -30.0;
            world.precipitation[i] = 0.0;
            world.soil_fertility[i] = 0.0;
        }
        world.ice_mask[0] = true;
        let mut reg = label_provinces(&mut world);
        compute_primary_productivity(&mut world);
        compute_richness(&mut world, &mut reg);
        assert!(
            world.biotic_richness.iter().all(|&r| r > 0.0),
            "no hex should be fully lifeless"
        );
    }

    #[test]
    fn richness_tracks_productivity_and_fills_arrays() {
        use crate::province::label_provinces;
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world").data;
        // Warm wet fertile land everywhere → high productivity → high R.
        for i in 0..world.cell_count() as usize {
            world.water_level_m[i] = WATER_NONE;
            world.elevation_mean[i] = 200.0;
            world.temperature_mean[i] = 22.0;
            world.precipitation[i] = 2000.0;
            world.soil_fertility[i] = 1.0;
            world.temperature_range[i] = 2.0;
        }
        let mut registry = label_provinces(&mut world);
        compute_primary_productivity(&mut world);
        compute_richness(&mut world, &mut registry);
        assert!(
            world
                .biotic_richness
                .iter()
                .all(|&r| (0.0..=1.0).contains(&r))
        );
        assert!(world.biotic_richness[0] > 0.2, "rich land should score");
        let province = registry.iter().next().unwrap();
        assert!(province.richness > 0.0);
        assert!(province.occupied_guild_count > 0);
    }
}
