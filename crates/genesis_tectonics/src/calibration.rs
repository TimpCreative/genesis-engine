//! Terrain calibration — the solve-to-target hypsometry transfer (Doc 10).
//!
//! The tectonic sim produces a *relative* structure field (where continents,
//! belts, ridges, and basins are). This pass maps that field's **rank** onto a
//! parameterized target hypsometric curve `H(p)`, so land coverage, the
//! shelf/slope/abyss profile, and the elevation distribution are guaranteed by
//! construction while the physics keeps deciding the *arrangement*. The
//! land/ocean datum is pinned at sea level = 0.
//!
//! Because the mapping is re-solved every tick, a morphology change upstream can
//! never break a target — we simply re-fit. This replaces the pile of corrective
//! elevation passes (Doc 06 §6–8) as the authority on absolute height.
//!
//! Deterministic: ranking is keyed `(smoothed Φ, HexId)`; the curve is monotone;
//! no RNG.

use genesis_core::HexId;
use genesis_core::data::WorldData;
use genesis_core::parameters::TerrainTargets;

/// Neighbor-average passes applied to Φ before **ranking** (not to the output).
/// Ranking on the low-frequency field pulls an isolated interior low up to its
/// surroundings' rank, so scattered one-hex pits never map below the datum —
/// the interior-perforation problem is dissolved rather than patched.
const SMOOTH_ITERS: usize = 2;
/// How much high-frequency structural detail (Φ − Φ_lo) is re-injected as local
/// relief on top of the curve. Keeps mountains textured and valleys cut instead
/// of an "equalized" beach ball. Phase 1 makes this regime-aware.
const RESIDUAL_GAIN: f32 = 0.5;
/// Residual is tapered to zero within this distance of the datum, so the
/// land/ocean crossing stays exact and coastlines stay crisp.
const TAPER_M: f32 = 800.0;
/// Hard datum guard: a cell the curve placed on land can never be pushed below
/// sea by the residual (and vice-versa). Guarantees exact land fraction and no
/// residual-induced interior sub-sea pits.
const MIN_LAND_M: f32 = 1.0;
const MAX_OCEAN_M: f32 = -1.0;

/// A monotone target hypsometric curve `H(p)`: area-percentile `p ∈ [0,1]`
/// (ascending) → elevation in meters, sea level = 0. Built from [`TerrainTargets`].
pub struct HypsometricCurve {
    /// Control points `(p, elevation_m)`, strictly increasing in `p`, monotone
    /// non-decreasing in elevation.
    points: Vec<(f32, f32)>,
}

impl HypsometricCurve {
    /// Build the Earth-like bimodal curve (trench → abyssal → slope → shelf →
    /// coast → continental mode → orogenic tail) from the target knobs. The
    /// crossing at elevation 0 sits exactly at `p = 1 − land_fraction`, so the
    /// land fraction is exact by construction.
    pub fn from_targets(t: &TerrainTargets) -> Self {
        let land = t.land_fraction.clamp(0.01, 0.99);
        let ocean_frac = 1.0 - land;
        let shelf = t.shelf_fraction.clamp(0.0, ocean_frac * 0.9);
        let slope = t.slope_width_frac.clamp(0.0, (ocean_frac - shelf) * 0.9);
        let shelf_break_p = ocean_frac - shelf;
        let slope_bottom_p = ocean_frac - shelf - slope;
        let abyssal_top_p = (0.02_f32).min(slope_bottom_p * 0.5);
        let coastal_band = (land * 0.15).min(0.05);
        let peak = t.continental_modal_height_m + 8000.0 * t.orogeny_intensity.max(0.0);

        let mut points = vec![
            (0.0, t.trench_depth_m),
            (abyssal_top_p, t.abyssal_depth_m),
            (slope_bottom_p, t.abyssal_depth_m),
            (shelf_break_p, t.shelf_depth_m),
            (ocean_frac, 0.0),
            (ocean_frac + coastal_band, t.continental_modal_height_m * 0.35),
            (ocean_frac + land * 0.6, t.continental_modal_height_m),
            (ocean_frac + land * 0.9, t.continental_modal_height_m + 1200.0),
            (1.0, peak),
        ];

        // Enforce strictly increasing p (nudge duplicates) and monotone
        // non-decreasing elevation, so the curve is a valid rank→height map.
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let eps = 1.0e-6;
        for i in 1..points.len() {
            if points[i].0 <= points[i - 1].0 {
                points[i].0 = points[i - 1].0 + eps;
            }
            if points[i].1 < points[i - 1].1 {
                points[i].1 = points[i - 1].1;
            }
        }
        Self { points }
    }

