//! River classes, waterfalls, and navigability (Doc 08 §4.4–§4.5).
//!
//! Class thresholds live in `genesis_core` so render can LOD without depending
//! on this crate. Everything here is a pure derivation over stored fields.

use genesis_core::data::{BedrockType, HydroFlags, WorldData};

pub use genesis_core::data::{
    MAJOR_CLASS_MIN_M3_YR, RIVER_CLASS_MIN_M3_YR, RiverClass, STREAM_CLASS_MIN_M3_YR, river_class,
};

use crate::routing::hex_area_m2;

/// Minimum drop to the flow target for a waterfall flag, m (§4.5).
pub const WATERFALL_MIN_DROP_M: f64 = 150.0;
/// Drop at a hard→soft bedrock contact sufficient for rapids, m (§4.5).
pub const WATERFALL_CONTACT_DROP_M: f64 = 60.0;

/// Maximum drop to the flow target for a fully navigable reach, m (§4.5
/// "slope below threshold").
pub const NAVIGABLE_MAX_DROP_M: f64 = 10.0;
/// Seasonality above which a navigable reach is only seasonally so (§4.5
/// "monsoonal/nival regime, high seasonality").
pub const SEASONAL_NAVIGABILITY_THRESHOLD: f64 = 2.0;

/// Bedrock hardness rank for the §4.5 hard→soft contact test (soft = 1).
fn bedrock_hardness(bedrock: BedrockType) -> u8 {
    match bedrock {
        BedrockType::Sedimentary | BedrockType::Limestone => 1,
        BedrockType::Unknown => 2,
        BedrockType::Metamorphic => 3,
        BedrockType::Igneous | BedrockType::OceanicCrust => 4,
    }
}

/// §4.5 waterfall/rapids predicate for a channel hex: the true-elevation
/// drop to its flow target exceeds [`WATERFALL_MIN_DROP_M`], or the reach
/// crosses a hard→soft bedrock contact with a drop of at least
/// [`WATERFALL_CONTACT_DROP_M`]. The fall line — the last waterfall before
/// the sea — emerges from reading this downstream.
pub fn is_waterfall(data: &WorldData, hex: u32) -> bool {
    let i = hex as usize;
    let Some(direction) = data.flow_direction[i] else {
        return false;
    };
    let Some(&target) = data
        .grid
        .neighbors(genesis_core::HexId(hex))
        .get(direction.index())
    else {
        return false;
    };
    let j = target.0 as usize;
    let drop = f64::from(data.elevation_mean[i]) - f64::from(data.elevation_mean[j]);
    if drop > WATERFALL_MIN_DROP_M {
        return true;
    }
    drop >= WATERFALL_CONTACT_DROP_M
        && bedrock_hardness(data.bedrock_type[i]) > bedrock_hardness(data.bedrock_type[j])
}

/// §4.5 navigability class of a channel hex.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Navigability {
    /// River+ class, gentle slope, perennial, modest seasonality.
    Navigable,
    /// Navigable reach whose regime peaks too hard to run year-round.
    SeasonallyNavigable,
    /// Rapids/waterfall/ephemeral, or below River class.
    Unnavigable,
}

/// §4.5: pure derivation from stored fields (`river_discharge_m3_yr`,
/// `discharge_seasonality`, `flow_direction`, `hydro_flags`, elevations).
pub fn navigability(data: &WorldData, hex: u32) -> Navigability {
    let i = hex as usize;
    let discharge = f64::from(data.river_discharge_m3_yr[i]);
    if river_class(discharge) < RiverClass::River {
        return Navigability::Unnavigable;
    }
    if data.hydro_flags[i].contains(HydroFlags::EPHEMERAL) || is_waterfall(data, hex) {
        return Navigability::Unnavigable;
    }
    if let Some(direction) = data.flow_direction[i]
        && let Some(&target) = data
            .grid
            .neighbors(genesis_core::HexId(hex))
            .get(direction.index())
    {
        let drop =
            f64::from(data.elevation_mean[i]) - f64::from(data.elevation_mean[target.0 as usize]);
        if drop > NAVIGABLE_MAX_DROP_M {
            return Navigability::Unnavigable;
        }
    }
    if f64::from(data.discharge_seasonality[i]) > SEASONAL_NAVIGABILITY_THRESHOLD {
        return Navigability::SeasonallyNavigable;
    }
    Navigability::Navigable
}

