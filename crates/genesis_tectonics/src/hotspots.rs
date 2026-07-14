//! Mantle hot spot model (Doc 06 §7).
//!
//! Formation positions use [`HOTSPOT_LOCATIONS_STREAM`] via [`WorldRng::stream`].
//! Per Geological tick, eruptions and rare spawns consume
//! [`HOTSPOT_ACTIVITY_STREAM`] via [`WorldRng::stream_at`] keyed by tick year.
//! RNG draw order each tick: ascending [`HotSpotId`] (activity roll, then magnitude
//! if erupting) → spawn gate roll → optional new-hot-spot samples.

use genesis_core::branches::BranchId;
use genesis_core::data::{BedrockType, WorldData};
use genesis_core::events::{Event, EventKind, EventLocation, Significance};
use genesis_core::grid::HexGrid;
use genesis_core::rng::WorldRng;
use genesis_core::time::WorldYear;
use genesis_core::{HexId, HotSpotId};
use glam::DVec3;
use rand::Rng;

use crate::events::{alloc_event_id, maybe_emit};
use crate::plate::{HotSpot, HotSpotRegistry, TectonicsState};
use crate::plate_surface::modify_surface_at_world_hex;

/// One-shot Formation stream for initial hot spot positions and parameters (§4.4).
pub const HOTSPOT_LOCATIONS_STREAM: &str = "tectonics.hotspot_locations";

/// Per-tick stream for eruption rolls and rare spawns (§4.4).
pub const HOTSPOT_ACTIVITY_STREAM: &str = "tectonics.hotspot_activity";

/// Probability of spawning one new hot spot per Geological tick (§7.3).
pub const SPAWN_PROBABILITY_PER_TICK: f64 = 0.0001;

pub const ACTIVITY_RATE_MIN: f64 = 0.01;
pub const ACTIVITY_RATE_MAX: f64 = 0.1;

pub const LIFESPAN_MIN_YEARS: i64 = 100_000_000;
pub const LIFESPAN_MAX_YEARS: i64 = 1_000_000_000;

pub const HOTSPOT_ELEVATION_CHANGE_MIN_M: f32 = 100.0;
pub const HOTSPOT_ELEVATION_CHANGE_MAX_M: f32 = 1000.0;

/// Cumulative uplift above this threshold makes an eruption `Notable` (§6.2).
pub const NOTABLE_CUMULATIVE_UPLIFT_M: f32 = 1000.0;

const EARTH_RADIUS_KM: f64 = 6371.0;

/// Seeds initial hot spots at Formation (§7.2).
pub fn generate_initial_hotspots(data: &WorldData, rng: &WorldRng) -> HotSpotRegistry {
    let count = hotspot_count_for_radius(data.grid.planet_radius_km());
    let mut loc_rng = rng.stream(HOTSPOT_LOCATIONS_STREAM);
    let mut registry = HotSpotRegistry::new();

    for i in 0..count {
        let anchor = sample_uniform_unit_sphere(&mut loc_rng);
        let activity_rate = loc_rng.gen_range(ACTIVITY_RATE_MIN..=ACTIVITY_RATE_MAX);
        let lifespan_years = loc_rng.gen_range(LIFESPAN_MIN_YEARS..=LIFESPAN_MAX_YEARS);

        registry.insert(HotSpot {
            id: HotSpotId(i as u16),
            anchor_position: anchor,
            activity_rate,
            age_year: WorldYear::FORMATION,
            lifespan_years,
            cumulative_uplift_m: 0.0,
        });
    }

    registry.seed_next_id(count as u16);
    registry
}

fn hotspot_count_for_radius(planet_radius_km: f64) -> usize {
    (8.0 + 16.0 * (planet_radius_km / EARTH_RADIUS_KM))
        .round()
        .max(0.0) as usize
}

fn sample_uniform_unit_sphere(rng: &mut rand::rngs::SmallRng) -> [f64; 3] {
    use std::f64::consts::PI;

    let u: f64 = rng.gen_range(0.0..1.0);
    let v: f64 = rng.gen_range(0.0..1.0);
    let theta = 2.0 * PI * u;
    let phi = (2.0_f64 * v - 1.0).acos();
    let axis = DVec3::new(phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos());
    [axis.x, axis.y, axis.z]
}

