//! Core (engine-defined) world parameters.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::events::Significance;
use crate::time::{Era, WorldYear};

/// Canonical integer seed and optional original user input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldSeed {
    /// The canonical integer seed. Immutable.
    pub value: u64,
    /// The original user input, if a string. Stored for display. Immutable.
    pub user_input: Option<String>,
}

impl WorldSeed {
    /// Constructs a seed from a numeric value.
    pub fn from_integer(value: u64) -> Self {
        Self {
            value,
            user_input: None,
        }
    }

    /// Hashes `s` with XXH3_64 (platform-independent) to produce the seed value.
    pub fn from_string(s: &str) -> Self {
        let value = xxhash_rust::xxh3::xxh3_64(s.as_bytes());
        Self {
            value,
            user_input: Some(s.to_string()),
        }
    }
}

/// Ordered list of active mods. Immutable.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModManifest {
    /// Mods in load order (order matters for conflict resolution).
    pub mods: Vec<ModEntry>,
}

impl ModManifest {
    /// Canonical byte representation for effective-seed hashing (mods sorted by `id`).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut sorted: Vec<&ModEntry> = self.mods.iter().collect();
        sorted.sort_by(|a, b| a.id.cmp(&b.id));

        let mut out = Vec::new();
        for entry in sorted {
            push_len_prefixed_str(&mut out, &entry.id);
            push_len_prefixed_str(&mut out, &entry.version);
            match &entry.content_hash {
                Some(h) => {
                    out.push(1);
                    push_len_prefixed_str(&mut out, h);
                }
                None => out.push(0),
            }
        }
        out
    }
}

fn push_len_prefixed_str(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// One mod in the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModEntry {
    pub id: String,
    pub version: String,
    pub content_hash: Option<String>,
}

/// Planet physical properties.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanetParameters {
    /// Planet radius in kilometers. Earth: 6371.0. Immutable.
    pub radius_km: f64,
    /// Surface gravity in g (Earth = 1.0). Immutable.
    pub gravity_g: f32,
    /// Axial tilt in degrees. Earth: 23.4. Immutable.
    pub axial_tilt_degrees: f32,
    /// Solar luminosity relative to Sol (Earth = 1.0). Scales insolation baseline.
    pub solar_luminosity_relative_to_sol: f32,
    /// Rotation period in hours. Earth: 24.0. Immutable.
    pub rotation_period_hours: f64,
    /// Orbital period in Earth-days. Earth: 365.25. Immutable.
    pub orbital_period_days: f64,
    /// Number of suns. v1 supports 1 only. Immutable.
    pub star_count: u8,
    /// Number of moons (0–2 in v1). Immutable.
    pub moon_count: u8,
    /// Tidally locked flag. v1 supports false only. Immutable.
    pub tidally_locked: bool,
}

/// Hex grid subdivision settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridParameters {
    /// ISEA3H subdivision level (v1: 5–9). Default 8. Immutable.
    pub subdivision_level: u8,
}

/// Simulation calendar bounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimeParameters {
    /// Year at which the world begins (almost always 0). Immutable.
    pub world_start_year: WorldYear,
    /// Default user placement year after generation. Immutable.
    pub default_user_year: WorldYear,
    /// Year when automatic event generation stops. Tunable via intervention.
    pub simulation_end_year: WorldYear,
}

