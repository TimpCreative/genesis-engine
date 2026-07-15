//! Top-level container bundling the in-memory state of a world.

use crate::branches::{BranchId, BranchTree};
use crate::data::WorldData;
use crate::rng::WorldRng;

/// Complete in-memory state of a Genesis Engine world.
///
/// Bundles the data layer, the branch tree, and the deterministic RNG.
/// This is the unit that `create_world`, `generate_full_history`,
/// `save_world`, and `load_world` operate on.
#[derive(Clone)]
pub struct World {
    pub data: WorldData,
    pub branch_tree: BranchTree,
    pub rng: WorldRng,
}

impl World {
    /// Returns the currently-loaded branch's ID. For now, always the root.
    /// Branch switching is wired in a future phase.
    pub fn current_branch(&self) -> BranchId {
        self.branch_tree.root_id()
    }
}

#[cfg(test)]
mod tests {
    use crate::branches::BranchId;
    use crate::lifecycle::create_world;
    use crate::parameters::WorldParameters;

    #[test]
    fn current_branch_returns_root() {
        let world = create_world(WorldParameters::default()).unwrap();
        assert_eq!(world.current_branch(), BranchId::ROOT);
    }
}
