//! Seasonal regime and floods (Doc 08 §7): per-channel regime
//! classification, `discharge_seasonality`, the perennial/ephemeral
//! distinction, and the permafrost drainage effects.
//!
//! Annual-mean simulation, seasonally honest characterization — no
//! sub-annual stepping (§7). Snowpack (§7.2) is intra-annual only: it sets
//! the Nival seasonality and is deliberately not a §3.2 reservoir.

use genesis_core::data::{ClimateRegimePlaceholder, HydroFlags, WorldData};

use crate::partition::PERMAFROST_TEMP_C;
use crate::rivers::STREAM_CLASS_MIN_M3_YR;
use crate::routing::{FlowAccumulation, RoutingSurface};

/// Accumulated baseflow at or above which a channel stays perennial, m³/yr
/// (§7.3). Set below the Creek class boundary: baseflow sustaining even a
/// modest creek marks real perennial flow.
pub const EPHEMERAL_BASEFLOW_MIN_M3_YR: f64 = 1.0e8;
/// Seasonality stored for ephemeral channels (§7.1 "effectively ∞" — capped
/// above the monsoonal band so quartile orderings stay sane).
pub const EPHEMERAL_SEASONALITY: f32 = 10.0;

/// Coastline proximity within which a Tropical/Subtropical wet regime reads
/// as monsoonal, km (§7.1 "monsoon-flagged coasts" approximation — climate
/// exposes no monsoon flag).
pub const MONSOON_MAX_COAST_KM: f32 = 300.0;
/// Minimum annual precipitation for the monsoonal class, mm (§7.1 "strong
/// wet season" approximation).
pub const MONSOON_MIN_PRECIP_MM: f32 = 1000.0;

/// Maximum filled-surface drop to neighbors for a permafrost hex to count
/// as wetland-prone flat, m (§7.4).
pub const PERMAFROST_WETLAND_MAX_GRADIENT_M: f32 = 25.0;

/// §7.1 flow regimes (stored implicitly via `discharge_seasonality`; the
/// enum is recomputed per tick and exposed for tests and later consumers).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum FlowRegime {
    /// Low temperature range, low precip variance: ratio 1.0–1.5.
    Stable,
    /// Monsoon coasts / tropical wet-dry: ratio 3–8.
    Monsoonal,
    /// Snowmelt-driven: winters freeze, summers thaw: ratio 2–5.
    Nival,
    /// Upstream ice mask, summer-melt-fed: ratio 1.5–2.5.
    Glacial,
    /// §7.3 non-perennial: flows only in the wet pulse.
    Ephemeral,
}

/// Classifies one land hex from its basin-weighted climate (§7.1). Priority:
/// Glacial (ice signal) → Nival (hard temperature test) → Monsoonal →
/// Stable. Ephemeral is applied separately as an override in
/// [`classify_regimes`].
pub fn classify_regime(
    data: &WorldData,
    hex: u32,
    basin_temperature_c: f64,
    basin_temperature_range_c: f64,
    upstream_ice_fraction: f64,
) -> (FlowRegime, f32) {
    let winter_c = basin_temperature_c - basin_temperature_range_c / 2.0;
    let summer_c = basin_temperature_c + basin_temperature_range_c / 2.0;
    if upstream_ice_fraction > 0.0 {
        let seasonality = (1.5 + upstream_ice_fraction.min(0.5) * 2.0) as f32;
        return (FlowRegime::Glacial, seasonality.clamp(1.5, 2.5));
    }
    if winter_c < 0.0 && summer_c > 0.0 {
        // Snowpack proxy (§7.2): fraction of the seasonal swing below freezing
        // banks and releases — boosts Nival seasonality without a §3.2 reservoir.
        let snow_fraction = ((-winter_c) / basin_temperature_range_c.max(1.0)).clamp(0.0, 0.8);
        let seasonality = (2.0 + basin_temperature_range_c * 0.15 + snow_fraction * 2.0) as f32;
        return (FlowRegime::Nival, seasonality.clamp(2.0, 5.0));
    }
    let i = hex as usize;
    let monsoonal = matches!(
        data.climate_regime[i],
        ClimateRegimePlaceholder::Tropical | ClimateRegimePlaceholder::Subtropical
    ) && data.distance_to_ocean_km[i] <= MONSOON_MAX_COAST_KM
        && data.precipitation[i] >= MONSOON_MIN_PRECIP_MM;
    if monsoonal {
        let seasonality = 3.0 + f64::from(data.precipitation[i]) / 1000.0;
        return (FlowRegime::Monsoonal, (seasonality as f32).clamp(3.0, 8.0));
    }
    let seasonality = (1.0 + basin_temperature_range_c / 16.0) as f32;
    (FlowRegime::Stable, seasonality.clamp(1.0, 1.5))
}

