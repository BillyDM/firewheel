pub use firewheel_core::*;
pub use firewheel_graph::*;

#[cfg(feature = "cpal")]
pub use firewheel_cpal::*;

#[cfg(feature = "cpal")]
pub type InactiveCtx = InactiveFwCpalCtx<(), DEFAULT_MAX_BLOCK_FRAMES>;
#[cfg(feature = "cpal")]
pub type ActiveCtx = ActiveFwCpalCtx<(), DEFAULT_MAX_BLOCK_FRAMES>;
#[cfg(feature = "cpal")]
pub type Context = FwCpalCtx<(), DEFAULT_MAX_BLOCK_FRAMES>;
