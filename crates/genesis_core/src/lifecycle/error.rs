//! Lifecycle error types.

use thiserror::Error;

/// Errors from [`crate::lifecycle::create_world`].
#[derive(Error, Debug)]
pub enum CreateWorldError {
    #[error("invalid parameters: {0}")]
    InvalidParameters(#[from] crate::parameters::ParameterValidationError),

    #[error("grid construction failed: {0}")]
    GridConstruction(#[from] crate::grid::GridError),
}

/// Errors from [`crate::lifecycle::generate_full_history`].
#[derive(Error, Debug)]
pub enum GenerationError {
    #[error("target year {target} is before current year {current}")]
    TargetInPast { target: i64, current: i64 },
}
