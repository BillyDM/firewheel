pub mod backend;
pub mod basic_nodes;
pub mod context;
pub mod graph;
pub mod processor;

pub use context::{ActiveFwCtx, InactiveFwCtx};

/// The default maximum number of frames that can appear in a
/// processing block.
///
/// This number is a balance between processing overhead and
/// cache efficiency. Lower values have better cache efficieny
/// but more overhead, and higher values have worse cache
/// efficiency but less overhead.
///
/// We may need to experiment with
/// different values to see what is the best for a typical game
/// audio graph. (The value must also be a power of two.)
pub const DEFAULT_MAX_BLOCK_FRAMES: usize = 512;
