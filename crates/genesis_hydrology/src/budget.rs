//! The conserved planetary water budget (Doc 08 §3.2–§3.3).
//!
//! All accounting is f64 in fixed order (ascending-`HexId` sums where per-hex
//! sums apply) and asserted per tick to 1e-6 relative. There are no leak
//! paths: the ocean term is the remainder of the partition, so the §3.2
//! identity holds by construction.

use genesis_core::parameters::WorldParameters;

/// End of the Molten stage / start of Cooling (Doc 08 §3.3; mirrors the
/// formation stage boundaries of Doc 07 §3.2).
pub const MOLTEN_END_YEAR: i64 = 50_000_000;
/// End of Cooling / start of Condensation.
pub const COOLING_END_YEAR: i64 = 200_000_000;
/// End of Condensation / start of Stabilization.
pub const CONDENSATION_END_YEAR: i64 = 350_000_000;
/// End of Formation; the inventory is fully condensed from here on.
pub const FORMATION_END_YEAR: i64 = 500_000_000;

/// Condensed inventory fraction during Cooling (§3.3).
pub const COOLING_CONDENSED_FRACTION: f64 = 0.35;
/// Condensed inventory fraction during Condensation (§3.3).
pub const CONDENSATION_CONDENSED_FRACTION: f64 = 0.90;

/// Groundwater relaxation time constant in years (§3.3 Slice-1 simple
/// relaxation; the aridity-equilibrium target is Slice 2's §6).
pub const GROUNDWATER_RELAXATION_TAU_YEARS: f64 = 50_000_000.0;

/// Relative tolerance for the per-tick conservation assert (§3.2).
pub const CONSERVATION_TOLERANCE_REL: f64 = 1e-6;

/// Condensed fraction of the water inventory at `year_value` (Doc 08 §3.3).
///
/// Piecewise-constant per formation stage — the literal §3.3 curve (Molten →
/// 0, Cooling → 0.35, Condensation → 0.90, Stabilization → 1.0). The Doc 07
/// curve interpolated across stage boundaries; §3.3 does not, so neither do
/// we. With `skip_planetary_formation` the inventory is condensed from the
/// start.
pub fn condensed_fraction_at_year(year_value: i64, skip_planetary_formation: bool) -> f64 {
    if skip_planetary_formation {
        return 1.0;
    }
    if year_value < MOLTEN_END_YEAR {
        0.0
    } else if year_value < COOLING_END_YEAR {
        COOLING_CONDENSED_FRACTION
    } else if year_value < CONDENSATION_END_YEAR {
        CONDENSATION_CONDENSED_FRACTION
    } else {
        1.0
    }
}

/// Planet surface area in m².
pub fn planet_surface_area_m2(params: &WorldParameters) -> f64 {
    let radius_m = params.core.planet.radius_km * 1000.0;
    4.0 * std::f64::consts::PI * radius_m * radius_m
}

/// Total water inventory volume in m³ (GEL meters over the whole sphere).
pub fn inventory_volume_m3(params: &WorldParameters) -> f64 {
    f64::from(params.core.hydrology.water_inventory_gel_m) * planet_surface_area_m2(params)
}

/// GEL-equivalent aquifer capacity as a volume in m³.
pub fn groundwater_capacity_m3(params: &WorldParameters) -> f64 {
    f64::from(params.core.hydrology.groundwater_capacity_m) * planet_surface_area_m2(params)
}

/// Relaxes groundwater storage toward capacity (§3.3). Exact-exponential
/// approach, deterministic in f64.
pub fn relax_groundwater(storage_m3: f64, capacity_m3: f64, interval_years: f64) -> f64 {
    if interval_years <= 0.0 {
        return storage_m3;
    }
    let step = 1.0 - (-interval_years / GROUNDWATER_RELAXATION_TAU_YEARS).exp();
    storage_m3 + (capacity_m3 - storage_m3) * step
}

