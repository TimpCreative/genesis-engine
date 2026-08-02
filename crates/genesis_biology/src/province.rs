//! Biogeographic provinces — the granularity biology simulates at (Doc 09 §5.1).
//!
//! A province is a connected region of one **realm** (marine / terrestrial /
//! freshwater), labeled by flood-fill over grid adjacency — the same
//! connected-component idea hydrology uses for ocean basins. Identity is
//! **deterministic**: components are discovered in ascending `HexId` order, so a
//! province's `ProvinceId` is stable given the same geography (its lowest member
//! `HexId` is the stable key across relabeling, per Doc 09 §5.1). Guild
//! occupancy, the food web, and richness (the rest of §5.1) arrive in P4-5.

use std::collections::{BTreeSet, VecDeque};

use genesis_core::data::{GuildId, ProvinceId, WATER_NONE, WaterBodyKind, WorldData};
use genesis_core::grid::HexId;

/// The three ecological realms (Doc 09 §5.1, §10.1a).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Realm {
    Marine,
    Terrestrial,
    Freshwater,
}

/// One biogeographic province (P4-4 subset of Doc 09 §5.1).
#[derive(Clone, Debug, PartialEq)]
pub struct BiogeographicProvince {
    pub id: ProvinceId,
    pub realm: Realm,
    /// Member hexes, ascending; `hexes[0]` is the stable identity key.
    pub hexes: Vec<HexId>,
    /// Dispersal-adjacent provinces (barriers gate crossings in later slices).
    pub neighbors: BTreeSet<ProvinceId>,
    /// Area-aggregated biotic richness R (Doc 09 §5.1); filled by
    /// [`crate::richness`]. 0 until computed.
    pub richness: f32,
    /// Number of guilds the richness supports (Doc 09 §4.4). 0 until computed.
    pub occupied_guild_count: u32,
    /// The guilds actually occupied, headline-first (guaranteed then contingent
    /// cascade, Doc 09 §4.3). Filled by [`crate::population`].
    pub occupied_guilds: Vec<GuildId>,
}

impl BiogeographicProvince {
    /// The lowest member `HexId` — the deterministic identity key (Doc 09 §5.1).
    pub fn key_hex(&self) -> HexId {
        self.hexes[0]
    }
}

/// The province registry for the current geography (Doc 09 §5.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProvinceRegistry {
    provinces: Vec<BiogeographicProvince>,
}

impl ProvinceRegistry {
    pub fn len(&self) -> usize {
        self.provinces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.provinces.is_empty()
    }

    /// Province by id (dense; `ProvinceId(k)` is `provinces[k]`).
    pub fn get(&self, id: ProvinceId) -> Option<&BiogeographicProvince> {
        self.provinces.get(id.0 as usize)
    }

    pub fn iter(&self) -> impl Iterator<Item = &BiogeographicProvince> {
        self.provinces.iter()
    }

    /// Mutable province slice, for [`crate::richness`] to fill R / guild counts.
    pub(crate) fn provinces_mut(&mut self) -> &mut [BiogeographicProvince] {
        &mut self.provinces
    }
}

/// A coarse **biome zone** from the climate regime — the "similar biome" key
/// that subdivides a realm into provinces (Doc 09 §5.1), so a continent's
/// tropical and boreal halves are distinct provinces rather than one. Grouped
/// (not the raw 11 regimes) to avoid over-fragmenting into tiny provinces.
pub(crate) fn biome_zone(world: &WorldData, i: usize) -> u8 {
    use genesis_core::data::ClimateRegimePlaceholder as R;
    match world.climate_regime.get(i).copied().unwrap_or(R::Unset) {
        R::Unset => 0,
        R::Tropical | R::Subtropical => 1,
        R::HotDesert | R::ColdDesert => 2,
        R::Mediterranean | R::Temperate | R::ContinentalCool => 3,
        R::Boreal => 4,
        R::Tundra | R::Polar => 5,
    }
}

/// The realm of a hex from its water state (Doc 09 §5.1).
fn realm_of(world: &WorldData, i: usize) -> Realm {
    let elev = world.elevation_mean[i];
    let water = world.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
    if !(water.is_finite() && water > elev) {
        return Realm::Terrestrial;
    }
    let body_id = world.water_body_id.get(i).copied().unwrap_or_default();
    match world.water_bodies.get(&body_id).map(|b| b.kind) {
        Some(WaterBodyKind::Lake | WaterBodyKind::SaltLake) => Realm::Freshwater,
        _ => Realm::Marine, // ocean/sea/inland-sea, or wet-but-unlabeled
    }
}

