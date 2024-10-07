use std::error::Error;

use crate::SilenceMask;

/// A globally unique identifier for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeID(pub(crate) thunderdome::Index);

pub trait AudioNode: 'static {
    fn info(&self) -> AudioNodeInfo;

    /// Activate the audio node for processing.
    fn activate(
        &mut self,
        sample_rate: u32,
        max_block_frames: usize,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor>, Box<dyn Error>>;

    /// Called when the processor counterpart has been deactivated
    /// and dropped.
    ///
    /// If the audio graph counterpart has gracefully shut down, then
    /// the processor counterpart is returned.
    #[allow(unused)]
    fn deactivate(&mut self, processor: Option<Box<dyn AudioNodeProcessor>>) {}
}

pub trait AudioNodeProcessor: 'static + Send {
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
        frames: usize,
        proc_info: ProcInfo,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) -> ProcessStatus;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Ok,
    Err { msg: &'static str },
}

/// Additional information for processing audio
pub struct ProcInfo<'a> {
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
}

pub struct DummyAudioNode;

impl AudioNode for DummyAudioNode {
    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 0,
            num_max_supported_inputs: u32::MAX,
            num_min_supported_outputs: 0,
            num_max_supported_outputs: u32::MAX,
        }
    }

    /// Activate the audio node for processing.
    fn activate(
        &mut self,
        _sample_rate: u32,
        _max_block_frames: usize,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor>, Box<dyn Error>> {
        Ok(Box::new(DummyAudioNodeProcessor))
    }
}

pub struct DummyAudioNodeProcessor;

impl AudioNodeProcessor for DummyAudioNodeProcessor {
    fn process(
        &mut self,
        _frames: usize,
        _proc_info: ProcInfo,
        _inputs: &[&[f32]],
        _outputs: &mut [&mut [f32]],
    ) -> ProcessStatus {
        ProcessStatus::Ok
    }
}