/// §7 step: writes `discharge_seasonality`, flags EPHEMERAL channels
/// (§7.3), and flags permafrost wetlands on frozen flats (§7.4).
///
/// Basin weighting uses the upstream aggregates from the §4.3 accumulation
/// (equal-area cells, so plain upstream means are area-weighted means).
pub fn classify_regimes(data: &mut WorldData, surface: &RoutingSurface, acc: &FlowAccumulation) {
    let n = data.cell_count() as usize;
    for i in 0..n {
        if surface.is_water(data, i as u32) {
            data.discharge_seasonality[i] = 1.0;
            continue;
        }
        let upstream = acc.upstream_cells[i].max(1.0);
        let basin_temperature = acc.temperature_sum[i] / upstream;
        let basin_range = acc.temperature_range_sum[i] / upstream;
        let ice_fraction = acc.upstream_ice[i] / upstream;
        let (_regime, mut seasonality) =
            classify_regime(data, i as u32, basin_temperature, basin_range, ice_fraction);

        // §7.3: baseflow alone must sustain the channel; else it flows only
        // in the wet pulse. Applies to rendered channels (Stream class and
        // up — below that the hex is sub-hex terrain, §12.3).
        if acc.discharge_m3_yr[i] >= STREAM_CLASS_MIN_M3_YR
            && acc.baseflow_m3_yr[i] < EPHEMERAL_BASEFLOW_MIN_M3_YR
        {
            data.hydro_flags[i] |= HydroFlags::EPHEMERAL;
            seasonality = EPHEMERAL_SEASONALITY;
        }
        data.discharge_seasonality[i] = seasonality;

        // §7.4: frozen, flat ground pins the water table (groundwater pass)
        // and flags wetland-prone flats.
        if data.temperature_mean[i] < PERMAFROST_TEMP_C {
            let flat = data
                .grid
                .neighbors(genesis_core::HexId(i as u32))
                .iter()
                .all(|neighbor| {
                    let j = neighbor.0 as usize;
                    j >= n
                        || surface.filled_m[i] - surface.filled_m[j]
                            < PERMAFROST_WETLAND_MAX_GRADIENT_M
                });
            if flat {
                data.hydro_flags[i] |= HydroFlags::WETLAND;
            }
        }
    }
}

