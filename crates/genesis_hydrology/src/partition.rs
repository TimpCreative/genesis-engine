//! The per-hex annual water partition (Doc 08 §4.2): precipitation splits
//! into evapotranspiration, groundwater recharge, and surface runoff.
//!
//! Also owns the §6.3 karst predicate, because the KARST flag modulates
//! infiltration here (×2.0) before it diverts runoff in the groundwater pass.

use genesis_core::data::{BedrockType, HydroFlags, SoilClass, WorldData};

use crate::routing::{RoutingSurface, hex_area_m2};

/// Potential evapotranspiration intercept, mm/yr (§4.2).
pub const PET_BASE_MM: f64 = 700.0;
/// Potential evapotranspiration slope per °C of `temperature_mean` (§4.2).
pub const PET_PER_C_MM: f64 = 40.0;

/// Base infiltration fraction of available water (§4.2).
pub const INFILTRATION_BASE: f64 = 0.35;
/// Infiltration multiplier on KARST hexes (§4.2, §6.3).
pub const KARST_INFILTRATION_MULT: f64 = 2.0;
/// Infiltration multiplier on deep or Sandy soil (§4.2).
pub const DEEP_SOIL_INFILTRATION_MULT: f64 = 1.5;
/// `soil_depth_m` at or above this counts as deep soil (§4.2 "deep/Sandy").
pub const DEEP_SOIL_THRESHOLD_M: f32 = 10.0;
/// Mean temperature below which ground is frozen (§4.2, §7.4), °C.
pub const PERMAFROST_TEMP_C: f32 = -8.0;
/// Infiltration multiplier on frozen ground — it sheds (§4.2, §7.4).
pub const PERMAFROST_INFILTRATION_MULT: f64 = 0.2;
/// Infiltration base on bare rock (§4.2 "→ 0.05"; replaces the base, then
/// modulates, so karst bedrock still doubles it).
pub const BARE_ROCK_INFILTRATION: f64 = 0.05;
/// Infiltration fraction clamp range (§4.2).
pub const INFILTRATION_MIN: f64 = 0.02;
/// See [`INFILTRATION_MIN`].
pub const INFILTRATION_MAX: f64 = 0.9;

/// Minimum annual precipitation for karst development, mm (§6.3).
pub const KARST_MIN_PRECIP_MM: f32 = 400.0;

/// §4.2 potential evapotranspiration, mm/yr.
pub fn pet_mm(temperature_mean_c: f32) -> f64 {
    (PET_BASE_MM + PET_PER_C_MM * f64::from(temperature_mean_c)).max(0.0)
}

/// §6.3 karst lithology: carbonate bedrock or calcareous soil. (Tectonics
/// does not yet assign `BedrockType::Limestone` and the soil system lands in
/// Slice 3, so on a live world this is currently false everywhere — the
/// predicate is wired for when those fields populate.)
pub fn is_karst_bedrock(data: &WorldData, hex: u32) -> bool {
    let i = hex as usize;
    data.bedrock_type[i] == BedrockType::Limestone
        || data.soil_class[i] == SoilClass::Calcareous
        || (data.bedrock_type[i] == BedrockType::Sedimentary && data.fertility[i] > 0.2)
}

/// §4.2 infiltration fraction for a land hex, given its karst status.
pub fn infiltration_fraction(data: &WorldData, hex: u32, karst: bool) -> f64 {
    let i = hex as usize;
    let bare_rock = data.soil_class[i] == SoilClass::None;
    let mut fraction = if bare_rock {
        BARE_ROCK_INFILTRATION
    } else {
        INFILTRATION_BASE
    };
    if karst {
        fraction *= KARST_INFILTRATION_MULT;
    }
    if data.soil_class[i] == SoilClass::Sandy || data.soil_depth_m[i] >= DEEP_SOIL_THRESHOLD_M {
        fraction *= DEEP_SOIL_INFILTRATION_MULT;
    }
    if data.temperature_mean[i] < PERMAFROST_TEMP_C {
        fraction *= PERMAFROST_INFILTRATION_MULT;
    }
    fraction.clamp(INFILTRATION_MIN, INFILTRATION_MAX)
}

/// Per-hex partition output (m³/yr volumes), indexed by cell.
#[derive(Clone, Debug, Default)]
pub struct Partition {
    /// Surface runoff volume per land hex, m³/yr (→ §4.3 routing).
    pub runoff_m3_yr: Vec<f64>,
    /// Groundwater recharge volume per land hex, m³/yr (→ §6.1).
    pub recharge_m3_yr: Vec<f64>,
}

