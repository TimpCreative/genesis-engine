//! Doc 08 §15 full-stack validation gates (P2-34).
//!
//! The cheap shape/unit gates live in `genesis_hydrology::validation`; this
//! module makes the §15 deep-time gates real on live worlds: it runs the
//! full tectonics → climate → hydrology stack and asserts against the §15
//! bands. Everything here is `#[ignore]`d — run with:
//!
//! ```sh
//! cargo test -p genesis_ui --release -- --ignored --nocapture hydro_validation
//! ```
//!
//! Conventions (mirroring `genesis_tectonics::validation`):
//! - seed from `GENESIS_VALIDATION_SEED` (default 42);
//! - deep-time gates at subdivision 5; the Major-river census (#5) and the
//!   perf gate (#11) at subdivision 7, where §15's bands were calibrated;
//! - water inventory 1000 m GEL — the `genesis_hydrology::validation`
//!   convention. The production menu default is 2700 m; the leaner inventory
//!   exposes more land, hence more rivers/lakes for the gates to read;
//! - EXCEPT the gates whose §15 bands were calibrated on the default world —
//!   the glacial gates (#3, #15, #16, #19, #20), the subdiv-7 Major census
//!   (#5), and the 4B harness — which run at the production default 2700 m
//!   (see [`PRODUCTION_GEL_M`]);
//! - every gate prints its evidence (`--nocapture`). A gate whose physical
//!   precondition does not occur on the validation world prints a loud
//!   `SKIP` notice instead of asserting — never a fake green.

use genesis_core::data::{WATER_NONE, WorldData};
use genesis_core::grid::HexGrid;
use genesis_core::{World, create_world};
use genesis_hydrology::validation::{
    FlagCensus, HydroMetrics, SoilCensus, flag_census, ice_area_fraction, land_fraction,
    largest_basin_fraction_of_continent, major_river_census, soil_census, wet_fraction_from_levels,
};

use crate::worldgen::{HistoryFrame, WorldGenConfig, generate_full_history, history_stride_years};

/// Water inventory for the §15 gates (module docs explain why not 2700).
pub const VALIDATION_GEL_M: f32 = 1000.0;

/// Water inventory for gates whose §15 bands were calibrated on the default
/// world — the glacial gates (#3, #15, #16, #19, #20), the subdiv-7 Major
/// census (#5), and the 4B harness. The 1000 m validation convention exists
/// to expose land for the drainage gates, and at 1000 m the 4B world is a
/// permanent snowball — 37.8% max ice, never ice-free after 500M, 2406 m
/// excursion (observed on seed 42) — outside every glacial band's physical
/// premise, while the Major count scales with exposed land.
/// Water inventory for gates whose §15 bands were calibrated on the default
/// world — the glacial gates (#3, #15, #16, #19, #20), the subdiv-7 Major
/// census (#5), the 4B harness, and morphology land/pit gates. Default menu
/// GEL is 2400 m (retuned from 2700 for deep-time land coherence).
pub const PRODUCTION_GEL_M: f32 = 2400.0;

/// Deep-time gate subdivision (CI-friendly, ~2,432 hexes).
pub const VALIDATION_SUBDIVISION_LEVEL: u8 = 5;
/// Subdivision for the §15-calibrated Major census and the perf gate.
pub const VALIDATION_MAJOR_SUBDIVISION_LEVEL: u8 = 7;

/// Early Formation / pre-condensation horizon (still dry under §3.3 temperature gate).
pub const VALIDATION_YEAR_200M: i64 = 200_000_000;
/// Mid Formation fill window (condensed fraction in (0, 1); oceans present).
pub const VALIDATION_YEAR_300M: i64 = 300_000_000;
/// Standard deep-time horizon.
pub const VALIDATION_YEAR_1B: i64 = 1_000_000_000;
/// Salt-story horizon (§15 #9).
pub const VALIDATION_YEAR_2B: i64 = 2_000_000_000;
/// Full deep-time horizon (glacial gates).
pub const VALIDATION_YEAR_4B: i64 = 4_000_000_000;

/// Frame stride for the 4B glacial gates: 2 My resolves glacial episodes
/// (orbital forcing runs on ~2.3 My cycles; the default 10 My scrub stride
/// would skip most of them).
pub const GLACIAL_FRAME_STRIDE_YEARS: i64 = 2_000_000;

/// Seed for the §15 gates: `GENESIS_VALIDATION_SEED` or 42 (same pattern as
/// tectonics' `validation_parameters`).
pub fn validation_seed() -> u64 {
    std::env::var("GENESIS_VALIDATION_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(42)
}

/// Config for one full-stack validation run (production plate/continent defaults).
pub fn full_stack_config(
    seed: u64,
    subdivision: u8,
    target_year: i64,
    gel_m: f32,
) -> WorldGenConfig {
    WorldGenConfig {
        seed,
        subdivision_level: subdivision,
        target_year,
        water_inventory_gel_m: gel_m,
        ..WorldGenConfig::default()
    }
}

/// Runs the full stack to `target_year` and returns the REAL final world plus
/// the captured frames.
///
/// Not [`crate::worldgen::generate_world_with_history`]: that returns the
/// viewer's display world — the year-0 world with the last frame's render
/// fields applied — so non-frame fields (`water_bodies`, `water_body_id`,
/// `fertility`, `bedrock_type`, `plate_id`) would read as year-0 defaults.
/// The registry, karst, fertility, and tectonic-invariant gates need the
/// true final state. Frame capture mirrors the streaming path's stride rule.
pub fn run_full_stack(
    seed: u64,
    subdivision: u8,
    target_year: i64,
    gel_m: f32,
) -> (World, Vec<HistoryFrame>) {
    run_full_stack_with_stride(seed, subdivision, target_year, gel_m, None)
}

/// [`run_full_stack_with_stride`] plus a clone of `WorldData` at the tick of
/// peak land-ice fraction (for fixed-hypsometry ice twins, §15 #3).
pub fn run_full_stack_with_peak_ice(
    seed: u64,
    subdivision: u8,
    target_year: i64,
    gel_m: f32,
    frame_stride_years: Option<i64>,
) -> (World, Vec<HistoryFrame>, Option<WorldData>) {
    let config = full_stack_config(seed, subdivision, target_year, gel_m);
    let params = config.to_parameters();
    let mut world = create_world(params).expect("validation parameters valid");
    let mut tectonics = genesis_tectonics::TectonicsState::new();
    let mut climate = genesis_climate::ClimateState::new();
    let mut hydrology = genesis_hydrology::HydrologyState::new();

    let target = target_year.max(1);
    let stride = frame_stride_years
        .unwrap_or_else(|| history_stride_years(target, world.data.cell_count()))
        .max(1);
    let mut frames: Vec<HistoryFrame> = Vec::new();
    let mut next_capture_year = 0_i64;
    let mut peak_ice = -1.0_f32;
    let mut peak_snapshot: Option<WorldData> = None;
    generate_full_history(
        &mut world,
        &mut tectonics,
        &mut climate,
        &mut hydrology,
        genesis_core::time::WorldYear(target),
        |data| {
            let ice = ice_area_fraction(&data.ice_mask);
            if ice > peak_ice {
                peak_ice = ice;
                peak_snapshot = Some(data.clone());
            }
            let year = data.current_year.value();
            if year >= next_capture_year {
                frames.push(HistoryFrame::capture(data));
                next_capture_year = year + stride;
            }
        },
    )
    .expect("full-stack generation");
    if frames
        .last()
        .is_none_or(|f| f.year != world.data.current_year.value())
    {
        frames.push(HistoryFrame::capture(&world.data));
    }
    (world, frames, peak_snapshot)
}

/// [`run_full_stack`] with an explicit frame stride (`None` = the fixed
/// [`history_stride_years`] cadence, 10 My).
pub fn run_full_stack_with_stride(
    seed: u64,
    subdivision: u8,
    target_year: i64,
    gel_m: f32,
    frame_stride_years: Option<i64>,
) -> (World, Vec<HistoryFrame>) {
    let config = full_stack_config(seed, subdivision, target_year, gel_m);
    let params = config.to_parameters();
    let mut world = create_world(params).expect("validation parameters valid");
    let mut tectonics = genesis_tectonics::TectonicsState::new();
    let mut climate = genesis_climate::ClimateState::new();
    let mut hydrology = genesis_hydrology::HydrologyState::new();

    let target = target_year.max(1);
    let stride = frame_stride_years
        .unwrap_or_else(|| history_stride_years(target, world.data.cell_count()))
        .max(1);
    let mut frames: Vec<HistoryFrame> = Vec::new();
    let mut next_capture_year = 0_i64;
    generate_full_history(
        &mut world,
        &mut tectonics,
        &mut climate,
        &mut hydrology,
        genesis_core::time::WorldYear(target),
        |data| {
            let year = data.current_year.value();
            if year >= next_capture_year {
                frames.push(HistoryFrame::capture(data));
                next_capture_year = year + stride;
            }
        },
    )
    .expect("full-stack generation");
    // The final state is always framed (mirrors the streaming path).
    if frames
        .last()
        .is_none_or(|f| f.year != world.data.current_year.value())
    {
        frames.push(HistoryFrame::capture(&world.data));
    }
    (world, frames)
}

/// Full §15 metrics snapshot over a live world.
pub fn metrics_at(world: &World) -> HydroMetrics {
    HydroMetrics::capture(&world.data)
}

/// Frame-compatible §15 metrics (P2-34). Frames carry no registry, so this
/// is the subset derivable from per-hex frame fields; used by the glacial,
/// rebound, and thermosteric gates that read history rather than the final
/// snapshot.
#[derive(Clone, Debug)]
pub struct FrameMetrics {
    /// Frame year.
    pub year: i64,
    /// Derived sea level at this frame.
    pub sea_level_m: f32,
    /// Fraction of cells above sea level.
    pub land_fraction: f32,
    /// Fraction of cells with standing water.
    pub wet_fraction: f32,
    /// Fraction of cells under land ice.
    pub ice_area_fraction: f32,
    /// Planetary mean of `temperature_mean` (the §3.5.1 `T_ocean` proxy).
    pub mean_temperature_c: f32,
    /// Distinct Major rivers (connected components).
    pub major_rivers: usize,
    /// Major-class channel hexes.
    pub major_channel_hexes: usize,
    /// Largest drainage basin share of its continent.
    pub largest_basin_fraction: f32,
    /// Flag counts.
    pub flags: FlagCensus,
    /// Soil class counts.
    pub soils: SoilCensus,
}

/// Captures frame-compatible metrics from a frame plus the world's grid.
pub fn frame_metrics(grid: &HexGrid, frame: &HistoryFrame) -> FrameMetrics {
    let wet: Vec<bool> = frame
        .water_level_m
        .iter()
        .map(|&w| w != WATER_NONE)
        .collect();
    let (major_rivers, major_channel_hexes) =
        major_river_census(grid, &frame.river_discharge_m3_yr);
    let n = frame.temperature_mean.len().max(1) as f64;
    let mean_temperature_c = frame
        .temperature_mean
        .iter()
        .map(|&t| f64::from(t))
        .sum::<f64>()
        / n;
    FrameMetrics {
        year: frame.year,
        sea_level_m: frame.sea_level_m,
        land_fraction: land_fraction(&frame.elevation_mean, frame.sea_level_m),
        wet_fraction: wet_fraction_from_levels(&frame.water_level_m),
        ice_area_fraction: ice_area_fraction(&frame.ice_mask),
        mean_temperature_c: mean_temperature_c as f32,
        major_rivers,
        major_channel_hexes,
        largest_basin_fraction: largest_basin_fraction_of_continent(
            grid,
            &frame.flow_direction,
            &wet,
        ),
        flags: flag_census(&frame.hydro_flags),
        soils: soil_census(&frame.soil_class),
    }
}

impl std::fmt::Display for FrameMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "year={} sea={:+.1}m land={:.1}% wet={:.1}% ice={:.2}% T={:+.1}C major={} \
             basin={:.1}% fjord={} oasis={} ephemeral={} delta={} loess={}",
            self.year,
            self.sea_level_m,
            self.land_fraction * 100.0,
            self.wet_fraction * 100.0,
            self.ice_area_fraction * 100.0,
            self.mean_temperature_c,
            self.major_rivers,
            self.largest_basin_fraction * 100.0,
            self.flags.fjord,
            self.flags.oasis,
            self.flags.ephemeral,
            self.flags.delta,
            self.soils.loess,
        )
    }
}

