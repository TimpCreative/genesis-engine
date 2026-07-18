//! World recipe parameters: core schema, extensions, validation, and TOML I/O.

mod core;
mod extensions;
mod serialization;
mod validation;

pub use core::{
    BiologyParameters, CivilizationParameters, ClimateInitialParameters, ClimateParameters,
    CoreParameters, GeologyParameters, GridParameters, HydrologyParameters, ModEntry, ModManifest,
    PlanetParameters, TimeParameters, WorldSeed,
};
pub use extensions::{ParameterExtensions, ParameterValue, ParameterValueData};
pub use validation::ParameterValidationError;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::events::Significance;
use crate::time::WorldYear;

/// Immutable world recipe: core parameters plus mod extensions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldParameters {
    pub core: CoreParameters,
    pub extensions: ParameterExtensions,
}

impl Default for WorldParameters {
    fn default() -> Self {
        Self {
            core: CoreParameters {
                seed: WorldSeed::from_integer(0),
                mod_manifest: ModManifest {
                    mods: vec![ModEntry {
                        id: "core".to_string(),
                        version: "0.1.0".to_string(),
                        content_hash: None,
                    }],
                },
                planet: PlanetParameters {
                    radius_km: 6371.0,
                    gravity_g: 1.0,
                    axial_tilt_degrees: 23.4,
                    solar_luminosity_relative_to_sol: 1.0,
                    rotation_period_hours: 24.0,
                    orbital_period_days: 365.25,
                    star_count: 1,
                    moon_count: 1,
                    tidally_locked: false,
                },
                grid: GridParameters {
                    subdivision_level: 7,
                },
                time: TimeParameters {
                    world_start_year: WorldYear::FORMATION,
                    default_user_year: WorldYear(4_500_000_000),
                    simulation_end_year: WorldYear(4_500_000_000),
                },
                geology: GeologyParameters {
                    initial_continental_fraction: 0.30,
                    plate_velocity_scale: 1.0,
                    volcanism_scale: 1.0,
                    geology_activity_scale: 1.0,
                    initial_major_plate_count: 7,
                    initial_minor_plate_count: 8,
                    event_granularity: Significance::Notable,
                    tick_interval_overrides_years: None,
                    base_erosion_rate_per_year: 5e-8,
                    max_ephemeral_island_hexes: 10,
                    max_ephemeral_island_height_m: 100.0,
                    max_ephemeral_island_relief_m: 250.0,
                    max_artifact_lake_hexes: 20,
                    min_geologic_lake_depth_m: 400.0,
                },
                climate_initial: ClimateInitialParameters {
                    initial_mean_temperature_c: 15.0,
                    initial_sea_level_m: 0.0,
                    surface_pressure_hpa: 1013.25,
                    greenhouse_intensity: 1.0,
                },
                climate: ClimateParameters::default(),
                hydrology: HydrologyParameters::default(),
                biology: BiologyParameters {
                    life_emergence_year: WorldYear(500_000_000),
                    mutation_rate_scale: 1.0,
                    extinction_scale: 1.0,
                },
                civilization: CivilizationParameters {
                    sapience_emergence_year: None,
                    tech_rate_scale: 1.0,
                    cultural_drift_scale: 1.0,
                    conflict_scale: 1.0,
                },
            },
            extensions: ParameterExtensions {
                fields: BTreeMap::new(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// XXH3_64 of UTF-8 `"hello world"` (xxhash-rust default XXH3_64, no custom seed).
    const HELLO_WORLD_HASH: u64 = 15_296_390_279_056_496_779;

    #[test]
    fn default_validates() {
        WorldParameters::default().validate().unwrap();
    }

    #[test]
    fn world_seed_from_string_known_hash() {
        let seed = WorldSeed::from_string("hello world");
        assert_eq!(seed.value, HELLO_WORLD_HASH);
        assert_eq!(seed.user_input.as_deref(), Some("hello world"));
    }

    #[test]
    fn world_seed_from_string_is_deterministic() {
        let a = WorldSeed::from_string("foo");
        let b = WorldSeed::from_string("foo");
        assert_eq!(a, b);
        assert_ne!(WorldSeed::from_string("foo"), WorldSeed::from_string("bar"));
    }

    #[test]
    fn toml_round_trip_default() {
        let original = WorldParameters::default();
        let text = original.to_toml_string().unwrap();
        let parsed = WorldParameters::from_toml_str(&text).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn toml_round_trip_byte_identical() {
        let params = WorldParameters::default();
        let a = params.to_toml_string().unwrap();
        let b = WorldParameters::from_toml_str(&a)
            .unwrap()
            .to_toml_string()
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_negative_radius() {
        let mut p = WorldParameters::default();
        p.core.planet.radius_km = -1.0;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidPlanetRadius(_))
        ));
    }

    #[test]
    fn rejects_zero_radius() {
        let mut p = WorldParameters::default();
        p.core.planet.radius_km = 0.0;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidPlanetRadius(_))
        ));
    }

