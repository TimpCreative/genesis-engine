//! Material-footprint plate ownership (Doc 06 §3, P1-17).
//!
//! A plate's footprint is the set of birth hexes carrying surface features.
//! Ownership of a world hex derives from forward-rotating every footprint into
//! current world space: overlaps resolve by crust buoyancy (continental beats
//! oceanic; losing oceanic crust is subducted and destroyed) and gaps left by
//! diverging plates are filled with newly accreted oceanic crust. Plates
//! therefore drift as coherent material bodies — continents keep their shapes —
//! instead of being re-clipped to moving Voronoi cells each tick.
//!
//! Where two continents genuinely converge, the overlap cannot hide and
//! re-emerge on the far side: the losing feature is consumed into the
//! collision orogen (crustal shortening — India–Asia turned >1000 km of map
//! area into the Himalaya/Tibet instead of the continents passing through
//! each other), the winning feature thickens, and the pair is reported so
//! the collision jam ([`crate::collision_jam`]) can lock their motions.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use genesis_core::data::{BedrockType, WorldData};
use genesis_core::{HexId, PlateId};
use glam::DVec3;

use crate::frames::current_world_to_birth_hex;
use crate::motion::{effective_position_direction, surface_velocity_m_per_year};
use crate::plate::PlateRegistry;
use crate::plate_surface::SurfaceFeature;
use crate::projection::ProjectionCache;

/// Elevation of freshly accreted oceanic crust at divergent gaps (m).
/// Young ridge crust sits above the abyssal equilibrium; thermal subsidence
/// sinks it toward [`crate::elevation::OCEAN_FLOOR_BASELINE_M`] as it ages.
pub const NEW_CRUST_ELEVATION_M: f32 = -2700.0;

/// Minimum closing speed (m/year) for a contested hex to count as real
/// convergence. Footprint projection jitters ±1 hex from quantization, so
/// contests and gaps at boundaries that are not actually moving toward or away
/// from each other must not subduct crust or mint ridges.
pub const CONVERGENCE_THRESHOLD_M_PER_YEAR: f64 = 0.005;

/// Per-plate motion snapshot for relative-velocity tests.
struct PlateMotion {
    center: DVec3,
    axis: [f64; 3],
    rate_rad_per_year: f64,
}

/// Signed closing speed between plates `a` and `b` at surface point `p`
/// (unit direction). Positive: converging; negative: diverging; near zero:
/// transform motion or static jitter. Symmetric in `a`/`b`.
fn closing_speed_m_per_year(p: DVec3, a: &PlateMotion, b: &PlateMotion, radius_km: f64) -> f64 {
    let v_a = surface_velocity_m_per_year([p.x, p.y, p.z], a.axis, a.rate_rad_per_year, radius_km);
    let v_b = surface_velocity_m_per_year([p.x, p.y, p.z], b.axis, b.rate_rad_per_year, radius_km);
    let v_rel = v_a - v_b;

    // Direction from a's center toward b's center, projected onto the tangent
    // plane at p; positive relative velocity along it closes the boundary.
    let d = b.center - a.center;
    let d_tangent = d - p * d.dot(p);
    if d_tangent.length_squared() < 1e-18 {
        return 0.0;
    }
    v_rel.dot(d_tangent.normalize())
}

/// One projected footprint claim on a world hex.
#[derive(Clone, Copy)]
struct Claim {
    plate_id: PlateId,
    birth_hex: HexId,
    continental: bool,
    /// Plate creation year (smaller = older plate).
    plate_age_year: i64,
    /// Among same-plate quantization collisions on this hex, the feature
    /// `world_rebuild` will display (priority: elevation, then age, then lower
    /// birth index). Recorded for the projection cache.
    disp_birth: HexId,
    disp_elev: f32,
    disp_age: i64,
}

/// Returns true when `a` outranks `b` for ownership of a contested hex.
/// Priority: continental crust (buoyant) > older plate > lower id.
///
/// Deliberately independent of per-hex elevation: a per-hex criterion would
/// flip ownership hex-by-hex across an overlap zone (checkerboard), turning the
/// whole zone into phantom cross-plate boundary. Plate-level criteria give the
/// overlap one coherent owner and one clean boundary line.
fn claim_beats(a: &Claim, b: &Claim) -> bool {
    (
        a.continental,
        std::cmp::Reverse(a.plate_age_year),
        std::cmp::Reverse(a.plate_id.0),
    ) > (
        b.continental,
        std::cmp::Reverse(b.plate_age_year),
        std::cmp::Reverse(b.plate_id.0),
    )
}

