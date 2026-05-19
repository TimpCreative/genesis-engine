#[derive(thiserror::Error, Debug)]
pub enum GridError {
    #[error("invalid subdivision level: {0} (valid range: 0-9)")]
    InvalidSubdivisionLevel(u8),

    #[error("invalid planet radius: {0} km (must be positive)")]
    InvalidPlanetRadius(f64),

    #[error(
        "neighbor construction failed for HexId({hex}): expected {expected} neighbors, found {found}"
    )]
    NeighborCountMismatch { hex: u32, expected: u8, found: u8 },
}
