use firewheel_core::{
    node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo},
    BlockFrames,
};

pub struct MonoToStereoNode;

impl<C, const MBF: usize> AudioNode<C, MBF> for MonoToStereoNode {
    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 1,
            num_max_supported_inputs: 1,
            num_min_supported_outputs: 2,
            num_max_supported_outputs: 2,
        }
    }

    fn activate(
        &mut self,
        _sample_rate: u32,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C, MBF>>, Box<dyn std::error::Error>> {
        Ok(Box::new(MonoToStereoProcessor))
    }
}

struct MonoToStereoProcessor;

impl<C, const MBF: usize> AudioNodeProcessor<C, MBF> for MonoToStereoProcessor {
    fn process(
        &mut self,
        frames: BlockFrames<MBF>,
        inputs: &[&[f32; MBF]],
        outputs: &mut [&mut [f32; MBF]],
        proc_info: ProcInfo<C>,
    ) {
        if proc_info.in_silence_mask.is_channel_silent(0) {
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        let frames = frames.get();

        let input = inputs[0];
        outputs[0][..frames].copy_from_slice(&input[..frames]);
        outputs[1][..frames].copy_from_slice(&input[..frames]);
    }
}

impl<C, const MBF: usize> Into<Box<dyn AudioNode<C, MBF>>> for MonoToStereoNode {
    fn into(self) -> Box<dyn AudioNode<C, MBF>> {
        Box::new(self)
    }
}
