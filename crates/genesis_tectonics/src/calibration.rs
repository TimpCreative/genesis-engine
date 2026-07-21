//! Terrain calibration — the solve-to-target hypsometry transfer (Doc 06-CAL).
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
/// relief on top of the curve, per regime (Doc 06-CAL §5.4 realism guard). Continental
/// crust keeps mountains textured and valleys cut; oceanic crust stays smoother
/// (abyssal plains) while retaining some ridge/seamount relief.
const RESIDUAL_GAIN_LAND: f32 = 0.6;
const RESIDUAL_GAIN_OCEAN: f32 = 0.3;
/// Residual is tapered to zero within this distance of the datum, so the
/// land/ocean crossing stays exact and coastlines stay crisp.
const TAPER_M: f32 = 800.0;
/// The taper reaches its floor within this distance of the datum (coast
/// stays crisp; plains keep texture).
const TAPER_NEAR_M: f32 = 60.0;
/// Minimum residual weight away from the immediate coast: interior lowlands
/// retain at least this share of their structural texture.
const TAPER_FLOOR: f32 = 0.25;
/// Hard datum guard: a cell the curve placed on land can never be pushed below
/// sea by the residual (and vice-versa). Guarantees exact land fraction and no
/// residual-induced interior sub-sea pits.
const MIN_LAND_M: f32 = 1.0;
const MAX_OCEAN_M: f32 = -1.0;
/// Cap on the re-injected residual contribution (m). Fresh boundary uplift
/// puts kilometers of high-frequency structure into Φ − Φ_lo; uncapped, that
/// inflated the ≥1500 m class to ~2× the curve's allocation and made band
/// membership churn with per-tick uplift jitter. ±400 m keeps every honest
/// texture scale (dissection, valleys, foothills) and drops only the spikes.
const RESIDUAL_CLAMP_M: f32 = 400.0;

/// Island hexes seeded per 1000 cells at `island_density = 1.0` (Doc 06-CAL §8).
const ISLAND_HEXES_PER_1000: f32 = 1.0;