/// Converging continental-continental contact hexes needed before the pair
/// counts as a collision and is reported via
/// [`RepartitionOutcome::colliding_pairs`]. The plates' slowdown is NOT
/// scripted here: the choked trench removes the pair's slab pull, and the
/// collision jam ([`crate::collision_jam`]) locks their motions into a shared
/// drift over ~10 My.
pub const COLLISION_CONTACT_HEXES: usize = 3;

/// Elevation credited to the overriding feature for each continental feature
/// consumed by crustal shortening (m), before the isostatic headroom taper
/// ([`crate::elevation::uplift_headroom_factor`]). The consumed column's
/// crustal volume mostly feeds the orogen's root; the surface expression per
/// consumed hex is small because the §5.2 orogeny pass carries the belt's
/// elevation. The consumption itself is the point: overlapped map area
/// disappears into the belt instead of tunneling through.
pub const SHORTENING_UPLIFT_M: f32 = 250.0;

/// Relief added per consumed feature, as a fraction of [`SHORTENING_UPLIFT_M`].
pub const SHORTENING_RELIEF_FRACTION: f32 = 0.3;

/// A continental feature destroyed by crustal shortening at a converging
/// continental-continental contest, and the overriding feature it feeds.
struct ConsumedCrust {
    loser_plate: PlateId,
    loser_birth: HexId,
    winner_plate: PlateId,
    winner_birth: HexId,
}

/// Result of a repartition pass: plates in active continental collision this
/// tick (derived from `colliding_pairs`, kept for tests), the colliding plate
/// pairs (for the suturing weld in [`crate::suture`]), and the world→birth
/// projection table derived from the claims (for the rest of the tick's
/// surface lookups).
pub struct RepartitionOutcome {
    pub colliding: BTreeSet<PlateId>,
    pub colliding_pairs: BTreeSet<(PlateId, PlateId)>,
    pub projection: ProjectionCache,
}