/// Hex whose center has maximum dot product with `anchor`; lowest `HexId` on ties.
pub fn hex_at_anchor(grid: &HexGrid, anchor: [f64; 3]) -> HexId {
    let anchor = DVec3::new(anchor[0], anchor[1], anchor[2]).normalize();
    let mut best_hex = HexId(0);
    let mut best_dot = f64::NEG_INFINITY;

    for hex in grid.iter() {
        let center = grid.cell_center_direction(hex);
        let center = DVec3::new(center[0], center[1], center[2]);
        let dot = anchor.dot(center);
        if dot > best_dot + f64::EPSILON {
            best_dot = dot;
            best_hex = hex;
        } else if (dot - best_dot).abs() <= f64::EPSILON && hex < best_hex {
            best_hex = hex;
        }
    }

    best_hex
}

/// Per Geological tick hot spot eruptions, expiry, and rare spawns (§7.3).
pub fn apply_hotspot_tick(
    data: &mut WorldData,
    state: &mut TectonicsState,
    rng: &WorldRng,
    tick_year: WorldYear,
    event_granularity: Significance,
    branch_id: BranchId,
) {
    let tick_value = tick_year.value();
    let mut activity_rng = rng.stream_at(HOTSPOT_ACTIVITY_STREAM, tick_value as u64);

    let ids: Vec<HotSpotId> = state.hotspots.hotspot_ids();
    let mut expired = Vec::new();

    for id in ids {
        let Some(hotspot) = state.hotspots.get(id) else {
            continue;
        };
        if tick_value - hotspot.age_year.value() > hotspot.lifespan_years {
            expired.push(id);
        }
    }

    for id in expired {
        state.hotspots.remove(id);
    }

    let active_ids = state.hotspots.hotspot_ids();
    for id in active_ids {
        let activity_rate = state.hotspots.get(id).expect("hotspot").activity_rate;
        let roll: f64 = activity_rng.gen_range(0.0..1.0);
        if roll >= activity_rate {
            continue;
        }

        let anchor = state.hotspots.get(id).expect("hotspot").anchor_position;
        let hex = hex_at_anchor(&data.grid, anchor);
        let idx = hex.0 as usize;
        if idx >= data.elevation_mean.len() {
            continue;
        }

        let elev_change: f32 =
            activity_rng.gen_range(HOTSPOT_ELEVATION_CHANGE_MIN_M..=HOTSPOT_ELEVATION_CHANGE_MAX_M);
        modify_surface_at_world_hex(&mut state.registry, data, hex, tick_value, |feature| {
            feature.elevation_m +=
                elev_change * crate::elevation::uplift_headroom_factor(feature.elevation_m);
            feature.bedrock = BedrockType::Igneous;
        });

        let hotspot = state.hotspots.hotspots_mut().get_mut(&id).expect("hotspot");
        hotspot.cumulative_uplift_m += elev_change;
        let significance = hotspot_significance(hotspot.cumulative_uplift_m);

        let event_id = alloc_event_id(state);
        maybe_emit(
            state,
            Event {
                id: event_id,
                year: tick_year,
                branch_id,
                location: EventLocation::Hex(hex),
                significance,
                kind: EventKind::HotSpotActivity {
                    hex,
                    hot_spot_id: id,
                    elevation_change_m: elev_change,
                },
            },
            event_granularity,
        );
    }

    let spawn_roll: f64 = activity_rng.gen_range(0.0..1.0);
    if spawn_roll < SPAWN_PROBABILITY_PER_TICK {
        let anchor = sample_uniform_unit_sphere(&mut activity_rng);
        let activity_rate = activity_rng.gen_range(ACTIVITY_RATE_MIN..=ACTIVITY_RATE_MAX);
        let lifespan_years = activity_rng.gen_range(LIFESPAN_MIN_YEARS..=LIFESPAN_MAX_YEARS);
        let id = state.hotspots.next_id();
        state.hotspots.insert(HotSpot {
            id,
            anchor_position: anchor,
            activity_rate,
            age_year: tick_year,
            lifespan_years,
            cumulative_uplift_m: 0.0,
        });
    }
}