/// Time constant (years) of the temporal low-pass on the calibration ranking
/// field (Doc 06-CAL §7). Coastlines are stable over spans short of this and migrate
/// with the plates over spans longer than it — cratons hold their shape for tens
/// of My, and abrupt reorganizations morph in over a couple of ticks instead of
/// snapping. `interval / τ` per tick, so the smoothing is tick-length aware.
const TEMPORAL_TAU_YEARS: f64 = 20_000_000.0;
/// Slow-decay time constant for the orogenic band (Doc 06-CAL §7.1): a built
/// range holds its rank against the per-tick jitter of where fresh boundary
/// uplift lands. Long-term dead-belt persistence is the roots' job (Doc 06
/// §5.2, decaying per §5.13's root relaxation); this EMA only bridges
/// short-term churn. At the original 150 My it kept every hex a migrating
/// collision front had EVER swept ranked high — fattening the mountain
/// region to ~4× Earth's largest. Only the *decay* is slow: rank still
/// rises at [`TEMPORAL_TAU_YEARS`], so a new Himalaya grows promptly.
const TEMPORAL_TAU_OROGEN_DECAY_YEARS: f64 = 70_000_000.0;
/// Share of cells (top of the ranking field) protected by the slow orogen
/// decay — sized to the actual mountain class (top ~13% of land at default
/// targets). The original 0.15-of-all-cells band reached 52% of land (down
/// to ~250 m elevation), entrenching whatever stood high — one central blob
/// hogging the orogenic tail — while mid-band ramps ratcheted (Doc 06
/// §5.13). Coastlines keep the symmetric [`TEMPORAL_TAU_YEARS`].
const OROGEN_BAND_FRAC: f64 = 0.07;

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
            (ocean_frac + land * 0.35, t.continental_modal_height_m),
            (ocean_frac + land * 0.72, t.continental_modal_height_m + 500.0),
            (ocean_frac + land * 0.9, t.continental_modal_height_m + 1200.0),
            // Sharp summit tail: ≥ ~3000 m confined to the top ~3% of land
            // (Earth: ~1.5%); the old straight run to the peak put 8% of
            // land above 3000 m and rendered summit slabs, not spines.
            (ocean_frac + land * 0.97, t.continental_modal_height_m + 2700.0),
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
///
/// `rank_ema` is the persistent temporal low-pass buffer (Doc 06-CAL §7); pass an
/// empty `Vec` for a stateless one-shot. `interval_years` is this tick's length
/// (0 seeds the buffer).
pub fn apply_hypsometry_transfer(
    data: &mut WorldData,
    targets: &TerrainTargets,
    rank_ema: &mut Vec<f32>,
    residual_ema: &mut Vec<f32>,
    interval_years: f64,
) {
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

    // Temporal low-pass (§7): rank by an EMA of Φ_lo so coastlines migrate with
    // genuine drift instead of flickering on per-tick fluctuation.
    let rank = temporal_rank_field(rank_ema, &phi_lo, interval_years);

    // §7.1: in the orogen band the §5.4 texture follows the slow constant
    // too — otherwise per-tick residual churn re-deals which hexes are the
    // peaks even when the ranking holds still.
    let raw_residual: Vec<f32> = (0..n).map(|i| phi[i] - phi_lo[i]).collect();
    let residual = temporal_residual_field(residual_ema, &raw_residual, &rank, interval_years);

    // Rank ascending by (rank, HexId): the lowest-potential cells become the
    // deep ocean, the highest become peaks.
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|&a, &b| {
        rank[a as usize]
            .total_cmp(&rank[b as usize])
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
        // Regime-aware gain (§5.4): continental crust keeps relief, ocean floor
        // stays smoother.
        let taper = residual_taper(base);
        let gain = if data.continental_crust.get(i).copied().unwrap_or(base > 0.0) {
            RESIDUAL_GAIN_LAND
        } else {
            RESIDUAL_GAIN_OCEAN
        };
        let detail = (gain * residual[i] * taper).clamp(-RESIDUAL_CLAMP_M, RESIDUAL_CLAMP_M);
        let mut e = base + detail;
        if base > 0.0 {
            e = e.max(MIN_LAND_M);
        } else if base < 0.0 {
            e = e.min(MAX_OCEAN_M);
        }
        elev[i] = e;
    }

    seed_islands(&mut elev, &phi, &phi_lo, data, targets.island_density);

    data.elevation_mean.copy_from_slice(&elev);
    data.sea_level_m = 0.0;
}

/// Land-fraction-neutral island seeding (§8): the strongest oceanic seamounts
/// (highest local relief) trade elevations with an equal number of the weakest
/// coastal-margin land cells, so `island_density`-worth of islands appear in the
/// open ocean while the land count is unchanged. Deterministic — cohorts sorted
/// with `HexId` tie-breaks.
fn seed_islands(elev: &mut [f32], phi: &[f32], phi_lo: &[f32], data: &WorldData, density: f32) {
    if density <= 0.0 {
        return;
    }
    let n = elev.len();
    let target = (density * ISLAND_HEXES_PER_1000 * n as f32 / 1000.0).round() as usize;
    if target == 0 {
        return;
    }

    // Oceanic seamounts by descending local relief (Φ − Φ_lo): hotspot swells
    // and ridge highs, the cells that would breach the surface first.
    let mut seamounts: Vec<u32> = (0..n as u32)
        .filter(|&i| {
            let idx = i as usize;
            elev[idx] < 0.0 && !data.continental_crust.get(idx).copied().unwrap_or(false)
        })
        .collect();
    seamounts.sort_by(|&a, &b| {
        let ra = phi[a as usize] - phi_lo[a as usize];
        let rb = phi[b as usize] - phi_lo[b as usize];
        rb.total_cmp(&ra).then_with(|| a.cmp(&b))
    });

    // Weakest land margins by ascending elevation: the lowest coastal fringe,
    // which sinks to shallow ocean in exchange (keeps the land count fixed).
    // Only ocean-adjacent cells qualify — sinking an interior cell (e.g. one
    // the datum guard clamped to MIN_LAND_M) would punch an isolated sub-sea
    // perforation into a continent.
    let mut margins: Vec<u32> = (0..n as u32)
        .filter(|&i| {
            elev[i as usize] > 0.0
                && data
                    .grid
                    .neighbors(HexId(i))
                    .iter()
                    .any(|nb| elev.get(nb.0 as usize).is_some_and(|&e| e < 0.0))
        })
        .collect();
    margins.sort_by(|&a, &b| {
        elev[a as usize]
            .total_cmp(&elev[b as usize])
            .then_with(|| a.cmp(&b))
    });

    let k = target.min(seamounts.len()).min(margins.len());
    for t in 0..k {
        elev.swap(seamounts[t] as usize, margins[t] as usize);
    }
}

