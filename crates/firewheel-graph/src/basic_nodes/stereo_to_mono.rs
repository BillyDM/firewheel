use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct StereoToMonoNode;

impl<C> AudioNode<C> for StereoToMonoNode {
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
        _max_block_frames: usize,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor<C>>, Box<dyn std::error::Error>> {
        Ok(Box::new(StereoToMonoProcessor))
    }
}

struct StereoToMonoProcessor;

impl<C> AudioNodeProcessor<C> for StereoToMonoProcessor {
    fn process(
        &mut self,
        _frames: usize,
        proc_info: ProcInfo<C>,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) {
        if proc_info.in_silence_mask.all_channels_silent(2) {
            firewheel_core::util::clear_all_outputs(outputs, proc_info.out_silence_mask);
            return;
        }

        let out = &mut *outputs[0];
        let in1 = &inputs[0][0..out.len()];
        let in2 = &inputs[1][0..out.len()];

        for i in 0..out.len() {
            out[i] = (in1[i] + in2[i]) * 0.5;
        }
    }
}