/// Convenience: mean hex flow distance (m) for slope-style thresholds —
/// sqrt of hex area, exposed for later slope-based consumers.
pub fn hex_width_m(data: &WorldData) -> f64 {
    hex_area_m2(&data.grid).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexId, WorldYear, create_world};

    /// A world with a two-hex channel: hex 10 → its first spatial neighbor
    /// → the ocean at hex 0. Returns the channel's downstream hex.
    fn world_with_channel() -> (genesis_core::World, u32) {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        world.data.elevation_mean[0] = -100.0;
        world.data.sea_level_m = 0.0;
        let downstream = world.data.grid.neighbors(HexId(10))[0].0;
        world.data.elevation_mean[downstream as usize] = 40.0;
        world.data.elevation_mean[10] = 45.0;
        // Point 10 at its neighbor by hand.
        let slot = world
            .data
            .grid
            .neighbors(HexId(10))
            .iter()
            .position(|h| h.0 == downstream)
            .expect("neighbor slot");
        world.data.flow_direction[10] = genesis_core::grid::Direction::from_index(slot);
        (world, downstream)
    }

    #[test]
    fn classes_match_the_spec_table() {
        assert_eq!(river_class(0.5e9), RiverClass::Creek);
        assert_eq!(river_class(1.0e9), RiverClass::Stream);
        assert_eq!(river_class(9.9e9), RiverClass::Stream);
        assert_eq!(river_class(1.0e10), RiverClass::River);
        assert_eq!(river_class(7.0e10), RiverClass::River); // Rhine
        assert_eq!(river_class(1.0e11), RiverClass::Major);
        assert_eq!(river_class(5.5e12), RiverClass::Major); // Amazon
    }

    #[test]
    fn waterfall_needs_a_real_drop() {
        let (mut world, _downstream) = world_with_channel();
        assert!(!is_waterfall(&world.data, 10), "5 m drop is no waterfall");
        world.data.elevation_mean[10] = 45.0 + 200.0;
        assert!(is_waterfall(&world.data, 10), ">150 m drop is a waterfall");
    }

    #[test]
    fn hard_to_soft_contact_makes_rapids() {
        let (mut world, downstream) = world_with_channel();
        world.data.elevation_mean[10] = 45.0 + 80.0; // 80 m drop: below 150
        world.data.bedrock_type[10] = BedrockType::Igneous;
        world.data.bedrock_type[downstream as usize] = BedrockType::Sedimentary;
        assert!(is_waterfall(&world.data, 10), "hard→soft contact ≥ 60 m");
        world.data.bedrock_type[10] = BedrockType::Sedimentary;
        world.data.bedrock_type[downstream as usize] = BedrockType::Igneous;
        assert!(
            !is_waterfall(&world.data, 10),
            "soft→hard is not a contact fall"
        );
    }

    #[test]
    fn navigability_reads_class_flags_and_seasonality() {
        let (mut world, _downstream) = world_with_channel();
        world.data.river_discharge_m3_yr[10] = 5.0e10; // River class
        world.data.discharge_seasonality[10] = 1.2;
        world.data.elevation_mean[10] = 45.0; // 5 m drop, gentle
        assert_eq!(navigability(&world.data, 10), Navigability::Navigable);

        world.data.discharge_seasonality[10] = 4.5;
        assert_eq!(
            navigability(&world.data, 10),
            Navigability::SeasonallyNavigable,
            "high seasonality downgrades to seasonal"
        );

        world.data.discharge_seasonality[10] = 1.2;
        world.data.hydro_flags[10] |= HydroFlags::EPHEMERAL;
        assert_eq!(navigability(&world.data, 10), Navigability::Unnavigable);

        world.data.hydro_flags[10] = HydroFlags::NONE;
        world.data.river_discharge_m3_yr[10] = 5.0e9; // Stream class
        assert_eq!(
            navigability(&world.data, 10),
            Navigability::Unnavigable,
            "below River class is unnavigable"
        );

        world.data.river_discharge_m3_yr[10] = 5.0e10;
        world.data.elevation_mean[10] = 45.0 + 200.0; // waterfall reach
        assert_eq!(navigability(&world.data, 10), Navigability::Unnavigable);
    }
}