/// The §3.2 accounting identity: every reservoir exactly once, f64.
///
/// `inventory = atmosphere_reserve + ocean + Σ lakes + ice + groundwater`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterBudget {
    /// Total surface-water inventory, m³ (world parameter; immutable).
    pub inventory_m3: f64,
    /// Uncondensed inventory held in the atmosphere, m³ (Formation only).
    pub atmosphere_reserve_m3: f64,
    /// Ocean term, m³ (reference-temperature mass volume; §3.5.1).
    pub ocean_volume_m3: f64,
    /// Summed lake volumes, m³ (previous tick's; §3.4).
    pub lake_volumes_m3: f64,
    /// Budgeted land-ice volume, m³.
    pub ice_volume_m3: f64,
    /// Aquifer storage, m³.
    pub groundwater_storage_m3: f64,
}

impl WaterBudget {
    /// Partitions the inventory at one tick (§3.4): the ocean term is the
    /// condensed fraction minus the non-ocean reservoirs, so the identity
    /// holds by construction.
    ///
    /// The ocean term is deliberately *not* clamped at zero: a negative value
    /// would mean the other reservoirs exceed the condensed inventory, and
    /// clamping would silently leak water out of the identity. The flooding
    /// solve treats `ocean_volume ≤ 0` as "no standing water" (§3.4 step 1).
    pub fn partition(
        inventory_m3: f64,
        condensed_fraction: f64,
        lake_volumes_m3: f64,
        ice_volume_m3: f64,
        groundwater_storage_m3: f64,
    ) -> Self {
        let condensed = inventory_m3 * condensed_fraction;
        Self {
            inventory_m3,
            atmosphere_reserve_m3: inventory_m3 - condensed,
            ocean_volume_m3: condensed - ice_volume_m3 - lake_volumes_m3 - groundwater_storage_m3,
            lake_volumes_m3,
            ice_volume_m3,
            groundwater_storage_m3,
        }
    }

    /// `inventory − (atmosphere + ocean + lakes + ice + groundwater)`; zero
    /// when the identity holds.
    pub fn conservation_error_m3(&self) -> f64 {
        self.inventory_m3
            - self.atmosphere_reserve_m3
            - self.ocean_volume_m3
            - self.lake_volumes_m3
            - self.ice_volume_m3
            - self.groundwater_storage_m3
    }