fn hotspot_significance(cumulative_uplift_m: f32) -> Significance {
    if cumulative_uplift_m > NOTABLE_CUMULATIVE_UPLIFT_M {
        Significance::Notable
    } else {
        Significance::Trace
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexGrid, create_world};

    use crate::plate::{Plate, PlateType, TectonicsState};
    use crate::plate_surface::SurfaceFeature;
    use crate::world_rebuild::rebuild_world_from_plate_surfaces;

    fn earth_world_data() -> WorldData {
        create_world(WorldParameters::default())
            .expect("world")
            .data
    }

    fn setup_single_plate(data: &mut WorldData, state: &mut TectonicsState) {
        let cell_count = data.cell_count() as usize;
        if state.registry.count() == 0 {
            state.registry.insert(Plate::test_plate(
                0,
                PlateType::Continental,
                0,
                1e-8,
                cell_count,
            ));
        }
        for pid in &mut data.plate_id {
            *pid = genesis_core::PlateId(0);
        }
        for hex in data.grid.iter() {
            let idx = hex.0 as usize;
            let Some(plate) = state
                .registry
                .plates_mut()
                .get_mut(&genesis_core::PlateId(0))
            else {
                continue;
            };
            plate.surface.set(
                hex,
                SurfaceFeature {
                    elevation_m: data.elevation_mean[idx],
                    relief_m: data.elevation_relief[idx],
                    bedrock: data.bedrock_type[idx],
                    fertility: data.fertility[idx],
                    age_year: 0,
                    continental_crust: false,
                },
            );
        }
    }

    fn apply_hotspot_and_rebuild(
        data: &mut WorldData,
        state: &mut TectonicsState,
        rng: &WorldRng,
        year: WorldYear,
        granularity: Significance,
    ) {
        setup_single_plate(data, state);
        apply_hotspot_tick(
            data,
            state,
            rng,
            year,
            granularity,
            genesis_core::branches::BranchId::ROOT,
        );
        rebuild_world_from_plate_surfaces(data, &state.registry);
    }

    #[test]
    fn earth_radius_hotspot_count_matches_formula() {
        let data = earth_world_data();
        let expected = hotspot_count_for_radius(data.grid.planet_radius_km());
        let registry = generate_initial_hotspots(&data, &WorldRng::from_effective_seed(1));
        assert_eq!(registry.count(), expected);
        // §7.2: round(8 + 16 * (r / r_earth)) → 24 at Earth radius (doc prose "12-20" is approximate).
        assert_eq!(expected, 24);
    }

    #[test]
    fn initial_anchors_are_unit_length() {
        let data = earth_world_data();
        let registry = generate_initial_hotspots(&data, &WorldRng::from_effective_seed(42));
        for id in registry.hotspot_ids() {
            let hs = registry.get(id).expect("hotspot");
            let v = DVec3::new(
                hs.anchor_position[0],
                hs.anchor_position[1],
                hs.anchor_position[2],
            );
            let len = v.length();
            assert!(
                (len - 1.0).abs() < 1e-10,
                "hot spot {id:?} anchor length {len}"
            );
        }
    }

    #[test]
    fn initial_hotspot_ids_unique_and_ascending() {
        let data = earth_world_data();
        let registry = generate_initial_hotspots(&data, &WorldRng::from_effective_seed(7));
        let ids = registry.hotspot_ids();
        for window in ids.windows(2) {
            assert!(window[0] < window[1]);
        }
    }

    #[test]
    fn hex_at_anchor_tie_breaks_to_lowest_hex_id() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut max_dot = f64::NEG_INFINITY;
        let mut tied: Vec<HexId> = Vec::new();

        let probe = [0.3, 0.5, 0.8];
        let anchor = DVec3::new(probe[0], probe[1], probe[2]).normalize();

        for hex in grid.iter() {
            let center = grid.cell_center_direction(hex);
            let center = DVec3::new(center[0], center[1], center[2]);
            let dot = anchor.dot(center);
            if dot > max_dot + f64::EPSILON {
                max_dot = dot;
                tied.clear();
                tied.push(hex);
            } else if (dot - max_dot).abs() <= f64::EPSILON {
                tied.push(hex);
            }
        }

        let expected = *tied.iter().min().expect("at least one hex");
        assert_eq!(hex_at_anchor(&grid, probe), expected);
    }

    #[test]
    fn forced_activity_rate_raises_elevation() {
        let mut data = earth_world_data();
        let mut state = TectonicsState::new();
        state.hotspots.insert(HotSpot {
            id: HotSpotId(0),
            anchor_position: data.grid.cell_center_direction(HexId(10)),
            activity_rate: 1.0,
            age_year: WorldYear::FORMATION,
            lifespan_years: LIFESPAN_MAX_YEARS,
            cumulative_uplift_m: 0.0,
        });

        let hex = hex_at_anchor(
            &data.grid,
            state.hotspots.get(HotSpotId(0)).unwrap().anchor_position,
        );
        let before = data.elevation_mean[hex.0 as usize];
        let rng = WorldRng::from_effective_seed(99);

        apply_hotspot_and_rebuild(
            &mut data,
            &mut state,
            &rng,
            WorldYear(500_000),
            Significance::Trace,
        );

        assert!(data.elevation_mean[hex.0 as usize] > before);
        assert_eq!(data.bedrock_type[hex.0 as usize], BedrockType::Igneous);
    }

    #[test]
    fn hotspot_tick_is_deterministic_for_same_year() {
        let mut data_a = earth_world_data();
        let mut data_b = earth_world_data();
        let mut state_a = TectonicsState::new();
        let mut state_b = TectonicsState::new();
        let rng_seed = WorldRng::from_effective_seed(12345);

        for state in [&mut state_a, &mut state_b] {
            state.hotspots = generate_initial_hotspots(&data_a, &rng_seed);
        }

        apply_hotspot_and_rebuild(
            &mut data_a,
            &mut state_a,
            &rng_seed,
            WorldYear(500_000),
            Significance::Trace,
        );
        apply_hotspot_and_rebuild(
            &mut data_b,
            &mut state_b,
            &rng_seed,
            WorldYear(500_000),
            Significance::Trace,
        );

        assert_eq!(data_a.elevation_mean, data_b.elevation_mean);
        assert_eq!(state_a.pending_events, state_b.pending_events);
    }

    #[test]
    fn hotspot_tick_can_differ_across_years() {
        let mut data_a = earth_world_data();
        let mut data_b = earth_world_data();
        let mut state_a = TectonicsState::new();
        let mut state_b = TectonicsState::new();
        let rng = WorldRng::from_effective_seed(77);

        for (data, state) in [(&mut data_a, &mut state_a), (&mut data_b, &mut state_b)] {
            state.hotspots = generate_initial_hotspots(data, &rng);
        }

        apply_hotspot_and_rebuild(
            &mut data_a,
            &mut state_a,
            &rng,
            WorldYear(500_000),
            Significance::Trace,
        );
        apply_hotspot_and_rebuild(
            &mut data_b,
            &mut state_b,
            &rng,
            WorldYear(1_000_000),
            Significance::Trace,
        );

        assert_ne!(data_a.elevation_mean, data_b.elevation_mean);
    }

    #[test]
    fn expired_hotspot_removed_without_eruption() {
        let mut data = earth_world_data();
        let mut state = TectonicsState::new();
        let hex = HexId(5);
        let anchor = data.grid.cell_center_direction(hex);
        let before = data.elevation_mean[hex.0 as usize];

        state.hotspots.insert(HotSpot {
            id: HotSpotId(0),
            anchor_position: anchor,
            activity_rate: 1.0,
            age_year: WorldYear::FORMATION,
            lifespan_years: 1,
            cumulative_uplift_m: 0.0,
        });

        apply_hotspot_tick(
            &mut data,
            &mut state,
            &WorldRng::from_effective_seed(1),
            WorldYear(10),
            Significance::Trace,
            genesis_core::branches::BranchId::ROOT,
        );

        assert_eq!(state.hotspots.count(), 0);
        assert_eq!(data.elevation_mean[hex.0 as usize], before);
    }

    #[test]
    fn cumulative_uplift_produces_notable_significance() {
        let mut data = earth_world_data();
        let mut state = TectonicsState::new();
        let hex = HexId(20);
        let anchor = data.grid.cell_center_direction(hex);

        state.hotspots.insert(HotSpot {
            id: HotSpotId(0),
            anchor_position: anchor,
            activity_rate: 1.0,
            age_year: WorldYear::FORMATION,
            lifespan_years: LIFESPAN_MAX_YEARS,
            cumulative_uplift_m: NOTABLE_CUMULATIVE_UPLIFT_M,
        });

        apply_hotspot_tick(
            &mut data,
            &mut state,
            &WorldRng::from_effective_seed(5),
            WorldYear(500_000),
            Significance::Trace,
            genesis_core::branches::BranchId::ROOT,
        );

        assert!(
            state
                .pending_events
                .iter()
                .any(|e| e.significance == Significance::Notable),
            "expected Notable HotSpotActivity after cumulative uplift exceeds threshold"
        );
    }
}
