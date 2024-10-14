use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct StereoToMonoNode;

impl<C> AudioNode<C> for StereoToMonoNode {
    fn debug_name(&self) -> &'static str {
        "stereo_to_mono"
    }

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
        frames: usize,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        proc_info: ProcInfo<C>,
    ) {
        if proc_info.in_silence_mask.all_channels_silent(2)
            || inputs.len() < 2
            || outputs.is_empty()
        {
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        for (out_s, (&in1, &in2)) in outputs[0]
            .iter_mut()
            .zip(inputs[0].iter().zip(inputs[1].iter()))
        {
            *out_s = (in1 + in2) * 0.5;
        }
    }
}

impl<C> Into<Box<dyn AudioNode<C>>> for StereoToMonoNode {
    fn into(self) -> Box<dyn AudioNode<C>> {
        Box::new(self)
    }
}
