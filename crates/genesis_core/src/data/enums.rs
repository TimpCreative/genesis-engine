//! Physical-layer categorical enums stored in bulk arrays.

/// Bedrock composition for a hex (Layer 0).
///
/// Stored as a Rust enum rather than a numeric code because the variant set is
/// small, fixed in core, and used in exhaustive matches during geology and soil
/// derivation. Moddable surface categories (biomes, technologies) use content IDs
/// instead (`BiomeId`, etc.).
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum BedrockType {
    /// Default for newly initialized cells before geology runs.
    #[default]
    Unknown,
    /// Magma-derived crystalline rock.
    Igneous,
    /// Deposited by water, wind, or ice.
    Sedimentary,
    /// Transformed by heat and pressure.
    Metamorphic,
    /// Basaltic crust under oceans.
    OceanicCrust,
    /// Carbonate rock; explicit variant for the soil-fertility chain (e.g. Cretaceous
    /// beach mechanic per Doc 04).
    Limestone,
}
