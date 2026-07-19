//! Shared standing-water / ocean mask helpers (Doc 08 §17.2).
//!
//! Climate runs before hydrology each tick, so these helpers read the
//! **previous-tick** hydrology geometry when present and fall back to
//! elevation vs `sea_level_m` during Formation / pre-hydrology.

use genesis_core::HexId;
use genesis_core::data::{WaterBodyId, WaterBodyKind, WorldData};

use crate::ocean_distance::LAKE_CLIMATE_MIN_HEXES;

/// True when hex `idx` is part of the world ocean for climate purposes.
///
/// Prefers hydrology's registry (`WaterBodyKind::Ocean`). If no ocean body
/// exists yet, falls back to `elevation < sea_level`.
pub fn is_hydro_ocean(data: &WorldData, idx: usize) -> bool {
    let id = data.water_body_id[idx];
    if id != WaterBodyId::NONE
        && let Some(body) = data.water_bodies.get(&id)
    {
        return body.kind == WaterBodyKind::Ocean;
    }
    if has_any_hydro_ocean(data) {
        return false;
    }
    data.elevation_mean[idx] < data.sea_level_m
}

/// True when any registered ocean body exists.
pub fn has_any_hydro_ocean(data: &WorldData) -> bool {
    data.water_bodies
        .values()
        .any(|b| b.kind == WaterBodyKind::Ocean)
}

/// True when this hex belongs to a large inland Sea/Lake moisture source.
pub fn is_large_inland_water(data: &WorldData, idx: usize) -> bool {
    let id = data.water_body_id[idx];
    if id == WaterBodyId::NONE {
        return false;
    }
    let Some(body) = data.water_bodies.get(&id) else {
        return false;
    };
    match body.kind {
        WaterBodyKind::Sea | WaterBodyKind::Lake => {
            let hex_area = data.grid.hex_area_km2(HexId(0)).max(1e-9);
            let cells = (body.area_km2 / hex_area).round() as usize;
            cells >= LAKE_CLIMATE_MIN_HEXES
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::data::WaterBody;
    use genesis_core::parameters::WorldParameters;

    #[test]
    fn hydro_ocean_overrides_elevation_land() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        world.data.elevation_mean.fill(100.0);
        world.data.sea_level_m = 0.0;
        world.data.water_body_id[0] = WaterBodyId(0);
        world.data.water_bodies.insert(
            WaterBodyId(0),
            WaterBody {
                id: WaterBodyId(0),
                kind: WaterBodyKind::Ocean,
                surface_m: 0.0,
                area_km2: 1.0e6,
                volume_km3: 1.0e6,
                salinity: 0.0,
                outlet: None,
            },
        );
        assert!(is_hydro_ocean(&world.data, 0));
        assert!(!is_hydro_ocean(&world.data, 1));
    }

    #[test]
    fn elevation_fallback_when_no_hydro_ocean() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        world.data.elevation_mean.fill(100.0);
        world.data.elevation_mean[0] = -50.0;
        world.data.sea_level_m = 0.0;
        assert!(is_hydro_ocean(&world.data, 0));
        assert!(!is_hydro_ocean(&world.data, 1));
    }
}
