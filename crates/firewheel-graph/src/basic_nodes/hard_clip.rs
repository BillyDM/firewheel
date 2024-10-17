use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct HardClipNode {
    threshold_gain: f32,
}

impl HardClipNode {
    pub fn new(threshold_db: f32) -> Self {
        Self {
            threshold_gain: firewheel_core::util::db_to_gain_clamped_neg_100_db(threshold_db),
        }
    }
}

impl AudioNode for HardClipNode {
    fn debug_name(&self) -> &'static str {
        "hard_clip"
    }

    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 1,
            num_max_supported_inputs: 64,
            num_min_supported_outputs: 1,
            num_max_supported_outputs: 64,
        }
    }

    fn activate(
        &mut self,
        _sample_rate: u32,
        _max_block_frames: usize,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor>, Box<dyn std::error::Error>> {
        if num_inputs != num_outputs {
            return Err(format!("The number of inputs on a HardClip node must equal the number of outputs. Got num_inputs: {}, num_outputs: {}", num_inputs, num_outputs).into());
        }

        Ok(Box::new(HardClipProcessor {
            threshold_gain: self.threshold_gain,
        }))
    }
}

struct HardClipProcessor {
    threshold_gain: f32,
}

impl AudioNodeProcessor for HardClipProcessor {
    fn process(
        &mut self,
        frames: usize,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        proc_info: ProcInfo,
    ) {
        // Provide an optimized loop for stereo.
        if inputs.len() == 2
            && outputs.len() == 2
            && !proc_info.in_silence_mask.any_channel_silent(2)
        {
            // Hint to the compiler to optimize loop.
            assert!(frames <= outputs[0].len());
            assert!(frames <= outputs[1].len());
            assert!(frames <= inputs[0].len());
            assert!(frames <= inputs[1].len());

            for i in 0..frames {
                outputs[0][i] = inputs[0][i]
                    .min(self.threshold_gain)
                    .max(-self.threshold_gain);
                outputs[1][i] = inputs[1][i]
                    .min(self.threshold_gain)
                    .max(-self.threshold_gain);
            }

            return;
        }

        for (i, (output, input)) in outputs.iter_mut().zip(inputs.iter()).enumerate() {
            if proc_info.in_silence_mask.is_channel_silent(i) {
                output[..frames].fill(0.0);
                continue;
            }

            for (out_s, in_s) in output.iter_mut().zip(input.iter()) {
                *out_s = in_s.min(self.threshold_gain).max(-self.threshold_gain);
            }
        }

        *proc_info.out_silence_mask = proc_info.in_silence_mask;
    }
}

impl Into<Box<dyn AudioNode>> for HardClipNode {
    fn into(self) -> Box<dyn AudioNode> {
        Box::new(self)
    }
}