    /// Elevation at percentile `p` (clamped to `[0,1]`), linear between control
    /// points.
    pub fn elev_at(&self, p: f32) -> f32 {
        let p = p.clamp(0.0, 1.0);
        let pts = &self.points;
        if p <= pts[0].0 {
            return pts[0].1;
        }
        let last = pts.len() - 1;
        if p >= pts[last].0 {
            return pts[last].1;
        }
        // Bracket: pts is small (9), a linear scan is fine and branch-simple.
        for w in pts.windows(2) {
            let (p0, e0) = w[0];
            let (p1, e1) = w[1];
            if p >= p0 && p <= p1 {
                let f = (p - p0) / (p1 - p0);
                return e0 + f * (e1 - e0);
            }
        }
        pts[last].1
    }
}

/// Map the structure field (`elevation_mean`) onto the target curve and pin the
/// datum to 0. No-op when `targets.enabled` is false (legacy path).
pub fn apply_hypsometry_transfer(data: &mut WorldData, targets: &TerrainTargets) {
    if !targets.enabled {
        return;
    }
    let n = data.cell_count() as usize;
    if n == 0 {
        return;
    }

    // Φ = the structure field this tick; Φ_lo = its low-frequency component.
    let phi: Vec<f32> = data.elevation_mean.clone();
    let phi_lo = smooth_field(&phi, data, SMOOTH_ITERS);

    // Rank ascending by (Φ_lo, HexId): the lowest-potential cells become the
    // deep ocean, the highest become peaks.
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|&a, &b| {
        phi_lo[a as usize]
            .total_cmp(&phi_lo[b as usize])
            .then_with(|| a.cmp(&b))
    });

    let curve = HypsometricCurve::from_targets(targets);
    let denom = n.saturating_sub(1).max(1) as f32;

    let mut elev = vec![0.0_f32; n];
    for (rank, &hex) in order.iter().enumerate() {
        let i = hex as usize;
        let p = rank as f32 / denom;
        let base = curve.elev_at(p);
        // Re-inject tapered local detail; the datum guard keeps the land/ocean
        // classification (hence the land fraction) exactly as the curve set it.
        let residual = phi[i] - phi_lo[i];
        let taper = residual_taper(base);
        let mut e = base + RESIDUAL_GAIN * residual * taper;
        if base > 0.0 {
            e = e.max(MIN_LAND_M);
        } else if base < 0.0 {
            e = e.min(MAX_OCEAN_M);
        }
        elev[i] = e;
    }

    data.elevation_mean.copy_from_slice(&elev);
    data.sea_level_m = 0.0;
}