/// Runs §4.2 for every land hex and sets KARST flags (§6.3's precipitation
/// gate lives here because infiltration needs the flag first).
pub fn partition_land(data: &mut WorldData, surface: &RoutingSurface) -> Partition {
    let n = data.cell_count() as usize;
    let area_m2 = hex_area_m2(&data.grid);
    let mut partition = Partition {
        runoff_m3_yr: vec![0.0; n],
        recharge_m3_yr: vec![0.0; n],
    };
    for i in 0..n {
        if surface.is_water(data, i as u32) {
            continue;
        }
        // §6.3: karst flag — carbonate ground with enough rain to dissolve it.
        let karst = is_karst_bedrock(data, i as u32) && data.precipitation[i] > KARST_MIN_PRECIP_MM;
        if karst {
            data.hydro_flags[i] |= HydroFlags::KARST;
        }

        let precip = f64::from(data.precipitation[i]);
        let aet = precip.min(pet_mm(data.temperature_mean[i]));
        let available = (precip - aet).max(0.0);
        let infiltration_mm = available * infiltration_fraction(data, i as u32, karst);
        let runoff_mm = available - infiltration_mm;
        partition.recharge_m3_yr[i] = infiltration_mm * area_m2 * 1.0e-3;
        partition.runoff_m3_yr[i] = runoff_mm * area_m2 * 1.0e-3;
    }
    partition
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexId, WorldYear, create_world};

    fn land_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean[0] = -100.0;
        world.data.sea_level_m = 0.0;
        world.data.water_level_m[0] = 0.0;
        world.data.water_body_id[0] = genesis_core::WaterBodyId(0);
        for i in 1..n {
            // Flat plain: routes through the fill's +epsilon gradient with
            // no spurious index-ordered pits.
            world.data.elevation_mean[i] = 500.0;
            world.data.soil_class[i] = SoilClass::Loamy;
        }
        world
    }

    #[test]
    fn pet_matches_the_spec_line() {
        assert_eq!(pet_mm(-17.5), 0.0);
        assert_eq!(pet_mm(0.0), 700.0);
        assert_eq!(pet_mm(15.0), 1300.0);
        assert_eq!(pet_mm(30.0), 1900.0);
    }

    #[test]
    fn partition_splits_available_water() {
        let mut world = land_world();
        let hex = 10_usize;
        world.data.temperature_mean[hex] = 0.0; // PET 700
        world.data.precipitation[hex] = 1000.0; // available 300
        let surface = RoutingSurface::build(&world.data, &[]);
        let partition = partition_land(&mut world.data, &surface);
        let area_m2 = hex_area_m2(&world.data.grid);
        let expected = 300.0 * area_m2 * 1.0e-3;
        let total = partition.runoff_m3_yr[hex] + partition.recharge_m3_yr[hex];
        assert!(
            (total - expected).abs() / expected < 1e-12,
            "runoff + recharge = available: {total} vs {expected}"
        );
        // Loamy soil, no karst, unfrozen: infiltration 0.35 of available.
        let recharge_expected = 300.0 * INFILTRATION_BASE * area_m2 * 1.0e-3;
        assert!((partition.recharge_m3_yr[hex] - recharge_expected).abs() < 1e-3);
    }

    #[test]
    fn arid_hexes_generate_nothing() {
        let mut world = land_world();
        let n = world.data.cell_count() as usize;
        for i in 1..n {
            world.data.precipitation[i] = 100.0;
            world.data.temperature_mean[i] = 30.0; // PET 1900 > P
        }
        let surface = RoutingSurface::build(&world.data, &[]);
        let partition = partition_land(&mut world.data, &surface);
        assert!(partition.runoff_m3_yr.iter().all(|&r| r == 0.0));
        assert!(partition.recharge_m3_yr.iter().all(|&r| r == 0.0));
    }

    #[test]
    fn karst_flag_and_infiltration_modifiers() {
        let mut world = land_world();
        let hex = 10_usize;
        world.data.precipitation[hex] = 1200.0;
        world.data.temperature_mean[hex] = 0.0;
        world.data.bedrock_type[hex] = BedrockType::Limestone;
        let surface = RoutingSurface::build(&world.data, &[]);
        let partition = partition_land(&mut world.data, &surface);
        assert!(world.data.hydro_flags[hex].contains(HydroFlags::KARST));
        let area_m2 = hex_area_m2(&world.data.grid);
        // available = 500; Loamy base 0.35 ×2 karst = 0.7.
        let recharge_expected = 500.0 * 0.7 * area_m2 * 1.0e-3;
        assert!(
            (partition.recharge_m3_yr[hex] - recharge_expected).abs() < 1e-3,
            "karst doubles infiltration"
        );

        // Below the precipitation gate: no karst flag.
        world.data.hydro_flags[hex] = HydroFlags::NONE;
        world.data.precipitation[hex] = 300.0;
        let partition = partition_land(&mut world.data, &surface);
        assert!(!world.data.hydro_flags[hex].contains(HydroFlags::KARST));
        assert_eq!(partition.recharge_m3_yr[hex], 0.0, "300 mm < PET 700");
    }

    #[test]
    fn permafrost_and_bare_rock_modulate_infiltration() {
        let mut world = land_world();
        let hex = 10_usize;
        world.data.precipitation[hex] = 1200.0;
        world.data.temperature_mean[hex] = -20.0; // frozen: PET 0 → all available
        let fraction = infiltration_fraction(&world.data, hex as u32, false);
        assert!((fraction - INFILTRATION_BASE * PERMAFROST_INFILTRATION_MULT).abs() < 1e-12);

        world.data.temperature_mean[hex] = 10.0;
        world.data.soil_class[hex] = SoilClass::None; // bare rock
        let fraction = infiltration_fraction(&world.data, hex as u32, false);
        assert!((fraction - BARE_ROCK_INFILTRATION).abs() < 1e-12);
        // Karst still doubles the bare-rock base.
        let fraction = infiltration_fraction(&world.data, hex as u32, true);
        assert!((fraction - BARE_ROCK_INFILTRATION * KARST_INFILTRATION_MULT).abs() < 1e-12);

        // Deep sandy loam stacks to the 0.9 clamp.
        world.data.soil_class[hex] = SoilClass::Sandy;
        world.data.soil_depth_m[hex] = 25.0;
        let fraction = infiltration_fraction(&world.data, hex as u32, true);
        assert!((fraction - INFILTRATION_MAX).abs() < 1e-12);
    }

    #[test]
    fn water_hexes_are_skipped() {
        let mut world = land_world();
        world.data.precipitation.fill(2000.0);
        let surface = RoutingSurface::build(&world.data, &[]);
        let partition = partition_land(&mut world.data, &surface);
        assert_eq!(partition.runoff_m3_yr[0], 0.0);
        assert_eq!(partition.recharge_m3_yr[0], 0.0);
        assert_eq!(HexId(0).0, 0);
    }
}
