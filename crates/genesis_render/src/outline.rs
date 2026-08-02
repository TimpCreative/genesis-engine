//! Selected-hex outline overlay on the equirectangular map.

use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use genesis_core::HexId;
use glam::DVec3;

use crate::polygon::hex_polygon_vertices;
use crate::projection::MapProjection;
use crate::resources::{CameraState, CurrentProjection, WorldResource};
use crate::systems::view_center;

/// Currently selected map hex (shared by UI inspector and outline).
#[derive(Resource, Default, Clone, Copy)]
pub struct SelectedHex(pub Option<HexId>);

#[derive(Component)]
pub(crate) struct SelectionOutline;

/// Rebuilds the bright outline ring when the selection, world, or (on the globe)
/// the view rotation changes.
pub(crate) fn sync_selection_outline(
    mut commands: Commands,
    selected: Res<SelectedHex>,
    projection_mode: Res<CurrentProjection>,
    camera: Res<CameraState>,
    world_res: Option<Res<WorldResource>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    existing: Query<Entity, With<SelectionOutline>>,
) {
    let projection = projection_mode.0;
    // The globe's outline is projection-space, so it must follow rotation.
    let globe_rotated = projection == MapProjection::Orthographic && camera.is_changed();
    if !selected.is_changed()
        && !projection_mode.is_changed()
        && !globe_rotated
        && !world_res.as_ref().is_some_and(|w| w.is_changed())
        && selected.0.is_some()
    {
        return;
    }

    for e in &existing {
        commands.entity(e).despawn();
    }

    let Some(hex) = selected.0 else {
        return;
    };
    let Some(world_res) = world_res else {
        return;
    };
    let data = &world_res.0.data;
    let grid = &data.grid;
    if hex.0 as usize >= data.cell_count() as usize {
        return;
    }
    let center_dir = DVec3::from(grid.cell_center_direction(hex));
    let view = view_center(&camera);
    if !projection.hex_visible(center_dir, view) {
        return; // polar-skipped (flat) or on the far hemisphere (globe)
    }

    let vertices = hex_polygon_vertices(grid, hex);
    let ring: Vec<[f32; 3]> = vertices
        .iter()
        .map(|v| {
            let (x, y) = projection.project(*v, center_dir, view);
            [x, y, 1.0]
        })
        .collect();
    if ring.len() < 3 {
        return;
    }

    // Thin triangle strip around the ring (inward offset toward center).
    let (cx, cy) = projection.project(center_dir, center_dir, view);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(ring.len() * 2);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(ring.len() * 2);
    let outline = [1.0_f32, 0.95, 0.35, 1.0];
    let inset = 0.92_f32;
    for p in &ring {
        positions.push(*p);
        colors.push(outline);
        positions.push([cx + (p[0] - cx) * inset, cy + (p[1] - cy) * inset, 1.0]);
        colors.push(outline);
    }
    let n = ring.len() as u32;
    let mut indices: Vec<u32> = Vec::with_capacity((n as usize) * 6);
    for i in 0..n {
        let a = i * 2;
        let b = i * 2 + 1;
        let c = ((i + 1) % n) * 2;
        let d = ((i + 1) % n) * 2 + 1;
        indices.extend_from_slice(&[a, c, b, b, c, d]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    let handle = meshes.add(mesh);
    let material = materials.add(ColorMaterial::default());
    commands.spawn((
        SelectionOutline,
        Mesh2d(handle),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, 1.0),
    ));
}
