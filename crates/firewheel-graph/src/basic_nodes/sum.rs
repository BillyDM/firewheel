use firewheel_core::node::{AudioNode, AudioNodeInfo, AudioNodeProcessor, ProcInfo};

pub struct SumNode;

impl AudioNode for SumNode {
    fn debug_name(&self) -> &'static str {
        "sum"
    }

    fn info(&self) -> AudioNodeInfo {
        AudioNodeInfo {
            num_min_supported_inputs: 1,
            num_max_supported_inputs: 64,
            num_min_supported_outputs: 1,
            num_max_supported_outputs: 64,
            updates: false,
        }
    }

    fn activate(
        &mut self,
        _sample_rate: u32,
        _max_block_frames: usize,
        num_inputs: usize,
        num_outputs: usize,
    ) -> Result<Box<dyn AudioNodeProcessor>, Box<dyn std::error::Error>> {
        if num_inputs % num_outputs != 0 {
            return Err(format!("The number of inputs on a SumNode must be a multiple of the number of outputs. Got num_inputs: {}, num_outputs: {}", num_inputs, num_outputs).into());
        }

        Ok(Box::new(SumNodeProcessor {
            num_in_ports: num_inputs / num_outputs,
        }))
    }
}

struct SumNodeProcessor {
    num_in_ports: usize,
}

impl AudioNodeProcessor for SumNodeProcessor {
    fn process(
        &mut self,
        frames: usize,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
        proc_info: ProcInfo,
    ) {
        let num_inputs = inputs.len();
        let num_outputs = outputs.len();

        if proc_info.in_silence_mask.all_channels_silent(inputs.len()) {
            // All inputs are silent. Just clear outputs and return.
            firewheel_core::util::clear_all_outputs(frames, outputs, proc_info.out_silence_mask);
            return;
        }

        if num_inputs == num_outputs {
            // No need to sum, just copy.
            for (out, input) in outputs.iter_mut().zip(inputs.iter()) {
                out[..frames].copy_from_slice(&input[..frames]);
            }
            *proc_info.out_silence_mask = proc_info.in_silence_mask;
            return;
        }

        match self.num_in_ports {
            // Provide a few optimized loops for common number of input ports.
            2 => {
                assert!(num_inputs >= (num_outputs * 2));

                for (ch_i, out) in outputs.iter_mut().enumerate() {
                    let in1 = &inputs[ch_i][..frames];
                    let in2 = &inputs[(num_outputs * 1) + ch_i][..frames];
                    let out = &mut out[0..frames];

                    for i in 0..frames {
                        out[i] = in1[i] + in2[i];
                    }
                }
            }
            3 => {
                assert!(num_inputs >= (num_outputs * 3));

                for (ch_i, out) in outputs.iter_mut().enumerate() {
                    let in1 = &inputs[ch_i][..frames];
                    let in2 = &inputs[(num_outputs * 1) + ch_i][..frames];
                    let in3 = &inputs[(num_outputs * 2) + ch_i][..frames];
                    let out = &mut out[0..frames];

                    for i in 0..frames {
                        out[i] = in1[i] + in2[i] + in3[i];
                    }
                }
            }
            4 => {
                assert!(num_inputs >= (num_outputs * 4));

                for (ch_i, out) in outputs.iter_mut().enumerate() {
                    let in1 = &inputs[ch_i][..frames];
                    let in2 = &inputs[(num_outputs * 1) + ch_i][..frames];
                    let in3 = &inputs[(num_outputs * 2) + ch_i][..frames];
                    let in4 = &inputs[(num_outputs * 3) + ch_i][..frames];
                    let out = &mut out[0..frames];

                    for i in 0..frames {
                        out[i] = in1[i] + in2[i] + in3[i] + in4[i];
                    }
                }
            }
            n => {
                assert!(num_inputs >= (num_outputs * n));

                for (ch_i, out) in outputs.iter_mut().enumerate() {
                    let out = &mut out[0..frames];

                    out.copy_from_slice(&inputs[ch_i][..frames]);

                    for in_port_i in 1..n {
                        let in_ch_i = (num_outputs * in_port_i) + ch_i;

                        if proc_info.in_silence_mask.is_channel_silent(in_ch_i) {
                            continue;
                        }

                        let input = &inputs[in_ch_i][..frames];

                        for i in 0..frames {
                            out[i] += input[i];
                        }
                    }
                }
            }
        }
    }
}

impl Into<Box<dyn AudioNode>> for SumNode {
    fn into(self) -> Box<dyn AudioNode> {
        Box::new(self)
    }
}
