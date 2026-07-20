//! `StubBiologyView` — the Prep-09 stand-in for Doc 09 biology (Prep-09 §2.3).
//!
//! Deterministic fabrication: same seed + same `WorldData` → same biomes,
//! richness, species, tree, and events, so screenshots and reviews are stable.
//! Biomes/richness are pure functions of existing physical fields (a crude
//! Whittaker table on temperature/precipitation plus `habitability`); species,
//! trees, and life-events are seeded fabrications. **Not ecologically valid** —
//! every method carries the `// STUB` intent and is discarded at Doc 09.

use bevy::prelude::Resource;
use genesis_core::biology_view::{
    Assemblage, BiologyView, GuildSummary, LifeEventCategory, LifeEventPip, SpeciesPeek,
    TreeNodePeek, TreePeek,
};
use genesis_core::data::{BiomeId, WorldData};
use genesis_core::grid::HexId;
use genesis_core::time::WorldYear;

/// The active biology view, wired at world load (stub now, Doc 09 adapter later).
#[derive(Resource)]
pub struct ActiveBiologyView(pub Box<dyn BiologyView>);

/// Stub biome catalog — `BiomeId(index)`; shared name/color scheme with the
/// render layer (`genesis_render::stub_biome_color`).
pub const STUB_BIOMES: [&str; 12] = [
    "Tropical rainforest", // 0
    "Tropical savanna",    // 1
    "Hot desert",          // 2
    "Mediterranean scrub", // 3
    "Temperate forest",    // 4
    "Temperate grassland", // 5
    "Boreal forest",       // 6
    "Tundra",              // 7
    "Polar desert",        // 8
    "Wetland",             // 9
    "Alpine",              // 10
    "Coastal shallows",    // 11
];

const GUILD_CAPACITY: u32 = 41;
const GUILDS: [&str; 12] = [
    "Primary producer",
    "Grazer",
    "Browser",
    "Apex predator",
    "Mesopredator",
    "Decomposer",
    "Filter feeder",
    "Pollinator",
    "Scavenger",
    "Burrower",
    "Nektonic hunter",
    "Insectivore",
];
const TRAITS: [&str; 18] = [
    "hypercarnivore",
    "herbivore",
    "limbed",
    "image-eye",
    "filter-feeder",
    "scaled",
    "warm-blooded",
    "nocturnal",
    "colonial",
    "photosynthetic",
    "burrowing",
    "aquatic",
    "winged",
    "armored",
    "venomous",
    "echolocation",
    "bioluminescent",
    "amorphous",
];
const NAME_PREFIX: [&str; 12] = [
    "kra", "vor", "thal", "ory", "nek", "sil", "dra", "mor", "pyx", "lue", "gan", "zeph",
];
const NAME_SUFFIX: [&str; 10] = [
    "id", "ax", "on", "ith", "us", "el", "yr", "oth", "ex", "ura",
];
const FAMILIES: [&str; 8] = [
    "Cryptobionts",
    "Nektomorphs",
    "Radiozoa",
    "Phytobionts",
    "Chelostomes",
    "Vermiforms",
    "Sessilids",
    "Aeronauts",
];

/// SplitMix64 finalizer — a cheap, well-mixed deterministic hash.
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// The deterministic stub. Holds only the world seed; everything else derives
/// from it plus the queried `WorldData`.
pub struct StubBiologyView {
    seed: u64,
}

impl StubBiologyView {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    fn h(&self, parts: &[u64]) -> u64 {
        let mut acc = self.seed ^ 0x9e37_79b9_7f4a_7c15;
        for &p in parts {
            acc = mix(acc ^ mix(p));
        }
        acc
    }

    fn is_land(data: &WorldData, i: usize) -> bool {
        i < data.elevation_mean.len() && data.elevation_mean[i] > data.sea_level_m
    }

    fn field(v: &[f32], i: usize, default: f32) -> f32 {
        v.get(i).copied().unwrap_or(default)
    }
}

/// Whittaker-ish biome from temperature (°C) and precipitation (mm/yr), plus an
/// alpine override for high ground. Pure and deterministic.
fn stub_biome(temp_c: f32, precip_mm: f32, elev_above_sea: f32) -> u16 {
    if elev_above_sea > 2800.0 {
        return 10; // Alpine
    }
    if precip_mm < 250.0 {
        if temp_c > 18.0 {
            2 // Hot desert
        } else if temp_c > 0.0 {
            5 // Temperate grassland
        } else {
            8 // Polar desert
        }
    } else if precip_mm < 600.0 {
        if temp_c > 20.0 {
            1 // Tropical savanna
        } else if temp_c > 7.0 {
            3 // Mediterranean scrub
        } else if temp_c > 0.0 {
            5 // Temperate grassland
        } else {
            7 // Tundra
        }
    } else if precip_mm < 1400.0 {
        if temp_c > 20.0 {
            1 // Tropical savanna
        } else if temp_c > 7.0 {
            4 // Temperate forest
        } else if temp_c > -2.0 {
            6 // Boreal forest
        } else {
            7 // Tundra
        }
    } else if temp_c > 20.0 {
        0 // Tropical rainforest
    } else if temp_c > 7.0 {
        4 // Temperate forest
    } else if temp_c > -2.0 {
        6 // Boreal forest
    } else {
        7 // Tundra
    }
}

