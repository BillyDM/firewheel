use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct HardClipNode {
    threshold_amp: f32,
}

impl HardClipNode {
    pub fn new(threshold_db: f32) -> Self {
        Self {
            threshold_amp: firewheel_core::util::db_to_amp_clamped_neg_100_db(threshold_db),
        }
    }
}

impl<C> AudioNode<C> for HardClipNode {
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
    ) -> Result<Box<dyn AudioNodeProcessor<C>>, Box<dyn std::error::Error>> {
        if num_inputs != num_outputs {
            return Err(format!("The number of inputs on a HardClip node must equal the number of outputs. Got num_inputs: {}, num_outputs: {}", num_inputs, num_outputs).into());
        }

        Ok(Box::new(HardClipProcessor {
            threshold_amp: self.threshold_amp,
        }))
    }
}

struct HardClipProcessor {
    threshold_amp: f32,
}

impl<C> AudioNodeProcessor<C> for HardClipProcessor {
    fn process(
        &mut self,
        _frames: usize,
        proc_info: ProcInfo<C>,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) {
        // Provide an optimized loop for stereo.
        if inputs.len() == 2
            && outputs.len() == 2
            && !proc_info.in_silence_mask.any_channel_silent(2)
        {
            let in1 = inputs[0];
            let in2 = &inputs[1][0..in1.len()];
            let (out1, out2) = outputs.split_first_mut().unwrap();
            let out1 = &mut out1[0..in1.len()];
            let out2 = &mut out2[0][0..in1.len()];

            for i in 0..in1.len() {
                out1[i] = in1[i].min(self.threshold_amp).max(-self.threshold_amp);
                out2[i] = in2[i].min(self.threshold_amp).max(-self.threshold_amp);
            }

            return;
        }

        for (i, (output, input)) in outputs.iter_mut().zip(inputs.iter()).enumerate() {
            if proc_info.in_silence_mask.is_channel_silent(i) {
                output.fill(0.0);
                continue;
            }

            for (out_s, in_s) in output.iter_mut().zip(input.iter()) {
                *out_s = in_s.min(self.threshold_amp).max(-self.threshold_amp);
            }
        }

        *proc_info.out_silence_mask = proc_info.in_silence_mask;
    }
}