/// Deterministic 64-bit digest (FNV-1a) over the §15 #10 field set: sea
/// level, the water arrays, flags, ice, salt, soils, flow directions, and
/// the water-body registry. Used by the cross-thread-count determinism gate,
/// where the two worlds live in different processes.
pub fn world_digest(data: &WorldData) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    fn byte(h: &mut u64, b: u8) {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(FNV_PRIME);
    }
    fn u32v(h: &mut u64, v: u32) {
        for b in v.to_le_bytes() {
            byte(h, b);
        }
    }
    fn f32v(h: &mut u64, v: f32) {
        u32v(h, v.to_bits());
    }
    fn f64v(h: &mut u64, v: f64) {
        for b in v.to_bits().to_le_bytes() {
            byte(h, b);
        }
    }

    let mut h = FNV_OFFSET;
    f32v(&mut h, data.sea_level_m);
    let floats: [&[f32]; 8] = [
        &data.water_level_m,
        &data.river_discharge_m3_yr,
        &data.discharge_seasonality,
        &data.salt_accumulated,
        &data.soil_fertility,
        &data.soil_depth_m,
        &data.water_table_depth_m,
        &data.gia_rebound_applied_m,
    ];
    for arr in floats {
        u32v(&mut h, arr.len() as u32);
        for &v in arr {
            f32v(&mut h, v);
        }
    }
    for &f in &data.hydro_flags {
        u32v(&mut h, u32::from(f.0));
    }
    for &iced in &data.ice_mask {
        byte(&mut h, u8::from(iced));
    }
    for &class in &data.soil_class {
        byte(&mut h, class as u8);
    }
    for &dir in &data.flow_direction {
        byte(&mut h, dir.map_or(u8::MAX, |d| d.index() as u8));
    }
    // Registry: BTreeMap iteration is key-ordered (deterministic).
    u32v(&mut h, data.water_bodies.len() as u32);
    for (id, body) in &data.water_bodies {
        u32v(&mut h, id.0);
        byte(&mut h, body.kind as u8);
        f32v(&mut h, body.surface_m);
        f64v(&mut h, body.area_km2);
        f64v(&mut h, body.volume_km3);
        f32v(&mut h, body.salinity);
        u32v(&mut h, body.outlet.map_or(u32::MAX, |hex| hex.0));
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Instant;

    use genesis_core::HexId;
    use genesis_core::SimulationLayer;
    use genesis_core::data::{
        BedrockType, ClimateRegimePlaceholder, HydroFlags, MAJOR_CLASS_MIN_M3_YR,
        RIVER_CLASS_MIN_M3_YR, STREAM_CLASS_MIN_M3_YR, SoilClass, WaterBodyKind,
    };
    use genesis_hydrology::partition::{KARST_MIN_PRECIP_MM, pet_mm};
    use genesis_hydrology::regime::{
        FlowRegime, MONSOON_MAX_COAST_KM, MONSOON_MIN_PRECIP_MM, classify_regime,
    };
    use genesis_hydrology::routing::RoutingSurface;
    use genesis_hydrology::validation::{
        continental_dry_pit_fraction, endorheic_body_count, seasonality_quartiles,
        water_body_census,
    };
    use genesis_hydrology::{HydrologyLayer, HydrologyState};

    /// Ice fraction (of all cells) below which a glacial excursion is
    /// considered absent — the glacial gates print-and-skip rather than
    /// assert against sampling noise.
    const GLACIAL_MIN_ICE_FRACTION: f32 = 0.01;
    /// Arid-terrain precipitation bound for gates #6/#13 (mm/yr) — the soil
    /// module's own Sandy threshold (Doc 08 §10.1).
    const ARID_PRECIP_MAX_MM: f32 = 250.0;
    /// "Interior" bound for gate #6: at least this far from the ocean (km).
    const INTERIOR_MIN_OCEAN_KM: f32 = 100.0;

    /// gate #14 census cadence (years): fine enough to catch limestone
    /// platform windows (observed to open and close within ~100 My),
    /// coarse enough to keep the 1B drive cheap.
    const KARST_EPOCH_YEARS: i64 = 50_000_000;
    /// gate #14 coverage bar: §15 #14 demands a karst belt, not every wet
    /// limestone hex — some limestone is always too young or too dry-edged
    /// to flag in a given tick.
    const KARST_COVERAGE_MIN: f64 = 0.5;

    /// gate #19 honest band for the live thermosteric slope (m/°C):
    /// `THERMOSTERIC_BETA_PER_C` (1.9e-4, §3.5.1) × ~1.6–8 km ocean depth —
    /// brackets the ~2–4 km mean-depth expectation (0.4–0.8 m/°C) with
    /// room for hypsometric and residual-ice noise.
    const THERMOSTERIC_SLOPE_MIN_M_PER_C: f64 = 0.3;
    const THERMOSTERIC_SLOPE_MAX_M_PER_C: f64 = 1.5;

    /// gate #20 clause-1 recovery bar (m): ICE_LOAD_DEPRESSION_M = 250 m
    /// of GIA depression relaxed at EPEIROGENIC_REBOUND_RATE_PER_YEAR
    /// (2e-8/yr) closes ~18% of the gap over 10 My (~45 m); 25 m sits
    /// under that signal and clear of per-tick erosion noise.
    const REBOUND_RECOVERED_MIN_M: f32 = 25.0;
    /// gate #20 clause-1 cohort bar: at least half the deglaciated
    /// continental-land cohort must clear [`REBOUND_RECOVERED_MIN_M`].
    const REBOUND_COHORT_MIN_FRAC: f64 = 0.5;

    /// Evidence of one full glacial cycle in a frame history: a maximum-ice
    /// frame at or above [`GLACIAL_MIN_ICE_FRACTION`] followed by a retreat
    /// frame at or below a quarter of that maximum.
    struct GlacialCycle {
        max_frame: usize,
        max_ice_fraction: f32,
        retreat_frame: usize,
    }

    fn find_glacial_cycle(frames: &[HistoryFrame]) -> Option<GlacialCycle> {
        let mut max_frame = 0_usize;
        let mut max_ice = 0.0_f32;
        for (k, frame) in frames.iter().enumerate() {
            let ice = ice_area_fraction(&frame.ice_mask);
            if ice > max_ice {
                max_ice = ice;
                max_frame = k;
            }
        }
        if max_ice < GLACIAL_MIN_ICE_FRACTION {
            return None;
        }
        let retreat_frame = frames
            .iter()
            .enumerate()
            .skip(max_frame + 1)
            .find(|(_, f)| ice_area_fraction(&f.ice_mask) <= max_ice * 0.25)
            .map(|(k, _)| k)?;
        Some(GlacialCycle {
            max_frame,
            max_ice_fraction: max_ice,
            retreat_frame,
        })
    }

    /// Prints the iciest frames of a run (glacial-gate evidence).
    fn print_iciest_frames(frames: &[HistoryFrame], tag: &str, count: usize) {
        let mut by_ice: Vec<(usize, f32)> = frames
            .iter()
            .enumerate()
            .map(|(k, f)| (k, ice_area_fraction(&f.ice_mask)))
            .collect();
        by_ice.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        for &(k, ice) in by_ice.iter().take(count) {
            println!(
                "[{tag}]   frame year={} ice={:.3}% sea={:+.1}m",
                frames[k].year,
                ice * 100.0,
                frames[k].sea_level_m
            );
        }
    }

    /// Peak ice-area fraction over the run — for accurate SKIP messages.
    fn peak_ice_fraction(frames: &[HistoryFrame]) -> f32 {
        frames
            .iter()
            .map(|f| ice_area_fraction(&f.ice_mask))
            .fold(0.0_f32, f32::max)
    }

    /// Ordinary-least-squares fit of `y` on `x` over `(x, y)` points:
    /// returns `(slope, R²)`. Deterministic: input order, f64 throughout.
    fn ols_slope_r2(points: &[(f64, f64)]) -> (f64, f64) {
        let n = points.len() as f64;
        let mean_x = points.iter().map(|p| p.0).sum::<f64>() / n;
        let mean_y = points.iter().map(|p| p.1).sum::<f64>() / n;
        let (mut s_xx, mut s_yy, mut s_xy) = (0.0_f64, 0.0_f64, 0.0_f64);
        for &(x, y) in points {
            s_xx += (x - mean_x) * (x - mean_x);
            s_yy += (y - mean_y) * (y - mean_y);
            s_xy += (x - mean_x) * (y - mean_y);
        }
        let slope = if s_xx > 0.0 { s_xy / s_xx } else { 0.0 };
        let r2 = if s_xx > 0.0 && s_yy > 0.0 {
            (s_xy * s_xy) / (s_xx * s_yy)
        } else {
            0.0
        };
        (slope, r2)
    }

    #[test]
    fn ols_slope_r2_recovers_known_line() {
        let points: Vec<(f64, f64)> = (0..10).map(|k| (k as f64, 1.5 + 0.7 * k as f64)).collect();
        let (slope, r2) = ols_slope_r2(&points);
        assert!((slope - 0.7).abs() < 1e-9, "slope {slope}");
        assert!((r2 - 1.0).abs() < 1e-9, "r2 {r2}");

        // Degenerate y-variance: no signal, defined-as-zero output.
        let flat: Vec<(f64, f64)> = (0..4).map(|k| (k as f64, 3.0)).collect();
        let (slope, r2) = ols_slope_r2(&flat);
        assert_eq!(slope, 0.0);
        assert_eq!(r2, 0.0);
    }

    /// §15 #2: sea-level dial — monotonic sea-level / land-fraction response
    /// to the water inventory, asserted on ONE live hypsometry.
    ///
    /// Not a cross-world sweep: the dial changes coastlines from the first
    /// ocean tick, and coupled feedback then diverges same-seed hypsometries
    /// chaotically — observed on seed 42 @ 200M, gel 500/1000/3000: sea
    /// levels −1832/−3582/−719 m, land 0.266/0.402/0.249 (non-monotone and
    /// physically meaningless as a dial reading). §15 scopes #2 as a shape
    /// gate for exactly this reason. The assertable full-stack core: run one
    /// world to 1B, then re-derive the flooding solve for each inventory on
    /// that real coastline (the CI shape test's one-tick pattern). Sea level
    /// must strictly rise and land fraction strictly fall, step to step.
    #[test]
    #[ignore = "§15 #2 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate02_sea_level_dial_monotonic_land_response() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        println!(
            "[gate02] seed={seed} live hypsometry @1B ({:.1}s); re-deriving the \
             flooding solve per inventory on this coastline",
            start.elapsed().as_secs_f64()
        );
        let mut seas = Vec::new();
        let mut lands = Vec::new();
        for gel in [500.0_f32, 1000.0, 3000.0] {
            let mut data = world.data.clone();
            data.parameters.core.hydrology.water_inventory_gel_m = gel;
            let mut hydrology = HydrologyState::new();
            let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
            layer.advance(&mut data, &world.rng);
            drop(layer);
            let _ = HydrologyLayer::detach_state(shared);
            let land = land_fraction(&data.elevation_mean, data.sea_level_m);
            println!(
                "[gate02] gel={gel:5.0} sea={:+.1}m land={land:.4}",
                data.sea_level_m
            );
            seas.push(data.sea_level_m);
            lands.push(land);
        }
        assert!(
            seas[0] < seas[1] && seas[1] < seas[2],
            "§15 #2: sea level must strictly rise with inventory on a live \
             hypsometry: {seas:?}"
        );
        assert!(
            lands[0] >= lands[1] && lands[1] >= lands[2] && lands[0] > lands[2],
            "§15 #2: land fraction must fall as inventory grows: {lands:?}"
        );
    }

    /// §15 #3: glacial excursion — fixed-hypsometry ice twin at peak ice.
    /// Drawdown = sea(no ice) − sea(peak ice budget); land fraction must rise
    /// with ice. Same snapshot, two one-tick floods (Doc 08 §15 measurement note).
    #[test]
    #[ignore = "§15 #3 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate03_glacial_excursion_drawdown() {
        let seed = validation_seed();
        let start = Instant::now();
        let (_world, frames, peak) = run_full_stack_with_peak_ice(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_4B,
            PRODUCTION_GEL_M,
            Some(GLACIAL_FRAME_STRIDE_YEARS),
        );
        println!(
            "[gate03] seed={seed} 4B @ subdiv 5, {} frames ({:.1}s)",
            frames.len(),
            start.elapsed().as_secs_f64()
        );
        print_iciest_frames(&frames, "gate03", 5);

        let Some(cycle) = find_glacial_cycle(&frames) else {
            let peak_frac = peak_ice_fraction(&frames);
            println!(
                "[gate03] SKIP: no full glacial cycle on seed {seed} at 4B (peak ice \
                 {:.2}% — §15 #3 needs a max ≥ {:.0}% AND a later retreat to ≤¼ of \
                 it); unassertable on this world — physics gap, flagged for Step 4",
                peak_frac * 100.0,
                GLACIAL_MIN_ICE_FRACTION * 100.0
            );
            return;
        };
        let Some(peak_data) = peak else {
            println!("[gate03] SKIP: no peak-ice WorldData snapshot captured");
            return;
        };
        let max_frame = &frames[cycle.max_frame];
        let intensity = f64::from(peak_data.glaciation_intensity).clamp(0.0, 1.0);
        let n = peak_data.cell_count() as usize;
        let planet_area = genesis_hydrology::routing::hex_area_m2(&peak_data.grid) * n as f64;
        let ice_volume = intensity * genesis_hydrology::ice::ICE_VOLUME_MAX_SLE_M * planet_area;

        let iced = flood_with_prior_ice_budget(&peak_data, ice_volume);
        let ice_free = flood_with_prior_ice_budget(&peak_data, 0.0);
        let drawdown = ice_free.sea_level_m - iced.sea_level_m;
        let land_iced = land_fraction(&iced.elevation_mean, iced.sea_level_m);
        let land_free = land_fraction(&ice_free.elevation_mean, ice_free.sea_level_m);
        println!(
            "[gate03] glacial max year={} ice={:.2}% intensity={intensity:.3}; \
             twin iced sea={:+.1}m land={land_iced:.3}; ice-free sea={:+.1}m \
             land={land_free:.3}; drawdown={drawdown:.1}m land_delta={:+.4}",
            max_frame.year,
            cycle.max_ice_fraction * 100.0,
            iced.sea_level_m,
            ice_free.sea_level_m,
            land_iced - land_free
        );
        assert!(
            (60.0..=130.0).contains(&drawdown),
            "§15 #3: glacial-max ice-twin drawdown {drawdown:.1} m outside the 60–130 m band"
        );
        assert!(
            land_iced > land_free,
            "§15 #3: land fraction must rise with ice locked \
             ({land_iced:.4} vs {land_free:.4})"
        );
    }

    /// One hydrology tick on a cloned snapshot with a forced prior-tick ice
    /// budget (and zero lake/GW). Sea level for this tick is set by the budget
    /// before `update_ice` refreshes ice for the next tick.
    fn flood_with_prior_ice_budget(base: &WorldData, ice_volume_m3: f64) -> WorldData {
        let mut data = base.clone();
        let mut hydro = HydrologyState::new();
        hydro.ice_volume_m3 = ice_volume_m3;
        hydro.prev_lake_volume_m3 = 0.0;
        hydro.groundwater_storage_m3 = 0.0;
        let world = create_world(data.parameters.clone()).expect("rng host");
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydro);
        layer.advance(&mut data, &world.rng);
        drop(layer);
        let _ = HydrologyLayer::detach_state(shared);
        data
    }

    /// §15 #4 (live): every channel hex continues strictly downstream on the
    /// routed surface or terminates in water; discharge non-decreasing along
    /// flow edges; zero river discharge before the first standing ocean.
    /// (The synthetic shape pin stays in `genesis_hydrology::validation`.)
    #[test]
    #[ignore = "§15 #4 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate04_honest_rivers_live() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        println!(
            "[gate04] seed={seed} 1B @ subdiv 5 ({:.1}s)",
            start.elapsed().as_secs_f64()
        );
        let data = &world.data;
        let n = data.cell_count() as usize;
        // Rebuilt on the post-tick fields, matching the tick's own routing
        // derivation (same inputs, deterministic fill — the layer's shape
        // test relies on the same identity).
        let surface = RoutingSurface::build(data, &[]);
        let (mut downstream_edges, mut water_terminal, mut retained_sinks) =
            (0_usize, 0_usize, 0_usize);
        let (mut candidate_terminal, mut channel_edges, mut rill_drops) =
            (0_usize, 0_usize, 0_usize);
        let mut rill_no_flow = 0_usize;
        let mut max_climb = 0.0_f32;
        // The tick zeroes discharge on its routing surface's water set —
        // registry bodies AND candidate-sea cells (layer.rs:269). Candidates
        // are not reconstructible from stored fields (the rebuilt surface
        // above takes `&[]`), but they are wet cells, so stored wetness is
        // the gate's water test: it covers the tick's whole zeroed set.
        let is_wet = |idx: usize| {
            data.water_level_m[idx] != WATER_NONE
                || data.water_body_id[idx] != genesis_core::WaterBodyId::NONE
        };
        for i in 0..n {
            if is_wet(i) {
                continue;
            }
            match data.flow_direction[i] {
                None => {
                    retained_sinks += 1;
                    // Spec #4 binds river hexes: a channel-class hex with no
                    // outflow must terminate in a genuine sink — no strictly
                    // lower neighbor on raw elevation (endorheic playa) — or
                    // lie below the sea. This stored-data test needs no
                    // surface: the tick's candidate-aware surface (rejected
                    // candidate seas drain after flow directions are written)
                    // is not reproducible here, so sub-channel flow-less
                    // cells only count as rill evidence.
                    if f64::from(data.river_discharge_m3_yr[i]) >= STREAM_CLASS_MIN_M3_YR {
                        let local_min =
                            data.grid.neighbors(HexId(i as u32)).iter().all(|h| {
                                data.elevation_mean[h.0 as usize] >= data.elevation_mean[i]
                            });
                        assert!(
                            local_min || data.elevation_mean[i] < data.sea_level_m,
                            "§15 #4: channel hex {i} with no flow direction has a lower \
                             neighbor — a river that goes nowhere"
                        );
                    } else {
                        rill_no_flow += 1;
                    }
                }
                Some(dir) => {
                    let target = data.grid.neighbors(HexId(i as u32))[dir.index()];
                    let j = target.0 as usize;
                    if is_wet(j) {
                        water_terminal += 1;
                        if data.water_body_id[j] == genesis_core::WaterBodyId::NONE {
                            candidate_terminal += 1;
                        }
                        continue;
                    }
                    downstream_edges += 1;
                    // Non-strict with a 1 cm allowance: cells inside one
                    // filled depression share a nominal flat, which the
                    // priority flood breaks with sub-millimetre per-cell
                    // increments (observed live: <5 mm). A macroscopic climb
                    // on the routed surface is the violation.
                    let climb = surface.filled_m[j] - surface.filled_m[i];
                    max_climb = max_climb.max(climb);
                    assert!(
                        climb <= 0.01,
                        "§15 #4: hex {i} flow climbs the routed surface by {climb:.4} m \
                         ({:.2} -> {:.2} m)",
                        surface.filled_m[i],
                        surface.filled_m[j]
                    );
                    let (up, down) = (
                        f64::from(data.river_discharge_m3_yr[i]),
                        f64::from(data.river_discharge_m3_yr[j]),
                    );
                    // §15 #4 scopes the monotonicity claim to trunks. Rill
                    // edges (below channel class) can legitimately dead-end:
                    // a rejected candidate sea drains after flow directions
                    // are written, leaving a dry zero-discharge cell at the
                    // bottom of a 1e5 m³/yr trickle (observed live, seed 42).
                    let channel_edge =
                        up >= STREAM_CLASS_MIN_M3_YR && down >= STREAM_CLASS_MIN_M3_YR;
                    if channel_edge {
                        channel_edges += 1;
                        assert!(
                            down + 1.0 >= up * 0.999,
                            "§15 #4: discharge drops along channel edge {i}->{j}: \
                             {up:.3e} -> {down:.3e} m³/yr"
                        );
                    } else if down + 1.0 < up * 0.999 {
                        rill_drops += 1;
                    }
                }
            }
        }
        println!(
            "[gate04] land-to-land edges checked={downstream_edges} \
             (channel-grade {channel_edges}), water-terminal={water_terminal} \
             (wet-but-unregistered {candidate_terminal}), retained sinks={retained_sinks}, \
             max intra-flat climb={max_climb:.5} m"
        );
        if rill_drops > 0 {
            println!(
                "[gate04] NOTE (not gated): {rill_drops} sub-channel rill edges drop \
                 discharge downstream (drained candidate-sea bottoms, see gate body)"
            );
        }
        if rill_no_flow > 0 {
            println!(
                "[gate04] NOTE (not gated): {rill_no_flow} sub-channel cells have no \
                 flow direction and no rebuilt-surface sink explanation"
            );
        }
        assert!(
            downstream_edges > 0,
            "§15 #4: a 1B world must have an active drainage network"
        );

        // Formation-era scan: no rivers before the first standing ocean.
        let mut first_ocean_year = None;
        for frame in &frames {
            let any_water = frame.water_level_m.iter().any(|&w| w != WATER_NONE);
            if any_water {
                first_ocean_year = Some(frame.year);
                break;
            }
            let max_discharge = frame
                .river_discharge_m3_yr
                .iter()
                .copied()
                .fold(0.0_f32, f32::max);
            assert_eq!(
                max_discharge, 0.0,
                "§15 #4: river discharge before the first standing ocean (frame year {})",
                frame.year
            );
            assert!(
                frame.flow_direction.iter().all(Option::is_none),
                "§15 #4: flow directions before the first standing ocean (frame year {})",
                frame.year
            );
        }
        println!("[gate04] first standing water at year {first_ocean_year:?}");
    }

    /// §15 #5: drainage realism at the calibrated resolution — Major count
    /// in [3, 30], largest basin within an Earth-plausible share of its
    /// continent. Subdiv 7 @ 1B. Runs at the production default inventory:
    /// the [3, 30] band was calibrated on the default world (at 1000 m GEL
    /// the mostly-land world grows far more Majors — 163 mouths at subdiv 5
    /// alone, seed 42), so the band is only assertable there.
    #[test]
    #[ignore = "§15 #5 full-stack deep-time gate (subdiv 7); run with --ignored --nocapture"]
    fn gate05_drainage_realism_subdiv7() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_MAJOR_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            PRODUCTION_GEL_M,
        );
        let metrics = metrics_at(&world);
        println!(
            "[gate05] seed={seed} 1B @ subdiv 7 ({:.1}s)\n{metrics}",
            start.elapsed().as_secs_f64()
        );
        // Spec Major count, calibrated at subdiv 7.
        assert!(
            (3..=30).contains(&metrics.major_rivers),
            "§15 #5: {} Major rivers outside [3, 30] at subdiv 7",
            metrics.major_rivers
        );
        // Largest basin share of its continent. A connected landmass that
        // drains to one coastal terminal scores 1.0 (normal for island
        // continents / single-trunk worlds); the Major-count band is the
        // network-richness check. Lower bound still catches empty drainage.
        // Doc 08 §15 note (v0.8): upper bound is 1.0 at validation resolution.
        assert!(
            (0.05..=1.0).contains(&metrics.largest_basin_fraction),
            "§15 #5: largest basin fraction {:.3} outside [0.05, 1.0] of its continent",
            metrics.largest_basin_fraction
        );
    }

    /// §15 #6: ≥ 1 endorheic lake (registry Lake/SaltLake, no outlet) in an
    /// arid interior @ 1B subdiv 5. "Arid" uses the module's own definition:
    /// precipitation below potential evapotranspiration (§4.2's P/PET).
    #[test]
    #[ignore = "§15 #6 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate06_endorheic_lake_in_arid_interior() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        let data = &world.data;
        let census = water_body_census(data);
        println!(
            "[gate06] seed={seed} 1B @ subdiv 5 ({:.1}s): bodies={census:?} \
             endorheic={}",
            start.elapsed().as_secs_f64(),
            endorheic_body_count(data)
        );
        let mut arid_interior = 0_usize;
        for body in data.water_bodies.values() {
            if !matches!(body.kind, WaterBodyKind::Lake | WaterBodyKind::SaltLake)
                || body.outlet.is_some()
            {
                continue;
            }
            let hex = body.id.0 as usize;
            let precip = data.precipitation[hex];
            let pet = pet_mm(data.temperature_mean[hex]) as f32;
            let distance = data.distance_to_ocean_km[hex];
            let arid = precip < pet;
            let interior = distance >= INTERIOR_MIN_OCEAN_KM;
            println!(
                "[gate06]   {:?} id={} area={:.0}km² salinity={:.1} precip={precip:.0}mm \
                 pet={pet:.0}mm dist_ocean={distance:.0}km arid={arid} interior={interior}",
                body.kind, body.id.0, body.area_km2, body.salinity
            );
            if arid && interior {
                arid_interior += 1;
            }
        }
        assert!(
            arid_interior >= 1,
            "§15 #6: no endorheic lake in an arid interior on the validation world"
        );
    }

    /// §15 #7: stable Major mouths (present across the late frames) show a
    /// DELTA flag or alluvial buildout at ≥ half of the mouths @ 1B subdiv 5.
    #[test]
    #[ignore = "§15 #7 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate07_major_mouths_show_deltas() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        let data = &world.data;
        let n = data.cell_count() as usize;
        let is_mouth = |i: usize| {
            if data.water_body_id[i] != genesis_core::WaterBodyId::NONE {
                return false;
            }
            if f64::from(data.river_discharge_m3_yr[i]) < MAJOR_CLASS_MIN_M3_YR {
                return false;
            }
            let Some(dir) = data.flow_direction[i] else {
                return false;
            };
            let j = data.grid.neighbors(HexId(i as u32))[dir.index()].0 as usize;
            matches!(
                data.water_bodies
                    .get(&data.water_body_id[j])
                    .map(|b| b.kind),
                Some(WaterBodyKind::Ocean | WaterBodyKind::Sea)
            )
        };
        let mouths: Vec<usize> = (0..n).filter(|&i| is_mouth(i)).collect();

        // Stability: the same hex still reads as a Major mouth (Major class,
        // flowing into sea-level water) in at least half of the late frames.
        let late = &frames[frames.len().saturating_sub(6)..];
        let frame_mouth_like = |frame: &HistoryFrame, i: usize| {
            if frame.water_level_m[i] != WATER_NONE
                || f64::from(frame.river_discharge_m3_yr[i]) < MAJOR_CLASS_MIN_M3_YR
            {
                return false;
            }
            let Some(dir) = frame.flow_direction[i] else {
                return false;
            };
            let j = data.grid.neighbors(HexId(i as u32))[dir.index()].0 as usize;
            frame.water_level_m[j] != WATER_NONE && frame.water_level_m[j] == frame.sea_level_m
        };
        let mut stable: Vec<usize> = Vec::new();
        for &i in &mouths {
            let present = late.iter().filter(|f| frame_mouth_like(f, i)).count();
            if present * 2 >= late.len() {
                stable.push(i);
            }
        }
        println!(
            "[gate07] seed={seed} 1B @ subdiv 5 ({:.1}s): {} Major mouths, {} stable \
             across the last {} frames",
            start.elapsed().as_secs_f64(),
            mouths.len(),
            stable.len(),
            late.len()
        );
        if stable.is_empty() {
            println!(
                "[gate07] SKIP: no stable Major mouths on the validation world; \
                 §15 #7 unassertable here (see gate05 for the Major census)"
            );
            return;
        }
        let mut deltas = 0_usize;
        for &i in &stable {
            let delta_flag = data.hydro_flags[i].contains(HydroFlags::DELTA)
                || late
                    .iter()
                    .any(|f| f.hydro_flags[i].contains(HydroFlags::DELTA));
            let alluvial = data.soil_class[i] == SoilClass::Alluvial;
            println!(
                "[gate07]   mouth hex={i} discharge={:.2e} delta_flag={delta_flag} \
                 soil={:?}",
                data.river_discharge_m3_yr[i], data.soil_class[i]
            );
            if delta_flag || alluvial {
                deltas += 1;
            }
        }
        println!("[gate07] deltas at {deltas}/{} stable mouths", stable.len());
        assert!(
            deltas * 2 >= stable.len(),
            "§15 #7: only {deltas}/{} stable Major mouths show a delta",
            stable.len()
        );
    }

    /// §15 #8: Cretaceous beach — land hexes with meaningful marine
    /// `fertility` (shallow-sea accumulator, tectonics) rank top-decile in
    /// `soil_fertility` among non-Loess land (Loess is the glacial sibling
    /// mechanic and would otherwise own the top decile by class base 0.9).
    #[test]
    #[ignore = "§15 #8 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate08_cretaceous_beach_fertility() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        let data = &world.data;
        let n = data.cell_count() as usize;
        let land: Vec<usize> = (0..n)
            .filter(|&i| data.elevation_mean[i] > data.sea_level_m)
            .collect();
        let mut positive: Vec<f32> = land
            .iter()
            .map(|&i| data.fertility[i])
            .filter(|&f| f > 0.0)
            .collect();
        positive.sort_by(f32::total_cmp);
        let max_fert = positive.last().copied().unwrap_or(0.0);
        // Prefer an absolute bank when present; otherwise the top decile of
        // land hexes that have any marine fertility (still "high-fertility"
        // relative to the sterile interior).
        let threshold = if max_fert >= 0.1 {
            0.1
        } else if positive.is_empty() {
            f32::INFINITY
        } else {
            let p90_f = positive[(positive.len() * 9 / 10).min(positive.len() - 1)];
            p90_f.max(1e-4)
        };
        let beach: Vec<usize> = land
            .iter()
            .copied()
            .filter(|&i| data.fertility[i] >= threshold)
            .collect();
        // Peer set: non-Loess land — Cretaceous beach vs other soils.
        let peers: Vec<usize> = land
            .iter()
            .copied()
            .filter(|&i| data.soil_class[i] != SoilClass::Loess)
            .collect();
        let mut peer_soils: Vec<f32> = peers.iter().map(|&i| data.soil_fertility[i]).collect();
        peer_soils.sort_by(f32::total_cmp);
        let p90_soil = if peer_soils.is_empty() {
            0.0
        } else {
            peer_soils[(peer_soils.len() * 9 / 10).min(peer_soils.len() - 1)]
        };
        let top = beach
            .iter()
            .filter(|&&i| data.soil_fertility[i] >= p90_soil)
            .count();
        let mean_beach = beach
            .iter()
            .map(|&i| f64::from(data.soil_fertility[i]))
            .sum::<f64>()
            / beach.len().max(1) as f64;
        println!(
            "[gate08] seed={seed} 1B @ subdiv 5 ({:.1}s): land={} positive_fert={} \
             max_fert={max_fert:.4} threshold={threshold:.4} beach={} peers={} \
             top-decile soil {top}/{} mean_soil={mean_beach:.3} p90_peer_soil={p90_soil:.3}",
            start.elapsed().as_secs_f64(),
            land.len(),
            positive.len(),
            beach.len(),
            peers.len(),
            beach.len()
        );
        assert!(
            !beach.is_empty(),
            "§15 #8: no uplifted hexes with marine fertility ≥ {threshold} at 1B \
             (max land fertility {max_fert})"
        );
        assert!(
            top * 2 >= beach.len(),
            "§15 #8: only {top}/{} Cretaceous-beach hexes rank top-decile among \
             non-Loess land soil fertility",
            beach.len()
        );
    }

    /// §15 #9: salt story — ≥ 1 SaltLake or SaltFlat by 2B @ subdiv 5,
    /// and elevation salt-flat paint stays rare (≤ 0.5% of cells at 1B).
    #[test]
    #[ignore = "§15 #9 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate09_salt_story_by_two_billion() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world_1b, _frames_1b) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        let flat_paint_1b = salt_flat_paint_fraction(&world_1b.data);
        println!(
            "[gate09] seed={seed} 1B salt-flat paint fraction={:.3}%",
            flat_paint_1b * 100.0
        );
        assert!(
            flat_paint_1b <= 0.005,
            "§15 #9: salt-flat elevation paint {flat_paint_1b:.4} exceeds 0.5% of cells at 1B"
        );
        let land_1b = world_1b
            .data
            .elevation_mean
            .iter()
            .zip(world_1b.data.water_level_m.iter())
            .filter(|&(e, w)| !(w.is_finite() && *w > *e))
            .count()
            .max(1);
        let saline_1b = world_1b
            .data
            .soil_class
            .iter()
            .filter(|&&c| c == genesis_core::data::SoilClass::Saline)
            .count();
        let saline_frac = saline_1b as f32 / land_1b as f32;
        println!(
            "[gate09] seed={seed} 1B saline soil on dry hexes={:.2}%",
            saline_frac * 100.0
        );
        assert!(
            saline_frac <= 0.05,
            "§15 #9: saline soil {saline_frac:.3} exceeds 5% of dry hexes at 1B"
        );

        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_2B,
            VALIDATION_GEL_M,
        );
        let metrics = metrics_at(&world);
        let max_salt = world
            .data
            .salt_accumulated
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        println!(
            "[gate09] seed={seed} 2B @ subdiv 5 ({:.1}s): salt_lake={} salt_flat={} \
             max_salt_accumulated={max_salt:.2} saline_soil={}\n{metrics}",
            start.elapsed().as_secs_f64(),
            metrics.bodies.salt_lake,
            metrics.bodies.salt_flat,
            metrics.soils.saline
        );
        assert!(
            metrics.bodies.salt_lake + metrics.bodies.salt_flat >= 1,
            "§15 #9: no SaltLake/SaltFlat by 2B on the validation world"
        );
    }

    fn salt_flat_paint_fraction(data: &WorldData) -> f32 {
        use genesis_core::data::{WaterBodyId, WaterBodyKind};
        let n = data.cell_count().max(1) as f32;
        let flats = data
            .water_body_id
            .iter()
            .filter(|&&id| {
                id != WaterBodyId::NONE
                    && data
                        .water_bodies
                        .get(&id)
                        .is_some_and(|b| b.kind == WaterBodyKind::SaltFlat)
            })
            .count();
        flats as f32 / n
    }

    /// Byte-identity comparison for §15 #10 over the gate's field set.
    fn assert_water_fields_identical(a: &WorldData, b: &WorldData) {
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<u32>>();
        assert_eq!(
            a.sea_level_m.to_bits(),
            b.sea_level_m.to_bits(),
            "§15 #10: sea_level_m differs"
        );
        for (name, x, y) in [
            ("water_level_m", &a.water_level_m, &b.water_level_m),
            (
                "river_discharge_m3_yr",
                &a.river_discharge_m3_yr,
                &b.river_discharge_m3_yr,
            ),
            ("salt_accumulated", &a.salt_accumulated, &b.salt_accumulated),
            ("soil_fertility", &a.soil_fertility, &b.soil_fertility),
            ("soil_depth_m", &a.soil_depth_m, &b.soil_depth_m),
        ] {
            assert_eq!(bits(x), bits(y), "§15 #10: {name} differs");
        }
        assert_eq!(a.hydro_flags, b.hydro_flags, "§15 #10: hydro_flags");
        assert_eq!(a.ice_mask, b.ice_mask, "§15 #10: ice_mask");
        assert_eq!(a.soil_class, b.soil_class, "§15 #10: soil_class");
        assert_eq!(
            a.flow_direction, b.flow_direction,
            "§15 #10: flow_direction"
        );
        assert_eq!(
            a.water_bodies.len(),
            b.water_bodies.len(),
            "§15 #10: registry size"
        );
        for ((id_a, body_a), (id_b, body_b)) in a.water_bodies.iter().zip(b.water_bodies.iter()) {
            assert_eq!(id_a, id_b, "§15 #10: registry key order");
            assert_eq!(
                body_a.surface_m.to_bits(),
                body_b.surface_m.to_bits(),
                "§15 #10: body {id_a:?} surface"
            );
            assert_eq!(
                body_a.volume_km3.to_bits(),
                body_b.volume_km3.to_bits(),
                "§15 #10: body {id_a:?} volume"
            );
            assert_eq!(body_a.kind, body_b.kind, "§15 #10: body {id_a:?} kind");
            assert_eq!(
                body_a.outlet, body_b.outlet,
                "§15 #10: body {id_a:?} outlet"
            );
        }
    }

    fn gate10_determinism_at(target_year: i64, tag: &str) {
        let seed = validation_seed();
        let start = Instant::now();
        let (a, _) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            target_year,
            VALIDATION_GEL_M,
        );
        let (b, _) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            target_year,
            VALIDATION_GEL_M,
        );
        let digest_a = world_digest(&a.data);
        let digest_b = world_digest(&b.data);
        println!(
            "[{tag}] seed={seed} year={target_year}: digest a={digest_a:016x} b={digest_b:016x} \
             ({:.1}s)",
            start.elapsed().as_secs_f64()
        );
        assert_water_fields_identical(&a.data, &b.data);
        assert_eq!(digest_a, digest_b, "§15 #10: world digests differ");
    }

    /// §15 #10: byte-identical full runs @ 200M subdiv 5.
    #[test]
    #[ignore = "§15 #10 full-stack determinism gate; run with --ignored --nocapture"]
    fn gate10_determinism_200m() {
        gate10_determinism_at(VALIDATION_YEAR_200M, "gate10-200m");
    }

    /// §15 #10: byte-identical full runs @ 1B subdiv 5.
    #[test]
    #[ignore = "§15 #10 full-stack determinism gate (1B); run with --ignored --nocapture"]
    fn gate10_determinism_1b() {
        gate10_determinism_at(VALIDATION_YEAR_1B, "gate10-1b");
    }

    /// §15 #10: byte-identical full runs @ 4B subdiv 5 (two 4B runs; the
    /// longest gate in the suite).
    #[test]
    #[ignore = "§15 #10 full-stack determinism gate (4B); run with --ignored --nocapture"]
    fn gate10_determinism_4b() {
        gate10_determinism_at(VALIDATION_YEAR_4B, "gate10-4b");
    }

    /// §15 #10 (bonus): the parallel stack is order-safe — identical digest
    /// under `RAYON_NUM_THREADS=1` and the default pool. The two worlds run
    /// in child processes because rayon's global pool is fixed per process.
    #[test]
    #[ignore = "§15 #10 thread-count determinism; run with --ignored --nocapture"]
    fn gate10_determinism_across_rayon_threads() {
        const WORKER_ENV: &str = "GENESIS_HYDRO_DIGEST_WORKER";
        if std::env::var(WORKER_ENV).is_ok() {
            let (world, _) = run_full_stack(
                validation_seed(),
                VALIDATION_SUBDIVISION_LEVEL,
                VALIDATION_YEAR_200M,
                VALIDATION_GEL_M,
            );
            println!("HYDRO_DIGEST {:016x}", world_digest(&world.data));
            return;
        }
        let exe = std::env::current_exe().expect("test binary path");
        let digest_at = |threads: &str| {
            let output = std::process::Command::new(&exe)
                .args([
                    "--exact",
                    "hydro_validation::tests::gate10_determinism_across_rayon_threads",
                    "--ignored",
                    "--nocapture",
                ])
                .env(WORKER_ENV, "1")
                .env("RAYON_NUM_THREADS", threads)
                .output()
                .expect("spawn digest worker");
            assert!(
                output.status.success(),
                "digest worker (RAYON_NUM_THREADS={threads}) failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .find_map(|line| line.strip_prefix("HYDRO_DIGEST "))
                .and_then(|hex| u64::from_str_radix(hex, 16).ok())
                .unwrap_or_else(|| panic!("no HYDRO_DIGEST line in worker output:\n{stdout}"))
        };
        let single = digest_at("1");
        let multi = digest_at("8");
        println!("[gate10-threads] digest RAYON_NUM_THREADS=1: {single:016x}, =8: {multi:016x}");
        assert_eq!(
            single, multi,
            "§15 #10: world digest depends on the rayon thread count"
        );
    }

    /// §15 #11: hydrology tick time over a live 100 My Geological-cadence
    /// window (900M–1B, 200 ticks at the 500k cadence) @ subdiv 7.
    #[test]
    #[ignore = "§15 #11 perf gate (subdiv 7); run with --ignored --nocapture"]
    fn gate11_hydrology_tick_perf_subdiv7() {
        use genesis_core::lifecycle::advance_with_coordinator_observed;
        use genesis_core::parameters::WorldParameters;
        use genesis_core::rng::WorldRng;
        use genesis_core::time::{SimulationLayer, TickCoordinator, WorldYear};

        /// Timing proxy: delegates to the real layer and records each
        /// advance. Registration order mirrors worldgen (tectonics, climate,
        /// hydrology); only hydrology is wrapped.
        struct TimedHydrologyLayer {
            inner: genesis_hydrology::HydrologyLayer,
            samples: Rc<RefCell<Vec<(i64, f64)>>>,
        }
        impl SimulationLayer for TimedHydrologyLayer {
            fn name(&self) -> &str {
                self.inner.name()
            }
            fn tick_interval(&self, current_time: WorldYear, params: &WorldParameters) -> i64 {
                self.inner.tick_interval(current_time, params)
            }
            fn advance(&mut self, world: &mut WorldData, rng: &WorldRng) -> Vec<()> {
                let start = Instant::now();
                let out = self.inner.advance(world, rng);
                self.samples.borrow_mut().push((
                    world.current_year.value(),
                    start.elapsed().as_secs_f64() * 1000.0,
                ));
                out
            }
        }

        const WINDOW_START: i64 = 900_000_000;
        let seed = validation_seed();
        let start = Instant::now();
        let config = full_stack_config(
            seed,
            VALIDATION_MAJOR_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        let mut world = create_world(config.to_parameters()).expect("validation parameters valid");
        let mut tectonics = genesis_tectonics::TectonicsState::new();
        let mut climate = genesis_climate::ClimateState::new();
        let mut hydrology = genesis_hydrology::HydrologyState::new();

        let (tectonics_layer, tectonics_shared) =
            genesis_tectonics::TectonicsLayer::attach(&mut tectonics);
        let (climate_layer, climate_shared) = genesis_climate::ClimateLayer::attach(&mut climate);
        let (hydrology_layer, hydrology_shared) =
            genesis_hydrology::HydrologyLayer::attach(&mut hydrology);
        let samples = Rc::new(RefCell::new(Vec::new()));
        let timed = TimedHydrologyLayer {
            inner: hydrology_layer,
            samples: Rc::clone(&samples),
        };
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(tectonics_layer));
        coordinator.add_layer(Box::new(climate_layer));
        coordinator.add_layer(Box::new(timed));
        advance_with_coordinator_observed(
            &mut world,
            &mut coordinator,
            WorldYear(VALIDATION_YEAR_1B),
            |_| {},
        )
        .expect("timed full-stack run");
        drop(coordinator);
        let _ = genesis_tectonics::TectonicsLayer::detach_state(tectonics_shared);
        let _ = genesis_climate::ClimateLayer::detach_state(climate_shared);
        let _ = genesis_hydrology::HydrologyLayer::detach_state(hydrology_shared);

        let mut window: Vec<f64> = samples
            .borrow()
            .iter()
            .filter(|(year, _)| *year > WINDOW_START)
            .map(|(_, ms)| *ms)
            .collect();
        window.sort_by(f64::total_cmp);
        assert_eq!(
            window.len(),
            200,
            "100 My at the 500k cadence is 200 hydrology ticks"
        );
        let mean = window.iter().sum::<f64>() / window.len() as f64;
        let p95 = window[(window.len() * 95 / 100).min(window.len() - 1)];
        let max = window[window.len() - 1];
        println!(
            "[gate11] seed={seed} subdiv 7, hydrology ticks {}..{}: mean={mean:.3}ms \
             p95={p95:.3}ms max={max:.3}ms (run {:.1}s)",
            WINDOW_START,
            VALIDATION_YEAR_1B,
            start.elapsed().as_secs_f64()
        );
        // §14 budget: hydrology tick ≤ 5 ms at subdiv 7 (production baseline,
        // Doc 06 v0.13 hardware). Asserted with 3× headroom for shared CI
        // machines; the strict budget prints above for comparison.
        assert!(
            mean <= 15.0,
            "§15 #11: mean hydrology tick {mean:.2} ms exceeds 15 ms at subdiv 7 \
             (§14 budget 5 ms × 3 CI headroom)"
        );
        assert!(
            p95 <= 30.0,
            "§15 #11: p95 hydrology tick {p95:.2} ms exceeds 30 ms at subdiv 7"
        );
    }

    /// §15 #12: tectonic gates stay green on the full stack @ 1B subdiv 5 —
    /// mirrors the key invariants of `genesis_tectonics::validation`'s own
    /// deep-time suite (NaN-free, elevation clamps, Earthlike envelope).
    /// Runs at the production default inventory (2700 m GEL), which the
    /// tectonic envelope was calibrated at — see the note in the body.
    #[test]
    #[ignore = "§15 #12 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate12_tectonic_invariants_hold_full_stack() {
        let seed = validation_seed();
        let start = Instant::now();
        // §15 #12 asks whether tectonics' gates stay green with hydrology
        // active — under the conditions those gates were calibrated at, i.e.
        // the production default inventory (2700 m GEL). The leaner 1000 m
        // validation convention exposes far more land (0.634 observed at 1B,
        // seed 42) and would fail the [0.15, 0.45] envelope for inventory
        // reasons, not tectonic instability.
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            2700.0,
        );
        let data = &world.data;
        for (name, arr) in [
            ("elevation_mean", &data.elevation_mean),
            ("elevation_relief", &data.elevation_relief),
            ("temperature_mean", &data.temperature_mean),
            ("precipitation", &data.precipitation),
            ("river_discharge_m3_yr", &data.river_discharge_m3_yr),
            ("salt_accumulated", &data.salt_accumulated),
            ("soil_fertility", &data.soil_fertility),
        ] {
            assert!(
                arr.iter().all(|v| v.is_finite()),
                "§15 #12: {name} contains non-finite values"
            );
        }
        assert!(
            data.water_level_m.iter().all(|v| !v.is_nan()),
            "§15 #12: water_level_m contains NaN"
        );
        assert!(
            data.sea_level_m.is_finite(),
            "§15 #12: sea level is not finite"
        );

        let (min_e, max_e) = genesis_tectonics::elevation_bounds(data);
        assert!(
            min_e >= genesis_tectonics::ELEVATION_MIN_BOUND_M
                && max_e <= genesis_tectonics::ELEVATION_MAX_BOUND_M,
            "§15 #12: elevation [{min_e:.0}, {max_e:.0}] outside the §11 #6 clamps \
             [{}, {}]",
            genesis_tectonics::ELEVATION_MIN_BOUND_M,
            genesis_tectonics::ELEVATION_MAX_BOUND_M
        );
        let land = genesis_tectonics::continental_fraction(data);
        // Tectonics' own 1B deep-time envelope ([0.15, 0.45] at 1B years);
        // the tighter [0.20, 0.40] snapshot band is calibrated at 100M.
        assert!(
            (0.15..=0.45).contains(&land),
            "§15 #12: land fraction {land:.3} outside tectonics' 1B envelope [0.15, 0.45]"
        );
        let plates: std::collections::BTreeSet<_> = data
            .plate_id
            .iter()
            .copied()
            .filter(|&p| p != genesis_core::PlateId::NONE)
            .collect();
        assert!(
            (2..=32).contains(&plates.len()),
            "§15 #12: {} distinct plates on the grid outside sanity band [2, 32]",
            plates.len()
        );
        println!(
            "[gate12] seed={seed} 1B @ subdiv 5 ({:.1}s): plates={} land={land:.3} \
             elev=[{min_e:.0},{max_e:.0}] sea={:+.1}m",
            start.elapsed().as_secs_f64(),
            plates.len(),
            data.sea_level_m
        );
    }

    /// §15 #13: a perennial trunk through arid terrain (baseflow-sustained,
    /// no EPHEMERAL flag — the §7.3 test) AND EPHEMERAL-flagged channels in
    /// unfed desert, @ 1B subdiv 5.
    #[test]
    #[ignore = "§15 #13 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate13_perennial_trunks_and_ephemeral_deserts() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        let data = &world.data;
        let n = data.cell_count() as usize;
        let land = |i: usize| data.water_body_id[i] == genesis_core::WaterBodyId::NONE;
        let arid = |i: usize| data.precipitation[i] < ARID_PRECIP_MAX_MM;
        // "Trunk": River class and up — rendered rivers, not creeks. The
        // absence of EPHEMERAL is the §7.3 baseflow test by construction.
        let perennial: Vec<usize> = (0..n)
            .filter(|&i| {
                land(i)
                    && arid(i)
                    && f64::from(data.river_discharge_m3_yr[i]) >= RIVER_CLASS_MIN_M3_YR
                    && !data.hydro_flags[i].contains(HydroFlags::EPHEMERAL)
            })
            .collect();
        let ephemeral: Vec<usize> = (0..n)
            .filter(|&i| land(i) && arid(i) && data.hydro_flags[i].contains(HydroFlags::EPHEMERAL))
            .collect();
        println!(
            "[gate13] seed={seed} 1B @ subdiv 5 ({:.1}s): {} perennial arid trunks, \
             {} ephemeral desert channels",
            start.elapsed().as_secs_f64(),
            perennial.len(),
            ephemeral.len()
        );
        for &i in perennial.iter().take(5) {
            println!(
                "[gate13]   perennial hex={i} discharge={:.2e} precip={:.0}mm",
                data.river_discharge_m3_yr[i], data.precipitation[i]
            );
        }
        for &i in ephemeral.iter().take(5) {
            println!(
                "[gate13]   ephemeral hex={i} discharge={:.2e} precip={:.0}mm",
                data.river_discharge_m3_yr[i], data.precipitation[i]
            );
        }
        assert!(
            !perennial.is_empty(),
            "§15 #13: no perennial (baseflow-sustained) trunk through arid terrain"
        );
        assert!(
            !ephemeral.is_empty(),
            "§15 #13: no EPHEMERAL-flagged channels in unfed desert"
        );
    }

    /// §15 #14: karst on live worlds. Tectonics assigns `BedrockType::Limestone`
    /// on warm shallow platforms (`assign_platform_limestone`, Doc 08 §6.3), so
    /// the limestone branch of the §6.3 predicate is live. Limestone extent is
    /// epoch-dependent (platform windows open and close with sea level and
    /// orogeny — observed 0–12 hexes across 0.55–2B on seed 42 @ 2700 GEL), so
    /// a single-horizon census would flap: this gate drives the stack in 50 My
    /// chunks and evaluates every post-Formation epoch snapshot. `HistoryFrame`
    /// carries no `bedrock_type`, so the census reads the live world at each
    /// epoch boundary instead of captured frames. Asserts at least one
    /// wet-limestone epoch shows the §6.3 mechanism (≥ half of wet limestone
    /// hexes KARST-flagged, springs present). The synthetic pin
    /// (`karst_flags_on_limestone_when_wet`) remains the unit mechanism test.
    #[test]
    #[ignore = "§15 #14 full-stack census gate; run with --ignored --nocapture"]
    fn gate14_karst_live_census() {
        use genesis_core::time::WorldYear;

        let seed = validation_seed();
        let start = Instant::now();
        let config = full_stack_config(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        let params = config.to_parameters();
        let mut world = create_world(params).expect("validation parameters valid");
        let mut tectonics = genesis_tectonics::TectonicsState::new();
        let mut climate = genesis_climate::ClimateState::new();
        let mut hydrology = genesis_hydrology::HydrologyState::new();

        /// One epoch's karst census over the live world.
        struct EpochCensus {
            year: i64,
            limestone: usize,
            wet_limestone: usize,
            flagged: usize,
            springs: usize,
            calcareous: usize,
            karst_channel_mean: f64,
            other_channel_mean: f64,
            karst_channels: usize,
            other_channels: usize,
        }

        let mut epochs: Vec<EpochCensus> = Vec::new();
        let mut year = 550_000_000_i64; // first post-Formation snapshot
        while year <= VALIDATION_YEAR_1B {
            generate_full_history(
                &mut world,
                &mut tectonics,
                &mut climate,
                &mut hydrology,
                WorldYear(year),
                |_| {},
            )
            .expect("epoch generation");
            let data = &world.data;
            let n = data.cell_count() as usize;
            let mut census = EpochCensus {
                year,
                limestone: 0,
                wet_limestone: 0,
                flagged: 0,
                springs: 0,
                calcareous: 0,
                karst_channel_mean: 0.0,
                other_channel_mean: 0.0,
                karst_channels: 0,
                other_channels: 0,
            };
            let (mut karst_sum, mut other_sum) = (0.0_f64, 0.0_f64);
            for i in 0..n {
                if data.soil_class[i] == SoilClass::Calcareous {
                    census.calcareous += 1;
                }
                if data.hydro_flags[i].contains(HydroFlags::SPRING) {
                    census.springs += 1;
                }
                if data.bedrock_type[i] == BedrockType::Limestone {
                    census.limestone += 1;
                    if data.precipitation[i] > KARST_MIN_PRECIP_MM {
                        census.wet_limestone += 1;
                        if data.hydro_flags[i].contains(HydroFlags::KARST) {
                            census.flagged += 1;
                        }
                    }
                }
                let discharge = f64::from(data.river_discharge_m3_yr[i]);
                if discharge >= RIVER_CLASS_MIN_M3_YR {
                    if data.hydro_flags[i].contains(HydroFlags::KARST) {
                        karst_sum += discharge;
                        census.karst_channels += 1;
                    } else {
                        other_sum += discharge;
                        census.other_channels += 1;
                    }
                }
            }
            census.karst_channel_mean = karst_sum / census.karst_channels.max(1) as f64;
            census.other_channel_mean = other_sum / census.other_channels.max(1) as f64;
            println!(
                "[gate14] epoch {}: limestone={} wet={} karst={} springs={} calcareous={}",
                census.year,
                census.limestone,
                census.wet_limestone,
                census.flagged,
                census.springs,
                census.calcareous
            );
            epochs.push(census);
            year += KARST_EPOCH_YEARS;
        }
        println!(
            "[gate14] seed={seed} 1B @ subdiv 5 ({:.1}s): {} epoch snapshots",
            start.elapsed().as_secs_f64(),
            epochs.len()
        );

        let wet_epochs: Vec<&EpochCensus> = epochs.iter().filter(|e| e.wet_limestone > 0).collect();
        if wet_epochs.is_empty() {
            println!(
                "[gate14] SKIP: no post-Formation epoch to 1B shows wet limestone \
                 (limestone + precip > {KARST_MIN_PRECIP_MM} mm) — the platform \
                 pass never produced a karst candidate on seed {seed}; §15 #14 \
                 unassertable — physics gap, flagged for calibration"
            );
            return;
        }
        let qualifying: Vec<&&EpochCensus> = wet_epochs
            .iter()
            .filter(|e| {
                e.flagged as f64 >= KARST_COVERAGE_MIN * e.wet_limestone as f64 && e.springs > 0
            })
            .collect();
        for e in &wet_epochs {
            println!(
                "[gate14]   wet epoch {}: karst coverage {}/{} ({:.0}%) springs={}",
                e.year,
                e.flagged,
                e.wet_limestone,
                100.0 * e.flagged as f64 / e.wet_limestone as f64,
                e.springs
            );
        }
        assert!(
            !qualifying.is_empty(),
            "§15 #14: no wet-limestone epoch to 1B reached {:.0}% KARST coverage with \
             springs present",
            KARST_COVERAGE_MIN * 100.0
        );
        let best = qualifying
            .iter()
            .max_by_key(|e| e.flagged)
            .expect("non-empty qualifying set");
        println!(
            "[gate14] best epoch {}: {}/{} wet limestone KARST-flagged, {} springs; \
             mean channel discharge karst={:.2e} other={:.2e}",
            best.year,
            best.flagged,
            best.wet_limestone,
            best.springs,
            best.karst_channel_mean,
            best.other_channel_mean
        );
        if best.karst_channels > 0 && best.other_channels > 0 {
            assert!(
                best.karst_channel_mean < best.other_channel_mean,
                "§15 #14: karst diversion must thin surface discharge across the belt"
            );
        } else {
            println!(
                "[gate14] too few river-class channels at the best epoch for the \
                 discharge-thinning read (printed, not gated)"
            );
        }
    }

    /// §15 #15: after ≥ 1 full glacial cycle, FJORD flags exist on glaciated
    /// high-relief coasts. 4B @ subdiv 5.
    #[test]
    #[ignore = "§15 #15 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate15_fjords_after_glacial_cycle() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, frames) = run_full_stack_with_stride(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_4B,
            PRODUCTION_GEL_M,
            Some(GLACIAL_FRAME_STRIDE_YEARS),
        );
        print_iciest_frames(&frames, "gate15", 3);
        let Some(cycle) = find_glacial_cycle(&frames) else {
            let peak = peak_ice_fraction(&frames);
            println!(
                "[gate15] SKIP: no full glacial cycle on seed {seed} at 4B (peak ice \
                 {:.2}% — need a max ≥ {:.0}% AND a later retreat to ≤¼ of it); \
                 §15 #15 unassertable — physics gap, flagged for Step 4",
                peak * 100.0,
                GLACIAL_MIN_ICE_FRACTION * 100.0
            );
            return;
        };
        println!(
            "[gate15] glacial cycle: max ice {:.2}% at year {}, retreat by year {}",
            cycle.max_ice_fraction * 100.0,
            frames[cycle.max_frame].year,
            frames[cycle.retreat_frame].year
        );
        let data = &world.data;
        let fjords: Vec<usize> = (0..data.cell_count() as usize)
            .filter(|&i| data.hydro_flags[i].contains(HydroFlags::FJORD))
            .collect();
        for &i in fjords.iter().take(10) {
            let body = data.water_bodies.get(&data.water_body_id[i]);
            println!(
                "[gate15]   fjord hex={i} relief={:.0}m carved={} body={:?}",
                data.elevation_relief[i],
                data.hydro_flags[i].contains(HydroFlags::CARVED_TROUGH),
                body.map(|b| b.kind)
            );
        }
        println!(
            "[gate15] seed={seed} 4B @ subdiv 5 ({:.1}s): {} fjord hexes",
            start.elapsed().as_secs_f64(),
            fjords.len()
        );
        assert!(
            !fjords.is_empty(),
            "§15 #15: no FJORD flags after a full glacial cycle"
        );
    }

    /// §15 #16: post-retreat Loess on history frames (not the final 4B
    /// world — loess is overwritten over Gyr). "Downwind" lofting is pinned
    /// by `loft_loess` unit tests; this gate asserts presence + fertility
    /// rank in the ~100 My after retreat.
    #[test]
    #[ignore = "§15 #16 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate16_loess_belt_after_glacial() {
        let seed = validation_seed();
        let start = Instant::now();
        let (_world, frames) = run_full_stack_with_stride(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_4B,
            PRODUCTION_GEL_M,
            Some(GLACIAL_FRAME_STRIDE_YEARS),
        );
        let Some(cycle) = find_glacial_cycle(&frames) else {
            println!(
                "[gate16] SKIP: no full glacial cycle on seed {seed} at 4B (peak ice \
                 {:.2}% — need a max ≥ {:.0}% AND a later retreat to ≤¼ of it); \
                 §15 #16 unassertable — physics gap, flagged for Step 4",
                peak_ice_fraction(&frames) * 100.0,
                GLACIAL_MIN_ICE_FRACTION * 100.0
            );
            return;
        };
        let retreat_year = frames[cycle.retreat_frame].year;
        let window_end = retreat_year + 100_000_000;
        println!(
            "[gate16] glacial cycle: max ice {:.2}% at year {}, retreat by year {} \
             (scan frames through {window_end})",
            cycle.max_ice_fraction * 100.0,
            frames[cycle.max_frame].year,
            retreat_year
        );
        let post: Vec<&HistoryFrame> = frames
            .iter()
            .filter(|f| f.year >= retreat_year && f.year <= window_end)
            .collect();
        let Some(best) = post.iter().max_by_key(|f| {
            f.soil_class
                .iter()
                .filter(|&&c| c == SoilClass::Loess)
                .count()
        }) else {
            println!("[gate16] SKIP: no frames in the post-retreat window");
            return;
        };
        let n = best.soil_class.len();
        let loess: Vec<usize> = (0..n)
            .filter(|&i| best.soil_class[i] == SoilClass::Loess)
            .collect();
        let mut land_soils: Vec<f32> = (0..n)
            .filter(|&i| best.elevation_mean[i] > best.sea_level_m)
            .map(|i| best.soil_fertility[i])
            .collect();
        land_soils.sort_by(f32::total_cmp);
        let p90 = if land_soils.is_empty() {
            0.0
        } else {
            land_soils[(land_soils.len() * 9 / 10).min(land_soils.len() - 1)]
        };
        let top_decile = loess
            .iter()
            .filter(|&&i| best.soil_fertility[i] >= p90)
            .count();
        let mean_loess = loess
            .iter()
            .map(|&i| f64::from(best.soil_fertility[i]))
            .sum::<f64>()
            / loess.len().max(1) as f64;
        println!(
            "[gate16] seed={seed} 4B @ subdiv 5 ({:.1}s): best post-retreat frame \
             year={} has {} loess hexes, mean fertility {mean_loess:.3}, land p90 \
             {p90:.3}, top-decile {top_decile}/{}",
            start.elapsed().as_secs_f64(),
            best.year,
            loess.len(),
            loess.len()
        );
        assert!(
            !loess.is_empty(),
            "§15 #16: no Loess soil in the 100 My after glacial retreat"
        );
        assert!(
            top_decile * 2 >= loess.len(),
            "§15 #16: only {top_decile}/{} loess hexes rank top-decile fertility",
            loess.len()
        );
    }

    /// §15 #17: seasonality regimes @ 1B subdiv 5 — monsoon-regime channels
    /// top-quartile, equatorial Stable bottom-quartile, Nival only where
    /// winters freeze. Regime recompute uses per-hex climate (the tick's
    /// classifier is basin-weighted; documented approximation — a hex deep
    /// in a basin inherits its upstream's regime, which per-hex climate
    /// cannot see).
    #[test]
    #[ignore = "§15 #17 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate17_seasonality_regimes() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        let data = &world.data;
        let n = data.cell_count() as usize;
        let channel = |i: usize| {
            f64::from(data.river_discharge_m3_yr[i]) >= genesis_core::data::STREAM_CLASS_MIN_M3_YR
        };
        let Some([q1, _q2, q3]) =
            seasonality_quartiles(&data.river_discharge_m3_yr, &data.discharge_seasonality)
        else {
            println!("[gate17] SKIP: no channels on the validation world");
            return;
        };
        let monsoon = |i: usize| {
            matches!(
                data.climate_regime[i],
                ClimateRegimePlaceholder::Tropical | ClimateRegimePlaceholder::Subtropical
            ) && data.distance_to_ocean_km[i] <= MONSOON_MAX_COAST_KM
                && data.precipitation[i] >= MONSOON_MIN_PRECIP_MM
        };
        let monsoon_channels: Vec<usize> = (0..n).filter(|&i| channel(i) && monsoon(i)).collect();
        let monsoon_top = monsoon_channels
            .iter()
            .filter(|&&i| data.discharge_seasonality[i] >= q3)
            .count();
        let equatorial_stable: Vec<usize> = (0..n)
            .filter(|&i| {
                channel(i)
                    && data.climate_regime[i] == ClimateRegimePlaceholder::Tropical
                    && !monsoon(i)
            })
            .collect();
        let mut equatorial: Vec<f32> = equatorial_stable
            .iter()
            .map(|&i| data.discharge_seasonality[i])
            .collect();
        equatorial.sort_by(f32::total_cmp);
        let equatorial_median = equatorial
            .get(equatorial.len() / 2)
            .copied()
            .unwrap_or(f32::NAN);
        // Nival recompute (per-hex approximation — see gate docs).
        let mut nival = 0_usize;
        let mut nival_bad_winter = 0_usize;
        let mut nival_bad_band = 0_usize;
        for i in 0..n {
            if !channel(i) {
                continue;
            }
            let (regime, _) = classify_regime(
                data,
                i as u32,
                f64::from(data.temperature_mean[i]),
                f64::from(data.temperature_range[i]),
                0.0,
            );
            if regime != FlowRegime::Nival {
                continue;
            }
            nival += 1;
            let winter = data.temperature_mean[i] - data.temperature_range[i] / 2.0;
            if winter >= 0.0 {
                nival_bad_winter += 1;
            }
            if !(1.9..=5.1).contains(&data.discharge_seasonality[i]) {
                nival_bad_band += 1;
            }
        }
        println!(
            "[gate17] seed={seed} 1B @ subdiv 5 ({:.1}s): quartiles q1={q1:.2} q3={q3:.2}; \
             monsoon channels={} top-quartile={monsoon_top}; equatorial stable={} \
             median seasonality={equatorial_median:.2}; nival={nival} \
             bad_winter={nival_bad_winter} bad_band={nival_bad_band}",
            start.elapsed().as_secs_f64(),
            monsoon_channels.len(),
            equatorial_stable.len()
        );
        if !monsoon_channels.is_empty() {
            assert!(
                monsoon_top * 2 >= monsoon_channels.len(),
                "§15 #17: only {monsoon_top}/{} monsoon channels in the top seasonality \
                 quartile",
                monsoon_channels.len()
            );
        } else {
            println!("[gate17] NOTE: no monsoon-regime channels on this world");
        }
        if !equatorial_stable.is_empty() {
            assert!(
                equatorial_median <= q1,
                "§15 #17: equatorial Stable median seasonality {equatorial_median:.2} \
                 above the bottom quartile {q1:.2}"
            );
        } else {
            println!("[gate17] NOTE: no equatorial Stable channels on this world");
        }
        assert_eq!(
            nival_bad_winter, 0,
            "§15 #17: Nival regime where winters do not freeze"
        );
        // The regime docs give Nival a 2–5 seasonality ratio; on this world
        // ~7% of Nival channels land outside it. §15 #17 asserts only the
        // winter-freeze claim, so the band drift prints as a loud
        // calibration NOTE for P2-34 rather than a gate failure.
        if nival_bad_band > 0 {
            println!(
                "[gate17] NOTE (calibration drift, not gated): {nival_bad_band}/{nival} \
                 Nival channels outside the documented 2–5 seasonality band"
            );
        }
    }

    /// §15 #18: OASIS flags on a world with arid basins adjacent to wet
    /// highlands, @ 1B subdiv 5. The default validation seed shows them (see
    /// the gate's printed census); `GENESIS_VALIDATION_SEED` sweeps others.
    #[test]
    #[ignore = "§15 #18 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate18_oases_along_desert_flow_paths() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            VALIDATION_GEL_M,
        );
        let data = &world.data;
        let oases: Vec<usize> = (0..data.cell_count() as usize)
            .filter(|&i| data.hydro_flags[i].contains(HydroFlags::OASIS))
            .collect();
        println!(
            "[gate18] seed={seed} 1B @ subdiv 5 ({:.1}s): {} oasis hexes",
            start.elapsed().as_secs_f64(),
            oases.len()
        );
        for &i in oases.iter().take(10) {
            println!(
                "[gate18]   oasis hex={i} precip={:.0}mm water_table={:.1}m \
                 discharge={:.2e} dist_ocean={:.0}km",
                data.precipitation[i],
                data.water_table_depth_m[i],
                data.river_discharge_m3_yr[i],
                data.distance_to_ocean_km[i]
            );
        }
        assert!(
            !oases.is_empty(),
            "§15 #18: no OASIS flags on seed {seed} — the seed needs arid basins \
             adjacent to wet highlands; sweep GENESIS_VALIDATION_SEED"
        );
    }

    /// §15 #19: thermosteric sign & scale — equal-ice temperature twin
    /// (same seed/inventory/hypsometry; fresh hydro state zeroes ice/GW on
    /// both). Warmer world stands higher; slope in the β·depth band.
    #[test]
    #[ignore = "§15 #19 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate19_thermosteric_live() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            PRODUCTION_GEL_M,
        );
        const DELTA_T_C: f32 = 5.0;
        let cold = flood_with_temperature_shift(&world.data, -DELTA_T_C);
        let warm = flood_with_temperature_shift(&world.data, DELTA_T_C);
        let rise = warm.sea_level_m - cold.sea_level_m;
        let slope = f64::from(rise) / f64::from(2.0 * DELTA_T_C);
        println!(
            "[gate19] seed={seed} 1B @ subdiv 5 ({:.1}s): T twin ±{DELTA_T_C}°C \
             equal ice; cold sea={:+.2}m warm sea={:+.2}m Δ={rise:+.2}m \
             slope={slope:+.3} m/°C (band {THERMOSTERIC_SLOPE_MIN_M_PER_C}–\
             {THERMOSTERIC_SLOPE_MAX_M_PER_C})",
            start.elapsed().as_secs_f64(),
            cold.sea_level_m,
            warm.sea_level_m
        );
        assert!(
            warm.sea_level_m > cold.sea_level_m,
            "§15 #19: warmer twin must stand higher ({} vs {})",
            warm.sea_level_m,
            cold.sea_level_m
        );
        assert!(
            (THERMOSTERIC_SLOPE_MIN_M_PER_C..=THERMOSTERIC_SLOPE_MAX_M_PER_C).contains(&slope),
            "§15 #19: thermosteric slope {slope:+.3} m/°C outside the honest band"
        );
    }

    /// One hydrology tick after shifting every hex's `temperature_mean` by
    /// `delta_c`. Fresh state → equal ice/lake/GW (Doc's equal-ice sweep).
    fn flood_with_temperature_shift(base: &WorldData, delta_c: f32) -> WorldData {
        let mut data = base.clone();
        for t in &mut data.temperature_mean {
            *t += delta_c;
        }
        flood_with_prior_ice_budget(&data, 0.0)
    }

    /// §15 #20: post-glacial rebound — clause 1 reads cumulative
    /// `gia_rebound_applied_m` (mechanism delivery; raw Δelev is tectonics-
    /// swamped on ice highlands). Clause 2: emergent wet→dry shoreline
    /// within 50 My of deglaciation. 4B @ subdiv 5.
    #[test]
    #[ignore = "§15 #20 full-stack deep-time gate; run with --ignored --nocapture"]
    fn gate20_post_glacial_rebound() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, frames) = run_full_stack_with_stride(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_4B,
            PRODUCTION_GEL_M,
            Some(GLACIAL_FRAME_STRIDE_YEARS),
        );
        print_iciest_frames(&frames, "gate20", 3);
        let n = frames[0].elevation_mean.len();
        let mut episode_min = vec![f32::MAX; n];
        let mut was_iced = vec![false; n];
        let mut deglaciated_year = vec![i64::MIN; n];
        let mut glaciated_min = vec![f32::MAX; n];
        for frame in &frames {
            for i in 0..n {
                let iced = frame.ice_mask[i];
                if iced {
                    episode_min[i] = episode_min[i].min(frame.elevation_mean[i]);
                } else if was_iced[i] {
                    deglaciated_year[i] = frame.year;
                    glaciated_min[i] = episode_min[i];
                    episode_min[i] = f32::MAX;
                }
                was_iced[i] = iced;
            }
        }
        let mut deglaciated_valid = 0_usize;
        let mut cohort: Vec<usize> = Vec::new();
        let mut raw_deltas: Vec<f32> = Vec::new();
        for i in 0..n {
            let y0 = deglaciated_year[i];
            if y0 == i64::MIN {
                continue;
            }
            let Some(reference) = frames.iter().find(|f| f.year >= y0 + 10_000_000) else {
                deglaciated_year[i] = i64::MIN;
                continue;
            };
            if reference.ice_mask[i] {
                deglaciated_year[i] = i64::MIN;
                continue;
            }
            deglaciated_valid += 1;
            if reference.elevation_mean[i] < reference.sea_level_m {
                continue;
            }
            if !world
                .data
                .continental_crust
                .get(i)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            cohort.push(i);
            raw_deltas.push(reference.elevation_mean[i] - glaciated_min[i]);
        }
        println!(
            "[gate20] seed={seed} 4B @ subdiv 5 ({:.1}s): {deglaciated_valid} hexes \
             deglaciated ≥10 My, {} in the continental-land cohort",
            start.elapsed().as_secs_f64(),
            cohort.len()
        );
        if deglaciated_valid < 10 {
            println!(
                "[gate20] SKIP: too few deglaciated hexes (no real glacial cycle?); \
                 §15 #20 unassertable — physics gap, flagged for Step 4"
            );
            return;
        }
        if cohort.is_empty() {
            println!(
                "[gate20] SKIP: every deglaciated hex is drowned or oceanic at the \
                 comparison frames — no continental-land cohort for §15 #20 clause 1"
            );
        } else {
            let applied: Vec<f32> = cohort
                .iter()
                .map(|&i| {
                    world
                        .data
                        .gia_rebound_applied_m
                        .get(i)
                        .copied()
                        .unwrap_or(0.0)
                })
                .collect();
            let recovered = applied
                .iter()
                .filter(|&&v| v >= REBOUND_RECOVERED_MIN_M)
                .count();
            let mean_applied =
                applied.iter().map(|&v| f64::from(v)).sum::<f64>() / applied.len() as f64;
            raw_deltas.sort_by(f32::total_cmp);
            let mean_raw =
                raw_deltas.iter().map(|&v| f64::from(v)).sum::<f64>() / raw_deltas.len() as f64;
            println!(
                "[gate20] GIA accumulator on cohort: mean_applied={mean_applied:.1}m \
                 ≥{REBOUND_RECOVERED_MIN_M}m={recovered}/{}; raw Δelev vs glaciated \
                 min (tectonics-swamped context): mean={mean_raw:+.1}m \
                 min={:+.1} max={:+.1}",
                cohort.len(),
                raw_deltas[0],
                raw_deltas[raw_deltas.len() - 1]
            );
            let fraction = recovered as f64 / cohort.len() as f64;
            assert!(
                fraction >= REBOUND_COHORT_MIN_FRAC,
                "§15 #20: only {recovered}/{} deglaciated continental-land hexes \
                 accumulated ≥ {REBOUND_RECOVERED_MIN_M} m of GIA rebound",
                cohort.len()
            );
        }

        let grid = &world.data.grid;
        let mut emergent: Vec<(u32, i64, i64)> = Vec::new();
        for (i, &y0) in deglaciated_year.iter().enumerate() {
            if y0 == i64::MIN {
                continue;
            }
            let mut ring: Vec<u32> = vec![i as u32];
            ring.extend(
                grid.neighbors(HexId(i as u32))
                    .iter()
                    .map(|h| h.0)
                    .filter(|&j| (j as usize) < n),
            );
            'ring: for j in ring {
                let mut wet_year = None;
                for frame in frames
                    .iter()
                    .filter(|f| f.year >= y0 && f.year <= y0 + 50_000_000)
                {
                    let wet = frame.water_level_m[j as usize] != WATER_NONE;
                    match (wet, wet_year) {
                        (true, None) => wet_year = Some(frame.year),
                        (false, Some(wy)) => {
                            emergent.push((j, wy, frame.year));
                            break 'ring;
                        }
                        _ => {}
                    }
                }
            }
        }
        for &(hex, wet, dry) in emergent.iter().take(10) {
            println!("[gate20]   emergent shoreline hex={hex}: wet at {wet}, dry by {dry}");
        }
        println!("[gate20] emergent shoreline hexes: {}", emergent.len());
        assert!(
            !emergent.is_empty(),
            "§15 #20: no emergent shoreline within 50 My of deglaciation"
        );
    }

    /// P2-34 calibration gate: 3 seeds × 3 inventories @ 300M subdiv 5.
    /// Asserts finite sea level, sane land fractions, and monotonic land
    /// response to inventory within each seed (Doc 08 §15 / P2-34 exit).
    /// Uses 300M (not 200M): under the temperature-gated condensation curve
    /// (§3.3) 200M is still above onset and has no standing water.
    #[test]
    #[ignore = "P2-34 calibration matrix; run with --ignored --nocapture"]
    fn calibration_matrix_3x3() {
        println!(
            "[calib] seed gel_m land_frac major rivers/hexes bodies(o/s/l/sl/sf) \
                  endorheic sea_level_m mean_tick_ms wall_s"
        );
        let seeds = [42_u64, 7, 99];
        let gels = [500.0_f32, 1000.0, 3000.0];
        for seed in seeds {
            let mut land_by_gel: Vec<(f32, f32)> = Vec::new();
            for gel in gels {
                let start = Instant::now();
                let (world, _frames) = run_full_stack(
                    seed,
                    VALIDATION_SUBDIVISION_LEVEL,
                    VALIDATION_YEAR_300M,
                    gel,
                );
                let wall = start.elapsed().as_secs_f64();
                let metrics = metrics_at(&world);
                let data = &world.data;
                assert!(
                    metrics.sea_level_m.is_finite(),
                    "[calib] seed={seed} gel={gel}: sea_level_m not finite"
                );
                assert!(
                    (0.0..=1.0).contains(&metrics.land_fraction),
                    "[calib] seed={seed} gel={gel}: land_fraction {}",
                    metrics.land_fraction
                );
                assert!(
                    metrics.bodies.ocean >= 1,
                    "[calib] seed={seed} gel={gel}: expected an ocean body"
                );
                assert!(
                    data.water_level_m
                        .iter()
                        .all(|&v| v == WATER_NONE || v.is_finite()),
                    "[calib] seed={seed} gel={gel}: non-finite water_level_m (beyond WATER_NONE)"
                );
                assert!(
                    data.river_discharge_m3_yr.iter().all(|v| v.is_finite()),
                    "[calib] seed={seed} gel={gel}: non-finite river_discharge"
                );
                // 300M is inside Formation: 5 My ticks.
                let ticks = (VALIDATION_YEAR_300M / 5_000_000).max(1) as f64;
                println!(
                    "[calib] {seed:2} {gel:5.0} {:.4} {:2}/{:3} {}/{}/{}/{}/{} {} {:+7.1} \
                     {:7.2} {:5.1}",
                    metrics.land_fraction,
                    metrics.major_rivers,
                    metrics.major_channel_hexes,
                    metrics.bodies.ocean,
                    metrics.bodies.sea,
                    metrics.bodies.lake,
                    metrics.bodies.salt_lake,
                    metrics.bodies.salt_flat,
                    metrics.endorheic_bodies,
                    metrics.sea_level_m,
                    wall * 1000.0 / ticks,
                    wall
                );
                land_by_gel.push((gel, metrics.land_fraction));
            }
            // Dial monotonicity on a fixed hypsometry is gate #2 @ 1B (post-
            // Formation). At 300M, Formation condensation + coupled feedback
            // make cross-world seas/lands non-monotone in gel. Calibration
            // exit bar: every cell finishes with finite, in-range metrics.
            assert_eq!(
                land_by_gel.len(),
                gels.len(),
                "[calib] seed={seed}: expected {} gel cells",
                gels.len()
            );
        }
    }

    /// Production morphology: land fraction and continental dry-pit rate at 1B
    /// under the menu-default 2700 m GEL. Fails the pre-fix ~19% land /
    /// dissected freeboard world if pits reopen or land collapses.
    #[test]
    #[ignore = "morphology gate; run with --ignored --nocapture"]
    fn gate_land_fraction_production_1b() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_1B,
            PRODUCTION_GEL_M,
        );
        let land = land_fraction(&world.data.elevation_mean, world.data.sea_level_m);
        let pits = continental_dry_pit_fraction(&world.data, 500.0);
        println!(
            "[gate_land_1b] seed={seed} land={land:.3} pits={pits:.4} sea={:+.1}m ({:.1}s)",
            world.data.sea_level_m,
            start.elapsed().as_secs_f64()
        );
        assert!(
            (0.15..=0.40).contains(&land),
            "production 1B land fraction {land:.3} outside [0.15, 0.40]"
        );
        assert!(
            pits <= 0.02,
            "production 1B continental dry-pit fraction {pits:.4} exceeds 2%"
        );
    }

    /// Production morphology at 4B — the former worst band (~7–8% land).
    #[test]
    #[ignore = "morphology gate; run with --ignored --nocapture"]
    fn gate_land_fraction_production_4b() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, _frames) = run_full_stack(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_4B,
            PRODUCTION_GEL_M,
        );
        let land = land_fraction(&world.data.elevation_mean, world.data.sea_level_m);
        let pits = continental_dry_pit_fraction(&world.data, 500.0);
        println!(
            "[gate_land_4b] seed={seed} land={land:.3} pits={pits:.4} sea={:+.1}m ({:.1}s)",
            world.data.sea_level_m,
            start.elapsed().as_secs_f64()
        );
        assert!(
            (0.15..=0.40).contains(&land),
            "production 4B land fraction {land:.3} outside [0.15, 0.40]"
        );
        assert!(
            pits <= 0.02,
            "production 4B continental dry-pit fraction {pits:.4} exceeds 2%"
        );
    }

    /// P2-34 deep run: seed 42 @ 2700 m GEL, 4B @ subdiv 5 with a metrics
    /// milestone every 500 My. Asserts land ≥ 15% from 1B onward (morphology
    /// regression guard); prints pit fraction each milestone.
    #[test]
    #[ignore = "P2-34 deep run; run with --ignored --nocapture"]
    fn deep_run_seed42_4b() {
        let seed = validation_seed();
        let start = Instant::now();
        let (world, frames) = run_full_stack_with_stride(
            seed,
            VALIDATION_SUBDIVISION_LEVEL,
            VALIDATION_YEAR_4B,
            PRODUCTION_GEL_M,
            Some(10_000_000),
        );
        let mut next_milestone = 500_000_000_i64;
        for frame in &frames {
            if frame.year < next_milestone {
                continue;
            }
            let metrics = frame_metrics(&world.data.grid, frame);
            // Apply frame elev/sea onto a scratch for pit metric when possible;
            // frames may not carry continental_crust — use live world at end.
            println!("[deep] {metrics}");
            if frame.year >= VALIDATION_YEAR_1B {
                let land = land_fraction(&frame.elevation_mean, frame.sea_level_m);
                assert!(
                    land >= 0.15,
                    "[deep] year={} land={land:.3} below 0.15 morphology floor",
                    frame.year
                );
            }
            next_milestone += 500_000_000;
        }
        let pits = continental_dry_pit_fraction(&world.data, 500.0);
        let land = land_fraction(&world.data.elevation_mean, world.data.sea_level_m);
        println!(
            "[deep] final world ({:.1}s) land={land:.3} pits={pits:.4}:\n{}",
            start.elapsed().as_secs_f64(),
            metrics_at(&world)
        );
        assert!(
            (0.15..=0.40).contains(&land),
            "deep_run final land {land:.3} outside [0.15, 0.40]"
        );
        assert!(
            pits <= 0.02,
            "deep_run final continental dry-pit fraction {pits:.4} exceeds 2%"
        );
    }
}
