//! Tectonic simulation for Genesis Engine.
//!
//! Phase 1 deliverable: initial plate generation only. Motion, boundary effects,
//! and per-tick simulation are added in subsequent prompts.

pub mod initial_generation;
pub mod plate;

pub use initial_generation::generate_initial_plates;
pub use plate::{Plate, PlateClass, PlateRegistry, PlateType};
