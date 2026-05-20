//! World time representation: years, eras, and tick coordination.

mod ticks;

pub use ticks::{SimulationLayer, TickCoordinator};

use serde::{Deserialize, Serialize};

use crate::parameters::WorldParameters;

/// A year in world time. Year 0 = world formation.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct WorldYear(pub i64);

impl WorldYear {
    /// World formation (year zero).
    pub const FORMATION: WorldYear = WorldYear(0);

    /// Returns the underlying year value.
    pub fn value(self) -> i64 {
        self.0
    }
}

impl std::ops::Add<i64> for WorldYear {
    type Output = WorldYear;

    fn add(self, rhs: i64) -> Self::Output {
        WorldYear(self.0.saturating_add(rhs))
    }
}

impl std::ops::Sub<i64> for WorldYear {
    type Output = WorldYear;

    fn sub(self, rhs: i64) -> Self::Output {
        WorldYear(self.0.saturating_sub(rhs))
    }
}

impl std::ops::Sub<WorldYear> for WorldYear {
    type Output = i64;

    fn sub(self, rhs: WorldYear) -> Self::Output {
        self.0 - rhs.0
    }
}

/// Simulation era derived from [`WorldParameters`] boundary years.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum Era {
    /// Instant at world formation (`world_start_year` only).
    Formation,
    /// From just after formation until life emerges.
    Geological,
    /// From life emergence until sapience emerges.
    Prehistoric,
    /// From sapience until the ancient/recent boundary.
    Ancient,
    /// From the ancient/recent boundary onward.
    Recent,
}

impl Era {
    /// Returns the era containing `year` given world parameters.
    ///
    /// Boundaries (half-open except Formation at `world_start_year`):
    /// - Formation: `year == world_start_year`
    /// - Geological: `world_start < year < life_emergence`
    /// - Prehistoric: `life_emergence <= year < sapience`
    /// - Ancient: `sapience <= year < recent_boundary`
    /// - Recent: `year >= recent_boundary`
    ///
    /// `sapience` defaults to 4_490_000_000 when unset. `recent_boundary` is
    /// `default_user_year - 2000`.
    pub fn for_year(year: WorldYear, params: &WorldParameters) -> Self {
        let y = year.value();
        let start = params.core.time.world_start_year.value();
        let life = params.core.biology.life_emergence_year.value();
        let sapience = params
            .core
            .civilization
            .sapience_emergence_year
            .unwrap_or(WorldYear(4_490_000_000))
            .value();
        let recent_boundary = params.core.time.default_user_year.value() - 2000;

        if y == start {
            return Era::Formation;
        }
        if y < life {
            return Era::Geological;
        }
        if y < sapience {
            return Era::Prehistoric;
        }
        if y < recent_boundary {
            return Era::Ancient;
        }
        Era::Recent
    }
}

/// Rich world time with precomputed era.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct WorldTime {
    pub year: WorldYear,
    pub era: Era,
}

impl WorldTime {
    /// Constructs world time at `year` with era from `params`.
    pub fn at(year: WorldYear, params: &WorldParameters) -> Self {
        Self {
            year,
            era: Era::for_year(year, params),
        }
    }

    /// Returns the year as a signed integer.
    pub fn year_value(&self) -> i64 {
        self.year.value()
    }

    /// Returns how many years before `now` this time is (non-negative if `self.year <= now`).
    pub fn years_before(&self, now: WorldYear) -> i64 {
        now.value() - self.year.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::WorldParameters;

    #[test]
    fn world_year_arithmetic() {
        assert_eq!(WorldYear(100) + 50, WorldYear(150));
        assert_eq!(WorldYear(100) - 50, WorldYear(50));
        assert_eq!(WorldYear(100) - WorldYear(50), 50);
    }

    #[test]
    fn era_formation_at_start() {
        let params = WorldParameters::default();
        assert_eq!(Era::for_year(WorldYear::FORMATION, &params), Era::Formation);
    }

    #[test]
    fn era_prehistoric_after_life_emergence() {
        let params = WorldParameters::default();
        let life = params.core.biology.life_emergence_year;
        assert_eq!(Era::for_year(life + 1, &params), Era::Prehistoric);
    }

    #[test]
    fn era_boundaries_are_sharp() {
        let params = WorldParameters::default();
        let life = params.core.biology.life_emergence_year;
        let sapience = WorldYear(4_490_000_000);
        let recent = params.core.time.default_user_year.value() - 2000;

        assert_eq!(Era::for_year(life - 1, &params), Era::Geological);
        assert_eq!(Era::for_year(life, &params), Era::Prehistoric);
        assert_eq!(Era::for_year(sapience - 1, &params), Era::Prehistoric);
        assert_eq!(Era::for_year(sapience, &params), Era::Ancient);
        assert_eq!(Era::for_year(WorldYear(recent - 1), &params), Era::Ancient);
        assert_eq!(Era::for_year(WorldYear(recent), &params), Era::Recent);
    }

    #[test]
    fn world_time_at_matches_era() {
        let params = WorldParameters::default();
        let year = WorldYear(1_000_000);
        let wt = WorldTime::at(year, &params);
        assert_eq!(wt.year, year);
        assert_eq!(wt.era, Era::for_year(year, &params));
    }
}
