use std::error::Error;

use crate::{BlockFrames, SilenceMask};

pub trait AudioNode<C, const MBF: usize>: 'static {
    fn info(&self) -> AudioNodeInfo;

    /// Activate the audio node for processing.
    fn activate(
        &mut self,
        sample_rate: u32,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C, MBF>>, Box<dyn Error>>;

    /// Called when the processor counterpart has been deactivated
    /// and dropped.
    ///
    /// If the audio graph counterpart has gracefully shut down, then
    /// the processor counterpart is returned.
    #[allow(unused)]
    fn deactivate(&mut self, processor: Option<Box<dyn AudioNodeProcessor<C, MBF>>>) {}
}

pub trait AudioNodeProcessor<C, const MBF: usize>: 'static + Send {
    /// Process the given block of audio. Only process data in the
    /// buffers up to `frames`.
    ///
    /// Note, all output buffers *MUST* be filled with data up to
    /// `frames`.
    ///
    /// If any output buffers contain all zeros up to `frames` (silent),
    /// then mark that buffer as silent in [`ProcInfo::out_silence_mask`].
    fn process(
        &mut self,
        frames: BlockFrames<MBF>,
        proc_info: ProcInfo<C>,
        inputs: &[&[f32; MBF]],
        outputs: &mut [&mut [f32; MBF]],
    );
}

/// Additional information about an [`AudioNode`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioNodeInfo {
    /// The minimum number of input buffers this node supports
    pub num_min_supported_inputs: u32,
    /// The maximum number of input buffers this node supports
    ///
    /// This value must be less than `64`.
    pub num_max_supported_inputs: u32,

    /// The minimum number of output buffers this node supports
    pub num_min_supported_outputs: u32,
    /// The maximum number of output buffers this node supports
    ///
    /// This value must be less than `64`.
    pub num_max_supported_outputs: u32,
}

/// Additional information for processing audio
pub struct ProcInfo<'a, C> {
    /// An optional optimization hint on which input channels contain
    /// all zeros (silence). The first bit (`0x1`) is the first channel,
    /// the second bit is the second channel, and so on.
    pub in_silence_mask: SilenceMask,

    /// An optional optimization hint to notify the host which output
    /// channels contain all zeros (silence). The first bit (`0x1`) is
    /// the first channel, the second bit is the second channel, and so
    /// on.
    ///
    /// By default no channels are flagged as silent.
    pub out_silence_mask: &'a mut SilenceMask,

    /// A global user-defined context.
    pub cx: &'a mut C,
}
