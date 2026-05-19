//! Neighbor table construction for [`super::HexGrid`].
//!
//! Neighbors are derived entirely from topological [`isea3h::coord_neighbors`], then
//! sorted by bearing on the tangent plane.

use std::collections::BTreeMap;

use crate::grid::error::GridError;
use crate::grid::geography;
use crate::grid::ids::HexId;
use crate::grid::isea3h::{self, Isea3hCoord, Vec3};

pub(crate) fn build_neighbor_table(
    coords: &[Isea3hCoord],
    centers: &[Vec3],
    subdivision_level: u8,
) -> Result<Vec<Vec<HexId>>, GridError> {
    let coord_to_id: BTreeMap<Isea3hCoord, HexId> = coords
        .iter()
        .enumerate()
        .map(|(i, &coord)| (coord, HexId(i as u32)))
        .collect();

    let mut neighbors = Vec::with_capacity(coords.len());

    for (index, &coord) in coords.iter().enumerate() {
        let expected = expected_neighbor_count(index as u32);
        let mut list: Vec<HexId> = isea3h::coord_neighbors(coord, subdivision_level)
            .into_iter()
            .map(|c| {
                coord_to_id
                    .get(&c)
                    .copied()
                    .ok_or(GridError::NeighborCountMismatch {
                        hex: index as u32,
                        expected,
                        found: 0,
                    })
            })
            .collect::<Result<_, _>>()?;
        list.sort_by_key(|id| id.0);
        list.dedup();
        let found = list.len() as u8;
        if found != expected {
            return Err(GridError::NeighborCountMismatch {
                hex: index as u32,
                expected,
                found,
            });
        }
        geography::sort_neighbors_by_bearing(centers[index], centers, &mut list);
        neighbors.push(list);
    }

    Ok(neighbors)
}

fn expected_neighbor_count(hex: u32) -> u8 {
    if hex < 12 { 5 } else { 6 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_levels_0_through_4() {
        for level in 0..=4u8 {
            let coords: Vec<_> = isea3h::all_cells(level).collect();
            let centers: Vec<_> = coords
                .iter()
                .map(|&c| isea3h::cell_center_vec3(c, level))
                .collect();
            build_neighbor_table(&coords, &centers, level)
                .unwrap_or_else(|e| panic!("level {level}: {e:?}"));
        }
    }
}