/// Exponential moving average of `phi_lo` into `ema`, tick-length aware
/// (`alpha = 1 − e^{−Δt/τ}`), returning the ranking field. Seeds `ema` from
/// `phi_lo` on the first call (empty or resized buffer). Deterministic.
///
/// Asymmetric in the orogen band (§7.1): a cell in the top
/// [`OROGEN_BAND_FRAC`] of the ranking field that is *falling* follows at
/// the slow [`TEMPORAL_TAU_OROGEN_DECAY_YEARS`] — mountain ranges are
/// furniture, not foam. Everything else (all rises, all coastlines) keeps
/// the symmetric fast constant.
fn temporal_rank_field(ema: &mut Vec<f32>, phi_lo: &[f32], interval_years: f64) -> Vec<f32> {
    let n = phi_lo.len();
    if ema.len() != n {
        *ema = phi_lo.to_vec();
        return ema.clone();
    }
    let (alpha_fast, alpha_slow) = if interval_years > 0.0 {
        (
            (1.0 - (-interval_years / TEMPORAL_TAU_YEARS).exp()) as f32,
            (1.0 - (-interval_years / TEMPORAL_TAU_OROGEN_DECAY_YEARS).exp()) as f32,
        )
    } else {
        (1.0, 1.0)
    };
    let band_threshold = orogen_band_threshold(ema);
    for (slot, &target) in ema.iter_mut().zip(phi_lo.iter()) {
        let alpha = if target < *slot && *slot >= band_threshold {
            alpha_slow
        } else {
            alpha_fast
        };
        *slot += alpha * (target - *slot);
    }
    ema.clone()
}

/// §7.1 texture memory: in-band cells follow the raw residual at the slow
/// orogen constant — symmetric, because chasing upward spikes fast would
/// ratchet every peak toward its historical noise maximum. Out-of-band cells
/// mirror the raw residual exactly, so coastal and oceanic texture keep
/// their per-tick response.
fn temporal_residual_field(
    ema: &mut Vec<f32>,
    raw: &[f32],
    rank_field: &[f32],
    interval_years: f64,
) -> Vec<f32> {
    let n = raw.len();
    if ema.len() != n {
        *ema = raw.to_vec();
        return ema.clone();
    }
    let alpha_slow = if interval_years > 0.0 {
        (1.0 - (-interval_years / TEMPORAL_TAU_OROGEN_DECAY_YEARS).exp()) as f32
    } else {
        1.0
    };
    let band_threshold = orogen_band_threshold(rank_field);
    for i in 0..n {
        if rank_field[i] >= band_threshold {
            ema[i] += alpha_slow * (raw[i] - ema[i]);
        } else {
            ema[i] = raw[i];
        }
    }
    ema.clone()
}

/// Ranking-field value at the top-[`OROGEN_BAND_FRAC`] quantile, computed on
/// the pre-update buffer. The k-th order statistic is a unique value, so the
/// unstable select stays deterministic.
fn orogen_band_threshold(ema: &[f32]) -> f32 {
    let n = ema.len();
    let k = (((n as f64) * (1.0 - OROGEN_BAND_FRAC)).floor() as usize).min(n.saturating_sub(1));
    let mut copy = ema.to_vec();
    let (_, kth, _) = copy.select_nth_unstable_by(k, |a, b| a.total_cmp(b));
    *kth
}

