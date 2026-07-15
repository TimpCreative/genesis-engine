//! River and lake overlay for [`RenderMode::Rivers`] (pre-Doc-08 provisional).
//!
//! Rivers are drawn as thin polylines along the downstream flow path — never
//! hex fills, which would read as 100+ km wide water. Hydrological honesty
//! within current Phase 2/3 scope: a river SEGMENT exists only where enough
//! discharge has accumulated (sources never appear from nothing), it always
//! runs from a hex center to its downstream neighbor's center, and where flow
//! cannot continue downhill (endorheic sinks) a lake disc pools instead.
//! Real lake filling/spill and groundwater are future Doc 08 scope.

use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::projection::{project, should_skip_for_equirectangular};
use crate::render_mode::{CurrentRenderMode, RenderMode};
use crate::resources::{ColorsDirty, WorldResource};

/// Discharge multiple of the mean local runoff above which a river is drawn:
/// a visible river needs a drainage basin of at least ~4 upstream hexes.
pub const RIVER_SOURCE_FLOW_MULTIPLE: f64 = 4.0;

/// River line width as a fraction of hex spacing at minimum discharge.
const RIVER_MIN_WIDTH_FRAC: f32 = 0.18;

/// River line width fraction at 100x the source threshold.
const RIVER_MAX_WIDTH_FRAC: f32 = 0.50;

/// Lake disc radius as a fraction of hex spacing.
const LAKE_RADIUS_FRAC: f32 = 0.55;

const WATER_COLOR: Color = Color::srgb(0.10, 0.35, 0.80);

/// Overlay entity for the active river layer (despawned on mode switch).
#[derive(Resource, Default)]
pub struct RiverOverlay {
    pub entity: Option<Entity>,
}

/// Rebuilds the river overlay whenever the Rivers mode is active and either
/// the mode just changed or the displayed frame changed (`ColorsDirty` is
/// still set — this system runs BEFORE `update_hex_colors` consumes it).
#[allow(clippy::too_many_arguments)]
pub fn update_river_overlay(
    mut commands: Commands,
    world_res: Option<Res<WorldResource>>,
    render_mode: Res<CurrentRenderMode>,
    colors_dirty: Res<ColorsDirty>,
    mut overlay: ResMut<RiverOverlay>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let is_rivers = render_mode.0 == RenderMode::Rivers;
    let needs_refresh = render_mode.is_changed() || colors_dirty.0;

    if (!is_rivers || needs_refresh)
        && let Some(entity) = overlay.entity.take()
    {
        commands.entity(entity).despawn();
    }
    if !is_rivers || !needs_refresh {
        return;
    }
    let Some(world_res) = world_res else {
        return;
    };
    let data = &world_res.0.data;
    let grid = &data.grid;
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;

    // Source threshold: rivers appear only where discharge has accumulated
    // well beyond one hex's LOCAL runoff — i.e. a real upstream basin drains
    // through here. Local runoff is precipitation × hex area × the runoff
    // coefficient (mirrors genesis_hydrology::RUNOFF_COEFFICIENT); using mean
    // ACCUMULATED flow instead would let trunk rivers dominate the average and
    // prune the whole network.
    const RUNOFF_COEFFICIENT: f64 = 0.4;
    let radius_m = grid.planet_radius_km() * 1000.0;
    let hex_area_m2 = 4.0 * std::f64::consts::PI * radius_m * radius_m / n as f64;
    let mut precip_sum = 0.0_f64;
    let mut count = 0_u64;
    for i in 0..n {
        if data.elevation_mean[i] >= sea {
            precip_sum += f64::from(data.precipitation[i]);
            count += 1;
        }
    }
    if count == 0 {
        return;
    }
    let mean_precip_mm = (precip_sum / count as f64).max(1.0);
    let mean_local_runoff = mean_precip_mm * hex_area_m2 * 1e-3 * RUNOFF_COEFFICIENT;
    let threshold = (mean_local_runoff * RIVER_SOURCE_FLOW_MULTIPLE) as f32;
    if threshold <= 0.0 {
        return;
    }

    // Approximate hex spacing in projected radians for width/radius scale.
    let spacing = (4.0 * std::f64::consts::PI / n as f64).sqrt() as f32;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let push_quad =
        |a: Vec2, b: Vec2, width: f32, positions: &mut Vec<[f32; 3]>, indices: &mut Vec<u32>| {
            let dir = b - a;
            if dir.length_squared() < 1e-12 {
                return;
            }
            let normal = Vec2::new(-dir.y, dir.x).normalize() * (width * 0.5);
            let base = positions.len() as u32;
            for p in [a - normal, a + normal, b + normal, b - normal] {
                positions.push([p.x, p.y, 0.0]);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        };

    for i in 0..n {
        if data.elevation_mean[i] < sea || data.flow_volume[i] < threshold {
            continue;
        }
        let hex = genesis_core::HexId(i as u32);
        let (lat, lon) = grid.center_lat_lon(hex);
        if should_skip_for_equirectangular(lat) {
            continue;
        }
        let here = project(lat, lon);
        let here = Vec2::new(here.0, here.1);

        // Width grows with discharge (log scale over two decades).
        let decades = (data.flow_volume[i] / threshold).max(1.0).log10().min(2.0);
        let width = spacing
            * (RIVER_MIN_WIDTH_FRAC
                + (RIVER_MAX_WIDTH_FRAC - RIVER_MIN_WIDTH_FRAC) * decades / 2.0);

        match data.flow_direction[i] {
            Some(direction) => {
                let neighbors = grid.neighbors(hex);
                let Some(&downstream) = neighbors.get(direction.index()) else {
                    continue;
                };
                let (nlat, nlon) = grid.center_lat_lon(downstream);
                if should_skip_for_equirectangular(nlat) {
                    continue;
                }
                // Skip antimeridian-crossing segments rather than tearing.
                if (nlon - lon).abs() > std::f64::consts::PI {
                    continue;
                }
                let there = project(nlat, nlon);
                push_quad(
                    here,
                    Vec2::new(there.0, there.1),
                    width,
                    &mut positions,
                    &mut indices,
                );
            }
            None => {
                // Endorheic sink: water pools where it cannot flow downhill.
                let radius = spacing * LAKE_RADIUS_FRAC;
                let base = positions.len() as u32;
                positions.push([here.x, here.y, 0.0]);
                const SEGS: u32 = 10;
                for s in 0..SEGS {
                    let angle = s as f32 / SEGS as f32 * std::f32::consts::TAU;
                    positions.push([
                        here.x + radius * angle.cos(),
                        here.y + radius * angle.sin(),
                        0.0,
                    ]);
                }
                for s in 0..SEGS {
                    indices.extend_from_slice(&[base, base + 1 + s, base + 1 + (s + 1) % SEGS]);
                }
            }
        }
    }

    if positions.is_empty() {
        return;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    let entity = commands
        .spawn((
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(materials.add(ColorMaterial::from_color(WATER_COLOR))),
            // Above the hex chunks (z 0), below the camera.
            Transform::from_xyz(0.0, 0.0, 1.0),
        ))
        .id();
    overlay.entity = Some(entity);
}
