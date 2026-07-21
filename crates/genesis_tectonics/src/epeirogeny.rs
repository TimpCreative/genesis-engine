//! Epeirogenic basins and swells (Doc 06 §5.12): deterministic, long-wavelength
//! undulation of the isostatic freeboard target.
//!
//! Earth's continental interiors are not flat at any wavelength: mantle
//! dynamic topography holds cratons in ±100–300 m basins and swells at
//! 1000+ km wavelengths (the Michigan and Williston basins, the Ozark and
//! Cincinnati arches). Without an equivalent, every interior process in this
//! crate (freeboard erosion from above, heal and epeirogenic rebound from
//! below) pinches the structure field onto the single scalar
//! `sea + CONTINENTAL_FREEBOARD_M + root`, the calibration transfer receives
//! continent-sized rank-tied cohorts, and its mid-band maps them into vast
//! uniform plains (the measured worst case: one connected sub-200 m-relief
//! plain covering 20% of world land — Earth's largest is ~2%).
//!
//! The swell field is a pure function of `(world seed, plate id, birth-frame
//! position)`: a sum of signed spherical Gaussian bumps. Folding it into the
//! attractor *target* (not the elevation directly) means every existing
//! convergence process now preserves the undulation instead of erasing it,
//! and because it is evaluated in the birth frame it advects rigidly with
//! the plate at zero storage cost. Deterministic: splitmix64 on the seed
//! tuple; no RNG streams, no state.

use genesis_core::{HexGrid, HexId, PlateId};

use crate::plate::PlateRegistry;
use crate::plate_surface::birth_hex_at;
use crate::projection::ProjectionCache;
use genesis_core::data::WorldData;