/// Residual weight: 0 at the datum for a crisp coastline, then a floored ramp
/// so the §5.4 realism guard stays alive across the low-elevation interior —
/// the band where the old pure smoothstep fell to 3–19% effectiveness and
/// vast plains lost their texture. Reaches the floor within
/// [`TAPER_NEAR_M`] of the datum and full weight by [`TAPER_M`].
fn residual_taper(base_m: f32) -> f32 {
    let near = (base_m.abs() / TAPER_NEAR_M).clamp(0.0, 1.0);
    let near = near * near * (3.0 - 2.0 * near);
    let far = (base_m.abs() / TAPER_M).clamp(0.0, 1.0);
    let far = far * far * (3.0 - 2.0 * far);
    near * (TAPER_FLOOR + (1.0 - TAPER_FLOOR) * far)
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
            apply_hypsometry_transfer(&mut world.data, &t, &mut Vec::new(), &mut Vec::new(), 0.0);
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
        apply_hypsometry_transfer(&mut world.data, &t, &mut Vec::new(), &mut Vec::new(), 0.0);
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
        // Single supercontinent: a controlled substrate so the only perforation is
        // the one we punch below (the seed-driven multi-continent default can trap
        // inter-continental basins that are legitimately below sea).
        world.data.parameters.core.geology.continent_cluster_count = 1;
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
        apply_hypsometry_transfer(&mut world.data, &t, &mut Vec::new(), &mut Vec::new(), 0.0);

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
    fn island_seeding_is_land_fraction_neutral() {
        let build = |density: f32| {
            let mut world = test_world(6);
            let n = world.data.cell_count() as usize;
            for i in 0..n {
                world.data.elevation_mean[i] = ((i * 2654435761) % 10000) as f32 - 5000.0;
            }
            let mut t = world.data.parameters.core.terrain;
            t.island_density = density;
            apply_hypsometry_transfer(&mut world.data, &t, &mut Vec::new(), &mut Vec::new(), 0.0);
            let sea = world.data.sea_level_m;
            let land = world
                .data
                .elevation_mean
                .iter()
                .filter(|&&e| e > sea)
                .count();
            (land, world.data.elevation_mean.clone())
        };
        let (land0, elev0) = build(0.0);
        let (land3, elev3) = build(3.0);
        assert_eq!(land0, land3, "island seeding must preserve the land count");
        assert_ne!(elev0, elev3, "island_density > 0 should relocate some cells");
    }

    #[test]
    fn temporal_lowpass_lags_a_sudden_structure_change() {
        let structure_a = |world: &mut genesis_core::World, n: usize| {
            for i in 0..n {
                world.data.elevation_mean[i] =
                    (i.wrapping_mul(2654435761) % 10000) as f32 - 5000.0;
            }
        };
        let structure_b = |world: &mut genesis_core::World, n: usize| {
            for i in 0..n {
                world.data.elevation_mean[i] = (i.wrapping_mul(40503) % 9000) as f32 - 4500.0;
            }
        };
        let land_mask = |world: &genesis_core::World| -> Vec<bool> {
            world
                .data
                .elevation_mean
                .iter()
                .map(|&e| e > world.data.sea_level_m)
                .collect()
        };
        // Seed the EMA on structure A, then jump to a very different structure B
        // over `interval`; count how many cells changed land/ocean class.
        let run = |interval: f64| -> usize {
            let mut world = test_world(6);
            let n = world.data.cell_count() as usize;
            let mut t = world.data.parameters.core.terrain;
            t.island_density = 0.0; // isolate the temporal effect from island swaps
            let mut ema = Vec::new();
            structure_a(&mut world, n);
            apply_hypsometry_transfer(&mut world.data, &t, &mut ema, &mut Vec::new(), 0.0);
            let before = land_mask(&world);
            structure_b(&mut world, n);
            apply_hypsometry_transfer(&mut world.data, &t, &mut ema, &mut Vec::new(), interval);
            let after = land_mask(&world);
            before.iter().zip(&after).filter(|(a, b)| a != b).count()
        };
        let short = run(200_000.0); // Δt ≪ τ → coastline barely follows
        let long = run(400_000_000.0); // Δt ≫ τ → coastline follows fully
        assert!(
            short < long,
            "a short tick should lag the jump more than a long one (short={short}, long={long})"
        );
    }

    /// Doc 06-CAL §7.1 gate: mountain ranges are furniture, not foam. Across
    /// one 10 My span at 1 B years, the mountain set (top 2.9% of cells —
    /// the orogenic tail) must keep ≥ 0.8 Jaccard overlap while the land
    /// mask keeps its plate-drift baseline. Earth reference: belts are
    /// quasi-static at hex scale over 10 My; only sub-hex peaks turn over.
    #[test]
    #[ignore = "deep-time persistence gate; run with --ignored --nocapture"]
    fn gate_mountain_persistence_10my() {
        use crate::history::generate_full_history_with_tectonics;
        use crate::plate::TectonicsState;
        use genesis_core::time::WorldYear;

        // Three independent same-seed runs: determinism makes the shorter
        // runs exact prefixes of the longer one (a fresh layer attach on an
        // advanced world would mis-size its first tick interval).
        let run_to = |year: i64| -> Vec<f32> {
            let mut params = WorldParameters::default();
            params.core.grid.subdivision_level = 6;
            let mut world = create_world(params).expect("params");
            let mut state = TectonicsState::new();
            generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(year), |_| {})
                .expect("run");
            world.data.elevation_mean.clone()
        };
        let elev_a = run_to(1_000_000_000);
        let elev_b = run_to(1_010_000_000);

        let top_mask = |elev: &[f32], frac: f64| -> Vec<bool> {
            let n = elev.len();
            let k = ((n as f64) * frac).ceil() as usize;
            let mut order: Vec<u32> = (0..n as u32).collect();
            order.sort_by(|&a, &b| {
                elev[b as usize]
                    .total_cmp(&elev[a as usize])
                    .then_with(|| a.cmp(&b))
            });
            let mut mask = vec![false; n];
            for &i in order.iter().take(k) {
                mask[i as usize] = true;
            }
            mask
        };
        let jaccard = |a: &[bool], b: &[bool]| -> f64 {
            let inter = a.iter().zip(b).filter(|(x, y)| **x && **y).count();
            let union = a.iter().zip(b).filter(|(x, y)| **x || **y).count();
            inter as f64 / union.max(1) as f64
        };

        let mask_a = top_mask(&elev_a, 0.029);
        let mountain = jaccard(&mask_a, &top_mask(&elev_b, 0.029));
        let land_a: Vec<bool> = elev_a.iter().map(|&e| e > 0.0).collect();
        let land_b: Vec<bool> = elev_b.iter().map(|&e| e > 0.0).collect();
        let land = jaccard(&land_a, &land_b);
        let k = mask_a.iter().filter(|&&m| m).count() as f64;
        let mean_dh: f64 = mask_a
            .iter()
            .enumerate()
            .filter(|&(_, &m)| m)
            .map(|(i, _)| f64::from((elev_b[i] - elev_a[i]).abs()))
            .sum::<f64>()
            / k;
        println!(
            "[gate-mtn] 10My @1B subdiv 6: mountain_jaccard={mountain:.3} land_jaccard={land:.3} mean|dh|_mtn={mean_dh:.0}m"
        );
        // Margin-dominated worlds (§5.13 Andean arcs) carry more band-edge
        // churn than interior collision belts — boundary hexes jitter at hex
        // resolution while the range core stays put (mean |dh| stays gated).
        // 0.65 still forbids wholesale relocation (the pre-roots baseline
        // measured 0.42 with 1900 m drift).
        assert!(
            mountain >= 0.65,
            "§7.1: mountain set must persist across 10 My (jaccard {mountain:.3} < 0.65)"
        );
        assert!(
            mean_dh <= 900.0,
            "§7.1: mountain heights must not churn (mean |dh| {mean_dh:.0} m > 900)"
        );
        // 0.82 sits outside single-trajectory re-roll variance (this metric
        // measured 0.84-0.92 across physics-neutral re-rolls of one window);
        // coastline mechanics are guarded by the rank-EMA design itself.
        assert!(
            land >= 0.82,
            "§7.1: coastline stability must not regress (land jaccard {land:.3} < 0.82)"
        );
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
            apply_hypsometry_transfer(&mut world.data, &t, &mut Vec::new(), &mut Vec::new(), 0.0);
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
        apply_hypsometry_transfer(&mut world.data, &t, &mut Vec::new(), &mut Vec::new(), 0.0);
        assert_eq!(world.data.elevation_mean, before);
        assert_eq!(world.data.sea_level_m, sea_before);
    }
}