/// Initial geology settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeologyParameters {
    /// Continental crust coverage at world formation, as a fraction of the
    /// sphere's AREA (filled with whole plates grown as a connected cluster,
    /// so the realized value overshoots by at most one plate). Default 0.22
    /// (Hadean-ish); ~0.29 is present-day Earth.
    pub initial_continental_fraction: f32,
    /// Plate motion scale factor relative to Earth-like values. Default 1.0.
    pub plate_velocity_scale: f32,
    /// Volcanism intensity multiplier. Default 1.0.
    pub volcanism_scale: f32,
    /// Plate reorganization and general geological activity scale. Default 1.0 (Doc 06 §4.5).
    pub geology_activity_scale: f32,
    /// Number of major (large) plates at world formation. Default 7. Valid range 6-9.
    pub initial_major_plate_count: u8,
    /// Number of minor (smaller) plates at world formation. Default 8. Valid range 6-10.
    pub initial_minor_plate_count: u8,
    /// Minimum event significance to record during tectonic simulation.
    /// Events below this threshold are computed and applied but NOT logged.
    /// Default `Significance::Notable`.
    pub event_granularity: Significance,
    /// Admin/debug override for tick interval per era (years). None = use the
    /// defaults from Doc 06 §4.1.
    pub tick_interval_overrides_years: Option<BTreeMap<Era, i64>>,
    /// Base erosion rate per year per meter of elevation above sea level.
    /// Default 5e-8 (collision belts persist a few hundred My). Climate
    /// modifies via climate_modifier (Phase 2).
    pub base_erosion_rate_per_year: f64,
    /// Max land hexes in a removable ephemeral island (Doc 06 coast cleanup).
    pub max_ephemeral_island_hexes: u32,
    /// Max elevation (m above sea level) for ephemeral island removal.
    pub max_ephemeral_island_height_m: f32,
    /// Max relief (m) for ephemeral island removal.
    pub max_ephemeral_island_relief_m: f32,
    /// Max ocean hexes in a fillable artifact inland puddle.
    pub max_artifact_lake_hexes: u32,
    /// Enclosed ocean deeper than this below sea level is kept as a geologic lake (m).
    pub min_geologic_lake_depth_m: f32,
}

/// Climate simulation parameters (Doc 07).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClimateParameters {
    /// Significance threshold for climate event emission.
    pub event_granularity: Significance,
    /// Whether to skip Formation era (start at stable planet for fast worldbuilding).
    pub skip_planetary_formation: bool,
    /// Orbital eccentricity (Doc 07 §14.1). 0.0 = circular. Default 0.0.
    pub orbital_eccentricity: f32,
    /// Climate chaos intensity (Doc 07 §14.2). 0.0 = off. Default 0.0.
    pub climate_chaos_intensity: f32,
    /// Land below this height above sea level still connects ocean basins (m). Default 50.
    pub ocean_basin_sill_height_m: f32,
}

impl Default for ClimateParameters {
    fn default() -> Self {
        Self {
            event_granularity: Significance::Notable,
            skip_planetary_formation: false,
            orbital_eccentricity: 0.0,
            climate_chaos_intensity: 0.0,
            ocean_basin_sill_height_m: 120.0,
        }
    }
}

/// Initial climate boundary conditions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClimateInitialParameters {
    /// Mean global surface temperature at start (°C). Immutable.
    pub initial_mean_temperature_c: f32,
    /// Initial sea level relative to mean elevation (m). Immutable.
    pub initial_sea_level_m: f32,
    /// Sea-level atmospheric pressure (hPa). Immutable.
    pub surface_pressure_hpa: f32,
    /// Greenhouse intensity multiplier. Immutable.
    pub greenhouse_intensity: f32,
}

/// Biology system activation and rates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BiologyParameters {
    /// Year when biology activates. Default 500_000_000. Immutable.
    pub life_emergence_year: WorldYear,
    /// Mutation rate scale. Immutable.
    pub mutation_rate_scale: f32,
    /// Extinction event probability scale. Immutable.
    pub extinction_scale: f32,
}

/// Civilization emergence and rates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CivilizationParameters {
    /// Sapience emergence year; derived from biology when unset. Tunable.
    pub sapience_emergence_year: Option<WorldYear>,
    /// Technology emergence rate scale. Tunable.
    pub tech_rate_scale: f32,
    /// Cultural drift rate scale. Tunable.
    pub cultural_drift_scale: f32,
    /// Conflict frequency scale. Tunable.
    pub conflict_scale: f32,
}

/// Engine-defined parameters. Always present.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoreParameters {
    pub seed: WorldSeed,
    pub mod_manifest: ModManifest,
    pub planet: PlanetParameters,
    pub grid: GridParameters,
    pub time: TimeParameters,
    pub geology: GeologyParameters,
    pub climate_initial: ClimateInitialParameters,
    pub climate: ClimateParameters,
    pub biology: BiologyParameters,
    pub civilization: CivilizationParameters,
}