    /// True when the identity holds to §3.2's 1e-6 relative tolerance.
    pub fn is_conserved(&self) -> bool {
        self.conservation_error_m3().abs()
            <= CONSERVATION_TOLERANCE_REL * self.inventory_m3.abs().max(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condensed_fraction_matches_stage_boundaries() {
        assert_eq!(condensed_fraction_at_year(0, false), 0.0);
        assert_eq!(condensed_fraction_at_year(MOLTEN_END_YEAR - 1, false), 0.0);
        assert_eq!(condensed_fraction_at_year(MOLTEN_END_YEAR, false), 0.35);
        assert_eq!(
            condensed_fraction_at_year(COOLING_END_YEAR - 1, false),
            0.35
        );
        assert_eq!(condensed_fraction_at_year(COOLING_END_YEAR, false), 0.90);
        assert_eq!(
            condensed_fraction_at_year(CONDENSATION_END_YEAR - 1, false),
            0.90
        );
        assert_eq!(
            condensed_fraction_at_year(CONDENSATION_END_YEAR, false),
            1.0
        );
        assert_eq!(condensed_fraction_at_year(FORMATION_END_YEAR, false), 1.0);
        assert_eq!(condensed_fraction_at_year(4_500_000_000, false), 1.0);
    }

    #[test]
    fn condensed_fraction_is_full_when_formation_skipped() {
        assert_eq!(condensed_fraction_at_year(0, true), 1.0);
        assert_eq!(condensed_fraction_at_year(MOLTEN_END_YEAR, true), 1.0);
    }

    #[test]
    fn partition_conserves_through_synthetic_reservoir_transfers() {
        // Gate §15 #1: drive the budget through reservoir transfers —
        // atmosphere → ocean (condensation), ocean → groundwater (recharge),
        // ocean → ice (glaciation), ice → lakes (melt) — the identity must
        // hold at every step to 1e-6 relative.
        fn assert_conserved(
            inventory: f64,
            atmosphere: f64,
            ocean: f64,
            lakes: f64,
            ice: f64,
            groundwater: f64,
            step: &str,
        ) {
            let budget = WaterBudget {
                inventory_m3: inventory,
                atmosphere_reserve_m3: atmosphere,
                ocean_volume_m3: ocean,
                lake_volumes_m3: lakes,
                ice_volume_m3: ice,
                groundwater_storage_m3: groundwater,
            };
            assert!(budget.is_conserved(), "conservation broke at {step}");
        }

        let inventory = 2700.0 * 4.0 * std::f64::consts::PI * 6.371e6_f64.powi(2);
        let mut atmosphere = inventory;
        let mut ocean = 0.0;
        let mut lakes = 0.0;
        let mut ice = 0.0;
        let mut groundwater = 0.0;

        assert_conserved(
            inventory,
            atmosphere,
            ocean,
            lakes,
            ice,
            groundwater,
            "initial",
        );

        // Condensation: atmosphere → ocean in ten steps.
        for i in 1..=10 {
            let moved = atmosphere * 0.1;
            atmosphere -= moved;
            ocean += moved;
            assert_conserved(
                inventory,
                atmosphere,
                ocean,
                lakes,
                ice,
                groundwater,
                &format!("condensation {i}"),
            );
        }

        // Recharge: ocean → groundwater.
        let recharge = ocean * 0.05;
        ocean -= recharge;
        groundwater += recharge;
        assert_conserved(
            inventory,
            atmosphere,
            ocean,
            lakes,
            ice,
            groundwater,
            "recharge",
        );

        // Glaciation: ocean → ice; melt: ice → lakes.
        let frozen = ocean * 0.2;
        ocean -= frozen;
        ice += frozen;
        assert_conserved(
            inventory,
            atmosphere,
            ocean,
            lakes,
            ice,
            groundwater,
            "glaciation",
        );
        let melted = ice * 0.5;
        ice -= melted;
        lakes += melted;
        assert_conserved(
            inventory,
            atmosphere,
            ocean,
            lakes,
            ice,
            groundwater,
            "melt",
        );

        // Drain: lakes + groundwater return to the ocean term.
        ocean += lakes + groundwater;
        lakes = 0.0;
        groundwater = 0.0;
        assert_conserved(
            inventory,
            atmosphere,
            ocean,
            lakes,
            ice,
            groundwater,
            "drain",
        );
    }

    #[test]
    fn partition_derives_ocean_as_remainder() {
        let budget = WaterBudget::partition(1000.0, 0.9, 10.0, 20.0, 30.0);
        assert_eq!(budget.atmosphere_reserve_m3, 100.0);
        assert_eq!(budget.ocean_volume_m3, 840.0);
        assert!(budget.is_conserved());
    }

    #[test]
    fn relax_groundwater_approaches_capacity_monotonically() {
        let capacity = 1.0e15;
        let mut storage = 0.0;
        let mut prev = -1.0;
        for _ in 0..100 {
            storage = relax_groundwater(storage, capacity, 5_000_000.0);
            assert!(storage > prev, "storage must rise every tick");
            assert!(storage <= capacity, "storage must not overshoot capacity");
            prev = storage;
        }
        assert!(
            storage > 0.99 * capacity,
            "after 500 My storage should be near capacity; got {storage}"
        );
    }

    #[test]
    fn relax_groundwater_noop_without_elapsed_time() {
        assert_eq!(relax_groundwater(42.0, 100.0, 0.0), 42.0);
    }
}