fn name_from(id: u64) -> String {
    let p = NAME_PREFIX[(id % NAME_PREFIX.len() as u64) as usize];
    let s = NAME_SUFFIX[((id >> 8) % NAME_SUFFIX.len() as u64) as usize];
    let mut name = String::new();
    let mut chars = p.chars();
    if let Some(first) = chars.next() {
        name.extend(first.to_uppercase());
        name.extend(chars);
    }
    name.push_str(s);
    name
}

impl BiologyView for StubBiologyView {
    fn biome_at(&self, data: &WorldData, hex: HexId) -> BiomeId {
        let i = hex.0 as usize;
        if !Self::is_land(data, i) {
            return BiomeId::NONE;
        }
        let temp = Self::field(&data.temperature_mean, i, 15.0);
        let precip = Self::field(&data.precipitation, i, 800.0);
        let elev_above = data.elevation_mean[i] - data.sea_level_m;
        BiomeId(stub_biome(temp, precip, elev_above))
    }

    fn biome_name(&self, biome: BiomeId) -> String {
        if biome == BiomeId::NONE {
            return "Ocean".to_string();
        }
        STUB_BIOMES
            .get(biome.0 as usize)
            .copied()
            .unwrap_or("Unknown")
            .to_string()
    }

    fn richness_at(&self, data: &WorldData, hex: HexId) -> f32 {
        let i = hex.0 as usize;
        if !Self::is_land(data, i) {
            // Shallow seas carry some richness; deep ocean little.
            let depth = (data.sea_level_m - Self::field(&data.elevation_mean, i, 0.0)).max(0.0);
            return (0.35 * (1.0 - (depth / 3000.0)).clamp(0.0, 1.0)).clamp(0.0, 1.0);
        }
        let hab = Self::field(&data.habitability, i, 0.4);
        let temp = Self::field(&data.temperature_mean, i, 15.0);
        // Latitudinal / thermal bonus peaking near 27 °C (the tropics).
        let tropical = (1.0 - ((temp - 27.0) / 30.0).abs()).clamp(0.0, 1.0);
        (hab * 0.7 + tropical * 0.3).clamp(0.0, 1.0)
    }

    fn biomass_at(&self, data: &WorldData, hex: HexId) -> f32 {
        let i = hex.0 as usize;
        let hab = Self::field(&data.habitability, i, 0.3);
        let precip = Self::field(&data.precipitation, i, 400.0);
        let wet = (precip / 2200.0).clamp(0.0, 1.0);
        if Self::is_land(data, i) {
            (hab * 0.6 + wet * 0.4).clamp(0.0, 1.0)
        } else {
            // Marine biomass: productive shallow shelves, sparse abyss.
            let depth = (data.sea_level_m - Self::field(&data.elevation_mean, i, 0.0)).max(0.0);
            (0.5 * (1.0 - (depth / 2500.0)).clamp(0.0, 1.0)).clamp(0.0, 1.0)
        }
    }

    fn occupied_guilds(&self, data: &WorldData, hex: HexId) -> Vec<GuildSummary> {
        let r = self.richness_at(data, hex);
        let occupied = (r * GUILDS.len() as f32).round() as usize;
        GUILDS
            .iter()
            .enumerate()
            .map(|(k, &name)| GuildSummary {
                name: name.to_string(),
                occupied: k < occupied,
            })
            .collect()
    }

    fn assemblage(&self, data: &WorldData, hex: HexId) -> Assemblage {
        let r = self.richness_at(data, hex);
        let biome = self.biome_at(data, hex);
        let occupied = (r * GUILD_CAPACITY as f32).round() as u32;
        let headline = self.occupied_guilds(data, hex);
        // 1–2 species per occupied headline guild, bounded.
        let mut species = Vec::new();
        for (gk, g) in headline.iter().enumerate().filter(|(_, g)| g.occupied) {
            let count = 1 + (self.h(&[hex.0 as u64, gk as u64, 7]) % 2) as usize;
            for si in 0..count {
                let id = self.h(&[hex.0 as u64, gk as u64, si as u64, 11]);
                let mut chips = Vec::new();
                for t in 0..3 {
                    let ti = (self.h(&[id, t, 3]) % TRAITS.len() as u64) as usize;
                    let chip = TRAITS[ti].to_string();
                    if !chips.contains(&chip) {
                        chips.push(chip);
                    }
                }
                let family = FAMILIES[(id % FAMILIES.len() as u64) as usize].to_string();
                let description = format!(
                    "A {} {} — {}.",
                    if r > 0.66 {
                        "specialized"
                    } else if r > 0.33 {
                        "adaptable"
                    } else {
                        "hardy"
                    },
                    g.name.to_lowercase(),
                    chips.join(", ")
                );
                species.push(SpeciesPeek {
                    species_id: id,
                    name: name_from(id),
                    guild: g.name.clone(),
                    family,
                    trait_chips: chips,
                    description,
                });
            }
        }
        Assemblage {
            biome_name: self.biome_name(biome),
            richness: r,
            occupied_guilds: occupied,
            guild_capacity: GUILD_CAPACITY,
            species,
        }
    }

