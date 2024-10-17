use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct MonoToStereoNode;

impl AudioNode for MonoToStereoNode {
    fn debug_name(&self) -> &'static str {
        "mono_to_stereo"
    }

    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 1,
            num_max_supported_inputs: 1,
            num_min_supported_outputs: 2,
            num_max_supported_outputs: 2,
            updates: false,
        }
    }

    fn activate(
        &mut self,
        _sample_rate: u32,
        _max_block_frames: usize,
        _num_inputs: usize,
        _num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor>, Box<dyn std::error::Error>> {
        Ok(Box::new(MonoToStereoProcessor))
    }
}

struct MonoToStereoProcessor;

impl AudioNodeProcessor for MonoToStereoProcessor {
    fn process(
        &mut self,
        frames: usize,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        proc_info: ProcInfo,
    ) {
        if proc_info.in_silence_mask.is_channel_silent(0) {
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        let input = inputs[0];
        outputs[0][..frames].copy_from_slice(&input[..frames]);
        outputs[1][..frames].copy_from_slice(&input[..frames]);
    }
}

impl Into<Box<dyn AudioNode>> for MonoToStereoNode {
    fn into(self) -> Box<dyn AudioNode> {
        Box::new(self)
    }
}