/// Smoothstep-tapered residual weight: 0 at the datum, ramping to 1 by
/// [`TAPER_M`] away on either side.
fn residual_taper(base_m: f32) -> f32 {
    let x = (base_m.abs() / TAPER_M).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// `iters` passes of self-plus-neighbor averaging over the grid. Deterministic
/// (a mean is order-independent; frontier uses `neighbors_sorted`).
fn smooth_field(field: &[f32], data: &WorldData, iters: usize) -> Vec<f32> {
    let n = field.len();
    let mut cur = field.to_vec();
    let mut next = vec![0.0_f32; n];
    for _ in 0..iters {
        for i in 0..n {
            let mut sum = cur[i];
            let mut count = 1.0_f32;
            for &nb in data.grid.neighbors_sorted(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n {
                    sum += cur[j];
                    count += 1.0;
                }
            }
            next[i] = sum / count;
        }
        std::mem::swap(&mut cur, &mut next);
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn test_world(level: u8) -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = level;
        create_world(params).expect("world")
    }

    fn land_fraction(data: &WorldData) -> f32 {
        let land = data
            .elevation_mean
            .iter()
            .filter(|&&e| e > data.sea_level_m)
            .count();
        land as f32 / data.elevation_mean.len() as f32
    }

    #[test]
    fn land_fraction_hits_target_exactly() {
        for target in [0.15_f32, 0.29, 0.5, 0.7] {
            let mut world = test_world(5);
            // Arbitrary structure field: a smooth-ish gradient plus noise.
            let n = world.data.cell_count() as usize;
            for i in 0..n {
                world.data.elevation_mean[i] =
                    ((i * 2654435761) % 10000) as f32 - 5000.0;
            }
            world.data.parameters.core.terrain.land_fraction = target;
            let t = world.data.parameters.core.terrain;
            apply_hypsometry_transfer(&mut world.data, &t);
            let realized = land_fraction(&world.data);
            assert!(
                (realized - target).abs() <= 1.0 / n as f32 + 1e-4,
                "target {target}, realized {realized}"
            );
            assert_eq!(world.data.sea_level_m, 0.0, "datum pinned to 0");
        }
    }

    #[test]
    fn shelf_band_is_present() {
        let mut world = test_world(6);
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = (i as f32) - (n as f32) / 2.0;
        }
        let t = world.data.parameters.core.terrain;
        apply_hypsometry_transfer(&mut world.data, &t);
        // Some ocean cells sit in the shallow shelf band (shelf_depth..0),
        // i.e. the profile is not a cliff straight to the abyss.
        let shelf = world
            .data
            .elevation_mean
            .iter()
            .filter(|&&e| e < 0.0 && e > t.shelf_depth_m)
            .count();
        assert!(shelf > 0, "expected a shelf band, found none");
    }

    #[test]
    fn no_isolated_sub_sea_perforations_on_a_real_world() {
        use crate::history::run_formation;
        use crate::plate::TectonicsState;

        let mut world = test_world(5);
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);

        // Punch a deep isolated pit into a continental-interior hex (a hex whose
        // neighbors are all above sea) to model the accreted-oceanic-crust lows.
        let sea = world.data.sea_level_m;
        let interior = (0..world.data.cell_count())
            .find(|&i| {
                world.data.elevation_mean[i as usize] > sea
                    && world
                        .data
                        .grid
                        .neighbors(HexId(i))
                        .iter()
                        .all(|nb| world.data.elevation_mean[nb.0 as usize] > sea)
            })
            .expect("an interior land hex");
        world.data.elevation_mean[interior as usize] = -4000.0;

        let t = world.data.parameters.core.terrain;
        apply_hypsometry_transfer(&mut world.data, &t);

        // After the transfer, no cell is a lone sub-sea hole ringed entirely by
        // land: smoothed ranking merges an isolated low into its neighborhood,
        // so the perforation is dissolved rather than deepened.
        let sea = world.data.sea_level_m;
        let perforations = (0..world.data.cell_count())
            .filter(|&i| {
                world.data.elevation_mean[i as usize] < sea
                    && world
                        .data
                        .grid
                        .neighbors(HexId(i))
                        .iter()
                        .all(|nb| world.data.elevation_mean[nb.0 as usize] > sea)
            })
            .count();
        assert_eq!(perforations, 0, "found {perforations} interior perforations");
    }

    #[test]
    fn transfer_is_deterministic() {
        let build = || {
            let mut world = test_world(6);
            let n = world.data.cell_count() as usize;
            for i in 0..n {
                world.data.elevation_mean[i] = ((i * 40503) % 7000) as f32 - 3500.0;
            }
            let t = world.data.parameters.core.terrain;
            apply_hypsometry_transfer(&mut world.data, &t);
            world.data.elevation_mean.clone()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn disabled_is_a_noop() {
        let mut world = test_world(5);
        let before = world.data.elevation_mean.clone();
        let sea_before = world.data.sea_level_m;
        let mut t = world.data.parameters.core.terrain;
        t.enabled = false;
        apply_hypsometry_transfer(&mut world.data, &t);
        assert_eq!(world.data.elevation_mean, before);
        assert_eq!(world.data.sea_level_m, sea_before);
    }
}
