pub(crate) mod dyn_future;
pub(crate) mod algorithm;
mod wheel;
mod handle;
mod registry;
mod storage;
pub(crate) mod stat;
mod unordered_algorithm;

pub use self::handle::{IdNum, SpawnParams, State, WheelHandle};
pub use self::wheel::{LockedWheel, SuspendError, Wheel};

use crate::dy::unordered_algorithm::UnorderedAlgorithm;
use crate::dy::registry::Registry;

// main algorithm for scheduling in Wheel
pub(crate) type Algorithm<'a> = UnorderedAlgorithm<Registry<'a>>;