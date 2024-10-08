use firewheel_core::{
    node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo},
    BlockFrames,
};

pub struct StereoToMonoNode;

impl<C, const MBF: usize> AudioNode<C, MBF> for StereoToMonoNode {
    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 2,
            num_max_supported_inputs: 2,
            num_min_supported_outputs: 1,
            num_max_supported_outputs: 1,
        }
    }

    fn activate(
        &mut self,
        _sample_rate: u32,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C, MBF>>, Box<dyn std::error::Error>> {
        Ok(Box::new(StereoToMonoProcessor))
    }
}

struct StereoToMonoProcessor;

impl<C, const MBF: usize> AudioNodeProcessor<C, MBF> for StereoToMonoProcessor {
    fn process(
        &mut self,
        frames: BlockFrames<MBF>,
        proc_info: ProcInfo<C>,
        inputs: &[&[f32; MBF]],
        outputs: &mut [&mut [f32; MBF]],
    ) {
        if proc_info.in_silence_mask.all_channels_silent(2)
            || inputs.len() < 2
            || outputs.is_empty()
        {
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        for i in 0..frames.get() {
            outputs[0][i] = (inputs[0][i] + inputs[1][i]) * 0.5;
        }
    }
}