/// Flood-fills the grid into same-realm provinces, writes `world.province_id`,
/// and returns the registry. Deterministic: components are discovered in
/// ascending `HexId` order, so ids are stable for a given geography.
pub fn label_provinces(world: &mut WorldData) -> ProvinceRegistry {
    let n = world.cell_count() as usize;
    let realm: Vec<Realm> = (0..n).map(|i| realm_of(world, i)).collect();
    // The "similar biome" key (Doc 09 §5.1): a province is one connected run of a
    // single realm *and* biome zone, so climate boundaries split provinces.
    let zone: Vec<u8> = (0..n).map(|i| biome_zone(world, i)).collect();
    let mut province_of = vec![ProvinceId::NONE; n];
    let mut provinces: Vec<BiogeographicProvince> = Vec::new();

    for start in 0..n {
        if province_of[start] != ProvinceId::NONE {
            continue;
        }
        assert!(
            provinces.len() < u16::MAX as usize,
            "province count exceeded ProvinceId space"
        );
        let id = ProvinceId(provinces.len() as u16);
        let r = realm[start];
        let z = zone[start];
        let mut hexes = Vec::new();
        let mut queue = VecDeque::new();
        province_of[start] = id;
        queue.push_back(start);
        while let Some(cur) = queue.pop_front() {
            hexes.push(HexId(cur as u32));
            for nb in world.grid.neighbors(HexId(cur as u32)) {
                let ni = nb.0 as usize;
                if province_of[ni] == ProvinceId::NONE && realm[ni] == r && zone[ni] == z {
                    province_of[ni] = id;
                    queue.push_back(ni);
                }
            }
        }
        hexes.sort_unstable();
        provinces.push(BiogeographicProvince {
            id,
            realm: r,
            hexes,
            neighbors: BTreeSet::new(),
            richness: 0.0,
            occupied_guild_count: 0,
            occupied_guilds: Vec::new(),
        });
    }

    // Dispersal adjacency: a differing province across any hex boundary.
    for i in 0..n {
        let pi = province_of[i];
        for nb in world.grid.neighbors(HexId(i as u32)) {
            let pj = province_of[nb.0 as usize];
            if pj != pi {
                provinces[pi.0 as usize].neighbors.insert(pj);
            }
        }
    }

    world.province_id.copy_from_slice(&province_of);
    ProvinceRegistry { provinces }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{create_world, data::ProvinceId};

    /// A world split into a dry hemisphere and a wet (ocean) hemisphere by hex
    /// index — deterministic and easy to reason about.
    fn split_world() -> WorldData {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world").data;
        let n = world.cell_count() as usize;
        for i in 0..n {
            if i < n / 2 {
                world.elevation_mean[i] = 500.0; // dry land
                world.water_level_m[i] = WATER_NONE;
            } else {
                world.elevation_mean[i] = -2000.0; // ocean floor
                world.water_level_m[i] = 0.0;
            }
        }
        world
    }

    #[test]
    fn labels_cover_every_hex_and_are_dense() {
        let mut world = split_world();
        let reg = label_provinces(&mut world);
        assert!(!reg.is_empty());
        // Every hex is assigned; ids are dense 0..len.
        assert!(world.province_id.iter().all(|&p| p != ProvinceId::NONE));
        for (k, p) in reg.iter().enumerate() {
            assert_eq!(p.id, ProvinceId(k as u16));
            assert_eq!(p.key_hex(), p.hexes[0]);
            assert!(p.hexes.windows(2).all(|w| w[0] < w[1]), "hexes ascending");
        }
    }

    #[test]
    fn realms_follow_water_state() {
        let mut world = split_world();
        let reg = label_provinces(&mut world);
        // Hex 0 is dry → terrestrial; the last hex is ocean → marine.
        let first = reg.get(world.province_id[0]).unwrap();
        let last = reg
            .get(world.province_id[world.cell_count() as usize - 1])
            .unwrap();
        assert_eq!(first.realm, Realm::Terrestrial);
        assert_eq!(last.realm, Realm::Marine);
    }

    #[test]
    fn biome_zones_subdivide_a_realm() {
        use genesis_core::data::ClimateRegimePlaceholder as R;
        let mut world = split_world();
        let n = world.cell_count() as usize;
        // Give the land half two biome zones (tropical vs boreal).
        for i in 0..n / 2 {
            world.climate_regime[i] = if i < n / 4 { R::Tropical } else { R::Boreal };
        }
        let reg = label_provinces(&mut world);
        // Tropical and boreal land can't share a province, so the realm splits.
        let land = reg.iter().filter(|p| p.realm == Realm::Terrestrial).count();
        assert!(land >= 2, "biome zones must split the land, got {land}");
        // Every province is biome-coherent: all its hexes share one zone.
        for p in reg.iter() {
            let z0 = biome_zone(&world, p.hexes[0].0 as usize);
            assert!(
                p.hexes
                    .iter()
                    .all(|h| biome_zone(&world, h.0 as usize) == z0),
                "province {:?} must not span biome zones",
                p.id
            );
        }
    }

    #[test]
    fn labeling_is_deterministic() {
        let mut a = split_world();
        let mut b = split_world();
        let ra = label_provinces(&mut a);
        let rb = label_provinces(&mut b);
        assert_eq!(a.province_id, b.province_id);
        assert_eq!(ra, rb);
    }
}