/// Recomputes hex ownership from plate footprints.
///
/// 1. Projects every plate's birth features to current world hexes (claims).
/// 2. Resolves contested hexes by [`claim_beats`]; oceanic crust that loses a
///    cross-plate contest is subducted (its birth feature is deleted).
/// 3. Fills unclaimed hexes by multi-source BFS from claimed hexes and accretes
///    new oceanic crust on the adopting plate: divergent gaps (touching ≥2
///    plates or no claimed neighbor) get young ridge crust, single-plate
///    quantization gaps are patched with the neighbors' mean elevation.
pub fn repartition_hexes(data: &mut WorldData, registry: &mut PlateRegistry) -> RepartitionOutcome {
    let n = data.plate_id.len();
    let tick_year = data.current_year.value();
    let radius_km = data.parameters.core.planet.radius_km;

    let motions: BTreeMap<PlateId, PlateMotion> = registry
        .iter_sorted()
        .map(|(id, plate)| {
            let c = effective_position_direction(&data.grid, plate);
            (
                id,
                PlateMotion {
                    center: DVec3::new(c[0], c[1], c[2]),
                    axis: plate.motion_axis,
                    rate_rad_per_year: plate.motion_rate_rad_per_year,
                },
            )
        })
        .collect();
    let converging_at = |hex: HexId, a: PlateId, b: PlateId| -> bool {
        let (Some(ma), Some(mb)) = (motions.get(&a), motions.get(&b)) else {
            return false;
        };
        let dir = data.grid.cell_center_direction(hex);
        let p = DVec3::new(dir[0], dir[1], dir[2]);
        closing_speed_m_per_year(p, ma, mb, radius_km) > CONVERGENCE_THRESHOLD_M_PER_YEAR
    };
    let diverging_at = |hex: HexId, a: PlateId, b: PlateId| -> bool {
        let (Some(ma), Some(mb)) = (motions.get(&a), motions.get(&b)) else {
            return false;
        };
        let dir = data.grid.cell_center_direction(hex);
        let p = DVec3::new(dir[0], dir[1], dir[2]);
        closing_speed_m_per_year(p, ma, mb, radius_km) < -CONVERGENCE_THRESHOLD_M_PER_YEAR
    };

    let mut winner: Vec<Option<Claim>> = vec![None; n];
    let mut subducted: Vec<(PlateId, HexId)> = Vec::new();
    let mut consumed: Vec<ConsumedCrust> = Vec::new();
    let mut cc_collisions: BTreeMap<(PlateId, PlateId), usize> = BTreeMap::new();

    {
        let grid = &data.grid;
        let mut forward_hints: Vec<(PlateId, HexId, HexId)> = Vec::new();
        for (plate_id, plate) in registry.iter_sorted() {
            let plate_age_year = plate.age_year.value();
            let q = crate::frames::plate_forward_quat(plate);
            for (birth_idx, slot) in plate.surface.features.iter().enumerate() {
                let Some(feature) = slot else {
                    continue;
                };
                // Buoyancy is a property of the crust, not the owning plate:
                // ocean floor accreted onto a continental plate subducts like
                // any other oceanic crust.
                let continental = feature.continental_crust;
                let birth_hex = HexId(birth_idx as u32);
                let hint = plate.forward_hint(birth_hex).unwrap_or(birth_hex);
                let current_world =
                    crate::frames::birth_hex_to_current_world_with_quat(grid, birth_hex, q, hint);
                forward_hints.push((plate_id, birth_hex, current_world));
                let w = current_world.0 as usize;
                if w >= n {
                    continue;
                }
                let candidate = Claim {
                    plate_id,
                    birth_hex,
                    continental,
                    plate_age_year,
                    disp_birth: birth_hex,
                    disp_elev: feature.elevation_m,
                    disp_age: feature.age_year,
                };
                match winner[w].as_mut() {
                    None => winner[w] = Some(candidate),
                    Some(existing) => {
                        // Same-plate collisions are quantization artifacts, not
                        // convergence; keep both features and let world_rebuild's
                        // display priority pick. Track that pick for the cache.
                        if existing.plate_id == plate_id {
                            let display_wins = feature.elevation_m > existing.disp_elev
                                || (feature.elevation_m == existing.disp_elev
                                    && feature.age_year > existing.disp_age);
                            if display_wins {
                                existing.disp_birth = birth_hex;
                                existing.disp_elev = feature.elevation_m;
                                existing.disp_age = feature.age_year;
                            }
                            continue;
                        }
                        // Crust is only destroyed where the plates are actually
                        // closing; overlaps from projection jitter at passive or
                        // transform contacts keep both features.
                        let real_convergence =
                            converging_at(current_world, plate_id, existing.plate_id);
                        let cc_shortening = real_convergence && existing.continental && continental;
                        if cc_shortening {
                            let pair = if existing.plate_id < plate_id {
                                (existing.plate_id, plate_id)
                            } else {
                                (plate_id, existing.plate_id)
                            };
                            *cc_collisions.entry(pair).or_default() += 1;
                        }
                        if claim_beats(&candidate, existing) {
                            if real_convergence && !existing.continental {
                                subducted.push((existing.plate_id, existing.birth_hex));
                            }
                            if cc_shortening {
                                consumed.push(ConsumedCrust {
                                    loser_plate: existing.plate_id,
                                    loser_birth: existing.birth_hex,
                                    winner_plate: plate_id,
                                    winner_birth: birth_hex,
                                });
                            }
                            *existing = candidate;
                        } else if cc_shortening {
                            consumed.push(ConsumedCrust {
                                loser_plate: plate_id,
                                loser_birth: birth_hex,
                                winner_plate: existing.plate_id,
                                winner_birth: existing.birth_hex,
                            });
                        } else if real_convergence && !continental {
                            subducted.push((plate_id, birth_hex));
                        }
                        // Continental crust at a passive or transform contact
                        // stays on its plate (buoyant); it is hidden while
                        // overridden and re-emerges if the plates separate.
                    }
                }
            }
        }
        for (plate_id, birth_hex, world_hex) in forward_hints {
            if let Some(plate) = registry.plates_mut().get_mut(&plate_id) {
                plate.set_forward_hint(birth_hex, world_hex);
            }
        }
    }

    // Destroy subducted oceanic crust.
    for (plate_id, birth_hex) in subducted {
        if let Some(plate) = registry.plates_mut().get_mut(&plate_id) {
            plate.surface.clear(birth_hex);
        }
    }

    // Crustal shortening: destroy continental crust consumed at converging
    // continental contacts and thicken the overriding feature. The overlap
    // becomes orogen instead of hiding under the other continent and
    // re-emerging on the far side — colliding continents weld into a mountain
    // belt with land on both sides, and continental area gets the sink that
    // balances accretion over deep time.
    for c in consumed {
        if let Some(loser) = registry.plates_mut().get_mut(&c.loser_plate) {
            loser.surface.clear(c.loser_birth);
        }
        if let Some(winner_plate) = registry.plates_mut().get_mut(&c.winner_plate)
            && let Some(feature) = winner_plate.surface.get(c.winner_birth).cloned()
        {
            let headroom = f64::from(crate::elevation::uplift_headroom_factor(
                feature.elevation_m,
            ));
            let uplift = f64::from(SHORTENING_UPLIFT_M) * headroom;
            winner_plate.surface.set(
                c.winner_birth,
                crate::plate_surface::SurfaceFeature {
                    elevation_m: (feature.elevation_m + uplift as f32)
                        .min(crate::elevation::MAX_ELEVATION_M),
                    relief_m: (feature.relief_m
                        + (uplift * f64::from(SHORTENING_RELIEF_FRACTION)) as f32)
                        .min(crate::elevation::MAX_RELIEF_M),
                    bedrock: BedrockType::Metamorphic,
                    fertility: feature.fertility,
                    age_year: tick_year,
                    continental_crust: feature.continental_crust,
                    // Doc 06 §5.2 roots: shortening thickens crust — the
                    // weld keeps its accumulated root and banks the uplift.
                    root_m: (feature.root_m
                        + (uplift * crate::elevation::ROOT_BANK_FRACTION) as f32)
                        .min(crate::elevation::ROOT_MAX_M),
                },
            );
        }
    }

    // Continental collision pairs: converging continental contacts past the
    // collision threshold. Reported to the next tick's collision jam
    // ([`crate::collision_jam`]), which locks the pair's motions — no rate
    // mutation here.
    let mut colliding: BTreeSet<PlateId> = BTreeSet::new();
    let mut colliding_pairs: BTreeSet<(PlateId, PlateId)> = BTreeSet::new();
    for ((a, b), count) in &cc_collisions {
        if *count < COLLISION_CONTACT_HEXES {
            continue;
        }
        colliding_pairs.insert((*a, *b));
        for id in [a, b] {
            colliding.insert(*id);
        }
    }

    // Ownership from claims; gaps filled by BFS adoption below.
    let mut owner: Vec<PlateId> = vec![PlateId::NONE; n];
    let mut queue = VecDeque::new();
    for (i, claim) in winner.iter().enumerate() {
        if let Some(claim) = claim {
            owner[i] = claim.plate_id;
            queue.push_back(i);
        }
    }

    // No claims at all (empty registry): leave ownership untouched.
    if queue.is_empty() {
        return RepartitionOutcome {
            colliding,
            colliding_pairs,
            projection: ProjectionCache::empty(),
        };
    }

    let mut adopted: Vec<usize> = Vec::new();
    {
        let grid = &data.grid;
        while let Some(i) = queue.pop_front() {
            let hex = HexId(i as u32);
            let neighbors = grid.neighbors_sorted(hex);
            for &neighbor in neighbors {
                let j = neighbor.0 as usize;
                if j >= n || owner[j] != PlateId::NONE {
                    continue;
                }
                owner[j] = owner[i];
                adopted.push(j);
                queue.push_back(j);
            }
        }
    }

    // Projection cache: claimed hexes map straight to the claim's displayed
    // birth hex; adopted hexes get theirs from the inverse rotation below.
    let mut projection = ProjectionCache::with_ownership(&owner);
    for (i, claim) in winner.iter().enumerate() {
        if let Some(claim) = claim {
            projection.record(i, claim.disp_birth, true);
        }
    }

    // Accrete new oceanic crust on adopted hexes (in BFS discovery order).
    for &j in &adopted {
        let hex = HexId(j as u32);
        let plate_id = owner[j];

        // Record the adopted hex's birth mapping so same-tick surface writes
        // and reads resolve without re-deriving the rotation.
        let Some(plate) = registry.get(plate_id) else {
            continue;
        };
        let birth_hex = current_world_to_birth_hex(&data.grid, hex, plate);
        projection.record(j, birth_hex, false);

        // Classify the gap from pre-fill claims. New crust is minted only for
        // genuinely opening gaps: zero claimed neighbors (interior of a wide
        // rift) or at least one pair of neighboring plates actually diverging
        // here. Quantization holes and jitter gaps at passive contacts adopt
        // ownership without minting, or footprints inflate every tick.
        let mut claim_plates: Vec<PlateId> = Vec::new();
        {
            let grid = &data.grid;
            let neighbors = grid.neighbors_sorted(hex);
            for &neighbor in neighbors {
                let k = neighbor.0 as usize;
                if k >= n {
                    continue;
                }
                if let Some(claim) = &winner[k]
                    && !claim_plates.contains(&claim.plate_id)
                {
                    claim_plates.push(claim.plate_id);
                }
            }
        }
        let opening = claim_plates.is_empty()
            || claim_plates.iter().enumerate().any(|(ai, &a)| {
                claim_plates[ai + 1..]
                    .iter()
                    .any(|&b| diverging_at(hex, a, b))
            });
        if !opening {
            continue;
        }

        let Some(plate) = registry.plates_mut().get_mut(&plate_id) else {
            continue;
        };
        // Never overwrite an existing feature: quantization can map the new
        // world hex onto an occupied birth slot.
        if plate.surface.get(birth_hex).is_none() {
            plate.surface.set(
                birth_hex,
                SurfaceFeature {
                    elevation_m: NEW_CRUST_ELEVATION_M,
                    relief_m: 0.0,
                    bedrock: BedrockType::OceanicCrust,
                    fertility: 0.0,
                    age_year: tick_year,
                    continental_crust: false,
                    root_m: 0.0,
                },
            );
            // Fresh ridge crust projects onto exactly this hex; rebuilds may
            // display it immediately.
            projection.record(j, birth_hex, true);
        }
    }

    data.plate_id.copy_from_slice(&owner);
    RepartitionOutcome {
        colliding,
        colliding_pairs,
        projection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::data::BedrockType;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexGrid, HexId, PlateId};

    use crate::frames::birth_hex_to_current_world;
    use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};
    use crate::plate_surface::{PlateSurface, SurfaceFeature};

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn plate_at(id: u16, seed: u32, axis: [f64; 3], rotation: f64, plate_type: PlateType) -> Plate {
        Plate {
            id: PlateId(id),
            plate_type,
            plate_class: PlateClass::Major,
            seed_hex: HexId(seed),
            motion_axis: axis,
            motion_rate_rad_per_year: 1e-8,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: rotation,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(10_000),
            forward_world_hint: Vec::new(),
        }
    }

    fn feature(elevation_m: f32, bedrock: BedrockType) -> SurfaceFeature {
        SurfaceFeature {
            elevation_m,
            relief_m: 0.0,
            bedrock,
            fertility: 0.0,
            age_year: 0,
            continental_crust: bedrock != BedrockType::OceanicCrust,
            root_m: 0.0,
        }
    }

    fn test_world(level: u8) -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = level;
        genesis_core::create_world(params).expect("world")
    }

    /// Fills plate 0 with features on hexes [0, split) and plate 1 on [split, n).
    fn two_plate_registry(n: usize, split: usize) -> PlateRegistry {
        let mut registry = PlateRegistry::new();
        let mut a = plate_at(0, 0, [0.0, 0.0, 1.0], 0.0, PlateType::Continental);
        let mut b = plate_at(1, 1000, [1.0, 0.0, 0.0], 0.0, PlateType::Oceanic);
        a.surface = PlateSurface::new(n);
        b.surface = PlateSurface::new(n);
        for i in 0..split {
            a.surface
                .set(HexId(i as u32), feature(800.0, BedrockType::Igneous));
        }
        for i in split..n {
            b.surface
                .set(HexId(i as u32), feature(-3500.0, BedrockType::OceanicCrust));
        }
        registry.insert(a);
        registry.insert(b);
        registry
    }

    #[test]
    fn zero_rotation_ownership_matches_footprints() {
        let mut world = test_world(5);
        let n = world.data.cell_count() as usize;
        let split = n / 2;
        let mut registry = two_plate_registry(n, split);

        world.data.plate_id.fill(PlateId::NONE);
        repartition_hexes(&mut world.data, &mut registry);

        for i in 0..n {
            let expected = if i < split { PlateId(0) } else { PlateId(1) };
            assert_eq!(world.data.plate_id[i], expected, "hex {i}");
        }
    }

    #[test]
    fn rotation_changes_some_assignments() {
        let mut world = test_world(5);
        let n = world.data.cell_count() as usize;
        let mut registry = two_plate_registry(n, n / 2);

        world.data.plate_id.fill(PlateId::NONE);
        repartition_hexes(&mut world.data, &mut registry);
        let before = world.data.plate_id.clone();

        if let Some(p) = registry.plates_mut().get_mut(&PlateId(1)) {
            p.accumulated_rotation_rad = 0.3;
        }
        repartition_hexes(&mut world.data, &mut registry);

        let changed = before
            .iter()
            .zip(world.data.plate_id.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 0,
            "expected some hexes to change plate after rotation"
        );
    }

    /// Brute-force nearest hex to a unit direction (test helper).
    fn nearest_hex_to(world: &genesis_core::World, dir: [f64; 3]) -> HexId {
        let mut best = HexId(0);
        let mut best_dot = f64::NEG_INFINITY;
        for hex in world.data.grid.iter() {
            let c = world.data.grid.cell_center_direction(hex);
            let d = c[0] * dir[0] + c[1] * dir[1] + c[2] * dir[2];
            if d > best_dot {
                best_dot = d;
                best = hex;
            }
        }
        best
    }

    /// Plates centered at ±X with opposite Z-axis spins. At a point near +Y the
    /// relative motion is purely along ±X: `a_spin = +1, b_spin = -1` converges,
    /// swapping the spins diverges.
    fn opposed_plates(
        world: &genesis_core::World,
        n: usize,
        a_spin: f64,
        b_spin: f64,
        a_type: PlateType,
        b_type: PlateType,
    ) -> (PlateRegistry, HexId) {
        let seed_a = nearest_hex_to(world, [1.0, 0.0, 0.0]);
        let seed_b = nearest_hex_to(world, [-1.0, 0.0, 0.0]);
        let contested = nearest_hex_to(world, [0.0, 1.0, 0.0]);

        let mut registry = PlateRegistry::new();
        let mut a = plate_at(0, seed_a.0, [0.0, 0.0, a_spin], 0.0, a_type);
        let mut b = plate_at(1, seed_b.0, [0.0, 0.0, b_spin], 0.0, b_type);
        // Fast spins so the closing speed clears the jitter threshold.
        a.motion_rate_rad_per_year = 1e-7;
        b.motion_rate_rad_per_year = 1e-7;
        a.surface = PlateSurface::new(n);
        b.surface = PlateSurface::new(n);
        registry.insert(a);
        registry.insert(b);
        (registry, contested)
    }

    #[test]
    fn continental_claim_beats_converging_oceanic_and_subducts_it() {
        let mut world = test_world(5);
        let n = world.data.cell_count() as usize;
        // a spins +Z, b spins -Z → converging near +Y.
        let (mut registry, contested) = opposed_plates(
            &world,
            n,
            1.0,
            -1.0,
            PlateType::Continental,
            PlateType::Oceanic,
        );
        registry
            .plates_mut()
            .get_mut(&PlateId(0))
            .unwrap()
            .surface
            .set(contested, feature(800.0, BedrockType::Igneous));
        registry
            .plates_mut()
            .get_mut(&PlateId(1))
            .unwrap()
            .surface
            .set(contested, feature(-3000.0, BedrockType::OceanicCrust));

        world.data.plate_id.fill(PlateId::NONE);
        repartition_hexes(&mut world.data, &mut registry);

        assert_eq!(world.data.plate_id[contested.0 as usize], PlateId(0));
        assert!(
            registry
                .get(PlateId(1))
                .unwrap()
                .surface
                .get(contested)
                .is_none(),
            "losing oceanic crust at a converging contact should be subducted"
        );
        assert!(
            registry
                .get(PlateId(0))
                .unwrap()
                .surface
                .get(contested)
                .is_some(),
            "winning continental crust remains"
        );
    }

    #[test]
    fn repartition_reports_continental_collision_without_scripted_damping() {
        let mut world = test_world(5);
        let n = world.data.cell_count() as usize;
        // Opposed spins → converging near +Y; both plates fully continental.
        let (mut registry, _contested) = opposed_plates(
            &world,
            n,
            1.0,
            -1.0,
            PlateType::Continental,
            PlateType::Continental,
        );
        // Full-sphere overlapping continental footprints: the converging
        // contact is far wider than the COLLISION_CONTACT_HEXES threshold.
        for id in [PlateId(0), PlateId(1)] {
            let plate = registry.plates_mut().get_mut(&id).unwrap();
            for i in 0..n {
                plate
                    .surface
                    .set(HexId(i as u32), feature(800.0, BedrockType::Igneous));
            }
        }
        let rate_of = |registry: &PlateRegistry, id: PlateId| {
            registry.get(id).unwrap().motion_rate_rad_per_year
        };
        let a_before = rate_of(&registry, PlateId(0));
        let b_before = rate_of(&registry, PlateId(1));

        world.data.plate_id.fill(PlateId::NONE);
        let outcome = repartition_hexes(&mut world.data, &mut registry);

        assert!(
            outcome.colliding.contains(&PlateId(0)),
            "wide converging CC contact must report a collision"
        );
        assert!(outcome.colliding.contains(&PlateId(1)));
        assert!(
            outcome.colliding_pairs.contains(&(PlateId(0), PlateId(1))),
            "the pair is reported for the suturing weld"
        );
        assert_eq!(
            rate_of(&registry, PlateId(0)),
            a_before,
            "collision slowdown is emergent (slab loss), never scripted damping"
        );
        assert_eq!(rate_of(&registry, PlateId(1)), b_before);
    }

    #[test]
    fn passive_overlap_does_not_subduct() {
        let mut world = test_world(5);
        let n = world.data.cell_count() as usize;
        // Same spin → zero relative motion anywhere: any overlap is jitter.
        let (mut registry, contested) = opposed_plates(
            &world,
            n,
            1.0,
            1.0,
            PlateType::Continental,
            PlateType::Oceanic,
        );
        registry
            .plates_mut()
            .get_mut(&PlateId(0))
            .unwrap()
            .surface
            .set(contested, feature(800.0, BedrockType::Igneous));
        registry
            .plates_mut()
            .get_mut(&PlateId(1))
            .unwrap()
            .surface
            .set(contested, feature(-3000.0, BedrockType::OceanicCrust));

        world.data.plate_id.fill(PlateId::NONE);
        repartition_hexes(&mut world.data, &mut registry);

        assert_eq!(world.data.plate_id[contested.0 as usize], PlateId(0));
        assert!(
            registry
                .get(PlateId(1))
                .unwrap()
                .surface
                .get(contested)
                .is_some(),
            "oceanic crust at a passive contact must NOT be subducted"
        );
    }

    #[test]
    fn converging_continental_contest_consumes_loser_into_orogen() {
        let mut world = test_world(5);
        let n = world.data.cell_count() as usize;
        // a spins +Z, b spins -Z → converging near +Y; both fully continental.
        let (mut registry, contested) = opposed_plates(
            &world,
            n,
            1.0,
            -1.0,
            PlateType::Continental,
            PlateType::Continental,
        );
        registry
            .plates_mut()
            .get_mut(&PlateId(0))
            .unwrap()
            .surface
            .set(contested, feature(800.0, BedrockType::Igneous));
        registry
            .plates_mut()
            .get_mut(&PlateId(1))
            .unwrap()
            .surface
            .set(contested, feature(900.0, BedrockType::Igneous));

        world.data.plate_id.fill(PlateId::NONE);
        repartition_hexes(&mut world.data, &mut registry);

        // The older/lower-id plate wins the claim; the loser's crust does NOT
        // hide and re-emerge later — it is consumed into the collision orogen.
        assert_eq!(world.data.plate_id[contested.0 as usize], PlateId(0));
        assert!(
            registry
                .get(PlateId(1))
                .unwrap()
                .surface
                .get(contested)
                .is_none(),
            "losing continental crust at a converging suture is consumed"
        );
        let winner_feature = registry
            .get(PlateId(0))
            .unwrap()
            .surface
            .get(contested)
            .expect("winner keeps its feature");
        assert!(
            winner_feature.elevation_m > 800.0,
            "overriding crust thickens by SHORTENING_UPLIFT_M: {}",
            winner_feature.elevation_m
        );
        assert_eq!(winner_feature.bedrock, BedrockType::Metamorphic);
    }

    #[test]
    fn losing_continental_crust_survives_contest() {
        let mut world = test_world(5);
        let n = world.data.cell_count() as usize;
        let mut registry = PlateRegistry::new();

        let contested = HexId(40);
        let mut a = plate_at(0, 0, [0.0, 0.0, 1.0], 0.0, PlateType::Continental);
        let mut b = plate_at(1, 1000, [1.0, 0.0, 0.0], 0.0, PlateType::Continental);
        a.surface = PlateSurface::new(n);
        b.surface = PlateSurface::new(n);
        a.surface
            .set(contested, feature(2000.0, BedrockType::Igneous));
        b.surface
            .set(contested, feature(900.0, BedrockType::Igneous));
        registry.insert(a);
        registry.insert(b);

        world.data.plate_id.fill(PlateId::NONE);
        repartition_hexes(&mut world.data, &mut registry);

        assert_eq!(
            world.data.plate_id[contested.0 as usize],
            PlateId(0),
            "higher continental crust wins the contest"
        );
        assert!(
            registry
                .get(PlateId(1))
                .unwrap()
                .surface
                .get(contested)
                .is_some(),
            "losing continental crust is buoyant and survives"
        );
    }

    #[test]
    fn single_plate_holes_adopt_ownership_without_minting_features() {
        let mut world = test_world(5);
        let n = world.data.cell_count() as usize;
        let split = n / 2;
        let mut registry = two_plate_registry(n, split);

        // Pick a hole hex whose whole 2-ring sits inside plate 1's footprint
        // (index adjacency is not spatial adjacency, so search for one).
        let grid = &world.data.grid;
        let deep_interior = |hex: HexId| -> bool {
            hex.0 as usize >= split
                && grid.neighbors(hex).iter().all(|a| {
                    a.0 as usize >= split
                        && grid.neighbors(*a).iter().all(|b| b.0 as usize >= split)
                })
        };
        let hole = (split..n)
            .map(|i| HexId(i as u32))
            .find(|h| deep_interior(*h))
            .expect("an interior plate-1 hex exists");
        if let Some(p) = registry.plates_mut().get_mut(&PlateId(1)) {
            p.surface.clear(hole);
        }

        world.data.plate_id.fill(PlateId::NONE);
        repartition_hexes(&mut world.data, &mut registry);

        // The hole is adopted by plate 1; a hex whose claimed neighbors all
        // belong to one plate must NOT mint a feature, or footprints inflate
        // every tick.
        let idx = hole.0 as usize;
        assert_eq!(
            world.data.plate_id[idx],
            PlateId(1),
            "hole adopted by plate 1"
        );
        let plate = registry.get(PlateId(1)).expect("plate");
        let birth = crate::frames::current_world_to_birth_hex(&world.data.grid, hole, plate);
        assert!(
            plate.surface.get(birth).is_none(),
            "single-plate quantization hole must not accrete crust"
        );
    }

    #[test]
    fn divergent_gaps_accrete_young_ridge_crust() {
        let mut world = test_world(5);
        let n = world.data.cell_count() as usize;
        // a spins -Z, b spins +Z → diverging near +Y.
        let (mut registry, gap_center) =
            opposed_plates(&world, n, -1.0, 1.0, PlateType::Oceanic, PlateType::Oceanic);

        // Spatial footprints: plate 0 owns x > 0.06, plate 1 owns x < -0.06;
        // the band in between is an open gap touching both plates.
        for hex in world.data.grid.iter() {
            let c = world.data.grid.cell_center_direction(hex);
            if c[0] > 0.06 {
                registry
                    .plates_mut()
                    .get_mut(&PlateId(0))
                    .unwrap()
                    .surface
                    .set(hex, feature(-3500.0, BedrockType::OceanicCrust));
            } else if c[0] < -0.06 {
                registry
                    .plates_mut()
                    .get_mut(&PlateId(1))
                    .unwrap()
                    .surface
                    .set(hex, feature(-3500.0, BedrockType::OceanicCrust));
            }
        }

        world.data.plate_id.fill(PlateId::NONE);
        repartition_hexes(&mut world.data, &mut registry);

        // Somewhere in the northern gap band (y > 0.8, where the plates are
        // diverging) young ridge crust must have been minted.
        let _ = gap_center;
        let mut minted_found = false;
        for hex in world.data.grid.iter() {
            let c = world.data.grid.cell_center_direction(hex);
            if c[0].abs() >= 0.06 || c[1] < 0.8 {
                continue;
            }
            let owner = world.data.plate_id[hex.0 as usize];
            assert_ne!(owner, PlateId::NONE, "gap hex must be adopted");
            let plate = registry.get(owner).expect("plate");
            let birth = crate::frames::current_world_to_birth_hex(&world.data.grid, hex, plate);
            if let Some(f) = plate.surface.get(birth)
                && f.elevation_m == NEW_CRUST_ELEVATION_M
                && f.bedrock == BedrockType::OceanicCrust
            {
                minted_found = true;
            }
        }
        assert!(
            minted_found,
            "diverging gap band should accrete young ridge crust"
        );
    }

    #[test]
    fn footprint_shape_is_preserved_under_rotation() {
        let grid = HexGrid::new(5, EARTH_RADIUS_KM).expect("grid");
        let n = grid.cell_count() as usize;
        let mut world = test_world(5);
        let mut registry = PlateRegistry::new();

        // Plate 0: a compact continental blob around hex 100; plate 1: everything oceanic.
        let mut a = plate_at(0, 100, [0.0, 0.0, 1.0], 0.0, PlateType::Continental);
        let mut b = plate_at(1, 2000, [0.0, 1.0, 0.0], 0.0, PlateType::Oceanic);
        a.surface = PlateSurface::new(n);
        b.surface = PlateSurface::new(n);
        let center = HexId(100);
        let mut blob = vec![center];
        blob.extend(grid.neighbors(center).iter().copied());
        for h in &blob {
            a.surface.set(*h, feature(800.0, BedrockType::Igneous));
        }
        for i in 0..n {
            let h = HexId(i as u32);
            if !blob.contains(&h) {
                b.surface
                    .set(h, feature(-3500.0, BedrockType::OceanicCrust));
            }
        }
        registry.insert(a);
        registry.insert(b);

        world.data.plate_id.fill(PlateId::NONE);
        repartition_hexes(&mut world.data, &mut registry);
        let count_before = world
            .data
            .plate_id
            .iter()
            .filter(|&&p| p == PlateId(0))
            .count();

        // Rotate the blob plate substantially and repartition.
        if let Some(p) = registry.plates_mut().get_mut(&PlateId(0)) {
            p.accumulated_rotation_rad = 0.8;
        }
        repartition_hexes(&mut world.data, &mut registry);
        let count_after = world
            .data
            .plate_id
            .iter()
            .filter(|&&p| p == PlateId(0))
            .count();

        // The blob may gain/lose a hex to quantization but must stay compact,
        // not smear or vanish.
        assert!(
            count_after >= count_before.saturating_sub(2) && count_after <= count_before + 2,
            "footprint size should be stable under rotation: before={count_before} after={count_after}"
        );

        // And it must have actually moved.
        let plate = registry.get(PlateId(0)).unwrap();
        let moved = birth_hex_to_current_world(&world.data.grid, center, plate);
        assert_ne!(moved, center, "blob should have moved");
        assert_eq!(world.data.plate_id[moved.0 as usize], PlateId(0));
    }
}