    fn tree_snapshot(&self, year: WorldYear) -> TreePeek {
        let y = year.value();
        let mut nodes = vec![TreeNodePeek {
            id: 0,
            parent: None,
            name: "Life".to_string(),
            rank: "root".to_string(),
            defining_trait: "self-replication".to_string(),
            origin_year: 500_000_000,
            extinction_year: None,
        }];
        // A small bounded tree: kingdoms → classes, stamped with plausible years.
        for kingdom in 0..4u64 {
            let kid = 1 + kingdom;
            let origin = 600_000_000 + (self.h(&[kid, 1]) % 900_000_000) as i64;
            if origin > y {
                continue;
            }
            let extinct = if self.h(&[kid, 2]) % 5 == 0 {
                Some(origin + 300_000_000 + (self.h(&[kid, 3]) % 500_000_000) as i64)
            } else {
                None
            };
            nodes.push(TreeNodePeek {
                id: kid,
                parent: Some(0),
                name: FAMILIES[(kid as usize) % FAMILIES.len()].to_string(),
                rank: "kingdom".to_string(),
                defining_trait: TRAITS[(self.h(&[kid, 4]) % TRAITS.len() as u64) as usize]
                    .to_string(),
                origin_year: origin,
                extinction_year: extinct.filter(|&e| e <= y),
            });
            for cls in 0..3u64 {
                let cid = 100 + kid * 10 + cls;
                let corigin = origin + 100_000_000 + (self.h(&[cid, 5]) % 400_000_000) as i64;
                if corigin > y {
                    continue;
                }
                nodes.push(TreeNodePeek {
                    id: cid,
                    parent: Some(kid),
                    name: name_from(self.h(&[cid, 6])),
                    rank: "class".to_string(),
                    defining_trait: TRAITS[(self.h(&[cid, 7]) % TRAITS.len() as u64) as usize]
                        .to_string(),
                    origin_year: corigin,
                    extinction_year: None,
                });
            }
        }
        TreePeek { nodes }
    }

    fn life_events(&self, from: WorldYear, to: WorldYear) -> Vec<LifeEventPip> {
        use LifeEventCategory as C;
        // A plausible fixed spine of biosphere history (stub — Doc 09 emits real).
        let spine = [
            (500_000_000i64, "Life emerges", C::Origin),
            (1_100_000_000, "Great Oxygenation", C::Milestone),
            (1_800_000_000, "Eukaryotes radiate", C::Innovation),
            (2_400_000_000, "First multicellular life", C::Innovation),
            (2_900_000_000, "Cambrian-style explosion", C::Innovation),
            (3_300_000_000, "Mass extinction", C::Extinction),
            (3_900_000_000, "Vertebrate-analog radiation", C::Innovation),
            (4_200_000_000, "Great dying", C::Extinction),
            (4_400_000_000, "Sapience emerges", C::Milestone),
        ];
        spine
            .iter()
            .filter(|(yr, _, _)| *yr >= from.value() && *yr <= to.value())
            .map(|(yr, label, cat)| LifeEventPip {
                year: *yr,
                label: (*label).to_string(),
                category: *cat,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn world() -> WorldData {
        let mut p = WorldParameters::default();
        p.core.grid.subdivision_level = 5;
        let mut d = create_world(p).expect("world").data;
        // Rich, warm, wet land so the stub actually fabricates species.
        d.sea_level_m = 0.0;
        for i in 0..d.cell_count() as usize {
            d.elevation_mean[i] = 400.0;
            d.habitability[i] = 0.8;
            d.temperature_mean[i] = 25.0;
            d.precipitation[i] = 1500.0;
        }
        d
    }

    #[test]
    fn stub_is_deterministic() {
        let data = world();
        let a = StubBiologyView::new(42);
        let b = StubBiologyView::new(42);
        let h = HexId(10);
        assert_eq!(a.assemblage(&data, h), b.assemblage(&data, h));
        assert_eq!(a.tree_snapshot(WorldYear(3_000_000_000)), b.tree_snapshot(WorldYear(3_000_000_000)));
        assert_eq!(a.richness_at(&data, h), b.richness_at(&data, h));
    }

    #[test]
    fn seed_changes_output() {
        let data = world();
        let a = StubBiologyView::new(1);
        let b = StubBiologyView::new(2);
        // Physical-derived fields (biome/richness) may match; fabricated species differ.
        assert_ne!(a.assemblage(&data, HexId(10)), b.assemblage(&data, HexId(10)));
    }

    #[test]
    fn tree_grows_with_year() {
        let a = StubBiologyView::new(7);
        let early = a.tree_snapshot(WorldYear(700_000_000)).nodes.len();
        let late = a.tree_snapshot(WorldYear(4_400_000_000)).nodes.len();
        assert!(late >= early, "tree should not shrink forward in time");
    }
}
