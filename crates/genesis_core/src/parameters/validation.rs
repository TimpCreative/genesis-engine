//! Validation for [`WorldParameters`].

use super::WorldParameters;

/// Errors from [`WorldParameters::validate`].
#[derive(thiserror::Error, Debug)]
pub enum ParameterValidationError {
    #[error("invalid planet radius: {0} km (must be positive and finite)")]
    InvalidPlanetRadius(f64),

    #[error("invalid axial tilt: {0}° (must be in 0..=90)")]
    InvalidAxialTilt(f32),

    #[error("invalid hex subdivision level: {0} (valid range: 5-9 for v1)")]
    InvalidSubdivisionLevel(u8),

    #[error("v1 does not support {feature}")]
    UnsupportedV1Feature { feature: &'static str },

    #[error("invalid value for {field}: {message}")]
    InvalidField { field: String, message: String },
}

impl WorldParameters {
    /// Validates v1 parameter constraints.
    pub fn validate(&self) -> Result<(), ParameterValidationError> {
        let p = &self.core.planet;
        if !p.radius_km.is_finite() || p.radius_km <= 0.0 {
            return Err(ParameterValidationError::InvalidPlanetRadius(p.radius_km));
        }
        if !p.gravity_g.is_finite() || p.gravity_g <= 0.0 {
            return Err(ParameterValidationError::InvalidField {
                field: "planet.gravity_g".into(),
                message: "must be positive and finite".into(),
            });
        }
        if !p.axial_tilt_degrees.is_finite() || !(0.0..=90.0).contains(&p.axial_tilt_degrees) {
            return Err(ParameterValidationError::InvalidAxialTilt(
                p.axial_tilt_degrees,
            ));
        }
        if !p.rotation_period_hours.is_finite() || p.rotation_period_hours <= 0.0 {
            return Err(ParameterValidationError::InvalidField {
                field: "planet.rotation_period_hours".into(),
                message: "must be positive and finite".into(),
            });
        }
        if !p.orbital_period_days.is_finite() || p.orbital_period_days <= 0.0 {
            return Err(ParameterValidationError::InvalidField {
                field: "planet.orbital_period_days".into(),
                message: "must be positive and finite".into(),
            });
        }
        if p.star_count != 1 {
            return Err(ParameterValidationError::UnsupportedV1Feature {
                feature: "multi-star systems",
            });
        }
        if p.moon_count > 2 {
            return Err(ParameterValidationError::InvalidField {
                field: "planet.moon_count".into(),
                message: "must be 0..=2 for v1".into(),
            });
        }
        if p.tidally_locked {
            return Err(ParameterValidationError::UnsupportedV1Feature {
                feature: "tidally locked planets",
            });
        }

        let level = self.core.grid.subdivision_level;
        if !(5..=9).contains(&level) {
            return Err(ParameterValidationError::InvalidSubdivisionLevel(level));
        }

        validate_scale(
            self.core.geology.plate_velocity_scale,
            "geology.plate_velocity_scale",
        )?;
        validate_scale(self.core.geology.volcanism_scale, "geology.volcanism_scale")?;
        validate_scale(
            self.core.biology.mutation_rate_scale,
            "biology.mutation_rate_scale",
        )?;
        validate_scale(
            self.core.biology.extinction_scale,
            "biology.extinction_scale",
        )?;
        validate_scale(
            self.core.civilization.tech_rate_scale,
            "civilization.tech_rate_scale",
        )?;
        validate_scale(
            self.core.civilization.cultural_drift_scale,
            "civilization.cultural_drift_scale",
        )?;
        validate_scale(
            self.core.civilization.conflict_scale,
            "civilization.conflict_scale",
        )?;
        validate_scale(
            self.core.climate_initial.greenhouse_intensity,
            "climate_initial.greenhouse_intensity",
        )?;

        let frac = self.core.geology.initial_continental_fraction;
        if !frac.is_finite() || !(0.0..=1.0).contains(&frac) {
            return Err(ParameterValidationError::InvalidField {
                field: "geology.initial_continental_fraction".into(),
                message: "must be in 0.0..=1.0".into(),
            });
        }
        if self.core.geology.initial_plate_count < 1 {
            return Err(ParameterValidationError::InvalidField {
                field: "geology.initial_plate_count".into(),
                message: "must be at least 1".into(),
            });
        }

        Ok(())
    }
}

fn validate_scale(value: f32, field: &str) -> Result<(), ParameterValidationError> {
    if !value.is_finite() || !(0.0..=10.0).contains(&value) {
        return Err(ParameterValidationError::InvalidField {
            field: field.to_string(),
            message: "must be finite and in 0.0..=10.0".into(),
        });
    }
    Ok(())
}
