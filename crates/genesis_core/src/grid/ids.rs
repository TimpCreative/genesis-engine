use serde::{Deserialize, Serialize};

/// Opaque, dense identifier for a hex cell.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HexId(pub u32);

/// Direction within a hex's local frame. Six standard directions; pentagons use only five.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum Direction {
    D0,
    D1,
    D2,
    D3,
    D4,
    D5,
}

impl Direction {
    pub const COUNT: usize = 6;

    pub fn index(self) -> usize {
        match self {
            Direction::D0 => 0,
            Direction::D1 => 1,
            Direction::D2 => 2,
            Direction::D3 => 3,
            Direction::D4 => 4,
            Direction::D5 => 5,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Direction::D0),
            1 => Some(Direction::D1),
            2 => Some(Direction::D2),
            3 => Some(Direction::D3),
            4 => Some(Direction::D4),
            5 => Some(Direction::D5),
            _ => None,
        }
    }
}
