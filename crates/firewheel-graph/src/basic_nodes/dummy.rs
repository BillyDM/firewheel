use std::error::Error;

use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct DummyAudioNode;

impl<C> AudioNode<C> for DummyAudioNode {
    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 0,
            num_max_supported_inputs: 64,
            num_min_supported_outputs: 0,
            num_max_supported_outputs: 64,
        }
    }

    fn activate(
        &mut self,
        _sample_rate: u32,
        _max_block_frames: usize,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C>>, Box<dyn Error>> {
        Ok(Box::new(DummyAudioNodeProcessor))
    }
}

pub struct DummyAudioNodeProcessor;

impl<C> AudioNodeProcessor<C> for DummyAudioNodeProcessor {
    fn process(
        &mut self,
        _frames: usize,
        _proc_info: ProcInfo<C>,
        _inputs: &[&[f32]],
        _outputs: &mut [&mut [f32]],
    ) {
    }
}