    #[test]
    fn rejects_nan_radius() {
        let mut p = WorldParameters::default();
        p.core.planet.radius_km = f64::NAN;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidPlanetRadius(_))
        ));
    }

    #[test]
    fn rejects_axial_tilt_above_90() {
        let mut p = WorldParameters::default();
        p.core.planet.axial_tilt_degrees = 91.0;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidAxialTilt(_))
        ));
    }

    #[test]
    fn rejects_subdivision_level_4() {
        let mut p = WorldParameters::default();
        p.core.grid.subdivision_level = 4;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidSubdivisionLevel(4))
        ));
    }

    #[test]
    fn rejects_subdivision_level_10() {
        let mut p = WorldParameters::default();
        p.core.grid.subdivision_level = 10;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidSubdivisionLevel(10))
        ));
    }

    #[test]
    fn rejects_multi_star() {
        let mut p = WorldParameters::default();
        p.core.planet.star_count = 2;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::UnsupportedV1Feature { .. })
        ));
    }

    #[test]
    fn rejects_tidally_locked() {
        let mut p = WorldParameters::default();
        p.core.planet.tidally_locked = true;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::UnsupportedV1Feature { .. })
        ));
    }

    #[test]
    fn rejects_major_plate_count_out_of_range() {
        let mut p = WorldParameters::default();
        p.core.geology.initial_major_plate_count = 5;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidField { field, .. })
            if field == "geology.initial_major_plate_count"
        ));
    }

    #[test]
    fn rejects_minor_plate_count_out_of_range() {
        let mut p = WorldParameters::default();
        p.core.geology.initial_minor_plate_count = 11;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidField { field, .. })
            if field == "geology.initial_minor_plate_count"
        ));
    }

    #[test]
    fn rejects_base_erosion_rate_out_of_range() {
        let mut p = WorldParameters::default();
        p.core.geology.base_erosion_rate_per_year = 1e-2;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidField { field, .. })
            if field == "geology.base_erosion_rate_per_year"
        ));
    }

    #[test]
    fn rejects_water_inventory_out_of_band() {
        let mut p = WorldParameters::default();
        p.core.hydrology.water_inventory_gel_m = 50.0;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidField { field, .. })
            if field == "hydrology.water_inventory_gel_m"
        ));
    }

    #[test]
    fn rejects_non_positive_hydrology_factors() {
        let mut p = WorldParameters::default();
        p.core.hydrology.runoff_coefficient_base = 0.0;
        assert!(matches!(
            p.validate(),
            Err(ParameterValidationError::InvalidField { field, .. })
            if field == "hydrology.runoff_coefficient_base"
        ));
    }

    #[test]
    fn canonical_bytes_order_independent() {
        let a = ModManifest {
            mods: vec![
                ModEntry {
                    id: "b_mod".into(),
                    version: "1.0.0".into(),
                    content_hash: None,
                },
                ModEntry {
                    id: "a_mod".into(),
                    version: "2.0.0".into(),
                    content_hash: Some("abc".into()),
                },
            ],
        };
        let b = ModManifest {
            mods: vec![
                ModEntry {
                    id: "a_mod".into(),
                    version: "2.0.0".into(),
                    content_hash: Some("abc".into()),
                },
                ModEntry {
                    id: "b_mod".into(),
                    version: "1.0.0".into(),
                    content_hash: None,
                },
            ],
        };
        assert_eq!(a.canonical_bytes(), b.canonical_bytes());
    }
}
