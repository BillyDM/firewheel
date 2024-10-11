use std::error::Error;

use firewheel_core::{
    node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo},
    BlockFrames,
};

pub struct DummyAudioNode;

impl<C, const MBF: usize> AudioNode<C, MBF> for DummyAudioNode {
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
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C, MBF>>, Box<dyn Error>> {
        Ok(Box::new(DummyAudioNodeProcessor))
    }
}

pub struct DummyAudioNodeProcessor;

impl<C, const MBF: usize> AudioNodeProcessor<C, MBF> for DummyAudioNodeProcessor {
    fn process(
        &mut self,
        _frames: BlockFrames<MBF>,
        _inputs: &[&[f32; MBF]],
        _outputs: &mut [&mut [f32; MBF]],
        _proc_info: ProcInfo<C>,
    ) {
    }
}

impl<C, const MBF: usize> Into<Box<dyn AudioNode<C, MBF>>> for DummyAudioNode {
    fn into(self) -> Box<dyn AudioNode<C, MBF>> {
        Box::new(self)
    }
}
