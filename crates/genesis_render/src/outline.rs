//! Selected-hex outline overlay on the equirectangular map.

use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use genesis_core::HexId;

use crate::polygon::{direction_to_lat_lon, hex_polygon_vertices, unwrap_lon_relative};
use crate::projection::{project, should_skip_for_equirectangular};
use crate::resources::WorldResource;

/// Currently selected map hex (shared by UI inspector and outline).
#[derive(Resource, Default, Clone, Copy)]
pub struct SelectedHex(pub Option<HexId>);

#[derive(Component)]
pub(crate) struct SelectionOutline;

/// Rebuilds the bright outline ring when [`SelectedHex`] changes.
pub(crate) fn sync_selection_outline(
    mut commands: Commands,
    selected: Res<SelectedHex>,
    world_res: Option<Res<WorldResource>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    existing: Query<Entity, With<SelectionOutline>>,
) {
    if !selected.is_changed()
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
    let (center_lat, center_lon) = grid.center_lat_lon(hex);
    if should_skip_for_equirectangular(center_lat) {
        return;
    }

    let vertices = hex_polygon_vertices(grid, hex);
    let ring: Vec<[f32; 3]> = vertices
        .iter()
        .map(|v| {
            let (lat, lon) = direction_to_lat_lon(*v);
            let unwrapped = unwrap_lon_relative(lon, center_lon);
            let (x, y) = project(lat, unwrapped);
            [x, y, 1.0]
        })
        .collect();
    if ring.len() < 3 {
        return;
    }

    // Thin triangle strip around the ring (inward offset toward center).
    let (cx, cy) = project(center_lat, center_lon);
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