/// Signed Gaussian bumps per plate. Eight gives 3–5 visible basins/swells on
/// a major plate — the cratonic-province scale.
pub const SWELL_BUMPS_PER_PLATE: u64 = 8;
/// Bump angular half-width, radians (~12° ≈ 1300 km on an Earth-radius
/// sphere — dynamic-topography wavelength).
pub const SWELL_SIGMA_RAD: f64 = 0.21;
/// Single-bump amplitude, m.
pub const SWELL_AMPLITUDE_M: f64 = 220.0;
/// Clamp on the summed field, m (Earth dynamic topography stays within
/// roughly ±300 m on cratons).
pub const SWELL_CLAMP_M: f32 = 300.0;

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The swell (m) at a birth-frame unit direction for one plate. Pure and
/// deterministic in `(world_seed, plate_id, direction)`.
pub fn epeirogenic_swell_m(world_seed: u64, plate_id: PlateId, direction: [f64; 3]) -> f32 {
    let base = splitmix64(world_seed ^ (u64::from(plate_id.0) << 32));
    let mut total = 0.0_f64;
    for bump in 0..SWELL_BUMPS_PER_PLATE {
        let h1 = splitmix64(base ^ bump);
        let h2 = splitmix64(h1);
        // Uniform point on the sphere: z ∈ [−1, 1], φ ∈ [0, τ).
        let z = (h1 >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
        let phi = (h2 >> 11) as f64 / (1u64 << 53) as f64 * std::f64::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let center = [r * phi.cos(), r * phi.sin(), z];
        let dot = (center[0] * direction[0] + center[1] * direction[1] + center[2] * direction[2])
            .clamp(-1.0, 1.0);
        let angle = dot.acos();
        let sign = if h1 & 1 == 0 { 1.0 } else { -1.0 };
        total += sign * SWELL_AMPLITUDE_M * (-(angle / SWELL_SIGMA_RAD).powi(2)).exp();
    }
    (total as f32).clamp(-SWELL_CLAMP_M, SWELL_CLAMP_M)
}

/// Swell at a plate's birth hex.
pub fn swell_at_birth(grid: &HexGrid, world_seed: u64, plate_id: PlateId, birth: HexId) -> f32 {
    epeirogenic_swell_m(world_seed, plate_id, grid.cell_center_direction(birth))
}

/// Dissection-texture wavelength, radians (~400 km on an Earth-radius
/// sphere): the hilly-province scale — dissected plateaus, basement highs,
/// cuesta belts. Physical, so texture is resolution-independent.
pub const TEXTURE_OMEGA_RAD: f64 = 100.0;
/// Peak dissection amplitude, m, reached only where the ruggedness mask
/// saturates. Uplands on Earth carry 100–400 m of hex-scale (~90 km) local
/// relief; smooth provinces (the Steppe, the Great Plains) carry ~none.
pub const TEXTURE_AMPLITUDE_M: f64 = 140.0;
/// Clamp on the combined swell + texture target offset, m.
pub const TARGET_OFFSET_CLAMP_M: f32 = 500.0;

/// Hex-scale dissection texture at a birth-frame direction: directional
/// sinusoids at the [`TEXTURE_OMEGA_RAD`] wavelength, modulated by a
/// province-scale ruggedness mask (a second Gaussian-bump field) so roughly
/// half of each plate's interior reads as smooth plains and the rest as
/// dissected upland. Pure and deterministic, like the swell.
pub fn terrain_texture_m(world_seed: u64, plate_id: PlateId, direction: [f64; 3]) -> f32 {
    let base = splitmix64(world_seed ^ (u64::from(plate_id.0) << 32) ^ TEXTURE_HASH_SALT);

    // Province ruggedness mask in [0, 1]: signed bumps at ~2200 km scale.
    let mut mask_sum = 0.0_f64;
    for bump in 0..6u64 {
        let h1 = splitmix64(base ^ (bump << 8) ^ 0xA5);
        let h2 = splitmix64(h1);
        let z = (h1 >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
        let phi = (h2 >> 11) as f64 / (1u64 << 53) as f64 * std::f64::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let center = [r * phi.cos(), r * phi.sin(), z];
        let dot = (center[0] * direction[0] + center[1] * direction[1] + center[2] * direction[2])
            .clamp(-1.0, 1.0);
        let sign = if h1 & 1 == 0 { 1.0 } else { -1.0 };
        mask_sum += sign * (-(dot.acos() / 0.35).powi(2)).exp();
    }
    let mask = (0.5 + mask_sum).clamp(0.0, 1.0);
    if mask <= 0.0 {
        return 0.0;
    }

    // Directional sinusoids: five random axes at the dissection wavelength.
    let mut t = 0.0_f64;
    for comp in 0..5u64 {
        let h1 = splitmix64(base ^ (comp << 16) ^ 0x3C);
        let h2 = splitmix64(h1);
        let h3 = splitmix64(h2);
        let z = (h1 >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0;
        let phi = (h2 >> 11) as f64 / (1u64 << 53) as f64 * std::f64::consts::TAU;
        let r = (1.0 - z * z).max(0.0).sqrt();
        let axis = [r * phi.cos(), r * phi.sin(), z];
        let phase = (h3 >> 11) as f64 / (1u64 << 53) as f64 * std::f64::consts::TAU;
        let projection =
            axis[0] * direction[0] + axis[1] * direction[1] + axis[2] * direction[2];
        t += (TEXTURE_OMEGA_RAD * projection + phase).sin();
    }
    // Five ±1 components: normalize by the ~2.2 rms-to-peak so the mask
    // amplitude is honest.
    (mask * TEXTURE_AMPLITUDE_M * (t / 2.2).clamp(-1.0, 1.0)) as f32
}

/// Salt separating the texture hash domain from the swell's.
const TEXTURE_HASH_SALT: u64 = 0x7ea2_91d4_5bc3_081f;

/// Combined attractor-target offset (m): epeirogenic swell plus dissection
/// texture, clamped. This is what the freeboard-convergence processes fold
/// into their targets (Doc 06 §5.12–5.13).
pub fn target_offset_m(world_seed: u64, plate_id: PlateId, direction: [f64; 3]) -> f32 {
    let total = epeirogenic_swell_m(world_seed, plate_id, direction)
        + terrain_texture_m(world_seed, plate_id, direction);
    total.clamp(-TARGET_OFFSET_CLAMP_M, TARGET_OFFSET_CLAMP_M)
}

/// Target offset at a plate's birth hex.
pub fn target_offset_at_birth(
    grid: &HexGrid,
    world_seed: u64,
    plate_id: PlateId,
    birth: HexId,
) -> f32 {
    target_offset_m(world_seed, plate_id, grid.cell_center_direction(birth))
}

/// Combined offset at a world hex (swell + texture), resolved through plate
/// ownership and the projection cache. Zero for unowned hexes.
pub fn target_offset_at_world(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    world_hex: HexId,
) -> f32 {
    let idx = world_hex.0 as usize;
    if idx >= data.plate_id.len() {
        return 0.0;
    }
    let plate_id = data.plate_id[idx];
    if plate_id == PlateId::NONE {
        return 0.0;
    }
    let Some(plate) = registry.get(plate_id) else {
        return 0.0;
    };
    let birth = birth_hex_at(data, plate, cache, world_hex);
    target_offset_at_birth(
        &data.grid,
        data.parameters.core.seed.value,
        plate_id,
        birth,
    )
}

/// Swell at a world hex, resolved through plate ownership and the projection
/// cache (the [`continental_crust_at`](crate::plate_surface::continental_crust_at)
/// pattern). Zero for unowned hexes.
pub fn swell_at_world(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    world_hex: HexId,
) -> f32 {
    let idx = world_hex.0 as usize;
    if idx >= data.plate_id.len() {
        return 0.0;
    }
    let plate_id = data.plate_id[idx];
    if plate_id == PlateId::NONE {
        return 0.0;
    }
    let Some(plate) = registry.get(plate_id) else {
        return 0.0;
    };
    let birth = birth_hex_at(data, plate, cache, world_hex);
    swell_at_birth(
        &data.grid,
        data.parameters.core.seed.value,
        plate_id,
        birth,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swell_is_deterministic_and_bounded() {
        let d = [0.3, -0.5, 0.812];
        let a = epeirogenic_swell_m(42, PlateId(3), d);
        let b = epeirogenic_swell_m(42, PlateId(3), d);
        assert_eq!(a, b, "pure function of its inputs");
        assert!(a.abs() <= SWELL_CLAMP_M);
        let c = epeirogenic_swell_m(43, PlateId(3), d);
        let e = epeirogenic_swell_m(42, PlateId(4), d);
        assert!(a != c || a != e, "seed and plate id both enter the field");
    }

    #[test]
    fn swell_has_long_wavelength_variation() {
        // Across a plate-sized span the field must actually vary (no dead
        // constant), and nearby points must be correlated (long wavelength).
        let near_a = epeirogenic_swell_m(7, PlateId(1), [1.0, 0.0, 0.0]);
        let near_b = epeirogenic_swell_m(7, PlateId(1), [0.9995, 0.0316, 0.0]);
        assert!(
            (near_a - near_b).abs() < 40.0,
            "points ~200 km apart stay correlated: {near_a} vs {near_b}"
        );
        // Fibonacci-sphere sweep: the field must vary at plate scale
        // somewhere on the globe (a single circle can miss the bumps).
        let mut values = Vec::new();
        let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
        for k in 0..128 {
            let z = 1.0 - 2.0 * (k as f64 + 0.5) / 128.0;
            let r = (1.0 - z * z).sqrt();
            let phi = golden * k as f64;
            values.push(epeirogenic_swell_m(7, PlateId(1), [r * phi.cos(), r * phi.sin(), z]));
        }
        let min = values.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max - min > 100.0,
            "the global field must hold basins and swells: range {}",
            max - min
        );
    }
}
