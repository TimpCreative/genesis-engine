//! Deterministic RNG streams derived from world seed and mod manifest.

use rand::SeedableRng;
use rand::rngs::SmallRng;

use crate::parameters::{ModManifest, WorldParameters, WorldSeed};

/// Named deterministic RNG streams for simulation subsystems.
#[derive(Debug)]
pub struct WorldRng {
    effective_seed: u64,
}

/// Computes the effective seed from seed value and canonical mod manifest bytes.
pub fn compute_effective_seed(seed: &WorldSeed, manifest: &ModManifest) -> u64 {
    let mut input = Vec::new();
    input.extend_from_slice(&seed.value.to_le_bytes());
    input.extend_from_slice(&manifest.canonical_bytes());
    xxhash_rust::xxh3::xxh3_64(&input)
}

impl WorldRng {
    /// Constructs from world parameters by computing the effective seed.
    pub fn from_parameters(params: &WorldParameters) -> Self {
        Self::from_effective_seed(compute_effective_seed(
            &params.core.seed,
            &params.core.mod_manifest,
        ))
    }

    /// Direct construction from a precomputed effective seed.
    pub fn from_effective_seed(effective_seed: u64) -> Self {
        Self { effective_seed }
    }

    /// Returns the effective seed (for serialization / debugging).
    pub fn effective_seed(&self) -> u64 {
        self.effective_seed
    }

    /// Derives a deterministic RNG for the given stream name.
    ///
    /// Each call returns a fresh RNG with identical initial state for the same name.
    pub fn stream(&self, name: &str) -> SmallRng {
        let mut input = Vec::new();
        input.extend_from_slice(&self.effective_seed.to_le_bytes());
        input.extend_from_slice(name.as_bytes());
        let stream_seed = xxhash_rust::xxh3::xxh3_64(&input);
        SmallRng::seed_from_u64(stream_seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

    use crate::parameters::ModEntry;

    fn draw_ten(rng: &mut SmallRng) -> [u64; 10] {
        std::array::from_fn(|_| rng.r#gen())
    }

    #[test]
    fn from_parameters_is_deterministic() {
        let params = WorldParameters::default();
        let a = WorldRng::from_parameters(&params).effective_seed();
        let b = WorldRng::from_parameters(&params).effective_seed();
        assert_eq!(a, b);
    }

    #[test]
    fn same_stream_name_identical_sequences() {
        let rng = WorldRng::from_effective_seed(42);
        let seq_a = draw_ten(&mut rng.stream("tectonics.plates"));
        let seq_b = draw_ten(&mut rng.stream("tectonics.plates"));
        assert_eq!(seq_a, seq_b);
    }

    #[test]
    fn different_stream_names_differ() {
        let rng = WorldRng::from_effective_seed(42);
        let a = draw_ten(&mut rng.stream("a"));
        let b = draw_ten(&mut rng.stream("b"));
        assert_ne!(a, b);
    }

    #[test]
    fn different_effective_seeds_differ() {
        let a = draw_ten(&mut WorldRng::from_effective_seed(1).stream("same"));
        let b = draw_ten(&mut WorldRng::from_effective_seed(2).stream("same"));
        assert_ne!(a, b);
    }

    #[test]
    fn manifest_change_changes_effective_seed() {
        let mut p1 = WorldParameters::default();
        let mut p2 = WorldParameters::default();
        p2.core.mod_manifest.mods.push(ModEntry {
            id: "extra".into(),
            version: "1.0.0".into(),
            content_hash: None,
        });
        assert_ne!(
            WorldRng::from_parameters(&p1).effective_seed(),
            WorldRng::from_parameters(&p2).effective_seed()
        );
        let _ = &mut p1;
    }

    #[test]
    fn seed_string_change_changes_effective_seed() {
        let mut p1 = WorldParameters::default();
        let mut p2 = WorldParameters::default();
        p2.core.seed = WorldSeed::from_string("other");
        assert_ne!(
            WorldRng::from_parameters(&p1).effective_seed(),
            WorldRng::from_parameters(&p2).effective_seed()
        );
        let _ = &mut p1;
    }
}
