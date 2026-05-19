//! Core infrastructure for Genesis Engine: hex grid, world data, time, RNG, persistence.

pub mod grid;

pub use grid::isea3h;
pub use grid::{Direction, GridError, HexGrid, HexId};