/// §7.1 flood magnitude: peak-season discharge, m³/yr — the floodplain-
/// hazard input habitability reads and §8.3 deposition weights by.
pub fn flood_magnitude_m3_yr(data: &WorldData, hex: u32) -> f64 {
    let i = hex as usize;
    f64::from(data.river_discharge_m3_yr[i]) * f64::from(data.discharge_seasonality[i])
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::data::SoilClass;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{WorldYear, create_world};

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

    fn accumulation_for(world: &WorldData, surface: &RoutingSurface) -> FlowAccumulation {
        let n = world.cell_count() as usize;
        FlowAccumulation::accumulate(
            world,
            surface,
            &vec![1.0e7; n],
            &vec![0.0; n],
            &vec![0.0; n],
        )
    }

    #[test]
    fn nival_requires_freezing_winters_and_thawing_summers() {
        let world = land_world();
        // Winter −5, summer +15: nival band.
        let (regime, seasonality) = classify_regime(&world.data, 1, 5.0, 20.0, 0.0);
        assert_eq!(regime, FlowRegime::Nival);
        assert!((2.0..=5.0).contains(&seasonality));
        // Never freezes: not nival.
        let (regime, _) = classify_regime(&world.data, 1, 15.0, 10.0, 0.0);
        assert_ne!(regime, FlowRegime::Nival);
        // Never thaws: not nival either (polar stable).
        let (regime, _) = classify_regime(&world.data, 1, -15.0, 10.0, 0.0);
        assert_ne!(regime, FlowRegime::Nival);
    }

    #[test]
    fn glacial_outranks_nival_with_upstream_ice() {
        let world = land_world();
        let (regime, seasonality) = classify_regime(&world.data, 1, 5.0, 20.0, 0.2);
        assert_eq!(regime, FlowRegime::Glacial);
        assert!((1.5..=2.5).contains(&seasonality));
    }

    #[test]
    fn monsoonal_needs_regime_coast_and_rain() {
        let mut world = land_world();
        world.data.climate_regime[1] = ClimateRegimePlaceholder::Tropical;
        world.data.distance_to_ocean_km[1] = 50.0;
        world.data.precipitation[1] = 1800.0;
        let (regime, seasonality) = classify_regime(&world.data, 1, 27.0, 6.0, 0.0);
        assert_eq!(regime, FlowRegime::Monsoonal);
        assert!((3.0..=8.0).contains(&seasonality));

        world.data.distance_to_ocean_km[1] = 800.0; // deep interior
        let (regime, _) = classify_regime(&world.data, 1, 27.0, 6.0, 0.0);
        assert_eq!(regime, FlowRegime::Stable);

        world.data.distance_to_ocean_km[1] = 50.0;
        world.data.precipitation[1] = 600.0; // too dry for a strong wet season
        let (regime, _) = classify_regime(&world.data, 1, 27.0, 6.0, 0.0);
        assert_eq!(regime, FlowRegime::Stable);
    }

    #[test]
    fn stable_is_the_low_seasonality_default() {
        let world = land_world();
        let (regime, seasonality) = classify_regime(&world.data, 1, 12.0, 8.0, 0.0);
        assert_eq!(regime, FlowRegime::Stable);
        assert!((1.0..=1.5).contains(&seasonality));
    }

    #[test]
    fn ephemeral_flags_unfed_channels_only() {
        let mut world = land_world();
        let surface = RoutingSurface::build(&world.data, &[]);
        let mut acc = accumulation_for(&world.data, &surface);
        // A Stream-class channel with negligible baseflow → ephemeral.
        let channel = 42_usize;
        acc.discharge_m3_yr[channel] = 2.0e9;
        acc.baseflow_m3_yr[channel] = 1.0e6;
        classify_regimes(&mut world.data, &surface, &acc);
        assert!(world.data.hydro_flags[channel].contains(HydroFlags::EPHEMERAL));
        assert_eq!(
            world.data.discharge_seasonality[channel],
            EPHEMERAL_SEASONALITY
        );

        // The same discharge with strong baseflow stays perennial.
        world.data.hydro_flags[channel] = HydroFlags::NONE;
        acc.baseflow_m3_yr[channel] = EPHEMERAL_BASEFLOW_MIN_M3_YR * 2.0;
        classify_regimes(&mut world.data, &surface, &acc);
        assert!(!world.data.hydro_flags[channel].contains(HydroFlags::EPHEMERAL));

        // Sub-hex flow is not a channel: no ephemeral flag.
        world.data.hydro_flags[channel] = HydroFlags::NONE;
        acc.discharge_m3_yr[channel] = 1.0e8;
        acc.baseflow_m3_yr[channel] = 1.0e6;
        classify_regimes(&mut world.data, &surface, &acc);
        assert!(!world.data.hydro_flags[channel].contains(HydroFlags::EPHEMERAL));
    }

    #[test]
    fn flood_magnitude_is_discharge_times_seasonality() {
        let mut world = land_world();
        world.data.river_discharge_m3_yr[7] = 2.0e10;
        world.data.discharge_seasonality[7] = 3.0;
        assert_eq!(flood_magnitude_m3_yr(&world.data, 7), 6.0e10);
    }
}
